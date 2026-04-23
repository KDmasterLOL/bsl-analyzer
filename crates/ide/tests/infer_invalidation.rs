//! End-to-end invalidation test for the metadata-bridge (M1 Task 1.8).
//!
//! Proves that `db.infer(file_id)` transitively depends on
//! `db.configurations(file_id)` through Salsa: changing the workspace config
//! set with `set_all_config_paths` invalidates inference and re-runs it, so
//! the diagnostic surface reflects the new visibility gate outcome.
//!
//! Prior to wiring `resolve_qualified_call` through the unified Resolver,
//! inference bypassed `db.configurations` entirely — flipping the config
//! set produced no observable change in the inference result.
//!
//! # Observable
//!
//! The fixture declares a 2-parameter exported method and a call-site that
//! passes zero arguments. Three states:
//!
//! 1. **No config registered** — Resolver falls back to `module_index` and
//!    resolves `ОбщегоНазначения.ЗначениеРеквизитаОбъекта` →
//!    `MismatchedArgCount(expected=2, found=0)` is emitted (positive proof
//!    that resolution succeeded).
//! 2. **Non-existent config registered** — `load_configuration` returns an
//!    empty `Configuration`; the visibility gate in the Resolver refuses
//!    the undeclared module → `UnresolvedMethodCall { MethodNotFound }`
//!    and no `MismatchedArgCount` (resolution didn't get far enough to
//!    check arity).
//! 3. **Reset to empty config list** — back to the baseline from state (1).
//!
//! Each transition can only flip the diagnostic surface if `db.infer` was
//! actually invalidated by `set_all_config_paths`. That's the plumbing
//! guarantee this test locks in.

use hir::{HirDatabase, InferenceDiagnostic, UnresolvedMethodKind};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use test_fixture::Fixture;
use vfs::FileId;

const FIXTURE: &str = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Функция ЗначениеРеквизитаОбъекта(Объект, ИмяРеквизита) Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщегоНазначения.ЗначениеРеквизитаОбъекта();
КонецПроцедуры
"#;

fn setup_fixture(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
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

#[test]
fn infer_invalidates_when_config_set_changes() {
    let (mut db, file_id) = setup_fixture(FIXTURE);

    // --- State 1: no config registered ------------------------------------
    // Resolver falls back to path-based `module_index`, so resolution
    // succeeds and inference emits MismatchedArgCount as positive evidence.
    assert_eq!(
        mismatched_arg_counts(&db, file_id),
        vec![(2, 0)],
        "baseline: without configs, resolution must succeed and emit \
         MismatchedArgCount — if this fails, the regression is outside the \
         invalidation path (likely module_index or symbol_tree)"
    );
    assert!(
        unresolved_kinds(&db, file_id).is_empty(),
        "baseline: no UnresolvedMethodCall expected, got {:?}",
        unresolved_kinds(&db, file_id)
    );

    // --- State 2: register a non-existent config path ---------------------
    // `load_configuration` produces an empty `Configuration`, so the gate
    // sees one config with zero declared common_modules. The Resolver must
    // reject the undeclared module, and inference must re-run — this only
    // happens if `db.infer` is transitively wired to `db.configurations`.
    db.set_all_config_paths(vec![(None, std::path::PathBuf::from("/does-not-exist"))]);

    let kinds = unresolved_kinds(&db, file_id);
    assert_eq!(
        kinds,
        vec![UnresolvedMethodKind::MethodNotFound],
        "after registering bogus config: the visibility gate must flip \
         resolution to UnresolvedMethodCall(MethodNotFound); if this fails, \
         `db.infer` is not invalidated by `set_all_config_paths` — the \
         metadata bridge is not wired through Salsa"
    );
    assert!(
        mismatched_arg_counts(&db, file_id).is_empty(),
        "after registering bogus config: resolution failed before arity \
         check, MismatchedArgCount must NOT be emitted, got {:?}",
        mismatched_arg_counts(&db, file_id)
    );

    // --- State 3: reset to empty config list ------------------------------
    // Removing the bogus config must also invalidate `db.infer` and restore
    // the baseline. This covers the "input removal" direction of Salsa
    // invalidation that the one-shot 1.6 test doesn't exercise.
    db.set_all_config_paths(vec![]);

    assert_eq!(
        mismatched_arg_counts(&db, file_id),
        vec![(2, 0)],
        "after resetting configs to empty: baseline resolution must be \
         restored — proves invalidation fires on input removal too"
    );
    assert!(
        unresolved_kinds(&db, file_id).is_empty(),
        "after resetting configs: no UnresolvedMethodCall expected, got {:?}",
        unresolved_kinds(&db, file_id)
    );
}
