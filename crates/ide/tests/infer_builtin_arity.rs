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
