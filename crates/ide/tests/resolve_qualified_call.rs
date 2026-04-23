//! Integration tests for `resolve_qualified_call` via the `db.infer()` query.
//!
//! These tests exercise the full inference pipeline for `Module.Method()`
//! calls and verify the outcomes visible to the inference layer:
//! - return type inferred, no diagnostic (happy path)
//! - `UnresolvedMethodKind::{MethodNotFound, MethodNotExport, ReceiverNotResolved}`
//! - case-insensitive module name lookup
//!
//! Prior to the `module_index`-based rewrite, this path went through
//! `db.workspace_symbols()` which forced `symbol_tree` on every file in the
//! source root. The tests below were added alongside that rewrite so the
//! behaviour is regression-proof regardless of which index is used.

use hir::{HirDatabase, InferenceDiagnostic, UnresolvedMethodKind};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use test_fixture::Fixture;
use vfs::FileId;

fn setup_fixture(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    let fixture = Fixture::parse(fixture_text);
    let mut db = RootDatabaseImpl::new();

    let mut file_set = vfs::FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
        db.set_file_text(*file_id, &file.content);
    }

    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    for file_id in fixture.files.keys() {
        db.set_file_source_root(*file_id, SourceRootId(0));
    }

    let test_file = *fixture.files.keys().last().expect("fixture must have at least one file");
    (db, test_file)
}

fn unresolved_kinds(db: &RootDatabaseImpl, file_id: FileId) -> Vec<UnresolvedMethodKind> {
    let infer = db.infer(file_id);
    infer
        .diagnostics
        .iter()
        .filter_map(|(_, diag)| match diag {
            InferenceDiagnostic::UnresolvedMethodCall { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect()
}

/// Returns `(expected, found)` pairs for every `MismatchedArgCount` in the
/// file's inference result.
///
/// `MismatchedArgCount` is emitted by `infer_qualified_call` *after*
/// `resolve_qualified_call` succeeds, which is the positive signal we want:
/// firing the diagnostic proves the module + method actually resolved
/// through `module_index` + `symbol_tree` and the looked-up method's
/// parameter count was compared against the call.
fn mismatched_arg_counts(db: &RootDatabaseImpl, file_id: FileId) -> Vec<(usize, usize)> {
    let infer = db.infer(file_id);
    infer
        .diagnostics
        .iter()
        .filter_map(|(_, diag)| match diag {
            InferenceDiagnostic::MismatchedArgCount { expected, found, .. } => {
                Some((*expected, *found))
            }
            _ => None,
        })
        .collect()
}

const EXPORTED_METHOD_FIXTURE: &str = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Функция ЗначениеРеквизитаОбъекта() Экспорт
    Возврат Истина;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщегоНазначения.ЗначениеРеквизитаОбъекта();
КонецПроцедуры
"#;

#[test]
fn exported_method_resolves_without_diagnostic() {
    let (db, file_id) = setup_fixture(EXPORTED_METHOD_FIXTURE);
    let kinds = unresolved_kinds(&db, file_id);
    assert!(kinds.is_empty(), "Expected no UnresolvedMethodCall diagnostics, got: {:?}", kinds);
}

const POSITIVE_RESOLUTION_FIXTURE: &str = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Функция ЗначениеРеквизитаОбъекта(Объект, ИмяРеквизита) Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщегоНазначения.ЗначениеРеквизитаОбъекта();
КонецПроцедуры
"#;

#[test]
fn successful_resolution_triggers_arg_count_check() {
    // Positive evidence that resolve_qualified_call actually found the
    // method: infer_qualified_call only emits MismatchedArgCount *after*
    // resolution succeeds, using the resolved method's parameter count.
    // The fixture declares 2 params and the call passes 0, so we must see
    // exactly `(expected: 2, found: 0)`.
    let (db, file_id) = setup_fixture(POSITIVE_RESOLUTION_FIXTURE);
    let unresolved = unresolved_kinds(&db, file_id);
    assert!(unresolved.is_empty(), "Resolution must succeed, got unresolved: {:?}", unresolved);

    let mismatches = mismatched_arg_counts(&db, file_id);
    assert_eq!(
        mismatches,
        vec![(2, 0)],
        "Expected MismatchedArgCount(expected=2, found=0) as positive proof of resolution"
    );
}

const NON_EXPORTED_METHOD_FIXTURE: &str = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Функция СкрытыйМетод()
    Возврат Ложь;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщегоНазначения.СкрытыйМетод();
КонецПроцедуры
"#;

#[test]
fn non_exported_method_reports_method_not_export() {
    let (db, file_id) = setup_fixture(NON_EXPORTED_METHOD_FIXTURE);
    let kinds = unresolved_kinds(&db, file_id);
    assert_eq!(
        kinds,
        vec![UnresolvedMethodKind::MethodNotExport],
        "Expected MethodNotExport for non-exported method"
    );
}

const MISSING_MODULE_FIXTURE: &str = r#"
//- /test.bsl
Процедура Тест()
    Результат = НесуществующийМодуль.Метод();
КонецПроцедуры
"#;

#[test]
fn missing_module_reports_method_not_found() {
    let (db, file_id) = setup_fixture(MISSING_MODULE_FIXTURE);
    let kinds = unresolved_kinds(&db, file_id);
    assert_eq!(
        kinds,
        vec![UnresolvedMethodKind::MethodNotFound],
        "Expected MethodNotFound when module_index has no entry"
    );
}

const MISSING_METHOD_FIXTURE: &str = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Процедура СуществующийМетод() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    Результат = ОбщегоНазначения.НесуществующийМетод();
КонецПроцедуры
"#;

#[test]
fn missing_method_reports_method_not_found() {
    let (db, file_id) = setup_fixture(MISSING_METHOD_FIXTURE);
    let kinds = unresolved_kinds(&db, file_id);
    assert_eq!(
        kinds,
        vec![UnresolvedMethodKind::MethodNotFound],
        "Expected MethodNotFound when module exists but method does not"
    );
}

const SHADOWING_FIXTURE: &str = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Функция ЗначениеРеквизитаОбъекта() Экспорт
    Возврат Истина;
КонецФункции

//- /test.bsl
Процедура Тест()
    Перем ОбщегоНазначения;
    ОбщегоНазначения = Новый Массив;
    ОбщегоНазначения.Добавить(1);
КонецПроцедуры
"#;

#[test]
fn local_shadowing_skips_qualified_resolution() {
    // When a local variable shadows a CommonModule name, HIR lowering
    // refuses to promote the call into `Expr::QualifiedPath` (see
    // `maybe_lower_as_qualified_call` in hir-def), so `resolve_qualified_call`
    // is never invoked for the shadowed receiver and no `UnresolvedMethodCall`
    // diagnostic is produced. The `resolver.resolve_local()` guard inside
    // `resolve_qualified_call` is therefore defensive — this test pins down
    // the observable behaviour from the inference layer.
    let (db, file_id) = setup_fixture(SHADOWING_FIXTURE);
    let kinds = unresolved_kinds(&db, file_id);
    assert!(
        kinds.is_empty(),
        "Shadowed receiver must not produce UnresolvedMethodCall, got: {:?}",
        kinds
    );
}

const CASE_INSENSITIVE_FIXTURE: &str = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Функция ЗначениеРеквизитаОбъекта(Параметр) Экспорт
    Возврат Истина;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = общегоназначения.ЗначениеРеквизитаОбъекта();
КонецПроцедуры
"#;

#[test]
fn case_insensitive_module_name_resolves() {
    // BSL is case-insensitive. module_index stores names lowercased, so
    // "общегоназначения" must resolve to "ОбщегоНазначения". We assert both
    // the negative (no UnresolvedMethodCall) and the positive evidence
    // (MismatchedArgCount fired → method was actually looked up).
    let (db, file_id) = setup_fixture(CASE_INSENSITIVE_FIXTURE);
    let kinds = unresolved_kinds(&db, file_id);
    assert!(kinds.is_empty(), "Expected case-insensitive module lookup, got: {:?}", kinds);
    let mismatches = mismatched_arg_counts(&db, file_id);
    assert_eq!(
        mismatches,
        vec![(1, 0)],
        "Case-insensitive lookup must reach arg-count check; got: {:?}",
        mismatches
    );
}

const RUSSIAN_LAYOUT_FIXTURE: &str = r#"
//- /ОбщиеМодули/ОбщегоНазначения/Ext/Module.bsl
Функция ЗначениеРеквизитаОбъекта(Параметр) Экспорт
    Возврат Истина;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщегоНазначения.ЗначениеРеквизитаОбъекта();
КонецПроцедуры
"#;

#[test]
fn russian_layout_resolves() {
    // module_index.parse_module_path accepts both Designer layouts:
    //   CommonModules/<Name>/Ext/Module.bsl  (English)
    //   ОбщиеМодули/<Name>/Ext/Module.bsl    (Russian)
    // Codex flagged the lack of an integration test for the Russian path.
    let (db, file_id) = setup_fixture(RUSSIAN_LAYOUT_FIXTURE);
    let kinds = unresolved_kinds(&db, file_id);
    assert!(kinds.is_empty(), "Russian-layout CommonModule must resolve, got: {:?}", kinds);
    let mismatches = mismatched_arg_counts(&db, file_id);
    assert_eq!(
        mismatches,
        vec![(1, 0)],
        "Russian-layout lookup must reach arg-count check; got: {:?}",
        mismatches
    );
}
