use hir::call_graph::{CallerId, GraphNode, ResolvedTarget};
use hir::{ConfigsDatabase, Definition, Method, MethodId, Semantics};
use ide_db::RootDatabase;
use rustc_hash::FxHashMap;
use syntax::{TextRange, TextSize};
use vfs::FileId;

/// A method node in the call hierarchy — the LSP `CallHierarchyItem` payload,
/// kept free of `lsp_types` so the adapter maps ranges with its own encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallHierarchyItem {
    pub name: String,
    /// `Функция` vs `Процедура`, so the adapter can pick the LSP symbol kind.
    pub is_function: bool,
    pub file_id: FileId,
    /// The full method span (LSP `range`).
    pub range: TextRange,
    /// The method name token (LSP `selectionRange`); the anchor incoming/outgoing
    /// requests re-resolve from.
    pub selection_range: TextRange,
    pub detail: Option<String>,
}

/// One edge of the hierarchy: a related method plus the call-site ranges that
/// connect it to the anchor. For incoming calls the ranges live in the caller
/// (`item`); for outgoing calls they live in the anchor method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallHierarchyCall {
    pub item: CallHierarchyItem,
    pub ranges: Vec<TextRange>,
}

/// Resolve the method under the cursor into a call-hierarchy anchor.
///
/// Returns `None` unless the position lands on a method — a declaration name or a
/// call site both resolve to the same `Definition::Method`.
pub fn prepare_call_hierarchy<DB: RootDatabase + ConfigsDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
) -> Option<CallHierarchyItem> {
    let method_id = method_at(db, file_id, offset)?;
    item_for_method(db, method_id)
}

/// Methods that call the method under the cursor, each with the ranges of the
/// call sites inside that caller.
pub fn incoming_calls<DB: RootDatabase + ConfigsDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
) -> Vec<CallHierarchyCall> {
    let Some(target) = method_at(db, file_id, offset) else {
        return Vec::new();
    };

    let source_root = db.file_source_root_input(file_id).source_root_id(db);
    let graph = db.workspace_call_graph(source_root);

    // The stored graph names callers but keeps no call-site ranges; take the
    // distinct caller methods here, then recover ranges from each caller's own
    // resolved summary below.
    //
    // Callers that are a module's top-level body (`GraphNode::ModuleCode`) are
    // intentionally skipped: a `CallHierarchyItem` is a method, and module code
    // has no name/declaration range to anchor one, nor a method position to
    // re-resolve on a follow-up request. Such callers are uncommon in BSL
    // (common modules carry no top-level code).
    let mut callers: Vec<MethodId> = Vec::new();
    for edge in graph.callers(&GraphNode::Method(target)) {
        if let GraphNode::Method(caller) = &edge.from {
            if !callers.contains(caller) {
                callers.push(*caller);
            }
        }
    }

    let mut calls = Vec::new();
    for caller in callers {
        let ranges = call_ranges_to_target(db, caller, target);
        if ranges.is_empty() {
            continue;
        }
        if let Some(item) = item_for_method(db, caller) {
            calls.push(CallHierarchyCall { item, ranges });
        }
    }
    calls
}

/// Methods called by the method under the cursor, each with the ranges of the
/// call sites inside the anchor method.
pub fn outgoing_calls<DB: RootDatabase + ConfigsDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
) -> Vec<CallHierarchyCall> {
    let Some(source) = method_at(db, file_id, offset) else {
        return Vec::new();
    };

    let summary = db.resolved_module_summary(source.module);
    let mut by_callee: FxHashMap<MethodId, Vec<TextRange>> = FxHashMap::default();
    let mut order: Vec<MethodId> = Vec::new();
    for edge in &summary.edges {
        if edge.caller != CallerId::Method(source.local_id) {
            continue;
        }
        if let ResolvedTarget::Method(callee) = &edge.target {
            let ranges = by_callee.entry(*callee).or_insert_with(|| {
                order.push(*callee);
                Vec::new()
            });
            ranges.push(edge.range);
        }
    }

    order
        .into_iter()
        .filter_map(|callee| {
            let ranges = by_callee.remove(&callee)?;
            let item = item_for_method(db, callee)?;
            Some(CallHierarchyCall { item, ranges })
        })
        .collect()
}

fn method_at<DB: RootDatabase>(db: &DB, file_id: FileId, offset: TextSize) -> Option<MethodId> {
    let sema = Semantics::new(db);
    match sema.symbol_at(file_id, offset)?.definition? {
        Definition::Method(id) => Some(id),
        _ => None,
    }
}

/// Call-site ranges inside `caller` that target `target` — the caller's resolved
/// summary carries the range each `ResolvedTarget::Method` edge was lowered from.
fn call_ranges_to_target<DB: ConfigsDatabase>(
    db: &DB,
    caller: MethodId,
    target: MethodId,
) -> Vec<TextRange> {
    let summary = db.resolved_module_summary(caller.module);
    summary
        .edges
        .iter()
        .filter(|edge| edge.caller == CallerId::Method(caller.local_id))
        .filter(|edge| matches!(&edge.target, ResolvedTarget::Method(m) if *m == target))
        .map(|edge| edge.range)
        .collect()
}

fn item_for_method<DB: RootDatabase>(db: &DB, id: MethodId) -> Option<CallHierarchyItem> {
    let method = Method::new(db, id);
    let range = method.source_range()?;
    let selection_range = method.name_range()?;
    Some(CallHierarchyItem {
        name: method.name().as_str().to_string(),
        is_function: method.is_function(),
        file_id: id.module.file_id,
        range,
        selection_range,
        detail: method.is_export().then(|| "Экспорт".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{file_set::FileSet, VfsPath};

    const MODULE: &str = r#"
Процедура Помощник()
КонецПроцедуры

Процедура Первый()
    Помощник();
КонецПроцедуры

Процедура Второй()
    Помощник();
    Помощник();
КонецПроцедуры
"#;

    fn single_file(source: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::default();
        let file_id = FileId(0);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, source);
        (db, file_id)
    }

    fn offset_of(source: &str, needle: &str) -> TextSize {
        TextSize::from(source.find(needle).unwrap() as u32)
    }

    #[test]
    fn prepare_reports_the_method_under_cursor() {
        let (db, file_id) = single_file(MODULE);
        let item = prepare_call_hierarchy(&db, file_id, offset_of(MODULE, "Помощник"))
            .expect("cursor is on a method");
        assert_eq!(item.name, "Помощник");
        assert!(!item.is_function, "Процедура is not a function");
        assert_eq!(&MODULE[item.selection_range], "Помощник");
        assert!(item.range.contains_range(item.selection_range));
    }

    #[test]
    fn prepare_declines_non_method_position() {
        let source = "Процедура Тест()\n    Перем Локаль;\n    Локаль = 1;\nКонецПроцедуры\n";
        let (db, file_id) = single_file(source);
        assert!(prepare_call_hierarchy(&db, file_id, offset_of(source, "Локаль")).is_none());
    }

    #[test]
    fn outgoing_lists_callees_with_every_call_site() {
        let (db, file_id) = single_file(MODULE);
        // Anchor on the declaration of Второй, which calls Помощник twice.
        let calls = outgoing_calls(&db, file_id, offset_of(MODULE, "Второй"));
        assert_eq!(calls.len(), 1, "Второй calls a single distinct method");
        assert_eq!(calls[0].item.name, "Помощник");
        assert_eq!(calls[0].ranges.len(), 2, "both call sites reported");
        assert!(calls[0].ranges.iter().all(|r| MODULE[*r].contains("Помощник")));
    }

    #[test]
    fn incoming_lists_callers_with_ranges() {
        let (db, file_id) = single_file(MODULE);
        let calls = incoming_calls(&db, file_id, offset_of(MODULE, "Помощник"));
        let mut names: Vec<&str> = calls.iter().map(|c| c.item.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["Второй", "Первый"]);

        let second = calls.iter().find(|c| c.item.name == "Второй").unwrap();
        assert_eq!(second.ranges.len(), 2, "Второй calls Помощник twice");
        let first = calls.iter().find(|c| c.item.name == "Первый").unwrap();
        assert_eq!(first.ranges.len(), 1);
    }

    #[test]
    fn outgoing_from_leaf_method_is_empty() {
        let (db, file_id) = single_file(MODULE);
        // Помощник calls nothing; anchor on its declaration (first occurrence).
        assert!(outgoing_calls(&db, file_id, offset_of(MODULE, "Помощник")).is_empty());
    }
}
