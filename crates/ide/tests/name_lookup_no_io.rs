//! Narrowing the question to categories no file can answer must not read files.
//!
//! Proven by substitution rather than by a counter: the module is replaced with
//! a FIFO, so any read of it blocks forever instead of merely being observed.
//! Permissions and deletion would not do — the first turns a read into an error
//! the code may swallow, the second into an empty file that looks like a module
//! with nothing in it.

#![cfg(unix)]

use ide::{lookup_names, NameCategory, NameQuery};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::sync::mpsc;
use std::time::Duration;
use vfs::{file_set::FileSet, FileId, VfsPath};

/// Long enough that a real disk read finishes, short enough to keep the suite
/// quick. A blocked FIFO read never finishes at all, so the margin is not fine.
const PATIENCE: Duration = Duration::from_secs(3);

struct Stand {
    _dir: tempfile::TempDir,
    module: std::path::PathBuf,
}

fn stand() -> Stand {
    let dir = tempfile::tempdir().expect("temp dir");
    let module = dir.path().join("CommonModules/Настройки/Ext/Module.bsl");
    std::fs::create_dir_all(module.parent().unwrap()).expect("module dir");

    let made = std::process::Command::new("mkfifo").arg(&module).status().expect("mkfifo runs");
    assert!(made.success(), "mkfifo failed for {}", module.display());

    Stand { _dir: dir, module }
}

fn db_over(stand: &Stand) -> RootDatabaseImpl {
    let mut db = RootDatabaseImpl::new();
    let mut file_set = FileSet::default();
    let file_id = FileId(0);
    file_set.insert(file_id, VfsPath::new(stand.module.to_string_lossy().as_ref()));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(file_id, SourceRootId(0));
    // No overlay: the text has to come from disk, which is where the FIFO is.
    db.set_file_revision_from_disk(file_id, 1);
    db
}

/// Runs a lookup on its own thread and reports whether it came back at all.
fn finishes(categories: Option<&'static [NameCategory]>) -> bool {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let stand = stand();
        let db = db_over(&stand);
        let mut query = NameQuery::new("СтрНайти", 20);
        query.categories = categories;
        let found = lookup_names(&db, &query, &[]);
        // Keep the stand alive until the lookup is done, or the FIFO would be
        // unlinked out from under a blocked read and the control would pass for
        // the wrong reason.
        drop(stand);
        let _ = tx.send(found.candidates.len());
    });
    rx.recv_timeout(PATIENCE).is_ok()
}

const PLATFORM_ONLY: &[NameCategory] = &[NameCategory::PlatformMember];

#[test]
fn a_platform_only_question_never_touches_a_module_file() {
    assert!(
        finishes(Some(PLATFORM_ONLY)),
        "the lookup blocked, so it opened the module it had no reason to read",
    );
}

/// The control that makes the test above mean something: the same stand, asked
/// the wide question, DOES read the module and therefore blocks. Without it a
/// lookup that never reads anything at all would pass both ways.
#[test]
fn the_same_stand_asked_widely_does_read_the_module() {
    assert!(
        !finishes(None),
        "the wide question returned without reading the module — the stand is not \
         sensitive to file reads, so the narrowed case proves nothing",
    );
}
