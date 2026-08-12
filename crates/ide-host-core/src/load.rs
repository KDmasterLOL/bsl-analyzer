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

/// How a file's text is registered as a salsa input. The DRIVER decides which
/// variant (is the buffer open? was the file deleted?); this is the single place
/// that maps each intent to the right salsa mutation, so the "overlay vs disk-backed
/// vs tombstone" encoding — the exact distinction whose accidental conflation pins a
/// whole workspace's text resident — lives in one tunable spot.
pub enum FileTextSource<'a> {
    /// An open editor buffer is authoritative: its text is pinned as a resident
    /// overlay (`set_file_text`), source of truth for unsaved content.
    Overlay(&'a str),
    /// A closed / disk-backed file: registered by content revision only (the text is
    /// re-read on demand under the salsa LRU). The caller passes the text it already
    /// read; the revision is derived here so callers cannot diverge on the hash.
    Disk(&'a str),
    /// A deleted file: an empty overlay so a later query yields `""` instead of
    /// panicking on a disk re-read. FileSet removal (if any) stays with the driver —
    /// this only sets the text state.
    Deleted,
    /// A file that is still there but whose bytes could not be read: the same empty
    /// overlay, plus the mark that says the emptiness is ignorance.
    ///
    /// Split from [`Deleted`](Self::Deleted) because one variant cannot carry two
    /// meanings and still be written down: name resolution has to tell "this module
    /// has no API" from "nobody could read this module's API", and both arrive here
    /// as the same empty text.
    Unreadable,
}

/// Apply one file's [`FileTextSource`] to `db`. A leaf salsa mutation only: it does
/// not touch the source root / FileSet, project config, or any driver lifecycle.
pub fn set_file_text_source(
    db: &mut RootDatabaseImpl,
    file_id: FileId,
    source: FileTextSource<'_>,
) {
    match source {
        FileTextSource::Overlay(text) => db.set_file_text(file_id, text),
        FileTextSource::Disk(text) => {
            db.set_file_revision_from_disk(file_id, content_revision(text))
        }
        FileTextSource::Deleted => db.set_file_text(file_id, ""),
        FileTextSource::Unreadable => db.set_file_unreadable(file_id),
    }
}

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
            Ok(text) => set_file_text_source(db, *file_id, FileTextSource::Disk(&text)),
            Err(e) => {
                tracing::warn!(path = %path.display(), "resident load: read failed: {e}");
                set_file_text_source(db, *file_id, FileTextSource::Unreadable);
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

    #[test]
    fn file_text_source_variants() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bsl");
        std::fs::write(&path, "Текст").unwrap();
        let files = vec![(FileId(0), path)];
        let root = base_db::SourceRootId(0);
        let mut db = RootDatabaseImpl::default();
        db.set_source_root(root, build_source_root(&files));
        db.set_file_source_root(FileId(0), root);

        let read = |db: &RootDatabaseImpl| Analysis::from_database(db.clone()).file_text(FileId(0));

        // Overlay: editor buffer pinned as a resident input.
        set_file_text_source(&mut db, FileId(0), FileTextSource::Overlay("буфер"));
        assert!(db.try_file_text(FileId(0)).is_some(), "overlay is pinned");
        assert_eq!(&*read(&db), "буфер");

        // Disk: overlay dropped, text re-read from disk and verified against the revision.
        set_file_text_source(&mut db, FileId(0), FileTextSource::Disk("Текст"));
        assert!(db.try_file_text(FileId(0)).is_none(), "disk-backed is not pinned");
        assert_eq!(&*read(&db), "Текст");

        // Deleted: empty overlay, and the emptiness stands for content.
        set_file_text_source(&mut db, FileId(0), FileTextSource::Deleted);
        assert_eq!(&*read(&db), "");
        assert!(!db.file_is_unread(FileId(0)), "a deleted file is not an unread one");

        // Unreadable: the same empty overlay, marked.
        set_file_text_source(&mut db, FileId(0), FileTextSource::Unreadable);
        assert_eq!(&*read(&db), "");
        assert!(db.file_is_unread(FileId(0)));
    }
}
