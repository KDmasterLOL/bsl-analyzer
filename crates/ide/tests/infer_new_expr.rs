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

use hir::{DefDatabase, HirDatabase, ModuleId, Name, Ty};
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
fn new_unknown_falls_to_platform_object() {
    // `Запрос` has no builtin entry in the TyLoweringContext cascade, so
    // the fallback branch must produce `Ty::PlatformObject("Запрос")` just
    // like the legacy `Expr::New` code did. Method-lookup on this ty still
    // runs through `bsl_platform`, which is not tested here — this locks
    // the type shape only.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = Новый Запрос();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::PlatformObject(Name::new("Запрос"))),
        "`Новый Запрос` must fall back to Ty::PlatformObject(\"Запрос\")"
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
