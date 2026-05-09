//! PrivilegedModuleMethodCall diagnostic.
//!
//! Reports calls to exported methods of privileged common modules.
//!
//! # Guard-predicate suppression (Track 2 §1.6 Group D)
//!
//! Calls that are dominated by a recognised guard predicate
//! (`РольДоступна`, `РольДоступнаПользователю` — see
//! [`hir::dataflow::guard_predicates::default_registry`]) are
//! suppressed. `БезопасныйРежим` and `ПривилегированныйРежим` are
//! intentionally absent from the default registry (the former
//! conflicts with `UnsafeSafeModeMethodCall`, the latter is
//! tautological in privileged modules — see the `default_registry`
//! doc-comment). The detector is `must-be-guarded`: every path
//! from method entry to the call site must cross a guard's true
//! branch. False negatives (a guarded call still flagged) are the
//! conservative direction — security alerts win over noise — but
//! the common pattern `Если РольДоступна("Администратор") Тогда
//! ПривилегированныйМодуль.Метод(); КонецЕсли;` is correctly
//! suppressed.

use bsl_metadata::traits::MdObject;

use crate::define_metadata;
use crate::metadata::*;
use crate::{common_module_helpers, Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::call_graph::{CallEdge, CallTarget, CallerId, EdgeKind};
use hir::dataflow::guard_predicates::{default_registry, is_stmt_guarded, GuardRegistry};
use hir::PathResolution;
use ide_db::TextRange;
use rustc_hash::FxHashSet;
use std::sync::Arc;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 60,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_VALIDATE_NESTED_CALLS: bool = true;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::PrivilegedModuleMethodCall;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let validate_nested_calls = ctx
        .config
        .get_bool(DiagnosticCode::PrivilegedModuleMethodCall, "validateNestedCalls")
        .unwrap_or(DEFAULT_VALIDATE_NESTED_CALLS);

    // Union privileged CommonModule names across main + CFE so a
    // privileged module declared by an extension is still flagged from
    // call sites in other (or the same) configuration.
    let privileged_modules: FxHashSet<String> = ctx
        .visible_configurations()
        .iter()
        .flat_map(|vc| vc.configuration.common_modules().to_vec())
        .filter(|m| m.is_privileged())
        .map(|m| m.name().to_lowercase())
        .collect();

    if privileged_modules.is_empty() {
        return Vec::new();
    }

    let current_module_name = common_module_helpers::find_common_module_for_file_anywhere(ctx)
        .map(|m| m.name().to_string());

    let is_current_privileged = current_module_name
        .as_ref()
        .is_some_and(|name| privileged_modules.contains(&name.to_lowercase()));

    if !validate_nested_calls && is_current_privileged {
        return Vec::new();
    }

    let summary = ctx.call_summary(hir::ModuleId::new(ctx.file_id));

    // Single registry per check(); allocation cost amortised across
    // every flagged call below.
    let guard_registry = default_registry();
    // Lazy fetches: we only consult the body / cfg batches when we
    // actually have a candidate flag to suppress. For modules with
    // no privileged calls, neither structure is touched.
    let mut module_bodies: Option<Arc<hir::ModuleBodies>> = None;
    let mut module_cfgs: Option<Arc<hir::cfg::ModuleCfgs>> = None;

    let mut diagnostics = Vec::new();

    for edge in &summary.call_edges {
        if edge.kind != EdgeKind::DirectQualifiedModule {
            continue;
        }

        if let CallTarget::QualifiedModule { module_name, method_name } = &edge.target {
            let module_lower = module_name.as_str().to_lowercase();

            if !privileged_modules.contains(&module_lower) {
                continue;
            }

            if !validate_nested_calls {
                if let Some(ref current) = current_module_name {
                    if current.to_lowercase() == module_lower {
                        continue;
                    }
                }
            }

            let resolution = ctx.resolve_qualified_path(module_name, method_name);
            if matches!(resolution, PathResolution::Method(_)) {
                // Track 2 §1.6 Group D: guard-predicate suppression.
                // Calls dominated by `РольДоступна(...) Тогда`-style
                // checks are an authorised pattern — emitting a
                // diagnostic for them is noise.
                if is_call_guarded(ctx, edge, &guard_registry, &mut module_bodies, &mut module_cfgs)
                {
                    continue;
                }

                diagnostics.push(Diagnostic {
                    code,
                    message: format!(
                        "Проверьте обращение к методу {} привилегированного модуля",
                        method_name.as_str()
                    ),
                    severity: ctx.severity(code),
                    range: edge.range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
            }
        }
    }

    diagnostics
}

/// Track 2 §1.6 Group D — query the §1.5 backward must-be-guarded
/// detector for the call edge in question.
///
/// Returns `true` only when every path from the caller method's CFG
/// entry to the basic block containing this call passes through a
/// recognised guard predicate's TRUE branch (see
/// [`hir::dataflow::guard_predicates::is_stmt_guarded`]).
///
/// Module-code edges (`CallerId::ModuleCode`) are not analysed today —
/// the §1.5 detector takes a method's CFG, and module-level code
/// has its own separate graph. Returns `false` for those, preserving
/// the legacy behaviour (no suppression at module scope).
///
/// Returns `false` on any internal failure (no CFG, no body, range
/// not found in any statement). Conservative direction: when in
/// doubt, surface the diagnostic. False negatives here would silently
/// suppress real warnings.
fn is_call_guarded(
    ctx: &DiagnosticsContext,
    edge: &CallEdge,
    registry: &GuardRegistry,
    bodies_cache: &mut Option<Arc<hir::ModuleBodies>>,
    cfgs_cache: &mut Option<Arc<hir::cfg::ModuleCfgs>>,
) -> bool {
    let CallerId::Method(caller_local_id) = edge.caller else {
        return false;
    };

    let bodies = bodies_cache.get_or_insert_with(|| ctx.module_bodies()).clone();
    let cfgs = cfgs_cache.get_or_insert_with(|| ctx.module_cfgs()).clone();

    let body = match bodies.body(caller_local_id) {
        Some(b) => b,
        None => return false,
    };
    let source_map = match bodies.source_map(caller_local_id) {
        Some(s) => s,
        None => return false,
    };
    let cfg = match cfgs.get(caller_local_id) {
        Some(c) => c,
        None => return false,
    };

    let stmt_id = match find_stmt_containing(body, source_map, edge.range) {
        Some(s) => s,
        None => return false,
    };

    is_stmt_guarded(cfg, body, stmt_id, registry)
}

/// Find the [`hir::StmtId`] whose source range contains `target`,
/// preferring the smallest-range candidate.
///
/// HIR `Stmt::If` / loops / `Stmt::Try` carry nested statement lists
/// that share the same arena (Codex §1.6 Group D MINOR fix), so a
/// call buried inside an If's then-branch is contained both by the
/// inner branch's leaf statement AND by the outer If's full source
/// range. Picking the smallest containing range zeroes in on the
/// leaf statement, which is the one the CFG models as a
/// `BasicBlockVertex` (compound statements like `If` are
/// `Conditional` vertices that don't list statements). Without this
/// preference, [`is_stmt_guarded`] would key off the outer compound
/// statement and `find_block_containing` inside it would fail to
/// find a basic block — surfacing the diagnostic conservatively
/// (the "first containing" version), but missing legitimate
/// suppressions that the smallest-range version catches.
fn find_stmt_containing(
    body: &hir::Body,
    source_map: &hir::BodySourceMap,
    target: TextRange,
) -> Option<hir::StmtId> {
    let mut best: Option<(hir::StmtId, TextRange)> = None;
    for (stmt_id, _) in body.stmts_iter() {
        let Some(stmt_range) = source_map.stmt_range(stmt_id) else { continue };
        if !stmt_range.contains_range(target) {
            continue;
        }
        match best {
            Some((_, best_range)) if best_range.len() <= stmt_range.len() => {}
            _ => best = Some((stmt_id, stmt_range)),
        }
    }
    best.map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{FileId, FileSet, VfsPath};
    #[test]
    fn test_no_metadata_returns_empty() {
        let code = r#"
Процедура Тест()
    ПривилегированныйМодуль.Метод();
КонецПроцедуры
"#;
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        let vfs_path = VfsPath::new("/test/Module.bsl");
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, code);

        let config = DiagnosticsConfig::default();

        let provider = ide_db::SalsaProvider::new(&db, None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        assert!(diagnostics.is_empty(), "No metadata should return empty diagnostics");
    }

    /// Track 2 §1.7-A — e2e: direct unguarded call to a privileged
    /// CommonModule method emits the diagnostic. Validates the full
    /// pipeline: Configuration → visible_configurations →
    /// is_privileged filter → call_summary edge → resolve_qualified_path
    /// → emit.
    const PRIVILEGED_MODULE_BODY: &str = r#"
Процедура Метод() Экспорт
КонецПроцедуры
"#;

    #[test]
    fn test_direct_unguarded_call_emits() {
        let code = r#"
Процедура Тест()
    ПривилегМодуль.Метод();
КонецПроцедуры
"#;
        let diagnostics = crate::test_utils::check_diagnostic_with_privileged_modules(
            code,
            &[("ПривилегМодуль", PRIVILEGED_MODULE_BODY)],
            check,
        );
        let priv_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PrivilegedModuleMethodCall)
            .collect();
        assert_eq!(priv_diags.len(), 1, "direct call to privileged module must emit");
    }

    /// Track 2 §1.7-A — e2e: a privileged-module call dominated by a
    /// `РольДоступна("Чтение")` guard is suppressed by the §1.6 Group D
    /// guard-predicate detector. The full pipeline is exercised end-to-
    /// end here (Configuration → visible_configurations → call_summary →
    /// guard_predicates::is_stmt_guarded → suppression).
    #[test]
    fn test_role_guarded_call_suppressed() {
        let code = r#"
Процедура Тест()
    Если РольДоступна("Чтение") Тогда
        ПривилегМодуль.Метод();
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = crate::test_utils::check_diagnostic_with_privileged_modules(
            code,
            &[("ПривилегМодуль", PRIVILEGED_MODULE_BODY)],
            check,
        );
        let priv_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PrivilegedModuleMethodCall)
            .collect();
        assert!(
            priv_diags.is_empty(),
            "RoleCheck-guarded privileged call must be suppressed; got {} diagnostic(s)",
            priv_diags.len()
        );
    }

    #[test]
    fn test_disabled_config() {
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        let vfs_path = VfsPath::new("/test/Module.bsl");
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, "");

        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::PrivilegedModuleMethodCall);

        let provider = ide_db::SalsaProvider::new(&db, None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        assert!(diagnostics.is_empty(), "Disabled config should return empty diagnostics");
    }
}
