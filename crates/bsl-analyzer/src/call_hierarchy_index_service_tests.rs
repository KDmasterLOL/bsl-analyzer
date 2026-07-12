use base_db::{content_revision, SourceDatabase, SourceRoot, SourceRootId};
use ide::RootDatabaseImpl;
use lsp_types::Url;
use vfs::{file_set::FileSet, FileId, VfsPath};

use super::*;
use crate::call_hierarchy_index_state::CallHierarchyIndexSnapshotId;
use crate::mem_docs::MemDocs;

struct Fixture {
    _directory: tempfile::TempDir,
    db: RootDatabaseImpl,
    mem_docs: MemDocs,
    source_root: SourceRootId,
    file_id: FileId,
}

fn fixture(text: &str) -> Fixture {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("Module.bsl");
    std::fs::write(&path, text).expect("disk module");
    let source_root = SourceRootId(0);
    let file_id = FileId(0);
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new(path.clone()));
    let mut db = RootDatabaseImpl::default();
    db.set_source_root(source_root, SourceRoot::new_local(file_set));
    db.set_file_source_root(file_id, source_root);
    db.set_file_revision_from_disk(file_id, content_revision(text));
    let uri = Url::from_file_path(path).expect("file URL");
    let mut mem_docs = MemDocs::default();
    mem_docs.insert(uri, text.to_owned(), 1);
    Fixture { _directory: directory, db, mem_docs, source_root, file_id }
}

#[test]
fn call_hierarchy_index_edit_journal_catches_up_body_only_buffer_edit() {
    // Given: a frozen buffer where one method calls another.
    let initial = "Процедура А()\nБ();\nКонецПроцедуры\n\nПроцедура Б()\nКонецПроцедуры";
    let mut fixture = fixture(initial);
    let snapshot = CallHierarchyIndexFrozenSnapshot::capture(
        &fixture.db,
        fixture.source_root,
        &fixture.mem_docs.freeze(),
        1,
    );
    let lifecycle = CallHierarchyIndexState::default();
    assert!(lifecycle.start_build(fixture.source_root, 1, CallHierarchyIndexSnapshotId(1)));
    let uri = Url::from_file_path(fixture._directory.path().join("Module.bsl")).expect("file URL");
    fixture.mem_docs.insert(
        uri,
        "Процедура А()\nКонецПроцедуры\n\nПроцедура Б()\nКонецПроцедуры".to_owned(),
        2,
    );
    assert!(lifecycle.record_body_edit_or_supersede_ready(fixture.source_root, 1, fixture.file_id,));

    // When: the base index completes and drains the journal against the latest buffer.
    let task = run_build(lifecycle.clone(), fixture.mem_docs.clone(), snapshot);

    // Then: the caught-up index publishes without the stale call pair.
    let Task::CallHierarchyIndexBuilt { index, .. } = task else {
        panic!("body-only edit must publish a caught-up index");
    };
    assert!(index.is_empty());
    assert!(lifecycle.is_ready_generation(fixture.source_root, 1));
}

#[test]
fn call_hierarchy_index_layout_edit_supersedes_instead_of_publishing() {
    // Given: a frozen module whose later edit inserts a top-level variable.
    let initial = "Процедура А()\nКонецПроцедуры";
    let mut fixture = fixture(initial);
    let snapshot = CallHierarchyIndexFrozenSnapshot::capture(
        &fixture.db,
        fixture.source_root,
        &fixture.mem_docs.freeze(),
        1,
    );
    let lifecycle = CallHierarchyIndexState::default();
    assert!(lifecycle.start_build(fixture.source_root, 1, CallHierarchyIndexSnapshotId(1)));
    let uri = Url::from_file_path(fixture._directory.path().join("Module.bsl")).expect("file URL");
    fixture.mem_docs.insert(uri, "Перем Счетчик;\n\nПроцедура А()\nКонецПроцедуры".to_owned(), 2);
    assert!(lifecycle.record_body_edit_or_supersede_ready(fixture.source_root, 1, fixture.file_id,));

    // When: reconciliation compares the old layout with the latest module layout.
    let task = run_build(lifecycle.clone(), fixture.mem_docs.clone(), snapshot);

    // Then: the changed layout is superseded and cannot publish the frozen generation.
    assert!(matches!(task, Task::CallHierarchyIndexSuperseded { .. }));
    assert!(lifecycle.finish_superseded(fixture.source_root, 1));
    assert!(!lifecycle.is_ready_generation(fixture.source_root, 1));
}

#[test]
fn call_hierarchy_index_catch_up_budget_supersedes_after_pass_or_time_limit() {
    // Given: catch-up at its pass and time boundaries.
    let now = Instant::now();

    // When: either limit is exhausted.
    let pass_exhausted = catch_up_exhausted(CATCH_UP_PASSES, now);
    let time_exhausted = catch_up_exhausted(0, now - CATCH_UP_LIMIT);

    // Then: the worker must supersede instead of entering another catch-up loop.
    assert!(pass_exhausted);
    assert!(time_exhausted);
}
