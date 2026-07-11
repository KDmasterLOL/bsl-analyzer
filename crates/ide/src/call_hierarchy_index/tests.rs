use std::{cell::Cell, rc::Rc};

use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use vfs::{file_set::FileSet, FileId, VfsPath};

use super::{
    build_call_hierarchy_index_with_observer, BatchObserver, CallHierarchyBatchEvent,
    CallHierarchyBatchEventKind, CallHierarchyBatchPhase, CallHierarchyIndexBuildRequest,
    CallHierarchyIndexBuildResult,
};
use crate::RootDatabaseImpl;
use hir::{MethodId, ModuleId};

const ROOT: SourceRootId = SourceRootId(0);

#[derive(Clone, Copy)]
struct FixtureFile {
    path: &'static str,
    text: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
enum ProbeEvent {
    Started(CallHierarchyBatchPhase, usize),
    Opened,
    Dropped(CallHierarchyBatchPhase, usize),
    CacheCleared(CallHierarchyBatchPhase, usize),
    Completed(CallHierarchyBatchPhase, usize),
}

struct Probe {
    events: Rc<std::cell::RefCell<Vec<ProbeEvent>>>,
    live: Rc<Cell<bool>>,
    cache_clears: usize,
}

impl BatchObserver for Probe {
    fn on_event(&mut self, event: &CallHierarchyBatchEvent) {
        let probe_event = match event.kind {
            CallHierarchyBatchEventKind::Started => {
                ProbeEvent::Started(event.phase, event.batch_index)
            }
            CallHierarchyBatchEventKind::DatabaseDropped => {
                assert!(self.live.replace(false), "database must be live until its drop event");
                ProbeEvent::Dropped(event.phase, event.batch_index)
            }
            CallHierarchyBatchEventKind::Completed => {
                ProbeEvent::Completed(event.phase, event.batch_index)
            }
        };
        self.events.borrow_mut().push(probe_event);
    }

    fn on_node_caches_cleared(&mut self, phase: CallHierarchyBatchPhase, batch_index: usize) {
        self.cache_clears += 1;
        self.events.borrow_mut().push(ProbeEvent::CacheCleared(phase, batch_index));
    }
}

fn fixture() -> [FixtureFile; 3] {
    [
        FixtureFile {
            path: "/src/CommonModules/Первый/Ext/Module.bsl",
            text: "Процедура Начать()\nВнутренний();\nКонецПроцедуры\n\nПроцедура Внутренний()\nКонецПроцедуры",
        },
        FixtureFile {
            path: "/src/CommonModules/Второй/Ext/Module.bsl",
            text: "Процедура Продолжить()\nЗавершить();\nКонецПроцедуры\n\nПроцедура Завершить()\nКонецПроцедуры",
        },
        FixtureFile {
            path: "/src/CommonModules/Третий/Ext/Module.bsl",
            text: "Процедура Закрыть()\nОчистить();\nКонецПроцедуры\n\nПроцедура Очистить()\nКонецПроцедуры",
        },
    ]
}

fn batch_database(files: &[FixtureFile], batch: &[ModuleId]) -> RootDatabaseImpl {
    let mut db = RootDatabaseImpl::new();
    let mut file_set = FileSet::new();
    for (index, file) in files.iter().enumerate() {
        file_set.insert(FileId(index as u32), VfsPath::new(file.path));
    }
    db.set_source_root(ROOT, SourceRoot::new_local(file_set));
    for module in batch {
        let file = files[module.file_id.0 as usize];
        db.set_file_source_root(module.file_id, ROOT);
        db.set_file_text(module.file_id, file.text);
    }
    db
}

fn build(batch_size: usize) -> (CallHierarchyIndexBuildResult, Vec<ProbeEvent>, usize) {
    let files = fixture();
    let modules = [ModuleId::new(FileId(0)), ModuleId::new(FileId(1)), ModuleId::new(FileId(2))];
    let events = Rc::new(std::cell::RefCell::new(Vec::new()));
    let live = Rc::new(Cell::new(false));
    let open_events = Rc::clone(&events);
    let open_live = Rc::clone(&live);
    let mut open_batch = move |batch: &[ModuleId]| {
        assert!(!open_live.replace(true), "at most one batch database may be live");
        open_events.borrow_mut().push(ProbeEvent::Opened);
        batch_database(&files, batch)
    };
    let mut probe = Probe { events: Rc::clone(&events), live: Rc::clone(&live), cache_clears: 0 };

    let result = build_call_hierarchy_index_with_observer(
        CallHierarchyIndexBuildRequest::new(&modules, batch_size),
        &mut open_batch,
        &mut probe,
    )
    .expect("fixture builder should succeed");

    assert!(!live.get(), "the final batch database must be dropped");
    let recorded_events = std::mem::take(&mut *events.borrow_mut());
    (result, recorded_events, probe.cache_clears)
}

mod parity;

#[derive(Debug, PartialEq, Eq)]
struct IndexDigest {
    layout_hashes: Vec<(ModuleId, u64)>,
    callers: Vec<(MethodId, Vec<MethodId>)>,
}

fn digest(result: &CallHierarchyIndexBuildResult) -> IndexDigest {
    let methods = [
        MethodId { module: ModuleId::new(FileId(0)), local_id: 0 },
        MethodId { module: ModuleId::new(FileId(0)), local_id: 1 },
        MethodId { module: ModuleId::new(FileId(1)), local_id: 0 },
        MethodId { module: ModuleId::new(FileId(1)), local_id: 1 },
        MethodId { module: ModuleId::new(FileId(2)), local_id: 0 },
        MethodId { module: ModuleId::new(FileId(2)), local_id: 1 },
    ];
    let modules = [ModuleId::new(FileId(0)), ModuleId::new(FileId(1)), ModuleId::new(FileId(2))];

    IndexDigest {
        layout_hashes: modules
            .into_iter()
            .map(|module| (module, result.index.layout_hash(module).expect("layout hash")))
            .collect(),
        callers: methods
            .into_iter()
            .map(|method| (method, result.index.callers(method).to_vec()))
            .collect(),
    }
}

#[test]
fn bounded_call_hierarchy_index_builder_drops_batches_and_clears_node_caches() {
    // Given: three modules processed one at a time.
    let (result, events, cache_clears) = build(1);

    // When: the builder completes both passes.

    // Then: every database is dropped before the next open and every batch clears caches.
    assert_eq!(result.method_count, 6);
    assert_eq!(result.pair_count, 3);
    assert_eq!(cache_clears, 6);
    assert_eq!(result.rss_samples.len(), 6);
    assert_eq!(
        events,
        vec![
            ProbeEvent::Started(CallHierarchyBatchPhase::Index, 0),
            ProbeEvent::Opened,
            ProbeEvent::Dropped(CallHierarchyBatchPhase::Index, 0),
            ProbeEvent::CacheCleared(CallHierarchyBatchPhase::Index, 0),
            ProbeEvent::Completed(CallHierarchyBatchPhase::Index, 0),
            ProbeEvent::Started(CallHierarchyBatchPhase::Index, 1),
            ProbeEvent::Opened,
            ProbeEvent::Dropped(CallHierarchyBatchPhase::Index, 1),
            ProbeEvent::CacheCleared(CallHierarchyBatchPhase::Index, 1),
            ProbeEvent::Completed(CallHierarchyBatchPhase::Index, 1),
            ProbeEvent::Started(CallHierarchyBatchPhase::Index, 2),
            ProbeEvent::Opened,
            ProbeEvent::Dropped(CallHierarchyBatchPhase::Index, 2),
            ProbeEvent::CacheCleared(CallHierarchyBatchPhase::Index, 2),
            ProbeEvent::Completed(CallHierarchyBatchPhase::Index, 2),
            ProbeEvent::Started(CallHierarchyBatchPhase::MethodPairs, 0),
            ProbeEvent::Opened,
            ProbeEvent::Dropped(CallHierarchyBatchPhase::MethodPairs, 0),
            ProbeEvent::CacheCleared(CallHierarchyBatchPhase::MethodPairs, 0),
            ProbeEvent::Completed(CallHierarchyBatchPhase::MethodPairs, 0),
            ProbeEvent::Started(CallHierarchyBatchPhase::MethodPairs, 1),
            ProbeEvent::Opened,
            ProbeEvent::Dropped(CallHierarchyBatchPhase::MethodPairs, 1),
            ProbeEvent::CacheCleared(CallHierarchyBatchPhase::MethodPairs, 1),
            ProbeEvent::Completed(CallHierarchyBatchPhase::MethodPairs, 1),
            ProbeEvent::Started(CallHierarchyBatchPhase::MethodPairs, 2),
            ProbeEvent::Opened,
            ProbeEvent::Dropped(CallHierarchyBatchPhase::MethodPairs, 2),
            ProbeEvent::CacheCleared(CallHierarchyBatchPhase::MethodPairs, 2),
            ProbeEvent::Completed(CallHierarchyBatchPhase::MethodPairs, 2),
        ],
    );
}

#[test]
fn bounded_call_hierarchy_index_builder_has_the_same_digest_for_every_batch_size() {
    // Given: the same modules with one, two, and all modules in a batch.
    let (single, _, _) = build(1);
    let (two, _, _) = build(2);
    let (all, _, _) = build(fixture().len());

    // When: each build completes.

    // Then: batching changes residency only, not the compact index.
    assert_eq!(digest(&single), digest(&two));
    assert_eq!(digest(&single), digest(&all));
}

#[test]
fn bounded_call_hierarchy_index_builder_does_not_enter_query_or_sdbl_phases() {
    // Given: a bounded method-only build.
    let (result, _, _) = build(2);

    // When: its phase log is inspected.

    // Then: the closed phase set proves only indexing and method-pair projection ran.
    for event in result.batch_events {
        match event.phase {
            CallHierarchyBatchPhase::Index | CallHierarchyBatchPhase::MethodPairs => {}
        }
    }
}
