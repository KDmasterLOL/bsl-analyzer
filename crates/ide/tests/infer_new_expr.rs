use hir::{Builders, DefDatabase, HirDatabase, ModuleId, TypeId, TypeKernelDb, TypeKind};
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
    let _ = db.module_bodies(ModuleId::new(test_file));
    (db, test_file)
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<TypeId> {
    db.infer(file_id).var_types.get(var_lower).copied()
}

fn query_no_projection(db: &RootDatabaseImpl, ty: TypeId) -> bool {
    match db.lookup_type(ty) {
        TypeKind::Query { projections } => projections.iter().all(Option::is_none),
        _ => false,
    }
}

#[test]
fn new_array_gives_array_ty() {
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Массив();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(db.array(None)),
        "`Новый Массив` must type the RHS as Ty::Array"
    );
}

#[test]
fn new_query_with_no_args_types_as_query_with_no_projection() {
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "х").expect("х must be inferred");
    assert!(
        query_no_projection(&db, ty),
        "`Новый Запрос()` without literal text must produce Ty::Query with no projection, got {ty:?}",
    );
}

#[test]
fn new_query_with_dynamic_text_types_as_query_with_no_projection() {
    let fixture = r#"//- /test.bsl
Функция Тест()
    Текст = "ВЫБРАТЬ 1";
    Х = Новый Запрос(Текст);
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "х").expect("х must be inferred");
    assert!(
        query_no_projection(&db, ty),
        "`Новый Запрос(<variable>)` must produce Ty::Query with no projection, got {ty:?}",
    );
}

#[test]
fn new_query_with_literal_text_types_as_query_with_projection() {
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос("ВЫБРАТЬ 1 КАК А");
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "х").expect("х must be inferred");
    let projections = match db.lookup_type(ty) {
        TypeKind::Query { projections } => projections.clone(),
        other => panic!("expected Ty::Query, got {other:?}"),
    };
    assert_eq!(
        projections.len(),
        1,
        "single-query package must yield one slice entry, got {projections:?}",
    );
    let projection = projections[0].as_ref().expect("literal SDBL must produce a projection");
    assert_eq!(
        projection.fields.len(),
        1,
        "single-column SELECT must yield one projection field, got {projection:?}",
    );
    assert_eq!(projection.fields[0].name.as_str(), "А");
    assert_eq!(projection.fields[0].ty, db.number(None, None));
}

#[test]
fn new_query_chain_propagates_projection_through_execute_select() {
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя").Выполнить().Выбрать().Имя;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(db.string(None, false)),
        "`Новый Запрос(\"...Имя\").Выполнить().Выбрать().Имя` must resolve to Ty::String",
    );
}

#[test]
fn new_query_with_parse_error_literal_falls_back_to_no_projection() {
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос("это не sdbl");
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "х").expect("х must be inferred");
    assert!(
        query_no_projection(&db, ty),
        "parse-error SDBL literal must collapse to Ty::Query with no projection, not PlatformObject — got {ty:?}",
    );
}

#[test]
fn execute_batch_literal_zero_index_yields_first_subquery_projection() {
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос("ВЫБРАТЬ 1 КАК ПерваяКолонка; ВЫБРАТЬ ""abc"" КАК ВтораяКолонка").ВыполнитьПакет()[0];
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "х").expect("х must be inferred");
    let projection = match db.lookup_type(ty) {
        TypeKind::QueryResult(facet) => facet.projection.as_ref(),
        other => panic!("expected Ty::QueryResult, got {other:?}"),
    };
    let projection = projection.expect("batch[0] must carry the first sub-query's projection");
    assert_eq!(projection.fields.len(), 1);
    assert_eq!(projection.fields[0].name.as_str(), "ПерваяКолонка");
    assert_eq!(projection.fields[0].ty, db.number(None, None));
}

#[test]
fn execute_batch_literal_one_index_yields_second_subquery_projection() {
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос("ВЫБРАТЬ 1 КАК ПерваяКолонка; ВЫБРАТЬ ""abc"" КАК ВтораяКолонка").ВыполнитьПакет()[1];
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "х").expect("х must be inferred");
    let projection = match db.lookup_type(ty) {
        TypeKind::QueryResult(facet) => facet.projection.as_ref(),
        other => panic!("expected Ty::QueryResult, got {other:?}"),
    };
    let projection = projection.expect("batch[1] must carry the second sub-query's projection");
    assert_eq!(projection.fields[0].name.as_str(), "ВтораяКолонка");
    assert_eq!(projection.fields[0].ty, db.string(None, false));
}

#[test]
fn execute_batch_out_of_range_index_yields_no_projection() {
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос("ВЫБРАТЬ 1 КАК А").ВыполнитьПакет()[5];
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "х").expect("х must be inferred");
    assert!(
        matches!(db.lookup_type(ty), TypeKind::QueryResult(facet) if facet.projection.is_none()),
        "out-of-range batch index must yield Ty::QueryResult{{None}}, got {:?}",
        db.lookup_type(ty)
    );
}

#[test]
fn execute_batch_dynamic_index_yields_no_projection() {
    let fixture = r#"//- /test.bsl
Функция Тест()
    Индекс = 0;
    Х = Новый Запрос("ВЫБРАТЬ 1 КАК А").ВыполнитьПакет()[Индекс];
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "х").expect("х must be inferred");
    assert!(
        matches!(db.lookup_type(ty), TypeKind::QueryResult(facet) if facet.projection.is_none()),
        "non-literal batch index must yield Ty::QueryResult{{None}}, got {:?}",
        db.lookup_type(ty)
    );
}

#[test]
fn execute_batch_chain_propagates_through_select() {
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос("ВЫБРАТЬ 1 КАК А; ВЫБРАТЬ ""abc"" КАК Имя").ВыполнитьПакет()[1].Выбрать().Имя;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(db.string(None, false)),
        "batch[1].Выбрать().Имя must resolve to Ty::String",
    );
}

#[test]
fn new_structure_gives_structure_ty() {
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Структура();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(db.structure(None)),
        "`Новый Структура` must type the RHS as Ty::Structure"
    );
}
