//! File change application for incrementally updating the database.
//!
//! FileChange represents a batch of file changes (from VFS) that need to be
//! applied to the database.

use std::sync::Arc;

use vfs::FileId;

use crate::{SourceRoot, SourceRootId};

/// A batch of file changes to apply to the database.
///
/// Changes are accumulated from the VFS and then applied atomically
/// with appropriate durability levels for incremental computation.
#[derive(Default, Debug)]
pub struct FileChange {
    /// Source roots to set/update
    pub roots: Option<Vec<SourceRoot>>,
    /// Files that changed (FileId, new content or None for deletion)
    pub files_changed: Vec<(FileId, Option<Arc<str>>)>,
}

impl FileChange {
    /// Create a new empty FileChange.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the source roots for this change.
    pub fn set_roots(&mut self, roots: Vec<SourceRoot>) {
        self.roots = Some(roots);
    }

    /// Add a file change (create, modify, or delete).
    ///
    /// - `new_text = Some(text)` for create/modify
    /// - `new_text = None` for delete
    pub fn change_file(&mut self, file_id: FileId, new_text: Option<Arc<str>>) {
        self.files_changed.push((file_id, new_text));
    }

    /// Apply all changes to the database.
    ///
    /// This method:
    /// 1. Sets source roots (if provided)
    /// 2. Maps files to their source roots
    /// 3. Updates file contents
    pub fn apply(self, db: &mut dyn crate::RootQueryDb) {
        let _span = tracing::info_span!(
            "FileChange::apply",
            files_changed = self.files_changed.len(),
            roots = self.roots.as_ref().map(|r| r.len())
        )
        .entered();

        // Apply source root changes
        if let Some(roots) = self.roots {
            for (idx, root) in roots.into_iter().enumerate() {
                let root_id = SourceRootId(idx as u32);

                // Map each file in this root to the root ID
                for file_id in root.iter() {
                    db.set_file_source_root(file_id, root_id);
                }

                // Store the source root
                db.set_source_root(root_id, Arc::new(root));
            }
        }

        // Apply file content changes
        for (file_id, text) in self.files_changed {
            // Update file text (empty string for deleted files)
            let text = text.unwrap_or_else(|| Arc::from(""));
            db.set_file_text(file_id, &text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_change_new() {
        let change = FileChange::new();
        assert!(change.roots.is_none());
        assert_eq!(change.files_changed.len(), 0);
    }

    #[test]
    fn test_file_change_add_file() {
        let mut change = FileChange::new();
        let file_id = FileId(0);
        let content: Arc<str> = Arc::from("test content");

        change.change_file(file_id, Some(content));
        assert_eq!(change.files_changed.len(), 1);
        assert_eq!(change.files_changed[0].0, file_id);
    }

    #[test]
    fn test_file_change_delete_file() {
        let mut change = FileChange::new();
        let file_id = FileId(0);

        change.change_file(file_id, None);
        assert_eq!(change.files_changed.len(), 1);
        assert!(change.files_changed[0].1.is_none());
    }
}
