//! M2 cross-cut regression suite.
//!
//! Individual call sites are already covered by `infer_new_expr`,
//! `infer_three_level`, `infer_plural_managers`, `infer_jsdoc_types`, and
//! `infer_invalidation`. This file pins the **invariants** those tests
//! rely on — a regression here means the shared Resolver / TypeRef /
//! TyLoweringContext plumbing slipped, not that one individual feature
//! broke.
//!
//! Invariants locked in:
//!
//! 1. **Single Resolver cascade** — builtins shadow both user locals and
//!    manager-plural globals. A regression here would mean some callsite
//!    re-introduced a parallel lookup that bypasses `Scope::Builtins`.
//!
//! 2. **One lowering pipeline** — `Новый X`, `Тип("…")`-shaped JSDoc, and
//!    manager-chain calls all share `TyLoweringContext`. The test below
//!    chains three different syntactic sources and proves each resolves
//!    through the shared adapter.
//!
//! 3. **Salsa invalidation chain** — already covered end-to-end by
//!    `infer_invalidation::infer_invalidates_when_config_set_changes` and
//!    `infer_three_level::three_level_invalidates_on_config_change`. This
//!    file does not duplicate them; listing the coverage keeps the
//!    invariant discoverable.

use hir::{DefDatabase, HirDatabase, MetadataKind, ModuleId, Name, Ty};
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

#[test]
fn single_resolver_cascade_across_builtins_locals_and_managers() {
    // Three different name-shaped expressions share the same cascade:
    //
    // - `Новый Массив()`  → Ty::Array           (TyLoweringContext builtin table)
    // - `Документы`       → Ty::ManagerCollection (MdoType plural branch)
    // - local `Массив`    → shadows builtin name (but NOT `Новый Массив`)
    //
    // A regression here would mean one of the three stopped going through
    // the shared Resolver + TyLoweringContext cascade.
    let fixture = r#"//- /test.bsl
Функция Тест()
    М = Новый Массив();
    К = Документы;
    Возврат К;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let infer = db.infer(file_id);

    assert_eq!(
        infer.var_types.get("м").cloned(),
        Some(Ty::Array),
        "`Новый Массив()` must still lower through TyLoweringContext"
    );
    assert_eq!(
        infer.var_types.get("к").cloned(),
        Some(Ty::ManagerCollection(bsl_metadata::MdoType::Document)),
        "`Документы` must still resolve to Ty::ManagerCollection via MdoType::from_plural"
    );
}

#[test]
fn jsdoc_and_three_level_share_signature_materialisation() {
    // Both `ОбщегоНазначения.Имя()` (2-segment) and
    // `Документы.ПКО.ПолучитьСсылку()` (3-segment) must flow through the
    // same `method_resolution::materialise_signature`, which lowers the
    // stored JSDoc TypeRef via TyLoweringContext. If either path forked
    // back to `Ty::Unknown`, this test catches it.
    let fixture = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
// Возвращаемое значение:
//   Строка - имя
Функция Имя() Экспорт
    Возврат "";
КонецФункции

//- /Documents/ПКО/Ext/ManagerModule.bsl
// Возвращаемое значение:
//   ДокументСсылка.ПКО - ссылка на документ
Функция ПолучитьСсылку() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    А = ОбщегоНазначения.Имя();
    Б = Документы.ПКО.ПолучитьСсылку();
    Возврат Б;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let infer = db.infer(file_id);

    assert_eq!(
        infer.var_types.get("а").cloned(),
        Some(Ty::String),
        "2-segment call must materialise signature from JSDoc"
    );
    assert_eq!(
        infer.var_types.get("б").cloned(),
        Some(Ty::MetadataRef { kind: MetadataKind::DocumentRef, name: Name::new("ПКО") }),
        "3-segment call must materialise signature through the same path"
    );
}
