//! Projections of one workspace scan into the graph's two legacy shapes.
//!
//! `SourceSet::scan` walks once and keeps every occurrence; each projection here
//! filters and de-duplicates the way ITS consumer historically keyed files, so the
//! two shapes can come from one traversal without changing either universe:
//! enumeration keys by canonical `PathBuf`, the stats scan keys by the canonical
//! lossy string, and both filter BEFORE de-duplicating — the order the old code
//! used, which decides who survives when several spellings collapse into one key.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use project_model::{SourceSet, WalkedFile};
use vfs::FileId;

use super::scan::FileStat;

/// One traversal of the scan roots, projected into every shape the graph's passes
/// consume. Passes handed the same instance provably share one universe: the
/// enumeration a build lowers, the stats its `files` table persists and the
/// fingerprint its bracket compares all come from the same walk, so no pass can
/// see a tree another pass did not.
pub(crate) struct ScannedUniverse {
    /// The `.bsl` enumeration — see [`bsl_files_from`].
    pub(crate) files: Vec<(FileId, PathBuf)>,
    /// The `.bsl` + `.xml` stats rows — see [`file_stats_from`].
    pub(crate) stats: Vec<FileStat>,
    clean: bool,
}

impl ScannedUniverse {
    /// One walk, all projections.
    pub(crate) fn scan(roots: &[PathBuf]) -> ScannedUniverse {
        let set = SourceSet::scan(roots);
        ScannedUniverse {
            files: bsl_files_from(&set),
            stats: file_stats_from(&set),
            clean: set.clean(),
        }
    }

    /// Whether the walk behind these projections may speak for the whole tree —
    /// see [`SourceSet::clean`]. A publication over an unclean universe must not
    /// claim a coherent snapshot, and a cache must not be adopted as fresh
    /// against one.
    pub(crate) fn clean(&self) -> bool {
        self.clean
    }
}

/// The graph's historical extension predicate: EXACT lowercase spelling, judged on
/// the walked path. The shared walker itself matches extensions case-insensitively;
/// widening the graph's universe to agree with it also changes every consumer that
/// decodes semantics from a file's full name, so the widening ships separately and
/// this filter is the single place it will land.
fn legacy_extension(file: &WalkedFile) -> Option<&'static str> {
    match file.walked.extension().and_then(|e| e.to_str()) {
        Some("bsl") => Some("bsl"),
        Some("xml") => Some("xml"),
        _ => None,
    }
}

/// The `.bsl` universe in enumeration shape: canonical paths with stable
/// [`FileId`]s assigned in scan order, first occurrence of each canonical path
/// winning — the keying `enumerate_bsl_files` has always used.
pub(crate) fn bsl_files_from(set: &SourceSet) -> Vec<(FileId, PathBuf)> {
    let mut entries: Vec<(FileId, PathBuf)> = Vec::new();
    let mut seen: HashSet<&PathBuf> = HashSet::new();
    let mut next_id = 0u32;
    for file in &set.files {
        if legacy_extension(file) != Some("bsl") {
            continue;
        }
        if !seen.insert(&file.canonical) {
            continue;
        }
        entries.push((FileId(next_id), file.canonical.clone()));
        next_id += 1;
    }
    entries
}

/// The `.bsl` + `.xml` universe in stats shape: `(canonical lossy string, mtime,
/// len)` rows, first occurrence of each STRING winning — the stats scan has always
/// keyed by the converted string, so two canonical paths that collapse into one
/// lossy spelling still yield one row.
pub(crate) fn file_stats_from(set: &SourceSet) -> Vec<FileStat> {
    let mut stats: Vec<FileStat> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for file in &set.files {
        if legacy_extension(file).is_none() {
            continue;
        }
        let path = file.canonical.to_string_lossy().into_owned();
        if !seen.insert(path.clone()) {
            continue;
        }
        let mtime = file
            .metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        stats.push(FileStat { path, mtime, len: file.metadata.len() });
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    fn scan(root: &Path) -> SourceSet {
        SourceSet::scan(std::slice::from_ref(&root.to_path_buf()))
    }

    /// An independent sequential reference for the enumeration universe: its own
    /// `WalkDir`, the exact-extension predicate and the canonical-`PathBuf`
    /// de-duplication the historical `enumerate_bsl_files` used — deliberately NOT
    /// built from the shared walker, so a defect in the walker or the projection
    /// cannot cancel out of the comparison.
    fn reference_bsl_files(root: &Path) -> Vec<PathBuf> {
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut files: Vec<PathBuf> = Vec::new();
        for entry in walkdir::WalkDir::new(root).follow_links(true) {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_file()
                || entry.path().extension().and_then(|e| e.to_str()) != Some("bsl")
            {
                continue;
            }
            let path = entry.path().canonicalize().unwrap_or_else(|_| entry.path().to_path_buf());
            if seen.insert(path.clone()) {
                files.push(path);
            }
        }
        files.sort();
        files
    }

    #[test]
    fn the_enumeration_projection_matches_an_independent_reference() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("CommonModules/M/Ext/Module.bsl"), "");
        write(&dir.path().join("Catalogs/C/Ext/ObjectModule.bsl"), "");
        write(&dir.path().join("Configuration.xml"), "<x/>");
        write(&dir.path().join("README.md"), "");

        let set = scan(dir.path());
        let mut projected: Vec<PathBuf> =
            bsl_files_from(&set).into_iter().map(|(_, p)| p).collect();
        projected.sort();

        assert_eq!(projected, reference_bsl_files(dir.path()));
        assert_eq!(projected.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn the_enumeration_projection_matches_the_reference_through_links() {
        let dir = tempfile::tempdir().unwrap();
        let tree = dir.path().join("tree");
        write(&tree.join("M.bsl"), "");
        let root = dir.path().join("root");
        fs::create_dir_all(&root).unwrap();
        write(&root.join("Own.bsl"), "");
        std::os::unix::fs::symlink(&tree, root.join("Linked")).unwrap();
        std::os::unix::fs::symlink(tree.join("M.bsl"), root.join("Alias.bsl")).unwrap();

        let set = scan(&root);
        let mut projected: Vec<PathBuf> =
            bsl_files_from(&set).into_iter().map(|(_, p)| p).collect();
        projected.sort();

        assert_eq!(projected, reference_bsl_files(&root));
    }

    #[test]
    fn the_shim_keeps_an_upper_case_extension_out_of_both_projections() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("Lower.bsl"), "");
        write(&dir.path().join("Upper.BSL"), "");
        write(&dir.path().join("Meta.XML"), "<x/>");

        let set = scan(dir.path());

        // The walker itself takes the upper-case spellings — the narrowing is the
        // projection's doing, so removing the shim makes this test fail.
        assert_eq!(set.files.len(), 3, "the shared walker is case-insensitive");
        let bsl: Vec<PathBuf> = bsl_files_from(&set).into_iter().map(|(_, p)| p).collect();
        assert_eq!(bsl.len(), 1);
        assert!(bsl[0].ends_with("Lower.bsl"));
        let stats = file_stats_from(&set);
        assert_eq!(stats.len(), 1);
        assert!(stats[0].path.ends_with("Lower.bsl"));
    }

    #[test]
    fn both_projections_come_from_the_one_scan_not_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("Kept.bsl"), "");
        write(&dir.path().join("Doomed.bsl"), "");

        let set = scan(dir.path());
        fs::remove_file(dir.path().join("Doomed.bsl")).unwrap();

        // The set was taken before the deletion, so every projection of it still
        // describes the pre-deletion universe — that is the whole point of sharing
        // one scan across an operation's passes.
        let bsl = bsl_files_from(&set);
        let stats = file_stats_from(&set);
        assert_eq!(bsl.len(), 2);
        assert_eq!(stats.len(), 2);
        let bsl_paths: HashSet<String> =
            bsl.iter().map(|(_, p)| p.to_string_lossy().into_owned()).collect();
        let stat_paths: HashSet<String> = stats.iter().map(|s| s.path.clone()).collect();
        assert_eq!(bsl_paths, stat_paths, "one universe, two shapes");

        let fresh = scan(dir.path());
        assert_eq!(bsl_files_from(&fresh).len(), 1, "a fresh scan does see the deletion");
    }

    #[cfg(unix)]
    #[test]
    fn a_lossy_collision_keeps_the_projections_on_their_own_keys() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        // Two distinct byte names, one lossy spelling. Different lengths, so the
        // surviving stats row is distinguishable.
        let first = dir.path().join(OsStr::from_bytes(b"\x80.bsl"));
        let second = dir.path().join(OsStr::from_bytes(b"\x81.bsl"));
        fs::write(&first, "x").unwrap();
        fs::write(&second, "xx").unwrap();

        let set = scan(dir.path());

        let bsl = bsl_files_from(&set);
        assert_eq!(bsl.len(), 2, "enumeration keys by PathBuf: both files stay");
        let stats = file_stats_from(&set);
        assert_eq!(stats.len(), 1, "stats key by the lossy string: the rows collapse");
        // The scan is name-sorted, so the surviving row is the byte-wise first
        // name — the deterministic winner that replaced the readdir lottery.
        assert_eq!(stats[0].len, 1, "the surviving row must be the first-sorted file's");
    }
}
