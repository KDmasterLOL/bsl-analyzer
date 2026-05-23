//! End-to-end regression for `Для каждого … Из …` loop variable
//! type inference.
//!
//! Pins the wiring from [`crate::iteration_lookup::resolve_iter_element_ty`]
//! (covered exhaustively at the unit-test layer in
//! `crates/hir-ty/src/iteration_lookup.rs::tests`) through the Salsa
//! `infer` pipeline so the loop variable lands in
//! `InferenceResult::var_types` and downstream IDE features
//! (hover, goto, diagnostics) see the real element type.
//!
//! Each scenario asserts the lower-cased loop variable name maps to the
//! element type the platform syntax help declares for the receiver.

use hir::{HirDatabase, Name, Ty};
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
    (db, test_file)
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    let id = db.infer(file_id).var_types.get(var_lower).copied()?;
    Some(hir::ty_bridge::typeid_to_ty(db, id))
}

#[test]
fn for_each_over_map_yields_kluch_i_znachenie() {
    // `Соответствие` iterates `КлючИЗначение` per HBK. The inference
    // pipeline must surface that as `Ty::PlatformObject("КлючИЗначение")`
    // on the loop variable so subsequent `.Ключ` / `.Значение` access
    // resolves through platform property lookup.
    let (db, file_id) = setup(
        r#"
//- /test.bsl
Процедура Тест()
    М = Новый Соответствие;
    Для Каждого КЗ Из М Цикл
        Х = КЗ;
    КонецЦикла;
КонецПроцедуры
"#,
    );

    let ty = var_ty(&db, file_id, "кз");
    assert_eq!(
        ty,
        Some(Ty::PlatformObject(Name::new("КлючИЗначение"))),
        "loop var over Соответствие must be КлючИЗначение, got {:?}",
        ty
    );
}

#[test]
fn for_each_over_value_table_yields_row() {
    // `ТаблицаЗначений` iterates `СтрокаТаблицыЗначений` per HBK.
    let (db, file_id) = setup(
        r#"
//- /test.bsl
Процедура Тест()
    Т = Новый ТаблицаЗначений;
    Для Каждого Стр Из Т Цикл
        Х = Стр;
    КонецЦикла;
КонецПроцедуры
"#,
    );

    let ty = var_ty(&db, file_id, "стр");
    assert_eq!(
        ty,
        Some(Ty::PlatformObject(Name::new("СтрокаТаблицыЗначений"))),
        "loop var over ТаблицаЗначений must be СтрокаТаблицыЗначений, got {:?}",
        ty
    );
}

#[test]
fn for_each_over_value_list_yields_list_item() {
    // `СписокЗначений` iterates `ЭлементСпискаЗначений` per HBK.
    let (db, file_id) = setup(
        r#"
//- /test.bsl
Процедура Тест()
    С = Новый СписокЗначений;
    Для Каждого Эл Из С Цикл
        Х = Эл;
    КонецЦикла;
КонецПроцедуры
"#,
    );

    let ty = var_ty(&db, file_id, "эл");
    assert_eq!(
        ty,
        Some(Ty::PlatformObject(Name::new("ЭлементСпискаЗначений"))),
        "loop var over СписокЗначений must be ЭлементСпискаЗначений, got {:?}",
        ty
    );
}

#[test]
fn for_each_over_array_overwrites_prior_binding_with_unknown() {
    // `Массив` iterates `Произвольный` → `Ty::Unknown`. BSL semantics
    // rebinds the loop variable for the duration of the loop and the
    // trailing tail (locals are procedure-scoped, no block scope), so
    // any prior precise type on the same name MUST be shadowed by the
    // loop's `Ty::Unknown` — leaking the prior `Number` into the body
    // would mistype `Эл` for everything inside the loop.
    let (db, file_id) = setup(
        r#"
//- /test.bsl
Процедура Тест()
    А = Новый Массив;
    Эл = 1;
    Для Каждого Эл Из А Цикл
        Х = Эл;
    КонецЦикла;
КонецПроцедуры
"#,
    );

    // Prior `Эл = 1` set var_types["эл"] = Number; the loop overwrote
    // it with the iteration element type (Unknown for Произвольный).
    assert_eq!(var_ty(&db, file_id, "эл"), Some(Ty::Unknown));
}

#[test]
fn for_each_over_string_leaves_var_types_empty() {
    // Plain strings have no `Элементы коллекции:` chapter, so
    // `resolve_iter_element_ty` returns `None` and inference does not
    // touch `var_types` for the loop variable.
    let (db, file_id) = setup(
        r#"
//- /test.bsl
Процедура Тест()
    С = "abc";
    Для Каждого СимВ Из С Цикл
        Х = СимВ;
    КонецЦикла;
КонецПроцедуры
"#,
    );

    assert_eq!(var_ty(&db, file_id, "симв"), None);
}
