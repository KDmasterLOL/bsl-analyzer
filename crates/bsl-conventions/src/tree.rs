//! Where a probe looks.
//!
//! The layout rules — which directory holds what, which sibling names an
//! object, where a module body hides — are written once, in the walkers that
//! use them. What varies is only where those rules read: the resident reads the
//! real filesystem, while a consumer that already scanned the tree reads its own
//! list of paths and must not walk the disk a second time (a fresh walk can see
//! a tree the scan did not).
//!
//! So the rules take a source instead of calling `fs` directly. Two operations
//! are enough, and both are needed: a cheap existence probe, because the walkers
//! construct a canonical spelling first and a listing per probe would be a real
//! cost on a large dump, and a listing, taken only when the exact probe misses.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What a path is, when it is anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    File,
    Dir,
}

/// One child of a listed directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: PathBuf,
    pub kind: EntryKind,
}

impl TreeEntry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }

    pub fn is_file(&self) -> bool {
        self.kind == EntryKind::File
    }
}

/// A directory tree the layout rules read.
pub trait DirTree {
    /// What `path` is, or `None` when it is nothing.
    fn kind_of(&self, path: &Path) -> Option<EntryKind>;

    /// The children of `dir`; empty when `dir` cannot be listed. Order is
    /// unspecified — every caller sorts its own output.
    fn entries(&self, dir: &Path) -> Vec<TreeEntry>;
}

/// The real filesystem.
pub struct RealFs;

impl DirTree for RealFs {
    fn kind_of(&self, path: &Path) -> Option<EntryKind> {
        let meta = std::fs::metadata(path).ok()?;
        Some(if meta.is_dir() { EntryKind::Dir } else { EntryKind::File })
    }

    fn entries(&self, dir: &Path) -> Vec<TreeEntry> {
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        entries
            .flatten()
            .filter_map(|entry| {
                let kind = match entry.file_type() {
                    Ok(file_type) if file_type.is_dir() => EntryKind::Dir,
                    Ok(_) => EntryKind::File,
                    Err(_) => return None,
                };
                Some(TreeEntry { path: entry.path(), kind })
            })
            .collect()
    }
}

/// A tree reconstructed from a list of file paths — the scanned universe.
///
/// Only files are listed anywhere, so a directory exists here exactly when some
/// listed file lies under it. A directory holding nothing that was scanned is
/// therefore invisible, and that is the one way this source differs from
/// [`RealFs`] on the same tree. The layout rules are written not to depend on
/// bare directory existence for that reason.
pub struct PathSetTree {
    children: HashMap<PathBuf, Vec<TreeEntry>>,
}

impl PathSetTree {
    pub fn from_files(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut children: HashMap<PathBuf, Vec<TreeEntry>> = HashMap::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();

        for path in paths {
            let mut kind = EntryKind::File;
            let mut child = path;
            while let Some(parent) = child.parent().map(Path::to_path_buf) {
                if !seen.insert(child.clone()) {
                    break;
                }
                children.entry(parent.clone()).or_default().push(TreeEntry { path: child, kind });
                kind = EntryKind::Dir;
                child = parent;
            }
        }
        Self { children }
    }
}

impl DirTree for PathSetTree {
    fn kind_of(&self, path: &Path) -> Option<EntryKind> {
        if self.children.contains_key(path) {
            return Some(EntryKind::Dir);
        }
        let parent = path.parent()?;
        self.children.get(parent)?.iter().find(|entry| entry.path == path).map(|entry| entry.kind)
    }

    fn entries(&self, dir: &Path) -> Vec<TreeEntry> {
        self.children.get(dir).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> PathSetTree {
        PathSetTree::from_files(paths.iter().map(PathBuf::from))
    }

    #[test]
    fn a_directory_exists_when_something_scanned_lies_under_it() {
        let tree = files(&["/ws/Catalogs/Товары/Товары.xml"]);
        assert_eq!(tree.kind_of(Path::new("/ws/Catalogs/Товары")), Some(EntryKind::Dir));
        assert_eq!(
            tree.kind_of(Path::new("/ws/Catalogs/Товары/Товары.xml")),
            Some(EntryKind::File),
        );
        assert_eq!(tree.kind_of(Path::new("/ws/Documents")), None);
    }

    /// Listing a directory names its immediate children only, each with the kind
    /// that decides which branch of a layout rule it takes.
    #[test]
    fn listing_names_immediate_children_with_their_kinds() {
        let tree = files(&[
            "/ws/CommonModules/Настройки/Ext/Module.bsl",
            "/ws/CommonModules/Настройки.xml",
        ]);
        let mut listed: Vec<_> = tree
            .entries(Path::new("/ws/CommonModules"))
            .into_iter()
            .map(|e| (e.path.file_name().unwrap().to_string_lossy().to_string(), e.kind))
            .collect();
        listed.sort();
        assert_eq!(
            listed,
            vec![
                ("Настройки".to_string(), EntryKind::Dir),
                ("Настройки.xml".to_string(), EntryKind::File),
            ],
        );
    }

    /// Two files under one directory list it once, not twice.
    #[test]
    fn a_shared_parent_is_listed_once() {
        let tree = files(&["/ws/Roles/Полные.xml", "/ws/Roles/Урезанные.xml"]);
        assert_eq!(tree.entries(Path::new("/ws")).len(), 1);
    }
}
