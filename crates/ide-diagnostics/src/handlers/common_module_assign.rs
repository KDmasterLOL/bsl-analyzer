//! CommonModuleAssign diagnostic
//!
//! Cannot assign value to CommonModule (will cause runtime error).
//!

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::traits::MdObject;
use hir::{AssignmentResolution, ExistingBindingKind};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from `hir_dispatch::dispatch_body_diagnostic` when
/// `BodyDiagnostic::CommonModuleAssign` fires.
///
/// ## Resolution sequence (Track 1 §4.6)
///
/// 1. Disabled? — return.
/// 2. Local/Param shadowing fast-path: if Step L's
///    `existing_binding_kind` payload reports `Some(_)`, the LHS is a
///    real local or parameter that shadows any CommonModule of the
///    same name — suppress without rebuilding a `Resolver`.
/// 3. Resolver pass: build `Resolver::for_module(...)` (no expression
///    scopes — locals are covered by the fast-path) and classify the
///    name. We only emit when it resolves to `CommonModule`. The
///    `ModuleVariable` arm catches a module-level `Перем` that
///    shadows a same-named CommonModule — a case Step L's payload
///    cannot see (lowering's `local_vars` / `param_names` tables
///    don't track module-level vars). The `Unknown` arm catches names
///    that simply don't refer to anything visible.
///
/// Streaming providers default the resolver pass to `Unknown` and
/// therefore suppress the diagnostic — that's intentional, since
/// without configuration access we cannot prove the name is a
/// CommonModule.
pub fn from_hir(
    variable_name: &str,
    range: TextRange,
    existing_binding_kind: Option<ExistingBindingKind>,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::CommonModuleAssign;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    if existing_binding_kind.is_some() {
        return None;
    }

    match ctx.assignment_target_kind(variable_name) {
        AssignmentResolution::CommonModule(_) => {}
        AssignmentResolution::Local
        | AssignmentResolution::Param
        | AssignmentResolution::ModuleVariable(_) => return None,
        AssignmentResolution::Unknown => {
            // Streaming providers default `assignment_target_kind` to
            // `Unknown` (no resolver available). Without a resolver
            // pass, "Unknown" is ambiguous — the name could be either
            // unrelated to anything visible (genuine suppress) **or** a
            // real CommonModule we just couldn't classify. Falling
            // back to `is_common_module_anywhere` recovers the Step M
            // (CFE-aware metadata-only) behaviour for streaming mode
            // so it keeps emitting on real CommonModule assignments;
            // for Salsa-backed providers the resolver returns
            // `CommonModule(_)` directly and this arm is unreachable
            // for valid CommonModule names.
            if !ctx.is_common_module_anywhere(variable_name) {
                return None;
            }
        }
    }

    // Fetch the canonical-cased CommonModule name from metadata so the
    // diagnostic message renders with the configuration's spelling
    // (`СвойМодуль`) rather than echoing whatever case the user typed
    // (`свОйМОдуль`).
    let display_name = ctx
        .find_common_module_anywhere(variable_name)
        .map(|(_visible, common_module)| common_module.name().to_string())
        .unwrap_or_else(|| variable_name.to_string());

    Some(Diagnostic {
        code,
        message: format!("Недопустимо присваивание значения общему модулю '{}'", display_name),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_hir_diagnostic, format_diags};
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_no_metadata() {
        // Without metadata, no CommonModuleAssign diagnostics should be emitted
        let code = r#"Процедура Тест()
    СвойМодуль = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let common_module_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::CommonModuleAssign)
            .collect();

        // No metadata available, so no diagnostics
        expect![[r#""#]].assert_eq(&format_diags(code, &common_module_diags));
    }

    #[test]
    fn test_property_access_no_diagnostic() {
        // Property access (field expression) should NOT trigger diagnostic
        let code = r#"Процедура Тест()
    СвойМодуль.Свойство = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let common_module_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::CommonModuleAssign)
            .collect();

        // Field access is not a simple identifier assignment
        expect![[r#""#]].assert_eq(&format_diags(code, &common_module_diags));
    }

    #[test]
    fn test_index_access_no_diagnostic() {
        // Index access should NOT trigger diagnostic
        let code = r#"Процедура Тест()
    Массив[0] = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let common_module_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::CommonModuleAssign)
            .collect();

        // Index access is not a simple identifier assignment
        expect![[r#""#]].assert_eq(&format_diags(code, &common_module_diags));
    }

    #[test]
    fn test_simple_variable_emits_candidate() {
        // Simple variable assignment should emit a candidate (filtered by metadata later)
        let code = r#"Процедура Тест()
    А = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        // Without metadata, candidates are filtered out
        let common_module_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::CommonModuleAssign)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &common_module_diags));
    }
}
