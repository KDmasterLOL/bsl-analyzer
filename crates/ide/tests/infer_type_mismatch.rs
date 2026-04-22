//! Behavioural tests for the `TypeMismatch` emitter wired in
//! `hir-ty::infer` alongside M4 Task 7.
//!
//! The handler / diagnostic code was declared back in M4 Task 1; the
//! emitter was deferred until `hir::Type::is_assignable_to` provided
//! the subtype predicate. These tests pin that the emitter:
//!
//! 1. Fires when a concrete-typed argument mismatches a concrete-typed
//!    parameter (the common real-code scenario).
//! 2. Stays silent when **either side** is `Ty::Unknown` — matches the
//!    gradual-typing rule in `hir_ty::subtype::is_assignable` and
//!    guards against diagnostic floods on under-annotated BSL code.
//! 3. Handles `Null ≤ ref-type` correctly (no spurious mismatch on
//!    the "clear the reference" idiom).
//! 4. Reaches the fluent / method-call path (`Expr::Call { callee:
//!    Expr::Field }`) as well as the qualified CommonModule path —
//!    both were plumbed by the same `emit_arg_type_mismatches` helper.
//!
//! The CommonModule `ПервыйОбщийМодуль` is declared in the designer
//! fixture's `Configuration.xml`, matching the convention used by
//! `infer_field_lookup` / `infer_this_object`.

use hir::{HirDatabase, InferenceDiagnostic, Ty};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use test_fixture::Fixture;
use vfs::FileId;

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn setup(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    setup_impl(fixture_text, /*attach_designer_config=*/ true)
}

/// Setup variant for 3-segment manager-chain fixtures: resolution is
/// VFS-based through `/Documents/<Name>/Ext/ManagerModule.bsl`, and
/// attaching the designer config path would overlay a `Configuration`
/// that does **not** declare `ПКО` — the resolver then refuses to
/// recognise the manager, and the call never reaches
/// `infer_three_level_call`'s Ok arm. Mirrors the VFS-only setup in
/// `infer_three_level.rs`.
fn setup_vfs_only(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    setup_impl(fixture_text, /*attach_designer_config=*/ false)
}

fn setup_impl(fixture_text: &str, attach_designer_config: bool) -> (RootDatabaseImpl, FileId) {
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
    if attach_designer_config {
        db.set_all_config_paths(vec![(None, designer_fixture_path())]);
    }

    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    (db, test_file)
}

fn mismatches(db: &RootDatabaseImpl, file_id: FileId) -> Vec<(Ty, Ty)> {
    db.infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::TypeMismatch { expected, actual, .. } => {
                Some((expected.clone(), actual.clone()))
            }
            _ => None,
        })
        .collect()
}

/// A CommonModule with a fully-JSDoc-typed function: parameter `П`
/// declared as `Число`, return declared as `Строка`. Used by several
/// of the tests below — duplicating the header is what keeps each
/// fixture self-contained.
const COMMON_MODULE_TYPED_NUMBER_PARAM: &str = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Параметры:
//   П - Число - численный аргумент
// Возвращаемое значение:
//   Строка - описание
Функция Привет(П) Экспорт
    Возврат "привет";
КонецФункции
"#;

#[test]
fn type_mismatch_fires_on_concrete_mismatch() {
    // Core case: the JSDoc declares `П: Число`, but the test passes a
    // literal string. The emitter must fire exactly one
    // `TypeMismatch { expected: Number, actual: String }` — a single
    // arg position, single diagnostic.
    let fixture = format!(
        "{COMMON_MODULE_TYPED_NUMBER_PARAM}\n\
//- /test.bsl\n\
Процедура Тест()\n\
    А = ПервыйОбщийМодуль.Привет(\"строка\");\n\
КонецПроцедуры\n"
    );
    let (db, file_id) = setup(&fixture);

    let mm = mismatches(&db, file_id);
    assert_eq!(
        mm,
        vec![(Ty::Number, Ty::String)],
        "concrete Number param + String arg must fire exactly one mismatch"
    );
}

#[test]
fn type_mismatch_silent_on_matching_arg() {
    // Negative control — same fixture but the arg matches the
    // declared param. No `TypeMismatch` at all. Pins that the emitter
    // doesn't fire on well-typed calls.
    let fixture = format!(
        "{COMMON_MODULE_TYPED_NUMBER_PARAM}\n\
//- /test.bsl\n\
Процедура Тест()\n\
    А = ПервыйОбщийМодуль.Привет(42);\n\
КонецПроцедуры\n"
    );
    let (db, file_id) = setup(&fixture);
    assert!(mismatches(&db, file_id).is_empty(), "matching arg must not produce any TypeMismatch");
}

#[test]
fn type_mismatch_silent_when_param_type_is_unknown() {
    // The common BSL case: a CommonModule function with no JSDoc
    // param annotation. `materialise_signature` lowers the param to
    // `Ty::Unknown`; gradual typing in `is_assignable` treats
    // `X ≤ Unknown` as true, so no diagnostic should fire no matter
    // how the caller types the argument. Guards the
    // "under-annotated project" regression path — without this rule
    // every CommonModule call in a real codebase would paint red.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
Функция Привет(П) Экспорт
    Возврат П;
КонецФункции

//- /test.bsl
Процедура Тест()
    А = ПервыйОбщийМодуль.Привет("строка");
    Б = ПервыйОбщийМодуль.Привет(42);
    В = ПервыйОбщийМодуль.Привет(Истина);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        mismatches(&db, file_id).is_empty(),
        "untyped param (Unknown) must accept any arg type — gradual bottom rule"
    );
}

#[test]
fn type_mismatch_silent_when_arg_type_is_unknown() {
    // Dual of the above: the arg comes from an expression the
    // inferrer bailed on (`Ty::Unknown`). Assigning an opaque method
    // return to a variable, then passing that variable, should stay
    // silent even when the param has a concrete declared type.
    // Without the `Unknown ≤ A` gradual rule, a single unresolved
    // call would cascade mismatches through every downstream
    // qualified call.
    let fixture = format!(
        "{COMMON_MODULE_TYPED_NUMBER_PARAM}\n\
//- /test.bsl\n\
Процедура Тест()\n\
    // `Опаковать` is not declared anywhere — its return stays Unknown.\n\
    Х = Опаковать();\n\
    А = ПервыйОбщийМодуль.Привет(Х);\n\
КонецПроцедуры\n"
    );
    let (db, file_id) = setup(&fixture);
    assert!(
        mismatches(&db, file_id).is_empty(),
        "Unknown arg must not trigger TypeMismatch — gradual top rule"
    );
}

#[test]
fn type_mismatch_respects_null_to_ref_rule() {
    // The `Null ≤ ref-type` rule in `is_assignable` carries through to
    // emission: `ПервыйОбщийМодуль.СохранитьСсылку(NULL)` against a
    // `CatalogRef.Справочник1` param should stay silent, matching the
    // BSL idiom of assigning `Null` to clear a reference field.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Параметры:
//   Ссылка - СправочникСсылка.Справочник1 - ссылка
Процедура СохранитьСсылку(Ссылка) Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ПервыйОбщийМодуль.СохранитьСсылку(NULL);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        mismatches(&db, file_id).is_empty(),
        "Null arg to a ref-typed param must not fire TypeMismatch — matches `Null ≤ ref-type` subtype rule"
    );
}

#[test]
fn type_mismatch_fires_on_three_level_manager_call() {
    // 3-segment call path (`Документы.ПКО.Метод()`) goes through
    // `infer_three_level_call`, a different branch from the
    // qualified-call test. The fixture is modeled on
    // `infer_three_level.rs::MANAGER_FIXTURE`: a manager-module
    // method defined in `Documents/ПКО/Ext/ManagerModule.bsl` with
    // JSDoc-typed params. Wrong-typed call must surface exactly one
    // TypeMismatch; a 3-level arity error is already covered by
    // `infer_three_level.rs` — here we lock the type emitter hook.
    let fixture = r#"
//- /Documents/ПКО/Ext/ManagerModule.bsl
// Параметры:
//   Код - Число - первый
Функция ПолучитьСсылку(Код) Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = Документы.ПКО.ПолучитьСсылку("not a number");
КонецПроцедуры
"#;
    let (db, file_id) = setup_vfs_only(fixture);
    assert_eq!(
        mismatches(&db, file_id),
        vec![(Ty::Number, Ty::String)],
        "3-level manager call must emit TypeMismatch through infer_three_level_call"
    );
}

#[test]
fn type_mismatch_does_not_double_fire_on_arg_count_mismatch() {
    // Pair with `MismatchedArgCount`: passing *fewer* args than the
    // param list must emit `MismatchedArgCount` (pre-existing) but
    // **not** a per-position `TypeMismatch` on the missing tail. The
    // emitter zips to `min(args, params)` precisely to avoid
    // double-firing.
    //
    // Fixture declares two params (`П - Число`, `Строка - Строка`);
    // call passes only one (`42` — matches the first). Expected
    // diagnostics: one `MismatchedArgCount`, zero `TypeMismatch`.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Параметры:
//   П - Число - первый
//   С - Строка - второй
Процедура Двойной(П, С) Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ПервыйОбщийМодуль.Двойной(42);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let diags = db.infer(file_id).diagnostics.clone();

    let count_mismatches: Vec<_> = diags
        .iter()
        .filter(|(_, d)| matches!(d, InferenceDiagnostic::MismatchedArgCount { .. }))
        .collect();
    assert_eq!(count_mismatches.len(), 1, "exactly one MismatchedArgCount expected");

    let type_mismatches: Vec<_> = diags
        .iter()
        .filter(|(_, d)| matches!(d, InferenceDiagnostic::TypeMismatch { .. }))
        .collect();
    assert!(
        type_mismatches.is_empty(),
        "no TypeMismatch must fire for the paired position or for the missing tail — \
         got {type_mismatches:?}"
    );
}
