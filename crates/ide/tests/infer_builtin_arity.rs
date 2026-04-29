//! End-to-end regression tests for `MismatchedArgCount` on platform
//! built-in functions whose signatures carry per-parameter `defaults`
//! and/or `is_variadic` flags.
//!
//! These pin the contract that user-driven the 3-slice fix (Slice 1:
//! adapter over `bsl_platform::PlatformData::instance()`, Slice 2:
//! `Ty::Function { defaults, is_variadic, .. }`, Slice 3: arity check
//! honouring both fields) actually closes the false-positive on calls
//! like `НСтр("ru = '...'", "ru")` which previously reported
//! `ожидалось 1, передано 2`.
//!
//! The fixtures only exercise builtin globals — no managers, no
//! `СтандартныеПодсистемыСервер`, no JSDoc — so a regression here points
//! squarely at the `Ty::Function` arity check or the builtin-signature
//! adapter. Other dimensions of `MismatchedArgCount` (user methods,
//! common-module functions, qualified manager calls) are covered by the
//! existing tests in `infer_invalidation.rs`, `infer_three_level.rs`,
//! and `resolve_qualified_call.rs`.

use hir::{HirDatabase, InferenceDiagnostic};
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
        db.set_file_text(*file_id, &file.content);
    }
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for file_id in fixture.files.keys() {
        db.set_file_source_root(*file_id, SourceRootId(0));
    }

    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    (db, test_file)
}

fn arg_count_diags(db: &RootDatabaseImpl, file_id: FileId) -> Vec<(usize, usize, usize)> {
    db.infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::MismatchedArgCount {
                required_count, total_count, found, ..
            } => Some((*required_count, *total_count, *found)),
            _ => None,
        })
        .collect()
}

#[test]
fn nstr_two_args_is_accepted() {
    // The exact regression: `НСтр("...", "ru")` must NOT fire
    // MismatchedArgCount because `КодЯзыка` is declared optional in
    // `platform_data.json`. Before Slice 3 the check did
    // `args.len() != params.len()` and falsely reported
    // `ожидалось 1, передано 2`.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Сообщить(НСтр("ru = 'Привет'", "ru"));
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "NStr('ru = ...', 'ru') is two valid args (required=1, total=2), \
         no MismatchedArgCount expected, got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn nstr_one_arg_is_accepted() {
    // The single-arg form has always been accepted; pin it so a future
    // regression to `args.len() < required` doesn't creep in.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Сообщить(НСтр("ru = 'Привет'"));
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "NStr('ru = ...') with one arg satisfies required=1, no diag expected, got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn nstr_zero_args_fires_mismatch() {
    // The lower bound (`required`) is still enforced — `НСтр()` with no
    // template string must produce one MismatchedArgCount.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Сообщить(НСтр());
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        arg_count_diags(&db, file_id),
        vec![(1, 2, 0)],
        "NStr() must emit (required=1, total=2, found=0) — required floor is preserved"
    );
}

#[test]
fn nstr_three_args_fires_mismatch() {
    // The upper bound is enforced too: НСтр does NOT have `is_variadic`,
    // so calling it with 3 arguments must fire one MismatchedArgCount.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Сообщить(НСтр("ru = '...'", "ru", "extra"));
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        arg_count_diags(&db, file_id),
        vec![(1, 2, 3)],
        "NStr(arg, arg, arg) must emit (required=1, total=2, found=3) — \
         non-variadic upper bound is preserved"
    );
}

#[test]
fn strtemplate_variadic_accepts_many_args() {
    // СтрШаблон has the platform-help idiom `Значение1-Значение10` which
    // the adapter lifts to `is_variadic = true`. Calls with 1, 5, 11 args
    // must all pass without diagnostics.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Сообщить(СтрШаблон("без подстановок"));
    Сообщить(СтрШаблон("%1 + %2 + %3 = %4", 1, 2, 3, 6));
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "СтрШаблон is variadic, none of the call sites should fire \
         MismatchedArgCount, got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn strtemplate_zero_args_fires_mismatch() {
    // Variadic does NOT mean "all args optional" — the leading required
    // template parameter still must be passed. `СтрШаблон()` must fire.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Сообщить(СтрШаблон());
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        arg_count_diags(&db, file_id),
        vec![(1, 2, 0)],
        "СтрШаблон() with no args must emit (required=1, total=2, found=0) — \
         the variadic flag does NOT relax the lower bound"
    );
}

#[test]
fn currentdate_no_args_is_accepted() {
    // `ТекущаяДата()` is the canonical zero-arity builtin. After Slice 3
    // the bound check is `0 < 0 || (!variadic && 0 > 0)` → both false.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Х = ТекущаяДата();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "ТекущаяДата() with zero args is correct, got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn min_max_accept_many_args() {
    // The user's repro: `Мин`/`Макс` accept any number of arguments. HBK
    // encodes this only in the `Синтаксис:` chapter (`<Значение1>,...,
    // <ЗначениеN>`); plan C wires the html-parser to detect the
    // `>,...,<` substring and lift `is_variadic = true` on the trailing
    // param. Confirmed by `bsl-platform/src/db.rs::test_is_variadic_marks_*`.
    let fixture = r#"
//- /test.bsl
Процедура Тест(Мин, Макс)
    Х = Мин(Макс(1, 2), 3, 4, 5);
    У = Макс(10, 20, 30, 40, 50);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "Мин/Макс are unbounded variadic, multi-arg calls must NOT fire \
         MismatchedArgCount, got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn min_max_zero_args_fires_mismatch() {
    // Unbounded-variadic does NOT mean "all optional" — the first
    // `Значение1` is required. `Мин()` / `Макс()` must still fire.
    // (`Значение1` carries `is_optional = false` in platform_data.json.)
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Х = Мин();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let diags = arg_count_diags(&db, file_id);
    // total_count = 1 (the single declared required param), max_args =
    // None (unbounded). Required floor still fails for zero args.
    assert_eq!(
        diags.len(),
        1,
        "Мин() with zero args must fire exactly one MismatchedArgCount, got {diags:?}"
    );
    assert_eq!(diags[0].2, 0, "found = 0");
    assert!(diags[0].0 >= 1, "required >= 1 for the leading param");
}

#[test]
fn user_repro_min_max_okr_intervals() {
    // Verbatim shape from the user's bug report: nested
    // `Мин(Макс(Окр(...), Мин), Макс)` with three-arg `Окр` (number,
    // digits, mode). All three builtins must accept their argument
    // counts without firing MismatchedArgCount.
    //
    // Local parameters `Мин` / `Макс` shadow the builtins as VALUES
    // (the `(Макс - Мин) * Случ` arithmetic) but in CALL position
    // (`Мин(...)`, `Макс(...)`) BSL resolves to the platform builtin.
    // This test pins both regressions: variadic arity for the calls,
    // and the absence of arity false-positives in the inner expressions.
    let fixture = r#"
//- /test.bsl
Процедура ВыбратьЧисло(Мин, Макс)
    Случ = 0.5;
    ЧислоИзИнтервала = Мин(Макс(Окр(Мин + (Макс - Мин) * Случ, 0, 1), Мин), Макс);
    Сообщить(ЧислоИзИнтервала);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "user repro line must produce zero MismatchedArgCount diagnostics, got {:?}",
        arg_count_diags(&db, file_id)
    );
}
