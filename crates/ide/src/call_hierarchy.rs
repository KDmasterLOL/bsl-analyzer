use hir::{
    CallHierarchyReverseIndex, ConfigsDatabase, Definition, Method, MethodCallPair, MethodId,
    Semantics,
};
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
    index: &CallHierarchyReverseIndex,
) -> Option<Vec<CallHierarchyCall>> {
    let target = method_at(db, file_id, offset)?;
    let calls = index
        .callers(target)
        .iter()
        .filter_map(|&caller| {
            let ranges = call_ranges_to_target(db, caller, target);
            if ranges.is_empty() {
                return None;
            }
            let item = item_for_method(db, caller)?;
            Some(CallHierarchyCall { item, ranges })
        })
        .collect::<Vec<_>>();
    (!calls.is_empty()).then_some(calls)
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
        let Some(pair) = MethodCallPair::from_resolved_edge(source.module, edge) else {
            continue;
        };
        if pair.caller != source {
            continue;
        }
        let ranges = by_callee.entry(pair.target).or_insert_with(|| {
            order.push(pair.target);
            Vec::new()
        });
        ranges.push(edge.range);
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
/// summary carries the range each method-to-method edge was lowered from.
fn call_ranges_to_target<DB: ConfigsDatabase>(
    db: &DB,
    caller: MethodId,
    target: MethodId,
) -> Vec<TextRange> {
    let summary = db.resolved_module_summary(caller.module);
    summary
        .edges
        .iter()
        .filter_map(|edge| {
            let pair = MethodCallPair::from_resolved_edge(caller.module, edge)?;
            if pair.caller == caller && pair.target == target {
                Some(edge.range)
            } else {
                None
            }
        })
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

    fn reverse_index(
        target: MethodId,
        callers: impl IntoIterator<Item = MethodId>,
    ) -> std::sync::Arc<hir::CallHierarchyReverseIndex> {
        let mut index = hir::CallHierarchyReverseIndex::new();
        index.replace_module(
            target.module,
            callers.into_iter().map(|caller| hir::MethodCallPair::new(caller, target)),
            0,
        );
        std::sync::Arc::new(index)
    }

    fn method_id(db: &RootDatabaseImpl, file_id: FileId, source: &str, name: &str) -> MethodId {
        method_at(db, file_id, offset_of(source, name)).expect("method name resolves")
    }

    #[test]
    fn incoming_uses_index_callers_with_every_live_call_site() {
        // Given: two callers, one of which invokes the target twice.
        let (db, file_id) = single_file(MODULE);
        let target = method_id(&db, file_id, MODULE, "Помощник");
        let index = reverse_index(
            target,
            [method_id(&db, file_id, MODULE, "Первый"), method_id(&db, file_id, MODULE, "Второй")],
        );

        // When: incoming calls resolve through the compact index.
        let calls = incoming_calls(&db, file_id, offset_of(MODULE, "Помощник"), &index)
            .expect("indexed callers resolve");

        // Then: both caller items and every current call-site range are returned.
        let mut names: Vec<&str> = calls.iter().map(|c| c.item.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["Второй", "Первый"]);

        let second = calls.iter().find(|c| c.item.name == "Второй").unwrap();
        assert_eq!(second.ranges.len(), 2, "Второй calls Помощник twice");
        let first = calls.iter().find(|c| c.item.name == "Первый").unwrap();
        assert_eq!(first.ranges.len(), 1);
    }

    #[test]
    fn incoming_reads_ranges_from_the_live_database_after_a_body_edit() {
        // Given: an index rebuilt for a body edit that adds a third call site.
        let (mut db, file_id) = single_file(MODULE);
        let edited = MODULE.replacen(
            "    Помощник();\nКонецПроцедуры\n",
            "    Помощник();\n    Помощник();\nКонецПроцедуры\n",
            1,
        );
        db.set_file_text(file_id, &edited);
        let target = method_id(&db, file_id, &edited, "Помощник");
        let index = reverse_index(
            target,
            [
                method_id(&db, file_id, &edited, "Первый"),
                method_id(&db, file_id, &edited, "Второй"),
            ],
        );

        // When: the rebuilt caller index is joined to the live database.
        let calls = incoming_calls(&db, file_id, offset_of(&edited, "Помощник"), &index)
            .expect("indexed callers resolve");

        // Then: identity comes from the index and ranges reflect the edited body.
        let first = calls.iter().find(|call| call.item.name == "Первый").unwrap();
        assert_eq!(first.ranges.len(), 2);
        let second = calls.iter().find(|call| call.item.name == "Второй").unwrap();
        assert_eq!(second.ranges.len(), 2);
    }

    #[test]
    fn incoming_rejects_an_index_after_a_method_layout_change() {
        // Given: an index built before a top-level declaration shifts method IDs.
        let (mut db, file_id) = single_file(MODULE);
        let target = method_id(&db, file_id, MODULE, "Помощник");
        let stale_index = reverse_index(
            target,
            [method_id(&db, file_id, MODULE, "Первый"), method_id(&db, file_id, MODULE, "Второй")],
        );
        let shifted = format!("Перем Сдвиг;\n{MODULE}");
        db.set_file_text(file_id, &shifted);

        // When: the request resolves the post-layout-change target.
        let calls = incoming_calls(&db, file_id, offset_of(&shifted, "Помощник"), &stale_index);

        // Then: stale durable IDs do not produce a caller response.
        assert!(calls.is_none());
    }

    #[test]
    fn outgoing_from_leaf_method_is_empty() {
        let (db, file_id) = single_file(MODULE);
        // Помощник calls nothing; anchor on its declaration (first occurrence).
        assert!(outgoing_calls(&db, file_id, offset_of(MODULE, "Помощник")).is_empty());
    }
}
