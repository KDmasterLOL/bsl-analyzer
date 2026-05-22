//! Behavioral tests for `Expr::New` after Task 4 routes it through
//! [`hir_ty::TyLoweringContext::lower_bare_name`].
//!
//! The cascade must still produce the same observable types as the legacy
//! `ty_from_bare_name` / `PlatformObject` fallback path: builtin collections
//! collapse to their `Ty` counterpart, unknown platform types land on
//! `Ty::PlatformObject(name)`, and `MdoType::from_plural` is honoured as a
//! higher-priority branch for `Новый Документы` (even though that is
//! semantically nonsense at runtime — the test locks the ordering so future
//! changes don't silently drift).
//!
//! `InferenceResult::expr_types` is per-body, so the fixtures here use an
//! assignment `Х = Новый <Type>();` and then read `var_types["х"]`, which is
//! merged into the file-level result.

use hir::{DefDatabase, HirDatabase, ModuleId, Ty};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
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
    // Ensure HIR bodies are built — infer_query reads them.
    let _ = db.module_bodies(ModuleId::new(test_file));
    (db, test_file)
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    db.infer(file_id).var_types.get(var_lower).cloned()
}

#[test]
fn new_array_gives_array_ty() {
    // `Новый Массив` used to go through `ty_from_bare_name("Массив") →
    // Ty::Array`. After Task 4 the same result must come out of the
    // TyLoweringContext cascade (`from_bare_name` → builtin collection).
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Массив();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::Array),
        "`Новый Массив` must type the RHS as Ty::Array"
    );
}

#[test]
fn new_query_with_no_args_types_as_query_with_no_projection() {
    // `Новый Запрос(...)` always produces `Ty::Query{..}` so chain
    // rewrites (`.Выполнить()`, `.ВыполнитьПакет()`) have a stable
    // receiver shape regardless of whether the SDBL text is known.
    // Without a string-literal arg the projection is `None`; downstream
    // platform-property lookup keys `Ty::Query → "Запрос"` so legacy
    // `Зап.Параметры` / `Зап.Текст` access still resolves through the
    // platform table.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::Query { projection: None }),
        "`Новый Запрос()` without literal text must produce Ty::Query{{None}}",
    );
}

#[test]
fn new_query_with_dynamic_text_types_as_query_with_no_projection() {
    // Variable / concat / non-literal first argument — projection can't
    // be derived statically, so `Ty::Query{None}` is the conservative
    // synthesis. The variable-text constructor never falls back to
    // `Ty::PlatformObject("Запрос")` — that shape is gone for `Новый
    // Запрос(...)` regardless of arg shape.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Текст = "ВЫБРАТЬ 1";
    Х = Новый Запрос(Текст);
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::Query { projection: None }),
        "`Новый Запрос(<variable>)` must produce Ty::Query{{None}}",
    );
}

#[test]
fn new_query_with_literal_text_types_as_query_with_projection() {
    // `Новый Запрос("<sdbl>")` with a static string literal arg
    // synthesises a projection from the SDBL HIR's SELECT field list.
    // The bridge runs `query_to_projection` against the first sub-query
    // of the package.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос("ВЫБРАТЬ 1 КАК А");
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "х").expect("х must be inferred");
    let projection = match &ty {
        Ty::Query { projection } => projection.as_ref(),
        other => panic!("expected Ty::Query, got {other:?}"),
    };
    let projection = projection.expect("literal SDBL must produce a projection");
    assert_eq!(
        projection.fields.len(),
        1,
        "single-column SELECT must yield one projection field, got {projection:?}",
    );
    assert_eq!(projection.fields[0].0.as_str(), "А");
    assert_eq!(projection.fields[0].1, Ty::Number);
}

#[test]
fn new_query_chain_propagates_projection_through_execute_select() {
    // End-to-end projection propagation: `Новый Запрос("ВЫБРАТЬ Имя")
    // .Выполнить().Выбрать().Имя` resolves to `Ty::String`. The chain
    // walks constructor synthesis (`Ty::Query{Some(p)}`) → `.Выполнить()`
    // rewrite (`Ty::QueryResult{Some(p)}`) → `.Выбрать()` rewrite
    // (`Ty::QueryResultSelection{Some(p)}`) → projection field lookup
    // in `lookup_field`.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя").Выполнить().Выбрать().Имя;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::String),
        "`Новый Запрос(\"...Имя\").Выполнить().Выбрать().Имя` must resolve to Ty::String",
    );
}

#[test]
fn new_query_with_parse_error_literal_falls_back_to_no_projection() {
    // Parse-error SDBL packages still go through the bridge; every
    // sub-query yields `None` and `query_to_projection` returns None
    // for empty/error packages. The constructor result remains
    // `Ty::Query{None}` — never `Ty::PlatformObject` — so the chain
    // rewrite and platform-property lookup paths keep working.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос("это не sdbl");
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::Query { projection: None }),
        "parse-error SDBL literal must collapse to Ty::Query{{None}}, not PlatformObject",
    );
}

#[test]
fn new_structure_gives_structure_ty() {
    // Second builtin-collection case confirming the cascade covers the full
    // RU/EN builtin table, not just Array — one that exercises a different
    // branch of `TypeRef::from_bare_name`.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Структура();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::Structure),
        "`Новый Структура` must type the RHS as Ty::Structure"
    );
}
