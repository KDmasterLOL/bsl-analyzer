use super::*;
use ide_db::base_db::{RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use ide_db::vfs::{file_set::FileSet, VfsPath};
use ide_db::RootDatabaseImpl;

fn single_file(source: &str) -> (RootDatabaseImpl, FileId) {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));
    db.set_file_text(file_id, source);
    (db, file_id)
}

fn two_files(source_a: &str, source_b: &str) -> (RootDatabaseImpl, FileId, FileId) {
    let mut db = RootDatabaseImpl::new();
    let file_a = FileId(0);
    let file_b = FileId(1);
    let mut file_set = FileSet::new();
    file_set.insert(file_a, VfsPath::new("/a.bsl"));
    file_set.insert(file_b, VfsPath::new("/b.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_a, SourceRootId(0));
    db.set_file_source_root(file_b, SourceRootId(0));
    db.set_file_text(file_a, source_a);
    db.set_file_text(file_b, source_b);
    (db, file_a, file_b)
}

fn arg_lists(db: &RootDatabaseImpl, file_id: FileId) -> Vec<SyntaxNode> {
    db.parse(file_id)
        .syntax_node()
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::ARG_LIST)
        .collect()
}

#[test]
fn callee_resolver_matches_offset_for_supported_call_forms() {
    let source = r#"
Функция Внешняя(Параметр)
    Возврат Параметр;
КонецФункции

Функция Внутренняя(Параметр)
    Возврат Параметр;
КонецФункции

Процедура Тест()
    Массив = Новый Массив;
    Список = Новый СписокЗначений;
    Внешняя(1);
    Новый Массив(10);
    Массив.Добавить(2);
    Список.Добавить(3);
    ОбщийМодуль.Метод(4);
    Справочники.Товары.ПолучитьФорму(5);
    Внешняя(Внутренняя(6));
КонецПроцедуры
"#;
    let (db, file_id) = single_file(source);
    let resolver = CalleeResolver::new(&db, file_id);

    let mut resolved: Vec<(String, CalleeKind)> = Vec::new();
    for arg_list in arg_lists(&db, file_id) {
        let Some(first_arg) = arg_list.children().next() else { continue };
        let offset_result = resolve_callee_at(&db, file_id, first_arg.text_range().start())
            .map(|(callee, _)| callee);
        let node_result = resolver.resolve_arg_list(&arg_list);

        assert_eq!(node_result, offset_result, "arg list: {}", arg_list.text());
        if let Some(callee) = node_result {
            resolved.push((arg_list.text().to_string(), callee));
        }
    }

    // Per-form expected resolution — each call shape must resolve to its
    // specific callee, not merely "some" callee of that kind.
    let find = |needle: &str| -> &CalleeKind {
        resolved
            .iter()
            .find(|(text, _)| text.as_str() == needle)
            .map(|(_, c)| c)
            .unwrap_or_else(|| panic!("no callee for {needle:?}: {resolved:?}"))
    };

    assert!(matches!(find("(1)"), CalleeKind::LocalMethod { method, .. }
        if method.as_str().eq_ignore_ascii_case("Внешняя")));
    assert!(matches!(find("(10)"), CalleeKind::PlatformConstructor { type_name }
        if type_name.eq_ignore_ascii_case("Массив")));
    assert!(
        matches!(find("(2)"), CalleeKind::PlatformMethod { type_name, method_name }
        if type_name.eq_ignore_ascii_case("Array")
            && method_name.eq_ignore_ascii_case("Добавить")),
        "(2): {resolved:?}"
    );
    assert!(
        matches!(find("(3)"), CalleeKind::PlatformMethod { type_name, method_name }
        if type_name.eq_ignore_ascii_case("ValueList")
            && method_name.eq_ignore_ascii_case("Добавить")),
        "(3): {resolved:?}"
    );
    assert_ne!(find("(2)"), find("(3)"), "Массив.Добавить and Список.Добавить must differ");
    assert!(matches!(find("(4)"), CalleeKind::CommonModuleMethod { module, method }
        if module.as_str().eq_ignore_ascii_case("ОбщийМодуль")
            && method.as_str().eq_ignore_ascii_case("Метод")));
    assert!(matches!(find("(5)"), CalleeKind::PlatformManagerMethod { .. }));
    assert!(matches!(find("(6)"), CalleeKind::LocalMethod { method, .. }
        if method.as_str().eq_ignore_ascii_case("Внутренняя")));
    assert!(matches!(find("(Внутренняя(6))"), CalleeKind::LocalMethod { method, .. }
        if method.as_str().eq_ignore_ascii_case("Внешняя")));
}

#[test]
fn callee_resolver_rejects_invalid_shapes() {
    let source = "Процедура Тест()\n    Локальная(1);\nКонецПроцедуры";
    let (db, file_id) = single_file(source);
    let resolver = CalleeResolver::new(&db, file_id);
    let root = db.parse(file_id).syntax_node();
    let arg_list = arg_lists(&db, file_id).pop().expect("call has an argument list");

    assert_eq!(resolver.resolve_arg_list(&root), None);
    assert_eq!(
        resolver.resolve_arg_list(&arg_list.parent().expect("argument list has a parent")),
        None,
    );

    let mut builder = syntax::SyntaxTreeBuilder::new();
    builder.start_node(SyntaxKind::SOURCE_FILE);
    builder.start_node(SyntaxKind::ARG_LIST);
    builder.token(SyntaxKind::L_PAREN, "(");
    builder.token(SyntaxKind::R_PAREN, ")");
    builder.finish_node();
    builder.finish_node();
    let wrong_parent_arg_list = builder
        .finish()
        .syntax_node()
        .first_child()
        .expect("synthetic source has an argument list");

    assert_eq!(resolver.resolve_arg_list(&wrong_parent_arg_list), None,);
}

#[test]
fn callee_resolver_rejects_arg_list_from_another_file() {
    let source =
        "Процедура Тест()\n    Массив = Новый Массив;\n    Массив.Добавить(10);\nКонецПроцедуры";
    let (db, file_a, file_b) = two_files(source, source);
    let arg_list = arg_lists(&db, file_a)
        .into_iter()
        .find(|node| node.text() == "(10)")
        .expect("file A has a normal call argument list");
    let resolver = CalleeResolver::new(&db, file_b);

    assert_eq!(
        arg_list.parent().expect("argument list has a parent").kind(),
        SyntaxKind::CALL_EXPR
    );
    assert_eq!(resolver.resolve_arg_list(&arg_list), None);
}

#[test]
fn resolve_callee_at_preserves_trailing_and_closing_cursor_behavior() {
    let source = r#"
Функция Локальная(Первый, Второй)
    Возврат Первый;
КонецФункции

Процедура Тест()
    Локальная(1, );
КонецПроцедуры
"#;
    let (db, file_id) = single_file(source);
    let resolver = CalleeResolver::new(&db, file_id);
    let arg_list = arg_lists(&db, file_id)
        .into_iter()
        .find(|node| node.text().to_string().contains("1,"))
        .expect("call has an argument list");
    let comma_offset = TextSize::from(source.find(", )").expect("trailing comma") as u32 + 1);
    let closing_offset = TextSize::from(
        source.find("Локальная(1, )").expect("call") as u32 + "Локальная(1, )".len() as u32,
    );

    let Some((offset_callee, active)) = resolve_callee_at(&db, file_id, comma_offset) else {
        panic!("trailing argument slot must resolve its callee");
    };

    assert_eq!(active, ActiveParam { index: 1 });
    assert_eq!(
        offset_callee,
        resolver.resolve_arg_list(&arg_list).expect("the same parsed argument list resolves"),
    );
    assert_eq!(resolve_callee_at(&db, file_id, closing_offset), None);
}

#[test]
fn callee_resolver_is_safe_on_malformed_recovery_call() {
    // Truncated call with a trailing comma and no closing paren: the
    // parser produces a recovery CALL_EXPR/ARG_LIST. Both entry points
    // must return None for an unresolvable callee, not panic.
    let source = "Процедура Тест()\n    Несуществующая(1,\nКонецПроцедуры";
    let (db, file_id) = single_file(source);
    let resolver = CalleeResolver::new(&db, file_id);
    let arg_list = arg_lists(&db, file_id)
        .into_iter()
        .find(|node| node.text().to_string().contains('1'))
        .expect("recovery parser must produce the call's argument list");

    let node_result = resolver.resolve_arg_list(&arg_list);
    assert_eq!(node_result, None, "unresolvable callee must return None");

    let cursor = arg_list
        .children()
        .next()
        .map(|child| child.text_range().start())
        .expect("recovery argument list has content");
    assert_eq!(
        resolve_callee_at(&db, file_id, cursor).map(|(callee, _)| callee),
        None,
        "offset entry must agree on the recovery shape",
    );
}
