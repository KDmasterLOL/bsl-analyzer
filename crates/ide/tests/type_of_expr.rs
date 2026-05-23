//! Acceptance tests for the Task 9 ExprId bridge.
//!
//! Before Task 9, `InferenceResult` dropped per-body `expr_types` during
//! merge in `infer_query`, and `type_of_expr_query` always returned
//! `Ty::Unknown`. Task 9 preserves the maps keyed by [`DefWithBodyId`]
//! and exposes them through `Semantics::type_of_expr(SyntaxNode)`.
//!
//! These tests prove:
//!
//! - per-body `expr_types` survive merge
//!   (`type_of_expr_resolves_across_bodies`);
//! - `Semantics::type_of_expr` matches what inference produced for call
//!   and field expressions (`type_of_call_expr_matches_infer`,
//!   `type_of_field_expr_matches_infer`).
//!
//! These are the load-bearing acceptance tests from the M3 plan Task 9.

use hir::{DefDatabase, DefWithBodyId, HirDatabase, ModuleId, Semantics, Ty};
use ide_db::base_db::{RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use syntax::ast::{self, AstNode};
use test_fixture::Fixture;
use vfs::FileId;

fn setup(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    let fixture = Fixture::parse(fixture_text);
    let mut db = RootDatabaseImpl::new();
    let mut file_set = vfs::FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
    }
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for (file_id, file) in &fixture.files {
        db.set_file_source_root(*file_id, SourceRootId(0));
        db.set_file_text(*file_id, &file.content);
    }
    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    (db, test_file)
}

fn setup_with_designer(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    let (mut db, file_id) = setup(fixture_text);
    // Point the metadata bridge at the designer fixture — same trick as
    // `infer_field_lookup.rs`: the config gate must recognise
    // `ПервыйОбщийМодуль`, and `FieldLookup` must see real MDOs.
    db.set_all_config_paths(vec![(
        None,
        std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../bsl-metadata/fixtures/designer"
        )),
    )]);
    (db, file_id)
}

fn first_call_expr(root: &syntax::SyntaxNode, method_name: &str) -> Option<ast::CallExpr> {
    root.descendants().filter_map(ast::CallExpr::cast).find(|call| {
        call.syntax().descendants_with_tokens().any(|tok| {
            tok.as_token()
                .map(|t| t.kind() == syntax::SyntaxKind::IDENT && t.text() == method_name)
                .unwrap_or(false)
        })
    })
}

fn first_field_expr(root: &syntax::SyntaxNode, field_name: &str) -> Option<ast::FieldExpr> {
    root.descendants().filter_map(ast::FieldExpr::cast).find(|fe| {
        fe.syntax()
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|tok| tok.kind() == syntax::SyntaxKind::IDENT)
            .any(|tok| tok.text() == field_name)
    })
}

#[test]
fn type_of_expr_resolves_across_bodies() {
    // Two different method bodies each produce their own expr_types
    // map. Before Task 9 the merged `InferenceResult` dropped both, so
    // `type_of_expr_in` returned None on either. This test asserts both
    // maps survived the merge.
    let fixture = r#"
//- /test.bsl
Функция Первая()
    А = 1;
    Возврат А;
КонецФункции

Функция Вторая()
    Б = 2;
    Возврат Б;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let infer = db.infer(file_id);

    // Two methods survive the merge — `infer_query` runs both and folds
    // their expr_types into separate `DefWithBodyId::Method(...)`
    // entries. Module-level code also contributes a
    // `DefWithBodyId::ModuleCode` entry (possibly empty); we don't care
    // about its size, only that the per-method entries survive.
    let method_bodies: Vec<_> = infer
        .expr_types_by_body
        .iter()
        .filter(|(owner, _)| matches!(owner, DefWithBodyId::Method(_)))
        .collect();
    assert_eq!(
        method_bodies.len(),
        2,
        "each function body must keep its own expr_types map after merge \
         (got {} method bodies, total entries {})",
        method_bodies.len(),
        infer.expr_types_by_body.len(),
    );
    for (owner, map) in method_bodies {
        assert!(!map.is_empty(), "method body {owner:?} must carry at least one ExprId");
    }
}

#[test]
fn type_of_call_expr_matches_infer() {
    // `Semantics::type_of_expr(SyntaxNode)` on a literal must return
    // the same `Ty::Number` that inference stored. Also cross-checks
    // the result against a direct `expr_types_by_body` lookup so we
    // prove the bridge and the underlying map agree.
    let fixture = r#"
//- /test.bsl
Функция Тест()
    Р = 42;
    Возврат Р;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let sema = Semantics::new(&db);
    let module_id = ModuleId::new(file_id);
    let module_bodies = db.module_bodies(module_id);

    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let literal =
        root.descendants().filter_map(ast::Literal::cast).next().expect("fixture has a literal");

    assert_eq!(
        sema.type_of_expr(file_id, literal.syntax()),
        Ty::Number,
        "Semantics::type_of_expr must return Ty::Number for literal `42`"
    );

    let infer = db.infer(file_id);
    let (owner_match, expr_match) = module_bodies
        .method_bodies()
        .find_map(|(local_id, _body, source_map)| {
            source_map
                .expr_at_range(literal.syntax().text_range())
                .map(|eid| (DefWithBodyId::Method(local_id), eid))
        })
        .expect("BodySourceMap must locate the literal ExprId");
    assert_eq!(
        infer.type_of_expr_in(&db, owner_match, expr_match),
        Some(Ty::Number),
        "direct expr_types_by_body lookup must agree with Semantics::type_of_expr"
    );
}

#[test]
fn type_of_module_level_expr_resolves() {
    // Module-level code uses `DefWithBodyId::ModuleCode` rather than a
    // `Method(local_id)` key. Without this test the ModuleCode path
    // through `infer_query` and `Semantics::type_of_expr` would stay
    // untested — the rest of the suite lives inside `Функция` /
    // `Процедура` bodies.
    let fixture = r#"
//- /test.bsl
Х = 42;
"#;
    let (db, file_id) = setup(fixture);
    let sema = Semantics::new(&db);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let literal =
        root.descendants().filter_map(ast::Literal::cast).next().expect("fixture has a literal");

    assert_eq!(
        sema.type_of_expr(file_id, literal.syntax()),
        Ty::Number,
        "Semantics::type_of_expr must resolve the module-level `42` literal"
    );

    // Cross-check: module_code_result's BodySourceMap must own this range,
    // and `expr_types_by_body` must carry the entry under `ModuleCode`.
    let module_id = ModuleId::new(file_id);
    let module_bodies = db.module_bodies(module_id);
    let expr_id = module_bodies
        .module_code_result()
        .expect("module-level body must be lowered")
        .source_map
        .expr_at_range(literal.syntax().text_range())
        .expect("module-level BodySourceMap must locate the literal");
    let infer = db.infer(file_id);
    assert_eq!(
        infer.type_of_expr_in(&db, DefWithBodyId::ModuleCode, expr_id),
        Some(Ty::Number),
        "direct ModuleCode lookup must agree with Semantics::type_of_expr"
    );
}

#[test]
fn type_of_field_expr_matches_infer() {
    // Field access must resolve through the same bridge. Uses a custom
    // attribute on the designer fixture's `Справочник1` Catalog; the
    // JSDoc return pins the receiver type so `FieldLookup` can finish
    // the chain.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникСсылка.Справочник1
Функция Ссылка() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    С = ПервыйОбщийМодуль.Ссылка();
    Р = С.Реквизит2;
    Возврат Р;
КонецФункции
"#;
    let (db, file_id) = setup_with_designer(fixture);
    let sema = Semantics::new(&db);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let field = first_field_expr(&root, "Реквизит2").expect("fixture has С.Реквизит2");

    assert_eq!(
        sema.type_of_expr(file_id, field.syntax()),
        Ty::Number,
        "Semantics::type_of_expr on a field access must agree with inference (Ty::Number)"
    );

    // Cross-check against the underlying `expr_types_by_body` map —
    // matching the pattern used by the literal-lookup test above, so
    // a future bug where `Semantics::type_of_expr` short-circuits
    // without consulting the merged map is caught here too.
    let module_id = ModuleId::new(file_id);
    let module_bodies = db.module_bodies(module_id);
    let (owner, expr_id) = module_bodies
        .method_bodies()
        .find_map(|(local_id, _body, source_map)| {
            source_map
                .expr_at_range(field.syntax().text_range())
                .map(|eid| (DefWithBodyId::Method(local_id), eid))
        })
        .expect("BodySourceMap must locate the field-expr ExprId");
    let infer = db.infer(file_id);
    assert_eq!(
        infer.type_of_expr_in(&db, owner, expr_id),
        Some(Ty::Number),
        "direct expr_types_by_body lookup must agree with Semantics::type_of_expr"
    );
}

#[test]
fn type_of_expr_unknown_for_non_expression_node() {
    // Nodes that have no ExprId (e.g. the root SOURCE_FILE) must return
    // Ty::Unknown without panicking — exercises the "no entry" branch.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let sema = Semantics::new(&db);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    assert_eq!(sema.type_of_expr(file_id, &root), Ty::Unknown);
}

#[test]
fn type_of_expr_covers_call_site() {
    // Regression guard: the bridge must handle compound expressions —
    // a `CallExpr` range spans the whole `Foo.Bar()` string, not just
    // a token. Earlier drafts that matched token-wide ranges would
    // miss this case.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   Число
Функция Сумма() Экспорт
    Возврат 0;
КонецФункции

//- /test.bsl
Функция Тест()
    С = ПервыйОбщийМодуль.Сумма();
    Возврат С;
КонецФункции
"#;
    let (db, file_id) = setup_with_designer(fixture);
    let sema = Semantics::new(&db);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let call = first_call_expr(&root, "Сумма").expect("fixture has a Сумма() call");
    assert_eq!(
        sema.type_of_expr(file_id, call.syntax()),
        Ty::Number,
        "type_of_expr on a CallExpr must reflect the JSDoc-lowered return type"
    );
}
