//! Integration tests for the narrowing-aware argument-validation
//! pipeline (`hir-ty::arg_diagnostics_query`).
//!
//! Inference no longer emits `InferenceDiagnostic::TypeMismatch` for
//! arguments inline; the downstream `arg_diagnostics_query` produces
//! them after consulting the [`hir::HirDatabase::narrow`] overlay. The
//! tests below pin:
//!
//! 1. **Positive narrowing** — a guard like `If X <> Undefined Then`
//!    suppresses the false-positive `TypeMismatch` that the legacy
//!    inference-stage check would have fired on a `Number | Undefined`
//!    receiver passed where `Number` is expected. This is the user-
//!    reported reproducer.
//! 2. **Else-branch negative** — symmetrically, an arg passed in the
//!    else-branch of the same guard *must* still fire `TypeMismatch`
//!    (else-state narrows to `Undefined`).
//! 3. **No-guard regression** — without any guard, the diagnostic
//!    must continue to fire (regression anchor for the original
//!    behaviour).
//! 4. **Arity-mismatch anchor** — `MismatchedArgCount` continues to
//!    fire from `infer_query` (it was deliberately not moved to
//!    `arg_diagnostics_query` because it has no narrowing dependency).
//! 5. **Feature-flag off** — `set_type_narrowing_enabled(false)`
//!    restores the pre-narrow baseline (diagnostic fires again).
//! 6. **Salsa invalidation** — flipping the narrowing flag on the
//!    same database recomputes `arg_diagnostics` instead of returning
//!    a stale cache.

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
    db.set_all_config_paths(vec![(None, designer_fixture_path())]);

    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    (db, test_file)
}

/// Collect `(expected, actual)` pairs from `arg_diagnostics_query`.
fn arg_mismatches(db: &RootDatabaseImpl, file_id: FileId) -> Vec<(Ty, Ty)> {
    db.arg_diagnostics(file_id)
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::TypeMismatch { expected, actual, .. } => {
                Some((expected.clone(), actual.clone()))
            }
            _ => None,
        })
        .collect()
}

/// CommonModule with a function returning `Число | Неопределено` and a
/// function expecting just `Число`. Used by the narrowing tests to
/// mimic the user's `Массив.Найти(...)` + `Массив.Удалить(...)` shape
/// without depending on platform-data signatures (which would couple
/// the tests to the platform-data version).
const NARROW_FIXTURE_MODULE: &str = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   Число, Неопределено - индекс или Неопределено если не найден
Функция Найти() Экспорт
    Возврат Неопределено;
КонецФункции

// Параметры:
//   Индекс - Число - индекс
Процедура Удалить(Индекс) Экспорт
КонецПроцедуры
"#;

#[test]
fn narrowed_arg_inside_guard_does_not_fire_mismatch() {
    // The user-reported reproducer:
    //   Index = Module.Find();   // Index: Number | Undefined
    //   If Index <> Undefined Then
    //       Module.Delete(Index);   // narrowed: Number → no mismatch
    //   EndIf
    //
    // Inside the true branch the guard refines `Index` from
    // `Number | Undefined` down to `Number`, which is assignable to
    // the declared parameter type — no diagnostic must fire.
    let fixture = format!(
        "{NARROW_FIXTURE_MODULE}\n\
//- /test.bsl\n\
Процедура Тест()\n\
    Индекс = ПервыйОбщийМодуль.Найти();\n\
    Если Индекс <> Неопределено Тогда\n\
        ПервыйОбщийМодуль.Удалить(Индекс);\n\
    КонецЕсли;\n\
КонецПроцедуры\n"
    );
    let (db, file_id) = setup(&fixture);

    assert!(
        arg_mismatches(&db, file_id).is_empty(),
        "narrowed `Number | Undefined → Number` arg in guard true-branch \
         must not emit TypeMismatch — got {:?}",
        arg_mismatches(&db, file_id),
    );
}

#[test]
fn unnarrowed_arg_outside_guard_still_fires_mismatch() {
    // Regression anchor for the legacy behaviour: outside any guard,
    // the same `Number | Undefined` arg passed where `Number` is
    // expected must still produce a `TypeMismatch`. This pins that
    // we did not accidentally widen acceptance on the no-guard path.
    let fixture = format!(
        "{NARROW_FIXTURE_MODULE}\n\
//- /test.bsl\n\
Процедура Тест()\n\
    Индекс = ПервыйОбщийМодуль.Найти();\n\
    ПервыйОбщийМодуль.Удалить(Индекс);\n\
КонецПроцедуры\n"
    );
    let (db, file_id) = setup(&fixture);

    let mm = arg_mismatches(&db, file_id);
    assert_eq!(
        mm.len(),
        1,
        "no-guard call with `Number | Undefined` arg must still emit one TypeMismatch — \
         got {mm:?}"
    );
    let (expected, actual) = &mm[0];
    assert_eq!(expected, &Ty::Number, "expected param type Number");
    // `actual` is the union — exact union shape is incidental, just
    // assert it isn't `Number` (i.e. no narrowing happened here).
    assert!(
        !matches!(actual, Ty::Number),
        "actual must be the unnarrowed union, not Number — got {actual:?}"
    );
}

#[test]
fn arg_mismatch_in_else_branch_still_fires() {
    // Symmetric to the positive narrowing case: in the else-branch of
    // `Index <> Undefined`, the variable is narrowed to `Undefined`,
    // which is **not** assignable to `Number`. Diagnostic must fire.
    //
    // (`ty_difference` collapsing to `Ty::Unknown` would mask this —
    // narrowing falls back to base, which is `Number | Undefined`,
    // still not assignable to `Number`. Either way, mismatch.)
    let fixture = format!(
        "{NARROW_FIXTURE_MODULE}\n\
//- /test.bsl\n\
Процедура Тест()\n\
    Индекс = ПервыйОбщийМодуль.Найти();\n\
    Если Индекс <> Неопределено Тогда\n\
        // Match — narrowed to Number.\n\
    Иначе\n\
        ПервыйОбщийМодуль.Удалить(Индекс);\n\
    КонецЕсли;\n\
КонецПроцедуры\n"
    );
    let (db, file_id) = setup(&fixture);

    let mm = arg_mismatches(&db, file_id);
    assert_eq!(
        mm.len(),
        1,
        "else-branch `Undefined` arg must still emit one TypeMismatch — got {mm:?}"
    );
}

#[test]
fn arity_mismatch_still_fires_from_infer_query() {
    // `MismatchedArgCount` was deliberately NOT moved into
    // `arg_diagnostics_query` (it has no narrowing dependency). This
    // test pins that decision: passing the wrong number of args
    // continues to surface the count diagnostic from inside
    // `infer_query`, independent of the new arg-validation pipeline.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Параметры:
//   А - Число
//   Б - Число
Процедура Двойной(А, Б) Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ПервыйОбщийМодуль.Двойной(1);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);

    let arity = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter(|(_, d)| matches!(d, InferenceDiagnostic::MismatchedArgCount { .. }))
        .count();
    assert_eq!(arity, 1, "exactly one MismatchedArgCount must continue to fire from infer_query");
}

#[test]
fn feature_flag_off_disables_narrowing_overlay() {
    // Toggling `type_narrowing_enabled` to `false` puts
    // `narrow_or_base` into pass-through mode — no overlay, no
    // narrowing. The same fixture that produces zero mismatches when
    // narrowing is on must again produce one mismatch when it's off,
    // matching the pre-narrow baseline.
    let fixture = format!(
        "{NARROW_FIXTURE_MODULE}\n\
//- /test.bsl\n\
Процедура Тест()\n\
    Индекс = ПервыйОбщийМодуль.Найти();\n\
    Если Индекс <> Неопределено Тогда\n\
        ПервыйОбщийМодуль.Удалить(Индекс);\n\
    КонецЕсли;\n\
КонецПроцедуры\n"
    );
    let (mut db, file_id) = setup(&fixture);
    db.set_type_narrowing_enabled(false);

    let mm = arg_mismatches(&db, file_id);
    assert_eq!(
        mm.len(),
        1,
        "with narrowing disabled, the guard must not suppress the mismatch — got {mm:?}"
    );
}

#[test]
fn flag_flip_invalidates_arg_diagnostics_cache() {
    // Same database, same file content — but flipping
    // `type_narrowing_enabled` between calls must produce different
    // results. This catches "fresh-DB green, incremental broken"
    // regressions where Salsa's caching could otherwise mask a
    // narrowing wiring break.
    let fixture = format!(
        "{NARROW_FIXTURE_MODULE}\n\
//- /test.bsl\n\
Процедура Тест()\n\
    Индекс = ПервыйОбщийМодуль.Найти();\n\
    Если Индекс <> Неопределено Тогда\n\
        ПервыйОбщийМодуль.Удалить(Индекс);\n\
    КонецЕсли;\n\
КонецПроцедуры\n"
    );
    let (mut db, file_id) = setup(&fixture);

    // 1) Narrowing on — no diagnostic.
    let on = arg_mismatches(&db, file_id);
    assert!(on.is_empty(), "with narrowing on: expected zero mismatches, got {on:?}");

    // 2) Toggle off — diagnostic must reappear (no stale cache).
    db.set_type_narrowing_enabled(false);
    let off = arg_mismatches(&db, file_id);
    assert_eq!(
        off.len(),
        1,
        "after flag flip to false: expected one mismatch, got {off:?} — \
         indicates stale arg_diagnostics cache",
    );

    // 3) Toggle back on — diagnostic must disappear again.
    db.set_type_narrowing_enabled(true);
    let on_again = arg_mismatches(&db, file_id);
    assert!(
        on_again.is_empty(),
        "after flag flip back to true: expected zero mismatches, got {on_again:?}"
    );
}
