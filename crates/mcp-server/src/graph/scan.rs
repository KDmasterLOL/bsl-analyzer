use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use walkdir::WalkDir;

use super::input::scan_roots;
use crate::change_hub::{ChangeEntry, ChangeKind};

/// How often the query-path freshness fold must come from a real walk instead of the
/// event-maintained map. Bounds how long a change the hub cannot observe can keep
/// freshness wrong.
pub(super) const WALK_VERIFY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// See `GraphState::fp_map`.
#[derive(Default)]
pub(super) struct FpMapState {
    /// `canonical path → (mtime nanos, len)` for every graph-relevant file, in the
    /// exact spelling the walk produces (hub entries carry the same canonical key).
    pub(super) map: Option<std::collections::BTreeMap<String, (u128, u64)>>,
    /// When the map was last anchored to a real walk.
    pub(super) walked_at: Option<Instant>,
}

/// Throttled cache of the last on-disk fingerprint scan. Guarded by its own mutex
/// held *across* the walk, so concurrent callers serialize onto one scan per
/// window rather than all walking the tree (no thundering herd).
pub(super) struct ScanCache {
    pub(super) at: Instant,
    pub(super) disk_fp: u64,
}

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
/// [`scan_file_stats`] produces, or `None` if it is absent or not a regular file.
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

/// Whether a drained change is relevant to the graph's fingerprint scan (`.bsl`/`.xml`
/// under a scan root). A removed file's `canonical` may fall back to the raw spelling, so
/// both are checked; a `SubtreeRemoved` names a vanished directory whose descendants'
/// extensions are unknown, so it is treated conservatively as relevant.
pub(super) fn entry_touches_scan_universe(entry: &ChangeEntry) -> bool {
    if entry.kind == ChangeKind::SubtreeRemoved {
        return true;
    }
    let is_scan_ext = |path: &Path| {
        matches!(path.extension().and_then(|e| e.to_str()), Some("bsl") | Some("xml"))
    };
    is_scan_ext(&entry.canonical) || is_scan_ext(&entry.raw)
}

/// Stat every graph-relevant file (`.bsl` sources + `.xml` metadata descriptors)
/// under the scan roots, once. Covers both extensions because graph resolution
/// depends on configuration visibility registered from the metadata, not only on
/// module text. Uses `(canonical path, mtime, len)` — stat only, no file reads —
/// and mirrors the loader's scan roots and symlink/canonicalization policy so it
/// compares the same file universe (otherwise it would report phantom drift).
pub(crate) fn scan_file_stats(workspace_root: &Path) -> Vec<FileStat> {
    scan_stats_over_roots(&scan_roots(workspace_root))
}

/// The parallel scan over an explicit set of roots (each a directory, or occasionally a
/// single file for a misconfigured extension path). Split out so it can be exercised
/// directly against the sequential reference in tests.
pub(super) fn scan_stats_over_roots(roots: &[PathBuf]) -> Vec<FileStat> {
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

/// A cheap, order-independent fingerprint of the graph-relevant files on disk.
/// Folds every file's `(path, mtime, len)` into one `u64`; B4 cache reuse compares
/// it for an exact whole-workspace match.
pub(super) fn workspace_fingerprint(workspace_root: &Path) -> u64 {
    let mut entries: Vec<(String, u128, u64)> =
        scan_file_stats(workspace_root).into_iter().map(|s| (s.path, s.mtime, s.len)).collect();
    entries.sort();
    fold_fingerprint_entries(&entries)
}

/// The one fold both fingerprint producers share, so the event-maintained map and the
/// walk agree bit-for-bit: `entries` must be sorted `(path, mtime, len)` tuples (paths
/// are unique, so path order alone determines it).
pub(super) fn fold_fingerprint_entries(entries: &[(String, u128, u64)]) -> u64 {
    let mut hasher = DefaultHasher::new();
    entries.hash(&mut hasher);
    hasher.finish()
}

/// The raw `(mtime nanos, len)` pair for the fingerprint map, matching what
/// [`scan_file_stats`] records for a present regular file.
pub(super) fn stat_pair(path: &Path) -> Option<(u128, u64)> {
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
    Some((mtime, meta.len()))
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
