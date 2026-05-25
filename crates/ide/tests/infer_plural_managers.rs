//! Behavioural tests for plural-form MDO globals after Task 8.
//!
//! `Документы`, `Справочники`, and the other manager collectives must
//! resolve to [`hir::Ty::ManagerCollection`] in `infer_path_name`. Implicit
//! locals still shadow manager collectives — BSL lets a user write
//! `Документы = 42;` to rebind the identifier, and the inference cascade
//! must honour that.

use bsl_metadata::MdoType;
use hir::{Builders, DefDatabase, HirDatabase, ModuleId, TypeId};
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

#[test]
fn bare_documenty_yields_manager_collection() {
    // Plural global `Документы` lowers to `Ty::ManagerCollection(Document)`
    // when no local variable shadows it — positive proof the cascade step
    // added in Task 8 fires between var_types and module-level resolution.
    let fixture = r#"//- /test.bsl
Функция Тест()
    М = Документы;
    Возврат М;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "м"),
        Some(db.manager_collection(MdoType::Document)),
        "`Документы` must lower to Ty::ManagerCollection(Document)"
    );
}

#[test]
fn bare_spravochniki_yields_manager_collection() {
    // Confirms the cascade covers more than Documents — `Справочники`
    // must reach the MdoType::from_plural branch too.
    let fixture = r#"//- /test.bsl
Функция Тест()
    С = Справочники;
    Возврат С;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "с"),
        Some(db.manager_collection(MdoType::Catalog)),
        "`Справочники` must lower to Ty::ManagerCollection(Catalog)"
    );
}

#[test]
fn implicit_local_shadows_manager_plural() {
    // BSL's assignment-as-declaration means `Документы = 42` rebinds the
    // name. The cascade must resolve through `var_types` before the MDO
    // plural branch, so `М = Документы` picks up Number, not
    // ManagerCollection. If this fails the cascade ordering regressed.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Документы = 42;
    М = Документы;
    Возврат М;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "м"),
        Some(db.number(None, None)),
        "local `Документы = 42` must shadow the manager collective"
    );
}
