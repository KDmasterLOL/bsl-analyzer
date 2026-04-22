//! Behavioural tests for three-segment manager-chain calls
//! (`Документы.ПКО.СоздатьДокумент()`) after Task 7 routes them through
//! [`hir_ty::method_resolution::resolve_three_level_call`].
//!
//! Fixtures put the exported manager-module method behind the canonical
//! Designer-format path (`Documents/<Name>/Ext/ManagerModule.bsl`); the
//! inference run then observes the diagnostic surface at the call-site in
//! `/test.bsl`. Because `infer` goes through `Resolver::resolve_three_level_method`
//! — which reads `db.configurations(...)` through Salsa — toggling the
//! workspace config set with `set_all_config_paths` re-runs inference. The
//! last test locks that invalidation path in.

use hir::{DefDatabase, HirDatabase, InferenceDiagnostic, ModuleId, UnresolvedMethodKind};
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

fn unresolved_kinds(db: &RootDatabaseImpl, file_id: FileId) -> Vec<UnresolvedMethodKind> {
    db.infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect()
}

fn mismatched_arg_counts(db: &RootDatabaseImpl, file_id: FileId) -> Vec<(usize, usize)> {
    db.infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::MismatchedArgCount { expected, found, .. } => {
                Some((*expected, *found))
            }
            _ => None,
        })
        .collect()
}

const MANAGER_FIXTURE: &str = r#"
//- /Documents/ПКО/Ext/ManagerModule.bsl
Функция ПолучитьСсылку(Код, Имя) Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = Документы.ПКО.ПолучитьСсылку("001", "Первый");
КонецПроцедуры
"#;

#[test]
fn three_level_call_resolves_against_manager_module() {
    // Manager method exists with arity 2, call passes 2 args → clean
    // inference, no diagnostics. Positive proof that
    // `resolve_three_level_call` hit the manager module's symbol_tree.
    let (db, file_id) = setup(MANAGER_FIXTURE);
    assert!(
        unresolved_kinds(&db, file_id).is_empty(),
        "three-level call must resolve cleanly, got {:?}",
        unresolved_kinds(&db, file_id)
    );
    assert!(
        mismatched_arg_counts(&db, file_id).is_empty(),
        "call passes 2 args to a 2-param method, arity must match; got {:?}",
        mismatched_arg_counts(&db, file_id)
    );
}

#[test]
fn three_level_arity_mismatch_emits_diagnostic() {
    // Same manager method, but call now passes zero args — arity check must
    // fire, and since the method did resolve no UnresolvedMethodCall is
    // emitted.
    let fixture = r#"
//- /Documents/ПКО/Ext/ManagerModule.bsl
Функция ПолучитьСсылку(Код, Имя) Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = Документы.ПКО.ПолучитьСсылку();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        mismatched_arg_counts(&db, file_id),
        vec![(2, 0)],
        "expected a single MismatchedArgCount(2, 0) for the 3-level call"
    );
    assert!(
        unresolved_kinds(&db, file_id).is_empty(),
        "method exists — UnresolvedMethodCall must not be emitted"
    );
}

#[test]
fn three_level_missing_mdo_emits_unresolved() {
    // The manager module does not exist; resolution collapses to
    // MethodNotFound. Inference must not silently type the call as Unknown
    // — the diagnostic is the observable signal that Task 7 routed the
    // call through the resolver.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Результат = Документы.НетТакогоДокумента.ПолучитьСсылку();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        unresolved_kinds(&db, file_id),
        vec![UnresolvedMethodKind::MethodNotFound],
        "missing MDO must emit MethodNotFound"
    );
    assert!(
        mismatched_arg_counts(&db, file_id).is_empty(),
        "resolution failed before arity check; got {:?}",
        mismatched_arg_counts(&db, file_id)
    );
}

#[test]
fn three_level_non_exported_method_emits_method_not_export() {
    // Same fixture, method now missing Экспорт — resolver returns the
    // MethodId with is_export=false, inference surfaces MethodNotExport.
    let fixture = r#"
//- /Documents/ПКО/Ext/ManagerModule.bsl
Функция ПолучитьСсылку(Код, Имя)
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = Документы.ПКО.ПолучитьСсылку("001", "Первый");
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        unresolved_kinds(&db, file_id),
        vec![UnresolvedMethodKind::MethodNotExport],
        "non-exported method must emit MethodNotExport, not MethodNotFound"
    );
    assert!(
        mismatched_arg_counts(&db, file_id).is_empty(),
        "non-exported is a visibility issue, not an arity issue"
    );
}

#[test]
fn three_level_invalidates_on_config_change() {
    // Locks the Salsa dependency chain for 3-level calls: registering a
    // bogus configuration forces the visibility gate in
    // `resolve_three_level_method` to refuse the MDO (no declaration in
    // any visible config), so the diagnostic surface flips. Resetting the
    // config list restores the baseline — proving invalidation fires in
    // both directions.
    let (mut db, file_id) = setup(MANAGER_FIXTURE);

    // Baseline: no config → path-based lookup succeeds, clean inference.
    assert!(unresolved_kinds(&db, file_id).is_empty(), "baseline must be clean");

    db.set_all_config_paths(vec![(None, std::path::PathBuf::from("/does-not-exist"))]);
    assert_eq!(
        unresolved_kinds(&db, file_id),
        vec![UnresolvedMethodKind::MethodNotFound],
        "bogus config must hide the MDO and flip to MethodNotFound"
    );

    db.set_all_config_paths(vec![]);
    assert!(
        unresolved_kinds(&db, file_id).is_empty(),
        "clearing configs must restore the baseline — invalidation fires on input removal"
    );
}
