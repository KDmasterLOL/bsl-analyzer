//! Walking a workspace's source roots under one explicit policy.
//!
//! Every place that enumerates the working tree — the graph's file universe, the
//! topology fingerprint, the search overlay, the boot reconcile — has to agree on
//! what a source file is, whether links are followed and what an incomplete walk
//! means. This module owns that policy; callers own identity: it deliberately does
//! not attribute a file to a root and does not de-duplicate, because the store keys
//! files by `(root, relative path)` while the graph keys them by canonical path, and
//! collapsing either one here would silently lose the other's entries.

use std::collections::HashMap;
use std::fs::Metadata;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::file_role::{file_role, FileRole};

/// One file the walk reached, in both spellings it can be named by.
#[derive(Debug, Clone)]
pub struct WalkedFile {
    /// Which of the passed roots the walk arrived through.
    pub root: PathBuf,
    pub role: FileRole,
    /// The spelling the walk used to get here — the only handle on a file reached
    /// through a link that leaves every declared root.
    pub walked: PathBuf,
    /// The physical spelling, after resolving every link in the path.
    pub canonical: PathBuf,
    /// Taken WITH the link followed: the target is what gets read, so the target is
    /// what a fingerprint built from `(len, mtime)` must describe.
    pub metadata: Metadata,
}

/// The result of one walk. Completeness is a property of THIS walk, not of the tree:
/// a caller with several independent passes over the same roots must answer for each.
#[derive(Debug, Default)]
pub struct WalkOutcome {
    pub files: Vec<WalkedFile>,
    /// A root or subtree could not be read — coverage is INCOMPLETE, so a caller must
    /// not treat the file list as authoritative (reconciling against it would delete
    /// healthy entries).
    pub unreadable: usize,
    /// A symlink loop. Coverage is COMPLETE: `walkdir` reports the loop and carries on,
    /// having already walked the tree the link points back into.
    pub loops: usize,
    /// A link whose target does not exist. Coverage is COMPLETE: there is no file behind
    /// it to index.
    pub dangling: usize,
    /// How many times the walk canonicalised a path. The per-directory cache is only
    /// observable here: the file list and the elapsed time look the same whether each
    /// directory or each file was resolved, so a gate on the cache needs the count of
    /// the operation itself.
    pub canonicalizations: usize,
}

/// Walk every root, yielding each source or metadata file once per way the walk reached
/// it.
pub fn walk_workspace_roots(roots: &[PathBuf]) -> WalkOutcome {
    let mut outcome = WalkOutcome::default();
    for root in roots {
        walk_one_root(root, &mut outcome);
    }
    outcome
}

fn walk_one_root(root: &Path, outcome: &mut WalkOutcome) {
    let mut dir_cache: HashMap<PathBuf, PathBuf> = HashMap::new();
    for entry in walkdir::WalkDir::new(root).follow_links(true) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                classify_walk_error(&e, outcome);
                continue;
            }
        };
        // Following links, so this is the TARGET's type: a link to a directory is not
        // a file, and a directory named `Foo.bsl` never becomes one.
        if !entry.file_type().is_file() {
            continue;
        }
        let walked = entry.path();
        // The role has to hold for BOTH spellings. Taking a file whose canonical name
        // alone qualifies would produce a key no walk of the target's root can rebuild,
        // and taking one whose walked name alone qualifies would produce a key the
        // point-update path rejects by suffix — the two together are the only files
        // every path in the system agrees about.
        let role = file_role(walked);
        if role == FileRole::Ignored {
            continue;
        }
        let canonical = canonical_path(walked, entry.path_is_symlink(), &mut dir_cache, outcome);
        if file_role(&canonical) != role {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            // The file was reachable a moment ago and is not now: the walk saw less
            // than the tree holds, which is exactly what `unreadable` means.
            Err(_) => {
                outcome.unreadable += 1;
                continue;
            }
        };
        outcome.files.push(WalkedFile {
            root: root.to_path_buf(),
            role,
            walked: walked.to_path_buf(),
            canonical,
            metadata,
        });
    }
}

/// Sort a walk error into the three classes that mean different things to a caller.
/// Only `unreadable` says the file list is short; treating either of the other two that
/// way would let one dead link or one benign loop block reconciliation forever.
fn classify_walk_error(error: &walkdir::Error, outcome: &mut WalkOutcome) {
    if error.loop_ancestor().is_some() {
        outcome.loops += 1;
    } else if error.path().is_some_and(is_dangling_link) {
        outcome.dangling += 1;
    } else {
        outcome.unreadable += 1;
    }
}

/// The canonical path of a walked file, reusing one canonicalisation per containing
/// directory: only directory components can hide links, so a plain file inherits its
/// directory's canonical prefix. A file that is ITSELF a link is resolved in full.
///
/// The cache is keyed by the WALKED directory, not the canonical one. Two directory
/// links to one target are two distinct ways to reach the same files, and each file
/// has to keep the spelling the walk actually used to get to it.
fn canonical_path(
    walked: &Path,
    is_symlink: bool,
    dir_cache: &mut HashMap<PathBuf, PathBuf>,
    outcome: &mut WalkOutcome,
) -> PathBuf {
    if is_symlink {
        outcome.canonicalizations += 1;
        return std::fs::canonicalize(walked).unwrap_or_else(|_| walked.to_path_buf());
    }
    let (Some(parent), Some(name)) = (walked.parent(), walked.file_name()) else {
        return walked.to_path_buf();
    };
    if let Some(canonical_parent) = dir_cache.get(parent) {
        return canonical_parent.join(name);
    }
    outcome.canonicalizations += 1;
    let canonical_parent = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    dir_cache.insert(parent.to_path_buf(), canonical_parent.clone());
    canonical_parent.join(name)
}

/// Whether a path is a link whose target is absent, as opposed to one whose target
/// merely could not be read. Only a real absence means there is no file to index; a
/// permission error on the target is a file that still exists, and calling it absent
/// would let a caller reconcile a live file out of its store.
fn is_dangling_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
        && matches!(std::fs::metadata(path), Err(e) if e.kind() == ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    fn walk(root: &Path) -> WalkOutcome {
        walk_workspace_roots(&[root.to_path_buf()])
    }

    fn walked_names(outcome: &WalkOutcome) -> Vec<String> {
        let mut names: Vec<String> = outcome
            .files
            .iter()
            .map(|f| f.walked.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[cfg(unix)]
    fn running_as_root() -> bool {
        // SAFETY: `geteuid` takes no arguments, cannot fail and touches no memory.
        unsafe { libc::geteuid() == 0 }
    }

    #[test]
    fn a_healthy_walk_reports_no_errors() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("CommonModules/M/Ext/Module.bsl"), "Процедура П() КонецПроцедуры");
        write(&dir.path().join("Configuration.xml"), "<x/>");

        let outcome = walk(dir.path());

        assert_eq!(outcome.files.len(), 2);
        assert_eq!((outcome.unreadable, outcome.loops, outcome.dangling), (0, 0, 0));
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_subtree_is_counted_as_incomplete_coverage() {
        if running_as_root() {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("Visible.bsl"), "");
        let closed = dir.path().join("closed");
        write(&closed.join("Hidden.bsl"), "");
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();

        let outcome = walk(dir.path());
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(outcome.unreadable > 0, "an unreadable subtree must not look like a full walk");
        assert_eq!(outcome.loops, 0);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_root_is_counted_rather_than_reported_as_an_empty_tree() {
        if running_as_root() {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        write(&root.join("M.bsl"), "");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();

        let outcome = walk(&root);
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(outcome.files.is_empty());
        assert!(outcome.unreadable > 0, "an unreadable root is not an empty tree");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_loop_leaves_coverage_complete() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        write(&a.join("b/M.bsl"), "");
        std::os::unix::fs::symlink(&a, a.join("b/loop")).unwrap();

        let outcome = walk(dir.path());

        assert!(outcome.loops > 0, "the loop must be reported");
        assert_eq!(outcome.unreadable, 0, "a loop does not make coverage incomplete");
        assert_eq!(walked_names(&outcome), vec!["M.bsl"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_loop_does_not_hang_and_yields_each_file_once() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        write(&a.join("b/M.bsl"), "");
        write(&a.join("b/N.bsl"), "");
        std::os::unix::fs::symlink(&a, a.join("b/loop")).unwrap();

        let outcome = walk(dir.path());

        assert_eq!(walked_names(&outcome), vec!["M.bsl", "N.bsl"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_link_leaves_coverage_complete() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("Live.bsl"), "");
        std::os::unix::fs::symlink(
            dir.path().join("missing/Target.bsl"),
            dir.path().join("Dead.bsl"),
        )
        .unwrap();

        let outcome = walk(dir.path());

        assert!(outcome.dangling > 0, "a link into nothing must be its own class");
        assert_eq!(
            outcome.unreadable, 0,
            "a dead link is not incomplete coverage: there is no file behind it"
        );
        assert_eq!(outcome.loops, 0);
        assert_eq!(walked_names(&outcome), vec!["Live.bsl"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_an_unreadable_target_is_incomplete_coverage_not_a_dead_link() {
        if running_as_root() {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let private = dir.path().join("private");
        write(&private.join("Target.bsl"), "");
        std::os::unix::fs::symlink(private.join("Target.bsl"), dir.path().join("Alias.bsl"))
            .unwrap();
        fs::set_permissions(&private, fs::Permissions::from_mode(0o000)).unwrap();

        let outcome = walk(dir.path());
        fs::set_permissions(&private, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(
            outcome.dangling, 0,
            "the target exists; calling it absent would let a caller delete a live file"
        );
        assert!(outcome.unreadable > 0);
    }

    #[test]
    fn a_directory_named_like_a_source_file_is_not_a_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Foo.bsl")).unwrap();
        write(&dir.path().join("Real.bsl"), "");

        let outcome = walk(dir.path());

        assert_eq!(walked_names(&outcome), vec!["Real.bsl"]);
    }

    #[test]
    fn both_roles_come_out_of_one_pass() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("M.bsl"), "");
        write(&dir.path().join("M.xml"), "");
        write(&dir.path().join("M.txt"), "");

        let outcome = walk(dir.path());

        let mut roles: Vec<(String, FileRole)> = outcome
            .files
            .iter()
            .map(|f| (f.walked.file_name().unwrap().to_string_lossy().into_owned(), f.role))
            .collect();
        roles.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            roles,
            vec![
                ("M.bsl".to_owned(), FileRole::Source),
                ("M.xml".to_owned(), FileRole::MetadataWatched),
            ]
        );
    }

    #[test]
    fn extension_matching_ignores_case() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("Upper.BSL"), "");
        write(&dir.path().join("Upper.XML"), "");

        let outcome = walk(dir.path());

        let mut roles: Vec<FileRole> = outcome.files.iter().map(|f| f.role).collect();
        roles.sort_by_key(|r| format!("{r:?}"));
        assert_eq!(roles, vec![FileRole::MetadataWatched, FileRole::Source]);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_is_taken_only_when_both_spellings_agree_on_its_role() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        write(&outside.join("Target.bsl"), "");
        write(&outside.join("Target.txt"), "");
        write(&outside.join("Target.xml"), "");
        let root = dir.path().join("root");
        fs::create_dir_all(&root).unwrap();
        write(&root.join("Plain.bsl"), "");
        let link = |name: &str, target: &Path| {
            std::os::unix::fs::symlink(target, root.join(name)).unwrap();
        };
        link("SameRole.bsl", &outside.join("Target.bsl"));
        link("ToNonSource.bsl", &outside.join("Target.txt"));
        link("FromNonSource.txt", &outside.join("Target.bsl"));
        link("RoleMismatch.xml", &outside.join("Target.bsl"));
        link("RoleMismatchBack.bsl", &outside.join("Target.xml"));

        let outcome = walk(&root);

        assert_eq!(walked_names(&outcome), vec!["Plain.bsl", "SameRole.bsl"]);
    }

    #[cfg(unix)]
    #[test]
    fn two_aliases_in_one_root_both_come_through() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("outside/Target.bsl");
        write(&target, "");
        let root = dir.path().join("root");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&target, root.join("Alias1.bsl")).unwrap();
        std::os::unix::fs::symlink(&target, root.join("Alias2.bsl")).unwrap();

        let outcome = walk(&root);

        assert_eq!(walked_names(&outcome), vec!["Alias1.bsl", "Alias2.bsl"]);
        let canonicals: std::collections::HashSet<&Path> =
            outcome.files.iter().map(|f| f.canonical.as_path()).collect();
        assert_eq!(canonicals.len(), 1, "both aliases name one physical file");
    }

    #[test]
    fn a_file_under_two_nested_roots_comes_through_for_each() {
        let dir = tempfile::tempdir().unwrap();
        let outer = dir.path().join("ws");
        let inner = outer.join("src/cf");
        write(&inner.join("M.bsl"), "");

        let outcome = walk_workspace_roots(&[outer.clone(), inner.clone()]);

        assert_eq!(outcome.files.len(), 2, "nesting is allowed; each root reports its own reach");
        let roots: std::collections::HashSet<&Path> =
            outcome.files.iter().map(|f| f.root.as_path()).collect();
        assert_eq!(roots.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_plain_file_under_a_directory_link_keeps_both_spellings() {
        let dir = tempfile::tempdir().unwrap();
        let tree = dir.path().join("tree");
        write(&tree.join("M.bsl"), "");
        let root = dir.path().join("root");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&tree, root.join("Linked")).unwrap();

        let outcome = walk(&root);

        assert_eq!(outcome.files.len(), 1);
        let file = &outcome.files[0];
        assert_eq!(file.walked, root.join("Linked/M.bsl"));
        assert_eq!(file.canonical, fs::canonicalize(tree.join("M.bsl")).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn two_directory_links_to_one_tree_keep_their_own_walked_spellings() {
        let dir = tempfile::tempdir().unwrap();
        let tree = dir.path().join("tree");
        write(&tree.join("M.bsl"), "");
        let root = dir.path().join("root");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&tree, root.join("A")).unwrap();
        std::os::unix::fs::symlink(&tree, root.join("B")).unwrap();

        let outcome = walk(&root);

        let mut walked: Vec<&Path> = outcome.files.iter().map(|f| f.walked.as_path()).collect();
        walked.sort();
        assert_eq!(walked, vec![root.join("A/M.bsl").as_path(), root.join("B/M.bsl").as_path()]);
        let canonicals: std::collections::HashSet<&Path> =
            outcome.files.iter().map(|f| f.canonical.as_path()).collect();
        assert_eq!(canonicals.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn metadata_describes_the_target_of_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("outside/Target.bsl");
        write(&target, &"x".repeat(4096));
        let root = dir.path().join("root");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&target, root.join("Alias.bsl")).unwrap();

        let outcome = walk(&root);

        assert_eq!(outcome.files.len(), 1);
        assert_eq!(
            outcome.files[0].metadata.len(),
            4096,
            "the read follows the link, so the fingerprint must describe the target"
        );
    }

    #[test]
    fn metadata_of_a_plain_file_is_its_own() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("M.bsl"), &"x".repeat(17));

        let outcome = walk(dir.path());

        assert_eq!(outcome.files[0].metadata.len(), 17);
    }

    #[test]
    fn no_directory_is_excluded_from_the_walk() {
        let dir = tempfile::tempdir().unwrap();
        for excluded in [".git", "target", ".build"] {
            write(&dir.path().join(excluded).join("M.bsl"), "");
        }

        let outcome = walk(dir.path());

        assert_eq!(
            outcome.files.len(),
            3,
            "narrowing belongs to the set of roots; a hidden exclusion here would silently \
             shrink the graph's file universe"
        );
    }

    #[test]
    fn canonicalisation_is_paid_per_directory_not_per_file() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = 3;
        let files_per_dir = 20;
        for d in 0..dirs {
            for f in 0..files_per_dir {
                write(&dir.path().join(format!("d{d}/M{f}.bsl")), "");
            }
        }

        let outcome = walk(dir.path());

        assert_eq!(outcome.files.len(), dirs * files_per_dir);
        assert!(
            outcome.canonicalizations <= dirs + 1,
            "expected one canonicalisation per directory, got {} for {} files",
            outcome.canonicalizations,
            outcome.files.len()
        );
    }

    #[cfg(unix)]
    #[test]
    fn canonicalisation_is_paid_per_file_when_every_file_is_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("outside/Target.bsl");
        write(&target, "");
        let root = dir.path().join("root");
        fs::create_dir_all(&root).unwrap();
        let files = 20;
        for f in 0..files {
            std::os::unix::fs::symlink(&target, root.join(format!("A{f}.bsl"))).unwrap();
        }

        let outcome = walk(&root);

        assert!(
            outcome.canonicalizations >= files,
            "the counter must track the operation it names: {} canonicalisations for {} links",
            outcome.canonicalizations,
            files
        );
    }
}
