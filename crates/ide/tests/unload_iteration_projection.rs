//! Phase H Slice 3 + end-to-end — projected `Ty::ValueTable` iterates
//! to a projected `Ty::ValueTableRow`, and `Стр.<column>` resolves to
//! the SDBL-bridged column type.
//!
//! This is the ERP idiom the phase plan is motivated by:
//!
//! ```bsl
//! Функция ПолучитьТЗ()
//!     Зап = Новый Запрос("ВЫБРАТЬ Имя, Цена ИЗ Справочник.Товары");
//!     Возврат Зап.Выполнить().Выгрузить();
//! КонецФункции
//!
//! Процедура Тест()
//!     ТЗ = ПолучитьТЗ();
//!     Для Каждого Стр Из ТЗ Цикл
//!         Х = Стр.Имя;   // ← must resolve through the projection
//!     КонецЦикла;
//! КонецПроцедуры
//! ```

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
    let _ = db.module_bodies(ModuleId::new(test_file));
    (db, test_file)
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    let id = db.infer(file_id).var_types.get(var_lower).copied()?;
    Some(hir::ty_bridge::typeid_to_ty(db, id))
}

#[test]
fn for_each_over_projected_value_table_yields_projected_row() {
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Зап = Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя");
    ТЗ = Зап.Выполнить().Выгрузить();
    Для Каждого Стр Из ТЗ Цикл
        Х = Стр;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let row_ty = var_ty(&db, file_id, "стр").expect("Стр must be inferred");
    let Ty::ValueTableRow { projection: Some(p) } = row_ty else {
        panic!("Стр must be Ty::ValueTableRow {{ projection: Some(..) }}, got {row_ty:?}");
    };
    assert_eq!(
        p.fields.iter().map(|(n, _)| n.as_str().to_string()).collect::<Vec<_>>(),
        vec!["Имя".to_string()],
    );
}

#[test]
fn projected_row_column_resolves_via_projection() {
    // `Стр.Имя` where Имя is a string literal alias should resolve
    // to `Ty::String` via the projection lookup, not via the
    // platform `СтрокаТаблицыЗначений` table.
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Зап = Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя");
    ТЗ = Зап.Выполнить().Выгрузить();
    Для Каждого Стр Из ТЗ Цикл
        Х = Стр.Имя;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let x_ty = var_ty(&db, file_id, "х").expect("Х must be inferred");
    assert!(
        matches!(x_ty, Ty::String),
        "Стр.Имя must resolve to Ty::String via projection — got {x_ty:?}",
    );
}

#[test]
fn helper_function_propagates_projection_through_unload_and_iteration() {
    // ERP-canonical shape: helper builds Запрос + Выгрузить inline,
    // caller iterates the result. Phase B constructor synthesis +
    // Phase J method-graph return-type inference + Phase H narrowing
    // compose to make `Стр.Имя` typed as Ty::String inside the loop.
    let fixture = r#"//- /test.bsl
Функция ПолучитьТЗ()
    Зап = Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя");
    Возврат Зап.Выполнить().Выгрузить();
КонецФункции

Процедура Тест()
    ТЗ = ПолучитьТЗ();
    Для Каждого Стр Из ТЗ Цикл
        Х = Стр.Имя;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let x_ty = var_ty(&db, file_id, "х").expect("Х must be inferred");
    assert!(
        matches!(x_ty, Ty::String),
        "helper-returned ТЗ's row.Имя must resolve via projection — got {x_ty:?}",
    );
}

#[test]
fn projection_less_value_table_keeps_platform_row() {
    // `Новый ТаблицаЗначений` has no projection; iteration must
    // still produce the platform `СтрокаТаблицыЗначений` row, not
    // an empty `Ty::ValueTableRow { None }` that would hide the
    // platform members.
    let fixture = r#"//- /test.bsl
Процедура Тест()
    ТЗ = Новый ТаблицаЗначений;
    Для Каждого Стр Из ТЗ Цикл
        Х = Стр;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let row_ty = var_ty(&db, file_id, "стр").expect("Стр must be inferred");
    // The legacy path returns `Ty::PlatformObject("СтрокаТаблицыЗначений")`
    // (platform-template iteration). The contract is "platform row,
    // no projection enrichment".
    match row_ty {
        Ty::PlatformObject(ref n) if n.as_str() == "СтрокаТаблицыЗначений" => {}
        // If a future refactor wires projection-None ValueTable to
        // `Ty::ValueTableRow { None }` directly, that's also acceptable
        // — the field surface would still match through `platform_type_name`.
        Ty::ValueTableRow { projection: None } => {}
        other => panic!(
            "non-projected ТЗ row must be platform СтрокаТаблицыЗначений (or its dedicated variant), got {other:?}",
        ),
    }
}
