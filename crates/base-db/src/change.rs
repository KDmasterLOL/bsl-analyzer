use std::sync::Arc;

use vfs::FileId;

use crate::{SourceRoot, SourceRootId};

#[derive(Default, Debug)]
pub struct FileChange {
    pub roots: Option<Vec<SourceRoot>>,
    pub files_changed: Vec<(FileId, Option<Arc<str>>)>,
}

impl FileChange {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_roots(&mut self, roots: Vec<SourceRoot>) {
        self.roots = Some(roots);
    }

    pub fn change_file(&mut self, file_id: FileId, new_text: Option<Arc<str>>) {
        self.files_changed.push((file_id, new_text));
    }

    pub fn apply(self, db: &mut dyn crate::RootQueryDb) {
        let _span = tracing::info_span!(
            "FileChange::apply",
            files_changed = self.files_changed.len(),
            roots = self.roots.as_ref().map(|r| r.len())
        )
        .entered();

        let mut library_files = 0;
        let mut user_files = 0;
        if let Some(roots) = self.roots {
            for (idx, root) in roots.into_iter().enumerate() {
                let root_id = SourceRootId(idx as u32);
                let file_count = root.iter().count();

                if root.is_library {
                    library_files += file_count;
                } else {
                    user_files += file_count;
                }

                for file_id in root.iter() {
                    db.set_file_source_root(file_id, root_id);
                }

                db.set_source_root(root_id, root);
            }
            tracing::debug!(
                library_files,
                user_files,
                "FileChange::apply: source roots classified"
            );
        }

        for (file_id, text) in self.files_changed {
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
