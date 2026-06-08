//! The disk-backed resident-load primitives shared by every consumer.
//!
//! A resident database that holds the whole workspace must NOT pin each file's text
//! as a salsa input (the overlay map lives outside the LRU and OOMs on a large
//! config). Instead each file is registered by its content revision and the text is
//! dropped; [`base_db::file_text_query`] re-reads it from disk on demand under its
//! LRU cap, verifying the bytes against the recorded revision. This is the model the
//! LSP server, the MCP diagnostics resident, and the CLI `analyze` pass all use, so
//! it lives here once.

use std::path::PathBuf;

use base_db::{content_revision, read_disk_text, SourceDatabase, SourceRoot};
use ide::RootDatabaseImpl;
use vfs::file_set::FileSet;
use vfs::{FileId, VfsPath};

/// Build a whole-workspace [`SourceRoot`]: a `FileId ↔ path` map over EVERY file, so
/// cross-module resolution through the module index can find any target's `FileId`.
/// Cheap to clone (the map is `Arc`-backed).
pub fn build_source_root(all_files: &[(FileId, PathBuf)]) -> SourceRoot {
    let mut file_set = FileSet::new();
    for (file_id, path) in all_files {
        file_set.insert(*file_id, VfsPath::new(path.clone()));
    }
    SourceRoot::new_local(file_set)
}

/// Register every file into `db` under `source_root_id` as a disk-backed content
/// revision — the text is read once to hash it, then dropped, so it is re-read on
/// demand under the salsa LRU rather than pinned resident.
///
/// `file_source_root` is set for every file because [`base_db::file_text_query`]
/// derives the on-disk path through it. An unreadable file is registered as an empty
/// overlay (so a later query yields `""` instead of panicking on the disk re-read)
/// and its `(path, error)` is returned, letting a strict caller (e.g. CLI) surface
/// the real read error while a lenient caller (e.g. the MCP resident) keeps serving.
pub fn register_files_disk_backed(
    db: &mut RootDatabaseImpl,
    source_root_id: base_db::SourceRootId,
    files: &[(FileId, PathBuf)],
) -> Vec<(PathBuf, std::io::Error)> {
    let mut unreadable = Vec::new();
    for (file_id, path) in files {
        db.set_file_source_root(*file_id, source_root_id);
        match read_disk_text(path) {
            Ok(text) => db.set_file_revision_from_disk(*file_id, content_revision(&text)),
            Err(e) => {
                tracing::warn!(path = %path.display(), "resident load: read failed: {e}");
                db.set_file_text(*file_id, "");
                unreadable.push((path.clone(), e));
            }
        }
    }
    unreadable
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide::Analysis;

    #[test]
    fn disk_backed_registration_and_unreadable_report() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("a.bsl");
        std::fs::write(&good, "Процедура П() КонецПроцедуры").unwrap();
        let missing = dir.path().join("missing.bsl");

        let files = vec![(FileId(0), good.clone()), (FileId(1), missing.clone())];
        let root = base_db::SourceRootId(0);
        let mut db = RootDatabaseImpl::default();
        db.set_source_root(root, build_source_root(&files));
        let unreadable = register_files_disk_backed(&mut db, root, &files);

        // The missing file is reported with its read error (and overlaid empty); the
        // readable file is disk-backed — no `FileTextInput` overlay pinned — yet queryable.
        assert_eq!(unreadable.len(), 1);
        assert_eq!(unreadable[0].0, missing);
        assert!(db.try_file_text(FileId(0)).is_none(), "readable file must be disk-backed");
        let text = Analysis::from_database(db.clone()).file_text(FileId(0));
        assert!(text.contains("Процедура"), "disk-backed text is read on demand");
    }
}
