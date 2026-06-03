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

fn mismatched_arg_counts(db: &RootDatabaseImpl, file_id: FileId) -> Vec<(usize, usize, usize)> {
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
        vec![(2, 2, 0)],
        "expected a single MismatchedArgCount(required=2, total=2, found=0) for the 3-level call"
    );
    assert!(
        unresolved_kinds(&db, file_id).is_empty(),
        "method exists — UnresolvedMethodCall must not be emitted"
    );
}

#[test]
fn three_level_missing_mdo_emits_unresolved() {
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
    let (mut db, file_id) = setup(MANAGER_FIXTURE);

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
