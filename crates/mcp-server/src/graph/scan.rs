use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use walkdir::WalkDir;

/// One graph-relevant file's stat-only identity: canonical `/`-normalised path,
/// mtime in nanos, and length. Produced once per scan and shared by the
/// whole-workspace fingerprint (which folds them) and the per-file `files` table
/// (which persists them for granular drift classification).
#[derive(Clone)]
pub(crate) struct FileStat {
    pub(crate) path: String,
    pub(super) mtime: u128,
    pub(super) len: u64,
}

impl FileStat {
    /// The per-file fingerprint stored in (and compared against) the `files` table.
    /// Must stay deterministic across runs so a reload's recomputed value matches the
    /// stored one for an unchanged file.
    pub(crate) fn fingerprint(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        (self.mtime, self.len).hash(&mut hasher);
        hasher.finish()
    }
}

/// The drift fingerprint of a single file on disk, matching the per-file value
/// the stats scan produces, or `None` if it is absent or not a regular file.
/// Lets the event-driven drift path re-stat only the changed paths instead of
/// walking the whole workspace — events are hints, this stat is the truth.
pub(crate) fn file_fingerprint(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Some(FileStat { path: String::new(), mtime, len: meta.len() }.fingerprint())
}

/// Stat every graph-relevant file (`.bsl` sources + `.xml` metadata descriptors)
/// under the scan roots, once. Covers both extensions because graph resolution
/// depends on configuration visibility registered from the metadata, not only on
/// module text. Uses `(canonical path, mtime, len)` — stat only, no file reads —
/// and mirrors the loader's scan roots and symlink/canonicalization policy so it
/// compares the same file universe (otherwise it would report phantom drift).
/// Retained as the test-side wrapper (production callers derive the roots from an
/// explicit `ProjectSnapshot` so stats and topology come from one project state).
#[cfg(test)]
pub(crate) fn scan_file_stats(workspace_root: &Path) -> Vec<FileStat> {
    scan_stats_over_roots(&super::input::scan_roots(workspace_root))
}

/// The parallel scan over an explicit set of roots (each a directory, or occasionally a
/// single file for a misconfigured extension path). Split out so it can be exercised
/// directly against the sequential reference in tests.
pub(crate) fn scan_stats_over_roots(roots: &[PathBuf]) -> Vec<FileStat> {
    use rayon::prelude::*;

    // Work units: each top-level entry under each scan-root directory. Directories are
    // walked in parallel; a file reachable through two roots (e.g. a symlinked subtree) is
    // de-duplicated when the per-unit results are merged, so the output set is independent
    // of how the work was partitioned.
    let mut units: Vec<PathBuf> = Vec::new();
    for root in roots {
        match std::fs::read_dir(root) {
            Ok(entries) => units.extend(entries.flatten().map(|e| e.path())),
            // Not a directory (a misconfigured extension path pointing at a file) or
            // unreadable: a file root is itself one work unit — matching the old
            // `WalkDir::new(file)` which yielded it; anything else contributes nothing.
            Err(_) => {
                if root.is_file() {
                    units.push(root.clone());
                }
            }
        }
    }

    let per_unit: Vec<Vec<FileStat>> = units.par_iter().map(|unit| walk_unit_stats(unit)).collect();

    // Merge and de-duplicate by canonical path. The kept `FileStat` is identical whichever
    // occurrence wins: `(mtime, len)` come from the same on-disk target.
    let mut seen: HashSet<String> = HashSet::new();
    let mut stats: Vec<FileStat> = Vec::new();
    for stat in per_unit.into_iter().flatten() {
        if seen.insert(stat.path.clone()) {
            stats.push(stat);
        }
    }
    stats
}

/// Walk one top-level unit (a directory subtree, or a single top-level file) and collect
/// the `(canonical path, mtime, len)` of every `.bsl`/`.xml` under it. A per-directory
/// cache canonicalises each containing directory once instead of every file, which
/// dominates the walk's syscall cost on a large configuration.
fn walk_unit_stats(unit: &Path) -> Vec<FileStat> {
    let mut out: Vec<FileStat> = Vec::new();
    let mut dir_cache: HashMap<PathBuf, PathBuf> = HashMap::new();
    for entry in WalkDir::new(unit).follow_links(true) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        match entry.path().extension().and_then(|e| e.to_str()) {
            Some("bsl") | Some("xml") => {}
            _ => continue,
        }
        let canonical = canonical_file_path(entry.path(), entry.path_is_symlink(), &mut dir_cache);
        let (mtime, len) = entry
            .metadata()
            .ok()
            .map(|m| {
                let mtime = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                (mtime, m.len())
            })
            .unwrap_or((0, 0));
        out.push(FileStat { path: canonical.to_string_lossy().into_owned(), mtime, len });
    }
    out
}

/// The canonical path of a walked file, matching `entry.path().canonicalize()` but reusing
/// a per-directory canonicalisation. A file that is ITSELF a symlink is canonicalised
/// directly (its target lies elsewhere); a plain file inherits its directory's canonical
/// prefix joined with its own name — identical to canonicalising the whole path, since only
/// the directory components could contain symlinks.
fn canonical_file_path(
    path: &Path,
    is_symlink: bool,
    dir_cache: &mut HashMap<PathBuf, PathBuf>,
) -> PathBuf {
    if is_symlink {
        return path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => {
            let canonical_parent = dir_cache
                .entry(parent.to_path_buf())
                .or_insert_with(|| parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf()));
            canonical_parent.join(name)
        }
        _ => path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
    }
}

/// A cheap fingerprint of the workspace identity: the order-independent fold of
/// every graph-relevant file's `(path, mtime, len)` plus the extension-topology
/// hash. Cache reuse compares it for an exact whole-workspace match.
pub(super) fn workspace_fingerprint(workspace_root: &Path) -> crate::graph_db::GraphFp {
    workspace_fingerprint_over(&super::input::ProjectSnapshot::load(workspace_root))
}

/// The fingerprint over an already-loaded project snapshot, so an operation
/// that brackets a build with pre/post scans stats the SAME root universe both
/// times instead of re-deriving the project mid-operation. This is the ONE fold
/// point for graph identity — the build/adoption bracket and the live query-path
/// freshness check both go through it, so a topology-only change (a `dependsOn`
/// edit, an auto-discovered extension) can never be fresh on one path and stale
/// on the other.
pub(crate) fn workspace_fingerprint_over(
    project: &super::input::ProjectSnapshot,
) -> crate::graph_db::GraphFp {
    let mut entries: Vec<(String, u128, u64)> = scan_stats_over_roots(&project.scan_roots)
        .into_iter()
        .map(|s| (s.path, s.mtime, s.len))
        .collect();
    entries.sort();
    crate::graph_db::GraphFp {
        files: super::snapshot::fold_fingerprint_entries(&entries),
        topology: topology_u64(&project.configs),
    }
}

/// Stable 64-bit hash of a snapshot's extension-topology fingerprint. BLAKE3 of
/// the full hex digest (not `DefaultHasher`), so the value persisted in a graph's
/// meta survives a toolchain upgrade. `None` — legacy path-only registration or
/// the invalid-project fallback — hashes as the empty input, distinct from every
/// real digest.
pub(crate) fn topology_u64(configs: &ide::WorkspaceConfigsSnapshot) -> u64 {
    topology_hex_u64(configs.fingerprint.as_deref().unwrap_or(""))
}

/// The BLAKE3-based 64-bit fold shared by every consumer that reduces the
/// topology hex digest to one word (graph freshness, broker identity), so the
/// same topology always reduces to the same value everywhere.
pub(crate) fn topology_hex_u64(hex: &str) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(hex.as_bytes());
    let bytes = hasher.finalize();
    u64::from_le_bytes(bytes.as_bytes()[..8].try_into().expect("blake3 yields >= 8 bytes"))
}

/// Granular drift between a built graph's stored per-file fingerprints and the
/// current on-disk state. The body-only fast path acts on this; today it is computed
/// for observability while the full rebuild still runs.
pub(crate) struct WorkspaceDiff {
    pub(crate) added: Vec<String>,
    pub(crate) removed: Vec<String>,
    pub(crate) modified: Vec<String>,
}

impl WorkspaceDiff {
    pub(crate) fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }

    /// Whether any changed file is `.xml` metadata. Metadata drift can change
    /// configuration visibility for *any* module, so it forces a full rebuild — no
    /// fast path is sound for it.
    pub(crate) fn touches_metadata(&self) -> bool {
        self.added.iter().chain(&self.removed).chain(&self.modified).any(|p| p.ends_with(".xml"))
    }
}

/// Classify per-file drift between the stored fingerprint map (read from a built
/// graph's `files` table) and the current on-disk stats. A path present only on disk
/// is `added`, present only in the store is `removed`, present in both with a
/// different fingerprint is `modified`.
pub(crate) fn classify_changes(
    stored: &std::collections::HashMap<String, u64>,
    current: &[FileStat],
) -> WorkspaceDiff {
    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut seen: HashSet<&str> = HashSet::with_capacity(current.len());

    for stat in current {
        seen.insert(stat.path.as_str());
        match stored.get(&stat.path) {
            None => added.push(stat.path.clone()),
            Some(&fp) if fp != stat.fingerprint() => modified.push(stat.path.clone()),
            Some(_) => {}
        }
    }
    let mut removed: Vec<String> =
        stored.keys().filter(|p| !seen.contains(p.as_str())).cloned().collect();

    added.sort();
    modified.sort();
    removed.sort();
    WorkspaceDiff { added, removed, modified }
}
