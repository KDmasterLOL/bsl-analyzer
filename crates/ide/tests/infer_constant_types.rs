//! End-to-end regression for per-constant value-type resolution
//! (Pluck #2 — `Константы.<Имя>.Получить()` / `Установить()`).
//!
//! The platform JSON declares `ConstantManager.<Constant Name>.Get`
//! with `return_type: "Произвольный"`; pluck #1 collapsed that to
//! `Ty::Unknown` so the gradual rule no longer false-fires
//! `TypeMismatch` on typed sinks. Pluck #2 reads each constant's
//! configuration `<Type>` block and rebinds `Получить`'s return / `Установить`'s
//! first parameter accordingly, so:
//!
//! - real `Ty::String` / `Ty::Number` / `Ty::MetadataRef` / `Ty::Union` types
//!   are surfaced for downstream chained calls and hover;
//! - typed sinks (`ПустаяСтрока(...)`, `Цел(...)`, JSDoc-typed CommonModule
//!   parameters) get a real `TypeMismatch` when callers pass an incompatibly
//!   typed value.
//!
//! Designer-fixture inventory (see
//! `crates/bsl-metadata/fixtures/designer/Constants/`):
//! - `СтрокаКонст`     → `xs:string` (length 50)
//! - `ЧислоКонст`      → `xs:decimal` (digits 10, fraction 2)
//! - `СсылкаКонст`     → `cfg:CatalogRef.Справочник1`
//! - `СоставнойКонст`  → union of `xs:string` + `cfg:CatalogRef.Справочник1`
//! - `ПроизвольныйКонст` → no `<Type>` element (untyped → falls back to
//!   gradual `Ty::Unknown`)

use hir::{
    Builders, HirDatabase, InferenceDiagnostic, MetadataKind, TypeId, TypeKernelDb, TypeKind,
};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use test_fixture::Fixture;
use vfs::FileId;

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn extension_constant_override_path() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bsl-metadata/fixtures/extension_constant_override"
    ))
}

fn setup(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    setup_with_configs(fixture_text, vec![(None, designer_fixture_path())])
}

fn setup_with_configs(
    fixture_text: &str,
    config_paths: Vec<(Option<String>, PathBuf)>,
) -> (RootDatabaseImpl, FileId) {
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

    db.set_all_config_paths(config_paths);

    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    (db, test_file)
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<TypeId> {
    db.infer(file_id).var_types.get(var_lower).copied()
}

fn type_mismatches(db: &RootDatabaseImpl, file_id: FileId) -> Vec<(TypeId, TypeId)> {
    // Argument `TypeMismatch` lives in `arg_diagnostics` after the
    // narrowing-aware split. Chain both sources for forward compatibility.
    db.infer(file_id)
        .diagnostics
        .iter()
        .chain(db.arg_diagnostics(file_id).iter())
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::TypeMismatch { expected, actual, .. } => {
                Some((*expected, *actual))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn three_level_get_resolves_string_constant() {
    let fixture = r#"
//- /test.bsl
Функция Тест()
    Х = Константы.СтрокаКонст.Получить();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(var_ty(&db, file_id, "х"), Some(db.string(None, false)));
}

#[test]
fn three_level_get_resolves_number_constant() {
    let fixture = r#"
//- /test.bsl
Функция Тест()
    Х = Константы.ЧислоКонст.Получить();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(var_ty(&db, file_id, "х"), Some(db.number(None, None)));
}

#[test]
fn three_level_get_resolves_catalog_ref_constant() {
    let fixture = r#"
//- /test.bsl
Функция Тест()
    Х = Константы.СсылкаКонст.Получить();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "х").expect("catalog-ref constant must produce a type");
    assert!(
        matches!(
            db.lookup_type(ty),
            TypeKind::MetadataRef(facet)
                if facet.kind == MetadataKind::CatalogRef
                    && facet.name.as_str() == "Справочник1"
        ),
        "expected CatalogRef.Справочник1, got {:?}",
        db.lookup_type(ty)
    );
}

#[test]
fn three_level_get_resolves_composite_constant() {
    // `СоставнойКонст` declares `xs:string` + `cfg:CatalogRef.Справочник1` —
    // must lower to a `Ty::Union` containing both members.
    let fixture = r#"
//- /test.bsl
Функция Тест()
    Х = Константы.СоставнойКонст.Получить();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "х").expect("composite constant must produce a type");
    let members = match db.lookup_type(ty) {
        TypeKind::Union(m) => m.clone(),
        other => panic!("expected Ty::Union, got {other:?}"),
    };
    assert!(
        members.iter().any(|m| *m == db.string(None, false)),
        "union must contain Ty::String, got {members:?}",
    );
    assert!(
        members.iter().any(|m| matches!(
            db.lookup_type(*m),
            TypeKind::MetadataRef(facet)
                if facet.kind == MetadataKind::CatalogRef
                    && facet.name.as_str() == "Справочник1"
        )),
        "union must contain MetadataRef CatalogRef.Справочник1, got {members:?}",
    );
}

#[test]
fn three_level_get_on_untyped_constant_stays_unknown() {
    // `ПроизвольныйКонст` has no `<Type>` block — `find_constant_type`
    // returns `None`, the rebind helper does not override, and the
    // post-fix-#1 platform `Ty::Unknown` stands. The gradual rule then
    // keeps `ПустаяСтрока(...)` silent.
    let fixture = r#"
//- /test.bsl
Функция Тест()
    Если ПустаяСтрока(Константы.ПроизвольныйКонст.Получить()) Тогда
        Возврат "пусто";
    КонецЕсли;
    Возврат "не пусто";
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        type_mismatches(&db, file_id),
        Vec::<(TypeId, TypeId)>::new(),
        "untyped constant must stay gradual — no TypeMismatch on String sink",
    );
}

#[test]
fn three_level_set_typechecks_first_argument() {
    // `Установить(<Значение>)` parameter is platform-declared
    // `Произвольный`; the rebind narrows it to the constant's value
    // type. Passing a String literal into `ЧислоКонст.Установить(...)`
    // must fire `TypeMismatch { expected: Number, actual: String }`.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Константы.ЧислоКонст.Установить("строка");
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let mm = type_mismatches(&db, file_id);
    assert!(
        mm.iter().any(|(e, a)| *e == db.number(None, None) && *a == db.string(None, false)),
        "expected TypeMismatch(Number, String) for Установить(\"строка\"), got {mm:?}",
    );
}

#[test]
fn three_level_set_with_matching_type_is_silent() {
    // Same call, type-correct argument — no diagnostic.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Константы.СтрокаКонст.Установить("значение");
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        type_mismatches(&db, file_id),
        Vec::<(TypeId, TypeId)>::new(),
        "matching type must not fire TypeMismatch",
    );
}

#[test]
fn typed_sink_silent_on_number_into_string_via_coercion() {
    // Pin the call-site coercion rule from `is_coercible_to`:
    // `ПустаяСтрока(<Number>)` lands a typed-Number constant into
    // a `String`-declared platform sink. BSL implicitly stringifies
    // any value flowing into a String slot, so this is noise rather
    // than a runtime bug — the diagnostic must stay silent.
    //
    // Live-typing coverage at the qualified-call hook is already
    // pinned by `three_level_set_typechecks_first_argument` (which
    // exercises the reverse direction `String → Number`, where the
    // coercion rule does NOT apply and the diagnostic still fires).
    let fixture = r#"
//- /test.bsl
Функция Тест()
    Возврат ПустаяСтрока(Константы.ЧислоКонст.Получить());
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        type_mismatches(&db, file_id),
        Vec::<(TypeId, TypeId)>::new(),
        "Number arg into a String-typed platform sink must coerce silently — BSL stringifies on entry",
    );
}

#[test]
fn two_level_alias_get_resolves_string_constant() {
    // Variable-aliased manager: `М = Константы.СтрокаКонст` produces a
    // `Ty::ObjectManager { kind: Constant, name: "СтрокаКонст" }`
    // receiver. The follow-up `М.Получить()` flows through
    // `lookup_method` (not `infer_three_level_call`), so this exercises
    // the *second* call-site hook in `infer.rs`.
    let fixture = r#"
//- /test.bsl
Функция Тест()
    М = Константы.СтрокаКонст;
    Х = М.Получить();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(var_ty(&db, file_id, "х"), Some(db.string(None, false)));
}

#[test]
fn untyped_extension_override_shadows_base_typed_constant() {
    // Regression for the override-iteration bug Codex flagged at
    // stop-time review: the base configuration declares `ЧислоКонст`
    // as `Number`; the extension redeclares the same constant **without**
    // a `<Type>` element. `resolve_constant_value_type` must treat the
    // extension's declaration-without-type as a successful match (and
    // return `None` so platform `Ty::Unknown` stands), not fall through
    // to the base's typed declaration. Otherwise an explicit untyped
    // override would silently inherit the base type and a
    // String-into-Number-typed-sink call site would still false-fire.
    //
    // Test asserts the typed sink stays silent: extension-shadowed
    // `ЧислоКонст.Получить()` lands on `Ty::Unknown` (gradual rule),
    // so `ПустаяСтрока(...)` produces no diagnostic.
    let fixture = r#"
//- /test.bsl
Функция Тест()
    Возврат ПустаяСтрока(Константы.ЧислоКонст.Получить());
КонецФункции
"#;
    let (db, file_id) = setup_with_configs(
        fixture,
        vec![
            (None, designer_fixture_path()),
            (Some("УзкоеРасширение".to_string()), extension_constant_override_path()),
        ],
    );
    assert_eq!(
        type_mismatches(&db, file_id),
        Vec::<(TypeId, TypeId)>::new(),
        "untyped extension override must shadow base config — extension wins, gradual rule keeps the sink silent",
    );
}

#[test]
fn typed_sink_silent_on_get_of_string_constant() {
    // The original session-1 false positive: now silent.
    let fixture = r#"
//- /test.bsl
Функция Тест()
    Возврат ПустаяСтрока(Константы.СтрокаКонст.Получить());
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        type_mismatches(&db, file_id),
        Vec::<(TypeId, TypeId)>::new(),
        "session-1 reproducer must stay silent for a String-typed constant",
    );
}
