use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use project_model::SourceSet;

use crate::graph_query::GraphDb;

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

/// The scan over an explicit set of roots (each a directory, or occasionally a
/// single file for a misconfigured extension path), projected into stats shape.
///
/// One call is one traversal (parallel across top-level directories inside
/// [`SourceSet::scan`]). An operation with several passes over the same universe
/// must take ONE `SourceSet` and project it instead of calling this per pass.
pub(crate) fn scan_stats_over_roots(roots: &[PathBuf]) -> Vec<FileStat> {
    super::universe::file_stats_from(&SourceSet::scan(roots))
}

/// A cheap fingerprint of the workspace identity: the order-independent fold of
/// every graph-relevant file's `(path, mtime, len)` plus the extension-topology
/// hash. Test-side wrapper — production callers scan a universe explicitly and
/// fold it with [`fingerprint_of`], so the verdict of the same scan stays in hand.
#[cfg(test)]
pub(super) fn workspace_fingerprint(workspace_root: &Path) -> crate::graph_db::GraphFp {
    workspace_fingerprint_over(&super::input::ProjectSnapshot::load(workspace_root))
}

/// The fingerprint over an already-loaded project snapshot. Test-side companion
/// of [`workspace_fingerprint`] — production brackets scan a universe explicitly
/// and fold it with [`fingerprint_of`], keeping the same scan's verdict in hand.
#[cfg(test)]
pub(crate) fn workspace_fingerprint_over(
    project: &super::input::ProjectSnapshot,
) -> crate::graph_db::GraphFp {
    fingerprint_of(&scan_stats_over_roots(&project.scan_roots), &project.configs)
}

/// The fold over ALREADY-SCANNED stats — the piece of the fingerprint an operation
/// can compute from a shared universe instead of paying another walk. The fold is
/// order-independent (rows are sorted here), so any projection of the same scan
/// folds to the same value.
pub(crate) fn fingerprint_of(
    stats: &[FileStat],
    configs: &ide::WorkspaceConfigsSnapshot,
) -> crate::graph_db::GraphFp {
    let mut entries: Vec<(String, u128, u64)> =
        stats.iter().map(|s| (s.path.clone(), s.mtime, s.len)).collect();
    entries.sort();
    crate::graph_db::GraphFp {
        files: super::snapshot::fold_fingerprint_entries(&entries),
        topology: topology_u64(configs),
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

/// Whether the graph database at `path` was built under the extension topology the
/// workspace declares right now.
///
/// The path is shared by every daemon over this workspace, so the file there is not
/// necessarily one WE wrote (see [`crate::workspace_lease`]): a generation keyed to another
/// topology publishes into the same name. A graph built under a different topology resolves
/// names differently, so adopting it — to serve, or to render search contexts from — would
/// persist another project shape's answers as this one's. Costs a config parse, no tree
/// walk, so it belongs on boot/publish paths rather than per query.
pub(crate) fn graph_file_matches_live_topology(workspace_root: &Path, graph: &GraphDb) -> bool {
    let Ok((_, fingerprint, _)) = graph.freshness_token() else {
        return false;
    };
    fingerprint.topology == topology_u64(&super::ProjectSnapshot::load(workspace_root).configs)
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
