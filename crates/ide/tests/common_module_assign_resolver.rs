//! End-to-end regression for Track 1 Step N — `CommonModuleAssign`
//! through the resolver.
//!
//! Step M switched the metadata source for this diagnostic from
//! main-only to CFE-aware (`find_common_module_anywhere`). Step N
//! layers a resolver-aware shadowing filter on top, per plan
//! `linear-tumbling-noodle.md` §4.6:
//!
//! 1. `BodyDiagnostic::CommonModuleAssign::existing_binding_kind`
//!    (Step L payload) fast-paths Local/Param shadowing without
//!    rebuilding a `Resolver`.
//! 2. `Resolver::for_module(...).resolve_assignment_target(...)` (Step F)
//!    catches module-level `Перем` shadowing — a case Step L's body-
//!    local tracking cannot see — and confirms the name actually
//!    resolves to a CommonModule before the diagnostic fires.
//!
//! The fixtures here exercise three shapes the unit-test layer cannot:
//! a real `set_all_config_paths`-registered configuration (so
//! `is_common_module_anywhere` actually finds something), a module-
//! level `Перем` whose visibility is Salsa-tracked, and the canonical-
//! cased CommonModule name in the diagnostic message.

use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::{RootDatabaseImpl, SalsaProvider};
use ide_diagnostics::{DiagnosticCode, DiagnosticsConfig, DiagnosticsContext};
use std::path::PathBuf;
use vfs::{FileId, FileSet, VfsPath};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

/// Common module file path inside the designer fixture. Using a real
/// CommonModule path means `find_configuration_root` locates the
/// designer fixture's root and the visible-configurations registry
/// surfaces every `CommonModules/*` declared there — including
/// `ПервыйОбщийМодуль`, the name we'll reuse below as both
/// "configured CommonModule" and "potential assignment target".
fn common_module_path() -> PathBuf {
    designer_fixture_path().join("CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl")
}

fn setup_diagnostics(text: &str) -> Vec<ide_diagnostics::Diagnostic> {
    let file_id = FileId::from_raw(1);
    let mut db = RootDatabaseImpl::new();

    let mut file_set = FileSet::default();
    file_set.insert(file_id, VfsPath::new(common_module_path().to_string_lossy().to_string()));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(file_id, SourceRootId(0));
    db.set_file_text(file_id, text);
    db.set_all_config_paths(vec![(None, designer_fixture_path())]);

    let config = DiagnosticsConfig::all_enabled();
    let provider = SalsaProvider::new(&db, None);
    let ctx = DiagnosticsContext::new(&config, file_id, &provider);

    ide_diagnostics::diagnostics(&ctx)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::CommonModuleAssign)
        .collect()
}

/// Pure positive case: a method assigns to the unqualified name of a
/// configured CommonModule — no local Перем, no parameter, no module-
/// level shadow — must emit the diagnostic with the canonical-cased
/// name. Anchors that the resolver path actually reaches the
/// `AssignmentResolution::CommonModule` arm and that the canonical
/// name comes from `find_common_module_anywhere`, not from the
/// user-typed identifier.
#[test]
fn common_module_assign_emits_for_unshadowed_assignment() {
    let text = r#"
Процедура Тест()
    ПервыйОбщийМодуль = 1;
КонецПроцедуры
"#;
    let diags = setup_diagnostics(text);
    assert_eq!(diags.len(), 1, "expected one CommonModuleAssign diagnostic");
    assert!(
        diags[0].message.contains("ПервыйОбщийМодуль"),
        "diagnostic must reference canonical CommonModule name, got: {}",
        diags[0].message
    );
}

/// Step L fast-path: a parameter named after a CommonModule shadows
/// the configuration binding for the entire method body. Lowering's
/// `existing_binding_kind = Some(Param)` payload suppresses the
/// diagnostic without invoking the resolver. This case was a real
/// false positive in the pre-Step-N implementation, which checked
/// metadata only.
#[test]
fn common_module_assign_suppressed_by_param_shadow() {
    let text = r#"
Процедура Тест(ПервыйОбщийМодуль)
    ПервыйОбщийМодуль = 1;
КонецПроцедуры
"#;
    let diags = setup_diagnostics(text);
    assert!(
        diags.is_empty(),
        "parameter shadow must suppress CommonModuleAssign, got {} diagnostic(s): {:?}",
        diags.len(),
        diags.iter().map(|d| &d.message).collect::<Vec<_>>(),
    );
}

/// Step L fast-path: a `Перем` declared at the start of a method body
/// shadows the same-named CommonModule for the rest of the body.
/// Lowering captures `existing_binding_kind = Some(Local)` for the
/// downstream assignment.
#[test]
fn common_module_assign_suppressed_by_local_shadow() {
    let text = r#"
Процедура Тест()
    Перем ПервыйОбщийМодуль;
    ПервыйОбщийМодуль = 1;
КонецПроцедуры
"#;
    let diags = setup_diagnostics(text);
    assert!(
        diags.is_empty(),
        "local Перем shadow must suppress CommonModuleAssign, got {} diagnostic(s): {:?}",
        diags.len(),
        diags.iter().map(|d| &d.message).collect::<Vec<_>>(),
    );
}

/// Step N's resolver pass: a module-level `Перем` shadows the same-
/// named CommonModule even when the assignment lives in a nested
/// method body where Step L's body-local tracking sees `None` for
/// `existing_binding_kind`. Without the Step F resolver hook this
/// would produce a false positive — Step L's payload only watches
/// `local_vars` / `param_names`, not `SymbolTree::variables`.
#[test]
fn common_module_assign_suppressed_by_module_variable_shadow() {
    let text = r#"
Перем ПервыйОбщийМодуль;

Процедура Тест()
    ПервыйОбщийМодуль = 1;
КонецПроцедуры
"#;
    let diags = setup_diagnostics(text);
    assert!(
        diags.is_empty(),
        "module-level `Перем` must suppress CommonModuleAssign via the \
         resolver pass, got {} diagnostic(s): {:?}",
        diags.len(),
        diags.iter().map(|d| &d.message).collect::<Vec<_>>(),
    );
}

/// Negative pinning: an assignment to a name that is neither a
/// CommonModule, nor a module variable, nor a local/param must not
/// emit. The diagnostic only fires for resolution arm
/// `AssignmentResolution::CommonModule`, so the `Unknown` fall-through
/// stays quiet (and so does the fallback `is_common_module_anywhere`
/// streaming branch — there is no such CommonModule in the designer
/// fixture).
#[test]
fn common_module_assign_quiet_for_unknown_name() {
    let text = r#"
Процедура Тест()
    СовершенноПроизвольноеИмя = 1;
КонецПроцедуры
"#;
    let diags = setup_diagnostics(text);
    assert!(
        diags.is_empty(),
        "unrelated identifier must not produce CommonModuleAssign, got {} diagnostic(s): {:?}",
        diags.len(),
        diags.iter().map(|d| &d.message).collect::<Vec<_>>(),
    );
}
