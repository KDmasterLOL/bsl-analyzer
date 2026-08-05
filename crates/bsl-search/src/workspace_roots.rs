//! Which source root a workspace file belongs to, and under which key it is
//! stored.
//!
//! A `cfe` extension repeats the configuration's directory layout, so the path
//! relative to a root is not unique across roots: `CommonModules/M/Ext/Module.bsl`
//! exists under the configuration and under every extension at once. Store rows
//! are therefore keyed by the pair `(root_id, path)`, and this table is the only
//! seam that turns an absolute path into that pair and back.
//!
//! The table knows nothing about configurations or extensions beyond which root
//! is *the* configuration one; the caller builds it from the project model.

use std::path::{Path, PathBuf};

/// The configuration root's identity. Reserved: an extension never takes it, so
/// rows written before roots existed — all of them the configuration's — keep
/// their meaning under the composite key without being rewritten.
pub const CONFIGURATION_ROOT_ID: &str = "";

/// A declared root that did not become a registered one. Reported rather than
/// swallowed: the caller has to be able to say which root it dropped and why,
/// because in both cases some files end up labelled differently than declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedRoot {
    pub path: PathBuf,
    pub reason: Rejection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    /// Canonically inside the configuration root. Its files stay searchable as
    /// the configuration's, but their root label is the configuration's too.
    InsideConfiguration { root: PathBuf },
    /// Its identifier is already taken by another root, so registering it would
    /// give two different directories one key space and let the second overwrite
    /// the first. Reachable because the identifier is a lossy string: two paths
    /// that differ only in bytes no `str` can hold render identically.
    IdentifierTaken { id: String },
}

/// The identity of an indexed file: the root it belongs to and its path relative
/// to that root.
///
/// A pair rather than one string because the root cannot be folded into the path
/// without a separator, and every separator collides with a legal configuration
/// path. It is also what the store rows are keyed by, so a key that travels as
/// one value cannot lose half of itself on the way to a lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileKey {
    pub root_id: String,
    pub path: String,
}

impl FileKey {
    pub fn new(root_id: impl Into<String>, path: impl Into<String>) -> Self {
        Self { root_id: root_id.into(), path: path.into() }
    }

    /// A file of the configuration root — the only identity a published baseline
    /// manifest can carry, since the publisher walks that root alone.
    pub fn configuration(path: impl Into<String>) -> Self {
        Self::new(CONFIGURATION_ROOT_ID, path)
    }

    pub fn is_configuration(&self) -> bool {
        self.root_id == CONFIGURATION_ROOT_ID
    }

    /// Whether this key names a file strictly inside the directory `dir` names.
    ///
    /// Compared by whole components, not by text: a textual prefix would let `Dir`
    /// swallow `Dir2`, and the two are unrelated directories. Roots must match — the
    /// same relative path under two roots is two different files.
    pub fn is_under(&self, dir: &FileKey) -> bool {
        self.root_id == dir.root_id && starts_at(Path::new(&self.path), Path::new(&dir.path))
    }
}

#[derive(Debug, Clone)]
struct Root {
    id: String,
    /// The spelling the project declared, which is what exists on disk for a
    /// caller that wants to read the file back.
    declared: PathBuf,
    /// The spelling every topology decision is made on.
    canonical: PathBuf,
}

/// The registered source roots of one workspace.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceRoots {
    /// The workspace itself. Not a source root — root ids are relative to it,
    /// and callers that speak of "the project directory" (the graph, the
    /// resident host) mean this and not the configuration root, which may sit in
    /// a subdirectory.
    workspace: PathBuf,
    roots: Vec<Root>,
}

impl WorkspaceRoots {
    /// Build the table from the configuration root and the declared extension
    /// roots, both spelled as the project declares them.
    ///
    /// `workspace_root` is what root ids are relative to. Extension roots that
    /// canonically lie inside the configuration root are left out and returned
    /// separately: those files are already reachable — and already published in
    /// a baseline — as the configuration's, and giving them a second identity
    /// would mean two rows for one file and a split from the published snapshot.
    pub fn build(
        workspace_root: &Path,
        configuration: &Path,
        extensions: &[PathBuf],
    ) -> (Self, Vec<RejectedRoot>) {
        let workspace_canonical = canonicalize(workspace_root);
        let configuration_canonical = canonicalize(configuration);
        let mut roots = vec![Root {
            id: CONFIGURATION_ROOT_ID.to_owned(),
            declared: configuration.to_path_buf(),
            canonical: configuration_canonical.clone(),
        }];
        let mut rejected = Vec::new();

        for extension in extensions {
            let canonical = canonicalize(extension);
            // Equal canonical paths are the same root, so the configuration —
            // which owns the rows already — keeps it. Being *inside* it is the
            // rejected case; containing it is not, and is handled by the
            // longest-prefix rule at lookup time.
            if canonical == configuration_canonical
                || starts_at(&canonical, &configuration_canonical)
            {
                rejected.push(RejectedRoot {
                    path: extension.clone(),
                    reason: Rejection::InsideConfiguration { root: configuration.to_path_buf() },
                });
                continue;
            }
            // A root already registered under another spelling is one root.
            if roots.iter().any(|root| root.canonical == canonical) {
                continue;
            }
            // The identifier is the key space of every stored row, so two roots
            // sharing one would make the second silently overwrite the first —
            // and `resolve` would hand back the wrong directory for both.
            let id = root_id_for(&workspace_canonical, &canonical, extension);
            if roots.iter().any(|root| root.id == id) {
                rejected.push(RejectedRoot {
                    path: extension.clone(),
                    reason: Rejection::IdentifierTaken { id },
                });
                continue;
            }
            roots.push(Root { id, declared: extension.clone(), canonical });
        }

        (Self { workspace: workspace_root.to_path_buf(), roots }, rejected)
    }

    /// The root a file belongs to and the key it is stored under, or `None` when
    /// the file lies outside every root.
    ///
    /// Two spellings go in because the enumerator has two: `canonical` is what
    /// the semantics rank roots by and what makes attribution independent of
    /// which alias the walk happened to arrive through, while `walked` is the
    /// only handle on a file the walk reached through a symlink that leaves
    /// every root. Such a file is not dropped — the graph has always seen it —
    /// it is keyed by the walking root instead.
    pub fn root_of(&self, walked: &Path, canonical: &Path) -> Option<FileKey> {
        self.longest_match(canonical, |root| &root.canonical)
            .or_else(|| self.longest_match(walked, |root| &root.declared))
            .or_else(|| self.lossy_match(walked, canonical))
    }

    /// The same ranking for a path that reached us through a LOSSY rendering: the graph
    /// keeps its file paths as strings, so a root directory whose name holds bytes no
    /// `str` can carry comes back with those bytes replaced, and no byte comparison finds
    /// it any more. Re-render the roots the same way and both sides speak one alphabet.
    ///
    /// The ranking is the same one, and the rendering is built only when the path actually
    /// carries a replacement character and nothing else matched, so the walking callers
    /// never pay for it.
    ///
    /// What the rendering CAN do is make two roots that the byte comparison keeps apart
    /// look alike — the identifier check at build time does not prevent it, because the
    /// configuration's identifier is reserved and never collides with anything, and
    /// identifiers are built from the canonical spellings while the second pass here ranks
    /// the declared ones. A tie is therefore answered with "unknown", not with the deeper
    /// or the later root: the two candidates are equally plausible, and a key guessed from
    /// them would mark a file that is not the one that changed.
    fn lossy_match(&self, walked: &Path, canonical: &Path) -> Option<FileKey> {
        let rendered = |path: &Path| {
            path.to_str().is_some_and(|path| path.contains(char::REPLACEMENT_CHARACTER))
        };
        if !rendered(walked) && !rendered(canonical) {
            return None;
        }
        let lossy = |path: &Path| PathBuf::from(path.to_string_lossy().into_owned());
        let view = Self {
            workspace: self.workspace.clone(),
            roots: self
                .roots
                .iter()
                .map(|root| Root {
                    id: root.id.clone(),
                    declared: lossy(&root.declared),
                    canonical: lossy(&root.canonical),
                })
                .collect(),
        };
        view.unambiguous_match(canonical, |root| &root.canonical)
            .or_else(|| view.unambiguous_match(walked, |root| &root.declared))
    }

    /// [`Self::longest_match`] that refuses a tie: the deepest match, and only when one
    /// root reaches that deep.
    fn unambiguous_match(
        &self,
        path: &Path,
        spelling: impl Fn(&Root) -> &PathBuf,
    ) -> Option<FileKey> {
        let mut matches: Vec<(usize, FileKey)> = self
            .roots
            .iter()
            .filter_map(|root| {
                let rel = path.strip_prefix(spelling(root)).ok()?;
                rel.components().next().is_some().then(|| {
                    (spelling(root).components().count(), FileKey::new(&root.id, key_of(rel)))
                })
            })
            .collect();
        let deepest = matches.iter().map(|(depth, _)| *depth).max()?;
        matches.retain(|(depth, _)| *depth == deepest);
        match matches.as_slice() {
            [(_, key)] => Some(key.clone()),
            _ => None,
        }
    }

    /// The owning root ranked by the DECLARED spellings alone. For a file whose canonical
    /// target must not take part in attribution (a non-source target), plain [`Self::root_of`]
    /// would still rank the walked path against the CANONICAL root spellings first — and under
    /// a root declared through a link, the walked path also lies under the enclosing root's
    /// canonical spelling, handing the key to the wrong root.
    pub(crate) fn root_of_declared(&self, walked: &Path) -> Option<FileKey> {
        self.longest_match(walked, |root| &root.declared)
    }

    /// The two spellings attribution ranks a path by.
    ///
    /// A relative path is read against the CONFIGURATION root: that is how every
    /// stored path with the reserved id is spelled, and it is the prefix callers
    /// strip before handing one over. The table's workspace exists to make root
    /// identifiers relative and is a directory higher whenever the configuration
    /// sits in a subdirectory.
    pub(crate) fn spellings_of(&self, path: &Path) -> (PathBuf, PathBuf) {
        let walked = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.configuration().unwrap_or_else(|| self.workspace()).join(path)
        };
        let canonical = canonical_spelling(&walked);
        (walked, canonical)
    }

    /// The key a workspace path carries whatever its role — the question "which
    /// root does this path belong to, and where in it" asked of a path that is
    /// not a stored document: a metadata descriptor, a directory.
    ///
    /// The role branch [`crate::SearchEngine`] applies to a `.bsl` has no meaning
    /// here: it exists to keep a key REMOVABLE under the spelling its file was
    /// indexed with, and nothing is indexed under this one. Where the path
    /// physically lies is the whole answer, so the canonical spelling ranks
    /// first, as it does for a live file.
    pub fn key_of_path(&self, path: &Path) -> Option<FileKey> {
        let (walked, canonical) = self.spellings_of(path);
        self.root_of(&walked, &canonical)
    }

    /// The file a stored key points at, spelled as the project declared it.
    pub fn resolve(&self, key: &FileKey) -> Option<PathBuf> {
        let root = self.roots.iter().find(|root| root.id == key.root_id)?;
        Some(root.declared.join(&key.path))
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// The declared spelling of the configuration root.
    pub fn configuration(&self) -> Option<&Path> {
        self.roots
            .iter()
            .find(|root| root.id == CONFIGURATION_ROOT_ID)
            .map(|root| root.declared.as_path())
    }

    /// Whether a root is registered under this id.
    pub fn contains_id(&self, root_id: &str) -> bool {
        self.roots.iter().any(|root| root.id == root_id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.roots.iter().map(|root| root.id.as_str())
    }

    /// Registered roots as `(id, declared path)`, in registration order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &Path)> {
        self.roots.iter().map(|root| (root.id.as_str(), root.declared.as_path()))
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// The root with the longest matching prefix, under the given spelling.
    ///
    /// Longest rather than first: roots may nest, and the innermost one is what
    /// the semantics call the file's owner. The configuration competes like any
    /// other root, so an extension declared *around* it keeps only the files
    /// outside it.
    fn longest_match(&self, path: &Path, spelling: impl Fn(&Root) -> &PathBuf) -> Option<FileKey> {
        self.roots
            .iter()
            .filter_map(|root| {
                let rel = path.strip_prefix(spelling(root)).ok()?;
                // `strip_prefix` compares whole components, so `cf` never
                // swallows `cf_ext`. An empty remainder means the path *is* the
                // root, which is not a file in it.
                (rel.components().next().is_some()).then(|| {
                    (spelling(root).components().count(), FileKey::new(&root.id, key_of(rel)))
                })
            })
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, key)| key)
    }
}

/// The store key of a path relative to its root.
fn key_of(rel: &Path) -> String {
    rel.to_string_lossy().into_owned()
}

fn canonicalize(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Whether `path` lies strictly inside `prefix`.
fn starts_at(path: &Path, prefix: &Path) -> bool {
    path.strip_prefix(prefix).is_ok_and(|rest| rest.components().next().is_some())
}

/// A root's identity: its path relative to the workspace, or absolute when it
/// lies outside.
///
/// Not the extension's name — several entries are allowed to share one — and
/// not an ordinal, which would not survive a restart.
fn root_id_for(workspace_canonical: &Path, canonical: &Path, declared: &Path) -> String {
    let relative = canonical.strip_prefix(workspace_canonical).ok().map(key_of);
    match relative {
        // A root spelled `.` relative to the workspace would take the
        // configuration's reserved empty id and overwrite its rows.
        Some(relative) if relative.is_empty() => ".".to_owned(),
        Some(relative) => relative,
        None if canonical.is_absolute() => key_of(canonical),
        // Neither under the workspace nor absolute: keep the declared spelling
        // rather than invent one, and make sure it cannot be read as empty.
        None => {
            let declared = key_of(declared);
            if declared.is_empty() || declared == "." {
                ".".to_owned()
            } else {
                declared
            }
        }
    }
}

/// The canonical spelling of a path, falling back to the canonical spelling of
/// its directory when the file itself is gone.
///
/// Deletion is the case this exists for: a removed file cannot be canonicalized,
/// and dropping all the way to the walked spelling would leave attribution
/// ranking roots by their declared paths alone. A file that lived under a root
/// reached through an alias would then be removed under a DIFFERENT root's key —
/// tombstone and all — while its real row stayed behind serving a dead hit.
pub(crate) fn canonical_spelling(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => std::fs::canonicalize(parent)
            .map(|parent| parent.join(name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Roots must exist on disk for the canonical spelling to mean anything, so
    /// every fixture builds real directories.
    fn dirs(root: &Path, rels: &[&str]) -> Vec<PathBuf> {
        rels.iter()
            .map(|rel| {
                let path = root.join(rel);
                std::fs::create_dir_all(&path).unwrap();
                path
            })
            .collect()
    }

    /// The attribution of a file that exists only as a path: both spellings are
    /// the same when no alias is involved.
    fn owner(roots: &WorkspaceRoots, file: &Path) -> Option<FileKey> {
        roots.root_of(file, file)
    }

    const MODULE: &str = "CommonModules/М/Ext/Module.bsl";

    #[test]
    fn each_root_keeps_its_own_copy_of_the_same_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let made = dirs(dir.path(), &["cf", "cfe/one", "cfe/two"]);
        let (roots, rejected) =
            WorkspaceRoots::build(dir.path(), &made[0], &[made[1].clone(), made[2].clone()]);

        assert!(rejected.is_empty(), "roots beside the configuration are all registered");
        for (root, expected_id) in
            [(&made[0], CONFIGURATION_ROOT_ID), (&made[1], "cfe/one"), (&made[2], "cfe/two")]
        {
            assert_eq!(
                owner(&roots, &root.join(MODULE)),
                Some(FileKey::new(expected_id, MODULE.to_owned())),
                "the same relative path under {root:?} must key to its own root"
            );
        }
    }

    #[test]
    fn the_innermost_root_owns_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let made = dirs(dir.path(), &["cf", "cfe/outer", "cfe/outer/inner"]);
        let (roots, _) =
            WorkspaceRoots::build(dir.path(), &made[0], &[made[1].clone(), made[2].clone()]);

        assert_eq!(
            owner(&roots, &made[2].join(MODULE)),
            Some(FileKey::new("cfe/outer/inner", MODULE.to_owned())),
            "the nested root is the owner, not the one that merely contains it"
        );
        assert_eq!(
            owner(&roots, &made[1].join(MODULE)),
            Some(FileKey::new("cfe/outer", MODULE.to_owned())),
            "a file outside the nested root still belongs to the outer one"
        );
    }

    /// Prefix matching that ignored component boundaries would hand every file
    /// of `cf_ext` to `cf`, keyed by a path starting with `_ext/`.
    #[test]
    fn a_root_never_swallows_a_sibling_whose_name_starts_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let made = dirs(dir.path(), &["cf", "cf_ext"]);
        let (roots, _) = WorkspaceRoots::build(dir.path(), &made[0], &[made[1].clone()]);

        assert_eq!(
            owner(&roots, &made[1].join(MODULE)),
            Some(FileKey::new("cf_ext", MODULE.to_owned())),
            "`cf` must not claim `cf_ext`"
        );
    }

    /// Declaration order decides nothing: the roots are ranked by how deep they
    /// reach, exactly as the semantics rank them.
    #[test]
    fn attribution_does_not_depend_on_declaration_order() {
        let dir = tempfile::tempdir().unwrap();
        let made = dirs(dir.path(), &["cf", "cfe/outer", "cfe/outer/inner"]);
        let file = made[2].join(MODULE);

        let (forward, _) =
            WorkspaceRoots::build(dir.path(), &made[0], &[made[1].clone(), made[2].clone()]);
        let (backward, _) =
            WorkspaceRoots::build(dir.path(), &made[0], &[made[2].clone(), made[1].clone()]);

        assert_eq!(owner(&forward, &file), owner(&backward, &file));
    }

    #[test]
    fn an_extension_inside_the_configuration_is_reported_and_left_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let made = dirs(dir.path(), &["cf", "cf/nested"]);
        let (roots, rejected) = WorkspaceRoots::build(dir.path(), &made[0], &[made[1].clone()]);

        assert_eq!(
            rejected,
            vec![RejectedRoot {
                path: made[1].clone(),
                reason: Rejection::InsideConfiguration { root: made[0].clone() },
            }],
            "the overlap must be named, not swallowed"
        );
        // Not lost: the configuration walk is recursive, so the file is still
        // found — as the configuration's, which is what the publisher and the
        // graph enumeration call it too.
        assert_eq!(
            owner(&roots, &made[1].join(MODULE)),
            Some(FileKey::new(CONFIGURATION_ROOT_ID, format!("nested/{MODULE}"))),
        );
    }

    /// The other direction of the same overlap: an extension declared *around*
    /// the configuration. Rejecting it would lose the files outside the
    /// configuration entirely — its walk never reaches them.
    #[test]
    fn an_extension_around_the_configuration_keeps_what_lies_outside_it() {
        let dir = tempfile::tempdir().unwrap();
        let made = dirs(dir.path(), &["src/cf", "src/own"]);
        let workspace = dir.path().to_path_buf();
        let (roots, rejected) =
            WorkspaceRoots::build(&workspace, &made[0], std::slice::from_ref(&workspace));

        assert!(rejected.is_empty(), "containing the configuration is not an overlap to reject");
        assert_eq!(
            owner(&roots, &made[0].join(MODULE)),
            Some(FileKey::new(CONFIGURATION_ROOT_ID, MODULE.to_owned())),
            "the configuration's own subtree stays its own"
        );
        assert_eq!(
            owner(&roots, &made[1].join(MODULE)),
            Some(FileKey::new(".", format!("src/own/{MODULE}"))),
            "everything outside it belongs to the surrounding root"
        );
    }

    /// A root equal to the workspace must not take the configuration's reserved
    /// id: the two would then share one key space, and the second write would
    /// overwrite the first.
    #[test]
    fn a_root_spanning_the_workspace_gets_a_non_empty_id() {
        let dir = tempfile::tempdir().unwrap();
        let made = dirs(dir.path(), &["src/cf"]);
        let (roots, _) = WorkspaceRoots::build(dir.path(), &made[0], &[dir.path().to_path_buf()]);

        let ids: Vec<&str> = roots.ids().collect();
        assert_eq!(ids, vec![CONFIGURATION_ROOT_ID, "."]);
    }

    #[test]
    fn a_root_equal_to_the_configuration_stays_the_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let made = dirs(dir.path(), &["cf"]);
        let (roots, rejected) = WorkspaceRoots::build(dir.path(), &made[0], &[made[0].clone()]);

        assert_eq!(roots.ids().collect::<Vec<_>>(), vec![CONFIGURATION_ROOT_ID]);
        assert_eq!(rejected.len(), 1, "the duplicate root is reported");
        assert_eq!(
            owner(&roots, &made[0].join(MODULE)),
            Some(FileKey::new(CONFIGURATION_ROOT_ID, MODULE.to_owned())),
            "one file must not produce both a configuration row and an extension row"
        );
    }

    /// The identifier is a lossy rendering of a path, so two directories that
    /// differ only in bytes no `str` can hold render the same. Registering both
    /// would give one key space to two roots: the second's rows would overwrite
    /// the first's, and `resolve` would hand back the wrong directory for either.
    #[cfg(unix)]
    #[test]
    fn a_root_whose_identifier_is_already_taken_is_rejected() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        let made = dirs(dir.path(), &["cf"]);
        let first = dir.path().join(OsString::from_vec(vec![b'a', 0x80]));
        let second = dir.path().join(OsString::from_vec(vec![b'a', 0x81]));
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();

        let (roots, rejected) =
            WorkspaceRoots::build(dir.path(), &made[0], &[first.clone(), second.clone()]);

        let ids: Vec<&str> = roots.ids().collect();
        assert_eq!(ids.len(), 2, "the configuration and exactly one of the two: {ids:?}");
        assert_eq!(
            rejected,
            vec![RejectedRoot {
                path: second,
                reason: Rejection::IdentifierTaken { id: ids[1].to_owned() },
            }],
            "the dropped root is named, not swallowed"
        );
        assert_eq!(
            roots.resolve(&FileKey::new(ids[1], MODULE)).as_deref(),
            Some(first.join(MODULE).as_path()),
            "the surviving root keeps its own directory"
        );
    }

    #[test]
    fn a_key_resolves_back_to_the_file_it_came_from() {
        let dir = tempfile::tempdir().unwrap();
        let made = dirs(dir.path(), &["cf", "cfe/one"]);
        let (roots, _) = WorkspaceRoots::build(dir.path(), &made[0], &[made[1].clone()]);

        for root in [&made[0], &made[1]] {
            let file = root.join(MODULE);
            let key = owner(&roots, &file).unwrap();
            assert_eq!(roots.resolve(&key).as_deref(), Some(file.as_path()));
        }
    }

    #[cfg(unix)]
    mod aliases {
        use super::*;

        fn link(target: &Path, at: &Path) {
            std::os::unix::fs::symlink(target, at).unwrap();
        }

        /// Reached through `one/Linked`, the file physically lives under `two`.
        /// Ranking by the canonical spelling is what makes the answer the same
        /// as the semantics', and the same in both declaration orders.
        #[test]
        fn an_alias_does_not_move_a_file_to_the_root_it_was_reached_through() {
            let dir = tempfile::tempdir().unwrap();
            let made = dirs(dir.path(), &["cf", "cfe/one", "cfe/two"]);
            link(&made[2], &made[1].join("Linked"));

            let (roots, _) =
                WorkspaceRoots::build(dir.path(), &made[0], &[made[1].clone(), made[2].clone()]);

            let walked = made[1].join("Linked").join(MODULE);
            let canonical = made[2].join(MODULE);
            assert_eq!(
                roots.root_of(&walked, &canonical),
                Some(FileKey::new("cfe/two", MODULE.to_owned())),
                "the root the file lives in owns it, not the one that links to it"
            );
        }

        /// A link out of every root. The graph has always followed it, so the
        /// file must stay in the universe; with no canonical owner it is kept by
        /// the root whose walk arrived there, keyed by the walked spelling.
        #[test]
        fn a_file_outside_every_root_is_kept_by_the_root_that_walked_to_it() {
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let made = dirs(dir.path(), &["cf"]);
            std::fs::create_dir_all(outside.path().join("tree")).unwrap();
            link(&outside.path().join("tree"), &made[0].join("Linked"));

            let (roots, _) = WorkspaceRoots::build(dir.path(), &made[0], &[]);

            let walked = made[0].join("Linked").join(MODULE);
            let canonical =
                std::fs::canonicalize(outside.path()).unwrap().join("tree").join(MODULE);
            assert_eq!(
                roots.root_of(&walked, &canonical),
                Some(FileKey::new(CONFIGURATION_ROOT_ID, format!("Linked/{MODULE}"))),
            );
        }

        /// An extension declared through an alias that sits inside the
        /// configuration, while the extension itself lies outside it. Deciding
        /// on the declared spelling would reject the root and lose its files.
        #[test]
        fn an_extension_declared_through_an_alias_inside_the_configuration_is_registered() {
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let made = dirs(dir.path(), &["cf"]);
            let real = outside.path().join("ext");
            std::fs::create_dir_all(&real).unwrap();
            let alias = made[0].join("Linked");
            link(&real, &alias);

            let (roots, rejected) =
                WorkspaceRoots::build(dir.path(), &made[0], std::slice::from_ref(&alias));

            assert!(
                rejected.is_empty(),
                "only the alias is inside the configuration, not the root"
            );
            let file = alias.join(MODULE);
            let canonical = std::fs::canonicalize(&real).unwrap().join(MODULE);
            let key = roots.root_of(&file, &canonical).unwrap();
            assert_ne!(key.root_id, CONFIGURATION_ROOT_ID, "the extension keeps its own identity");
            assert_eq!(key.path, MODULE);
        }
    }

    /// Attribution of a path that is not a stored document: a metadata descriptor asks the
    /// same question a `.bsl` does, and must not get a second answer.
    mod any_path {
        use super::*;

        #[test]
        fn a_descriptor_belongs_to_the_root_it_lies_in() {
            let dir = tempfile::tempdir().unwrap();
            let made = dirs(dir.path(), &["cf", "cfe/one"]);
            let (roots, _) = WorkspaceRoots::build(dir.path(), &made[0], &[made[1].clone()]);

            assert_eq!(
                roots.key_of_path(&made[1].join("Configuration.xml")),
                Some(FileKey::new("cfe/one", "Configuration.xml".to_owned())),
            );
            assert_eq!(
                roots.key_of_path(&made[0].join("Catalogs/Товары.xml")),
                Some(FileKey::configuration("Catalogs/Товары.xml")),
            );
        }

        /// The reading a relative path gets everywhere in the engine: the configuration
        /// root, which is where every stored path with the reserved id is spelled from.
        #[test]
        fn a_relative_path_is_read_against_the_configuration_root() {
            let dir = tempfile::tempdir().unwrap();
            let made = dirs(dir.path(), &["src/cf"]);
            let (roots, _) = WorkspaceRoots::build(dir.path(), &made[0], &[]);

            assert_eq!(
                roots.key_of_path(Path::new("Configuration.xml")),
                Some(FileKey::configuration("Configuration.xml")),
            );
        }

        #[test]
        fn a_path_outside_every_root_has_no_key() {
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let made = dirs(dir.path(), &["cf"]);
            let (roots, _) = WorkspaceRoots::build(dir.path(), &made[0], &[]);

            assert_eq!(roots.key_of_path(&outside.path().join("Configuration.xml")), None);
        }
    }

    /// A root directory whose name holds bytes no `str` can carry. Its files are indexed and
    /// its identifier is a lossy rendering already; what must not happen is a path coming
    /// back from a string-keyed store and finding no root at all.
    #[cfg(unix)]
    mod lossy_paths {
        use super::*;
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        #[test]
        fn a_path_that_survived_a_lossy_rendering_still_finds_its_root() {
            let dir = tempfile::tempdir().unwrap();
            let workspace = dir.path().join(OsString::from_vec(b"ws\xff".to_vec()));
            let made = dirs(&workspace, &["cf", "cfe"]);
            let (roots, _) = WorkspaceRoots::build(&workspace, &made[0], &[made[1].clone()]);
            let expected = Some(FileKey::new("cfe", MODULE.to_owned()));

            let file = made[1].join(MODULE);
            assert_eq!(owner(&roots, &file), expected, "the real bytes attribute as always");

            let rendered = PathBuf::from(file.to_string_lossy().into_owned());
            assert_eq!(owner(&roots, &rendered), expected, "and so does what a store gave back");
        }

        /// Two roots that differ only in bytes no `str` can hold render alike, and the
        /// identifier check does not reject the pair: the configuration's identifier is
        /// reserved, so it collides with nothing. Neither candidate is more plausible than
        /// the other, and a key picked from them would name a file that did not change —
        /// so the rendering answers "unknown" and the byte-exact spelling keeps working.
        #[test]
        fn a_rendering_that_fits_two_roots_attributes_to_neither() {
            let dir = tempfile::tempdir().unwrap();
            let configuration = dir.path().join(OsString::from_vec(b"c\xff".to_vec()));
            let extension = dir.path().join(OsString::from_vec(b"c\xfe".to_vec()));
            std::fs::create_dir_all(&configuration).unwrap();
            std::fs::create_dir_all(&extension).unwrap();
            let (roots, rejected) =
                WorkspaceRoots::build(dir.path(), &configuration, std::slice::from_ref(&extension));
            assert!(rejected.is_empty(), "the reserved identifier collides with nothing");

            let file = configuration.join(MODULE);
            assert_eq!(
                owner(&roots, &file),
                Some(FileKey::configuration(MODULE.to_owned())),
                "the real bytes still tell the two apart",
            );
            let rendered = PathBuf::from(file.to_string_lossy().into_owned());
            assert_eq!(owner(&roots, &rendered), None, "the rendering tells them apart no more");
        }

        /// The fallback ranks, it does not wave through: a rendered path outside every root
        /// stays outside.
        #[test]
        fn a_rendered_path_outside_every_root_still_has_no_key() {
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let workspace = dir.path().join(OsString::from_vec(b"ws\xff".to_vec()));
            let made = dirs(&workspace, &["cf"]);
            let (roots, _) = WorkspaceRoots::build(&workspace, &made[0], &[]);

            let file = outside.path().join(OsString::from_vec(b"tree\xff".to_vec())).join(MODULE);
            let rendered = PathBuf::from(file.to_string_lossy().into_owned());
            assert_eq!(owner(&roots, &rendered), None);
        }
    }

    mod containment {
        use super::*;

        #[test]
        fn a_directory_contains_its_files_at_any_depth_but_not_itself() {
            let dir = FileKey::configuration("Dir");
            assert!(FileKey::configuration("Dir/A.bsl").is_under(&dir));
            assert!(FileKey::configuration("Dir/Deep/B.bsl").is_under(&dir));
            assert!(!dir.is_under(&dir), "a directory is not a file inside itself");
        }

        /// The whole reason containment is compared by components: `Dir2` merely starts
        /// with the same text and is an unrelated directory.
        #[test]
        fn a_namesake_directory_is_not_inside() {
            assert!(!FileKey::configuration("Dir2/A.bsl").is_under(&FileKey::configuration("Dir")));
        }

        /// The same relative path under two roots is two different files, so a removal
        /// aimed at one root must not reach the other's.
        #[test]
        fn containment_does_not_cross_roots() {
            let file = FileKey::new("ext-1", "Dir/A.bsl");
            assert!(file.is_under(&FileKey::new("ext-1", "Dir")));
            assert!(!file.is_under(&FileKey::configuration("Dir")));
        }
    }
}
