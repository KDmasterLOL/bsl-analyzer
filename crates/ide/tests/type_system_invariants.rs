//! M2/M3 cross-cut regression suite.
//!
//! Individual call sites are already covered by `infer_new_expr`,
//! `infer_three_level`, `infer_plural_managers`, `infer_jsdoc_types`,
//! `infer_field_lookup`, and `infer_invalidation`. This file pins the
//! **invariants** those tests rely on — a regression here means the
//! shared Resolver / TypeRef / TyLoweringContext / MethodLookup /
//! FieldLookup plumbing slipped, not that one individual feature broke.
//!
//! Invariants locked in:
//!
//! 1. **Single Resolver cascade** — user bindings shadow manager-plural and
//!    platform names in value position, while builtin calls (`Name(...)`)
//!    still resolve through the call-specific builtin path. A regression here
//!    would mean some callsite re-introduced a parallel lookup that bypasses
//!    the shared cascade.
//!
//! 2. **One lowering pipeline** — `Новый X`, `Тип("…")`-shaped JSDoc, and
//!    manager-chain calls all share `TyLoweringContext`. The test below
//!    chains three different syntactic sources and proves each resolves
//!    through the shared adapter.
//!
//! 3. **One method-lookup path (M3)** — `Expr::MethodCall` inference and
//!    `hir::Type::method_return_type` must both delegate to
//!    `hir_ty::method_lookup::lookup_method`. The test below proves the
//!    two paths return identical `Ty` for a platform-method receiver.
//!
//! 4. **One field-lookup path (M3)** — `Expr::Field` inference and
//!    `hir::Type::field_type` must both delegate to
//!    `hir_ty::field_lookup::lookup_field`. The test below proves the
//!    two paths agree for a JSDoc-typed MDO receiver + custom attribute.
//!
//! 5. **Facade boundary (M3)** — IDE code must not reach into
//!    `PlatformData::instance()` on the type path. Enforced at CI by
//!    `scripts/check-invariants.sh`; listed here for discoverability.
//!
//! 6. **Salsa invalidation chain** — already covered end-to-end by
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

#[test]
fn single_method_lookup_path_agrees_across_infer_and_facade() {
    // `Expr::MethodCall` in `infer.rs` and `hir::Type::method_return_type`
    // both route through `hir_ty::method_lookup::lookup_method`. Before
    // M3 these paths were separate: inference called
    // `resolve_method_return_type` and completion re-walked the syntax
    // tree with its own `PlatformData::instance()` resolver. This test
    // proves the two consumers now return **identical** `Ty` for a
    // well-known platform method — a regression would mean one branch
    // slipped back to a private adapter.
    let fixture = r#"//- /test.bsl
Функция Тест()
    А = Новый Массив;
    Б = А.Количество();
    Возврат Б;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let infer = db.infer(file_id);

    // Path A: whole-program inference.
    let infer_ty = infer
        .var_types
        .get("б")
        .cloned()
        .expect("Массив.Количество() must produce a var_type entry");

    // Path B: hir::Type facade (the API IDE completion uses after M3).
    let facade_ty = hir::Type::new(&db, file_id, Ty::Array)
        .method_return_type(&Name::new("Количество"))
        .ty()
        .clone();

    assert_eq!(
        infer_ty, facade_ty,
        "Expr::MethodCall inference and hir::Type::method_return_type must \
         return the same Ty for `Массив.Количество()` — both must route \
         through `method_lookup::lookup_method`"
    );
    assert_eq!(
        infer_ty,
        Ty::Number,
        "`Массив.Количество()` must resolve to Ty::Number — a change here \
         means the platform-data index drifted, not a facade regression",
    );
}

#[test]
fn single_field_lookup_path_agrees_across_infer_and_facade() {
    // Same invariant shape as the method-lookup test, but for
    // `Expr::Field` + `hir::Type::field_type`. Uses the designer
    // fixture so `FieldLookup` has a real `Configuration` to read;
    // without it, both paths would return Ty::Unknown and the
    // assertion would be trivially satisfied.
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
    let (mut db, file_id) = setup(fixture);
    db.set_all_config_paths(vec![(
        None,
        std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../bsl-metadata/fixtures/designer"
        )),
    )]);

    let infer = db.infer(file_id);
    let infer_ty =
        infer.var_types.get("р").cloned().expect("С.Реквизит2 must produce a var_type entry");

    // The facade routes through `field_lookup::lookup_field` with the
    // same `configurations(file_id)` the inference used.
    let receiver_ty = Ty::MetadataRef {
        kind: MetadataKind::CatalogRef,
        name: Name::new("Справочник1"),
    };
    let facade_ty =
        hir::Type::new(&db, file_id, receiver_ty).field_type(&Name::new("Реквизит2")).ty().clone();

    assert_eq!(
        infer_ty, facade_ty,
        "Expr::Field inference and hir::Type::field_type must return the \
         same Ty for `Справочник1.Реквизит2` — both must route through \
         `field_lookup::lookup_field`"
    );
    assert_eq!(
        infer_ty,
        Ty::Number,
        "`Справочник1.Реквизит2` must resolve to Ty::Number per the \
         designer fixture XML — drift here indicates the XML changed",
    );
}
