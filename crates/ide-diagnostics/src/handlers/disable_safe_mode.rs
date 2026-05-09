//! Reports calls that disable or weaken safe mode.
//!
//! # Track 2 §1.6 Group C
//!
//! Detection moved out of HIR lowering into this handler's [`check`]
//! function, consuming the §1.2 saturating-counter lattice + §1.3 const-
//! propagation overlay through
//! [`hir::dataflow::security_state::open_events`]. The lattice
//! understands both call shapes via the curated security registry:
//! `УстановитьБезопасныйРежим(Ложь)` and
//! `УстановитьОтключениеБезопасногоРежима(Истина)` both push the
//! unsafe-frame counter; their opposite-polarity arguments pop it. The
//! handler emits one diagnostic per yielded `OpenEvent` whose category
//! is [`Category::SafeMode`].
//!
//! Const-prop precision: `Значение = Ложь;
//! УстановитьБезопасныйРежим(Значение)` is folded to `KnownFalse` and
//! emits (lattice categorises it as opening the unsafe frame, just like
//! the literal-`Ложь` form).
//!
//! # Coverage
//!
//! Both per-method bodies AND module-level top-level code are scanned —
//! see [`hir::dataflow::security_state::open_events`] for the lattice
//! event surface. Parity with the legacy HIR-side detector is preserved.

use std::sync::Arc;

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::dataflow::security_state::{open_events, Category};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Track 2 §1.6 Group C — lattice-driven detection. Replaces the old
/// `from_hir(method_name, range, ctx)` adapter.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::DisableSafeMode;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let module_security: Arc<ide_db::effects::ModuleSecurityState> = ctx.module_security_state();
    if module_security.is_empty() {
        return Vec::new();
    }
    let module_bodies = ctx.module_bodies();

    let mut diagnostics = Vec::new();
    for (local_id, _body) in module_bodies.iter_bodies() {
        let Some(result) = module_security.get(local_id) else { continue };
        let Some(source_map) = module_bodies.source_map(local_id) else { continue };
        emit_for_result(&result, source_map, code, ctx, &mut diagnostics);
    }
    if let Some(result) = module_security.module_level() {
        if let Some(lower_result) = module_bodies.module_code_result() {
            emit_for_result(&result, &lower_result.source_map, code, ctx, &mut diagnostics);
        }
    }
    // Codex round-1 NIT: emit in source order.
    diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));
    diagnostics
}

fn emit_for_result(
    result: &hir::dataflow::DataflowResult<hir::dataflow::security_state::SecurityModeState>,
    source_map: &hir::BodySourceMap,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    out: &mut Vec<Diagnostic>,
) {
    let body = result.body();
    for event in open_events(result) {
        if !matches!(event.category, Category::SafeMode) {
            continue;
        }
        let Some(range) = source_map.expr_range(event.callee) else { continue };
        let method_name = match body.expr(event.callee) {
            hir::Expr::Path(name) => name.as_str(),
            _ => continue,
        };
        out.push(Diagnostic {
            code,
            message: get_message(method_name),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

fn get_message(method_name: &str) -> String {
    let lower = method_name.to_lowercase();
    match lower.as_str() {
        "установитьбезопасныйрежим" | "setsafemode" => {
            "Отключение безопасного режима создает уязвимость безопасности. \
             Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)"
                .to_string()
        }
        "установитьотключениебезопасногорежима" | "setsafemodedisabled" => {
            "Отключение безопасного режима через УстановитьОтключениеБезопасногоРежима \
             создает уязвимость безопасности"
                .to_string()
        }
        _ => "Отключение безопасного режима создает уязвимость безопасности".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::Severity;
    #[test]
    fn test_set_safe_mode_false() {
        let code = r#"
Процедура Тест()
    УстановитьБезопасныйРежим(Ложь);
КонецПроцедуры
"#;
        let diagnostics = check_dataflow_diagnostic(code, check);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 1);
        assert_eq!(safe_mode_diags[0].severity, Severity::Major); // VULNERABILITY + MAJOR maps to Major
    }

    #[test]
    fn test_set_safe_mode_true() {
        let code = r#"
Процедура Тест()
    УстановитьБезопасныйРежим(Истина);
КонецПроцедуры
"#;
        let diagnostics = check_dataflow_diagnostic(code, check);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 0);
    }

    #[test]
    fn test_set_safe_mode_variable() {
        let code = r#"
Процедура Тест()
    Значение = Ложь;
    УстановитьБезопасныйРежим(Значение);
КонецПроцедуры
"#;
        let diagnostics = check_dataflow_diagnostic(code, check);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 1);
    }

    #[test]
    fn test_set_disabled_true() {
        let code = r#"
Процедура Тест()
    УстановитьОтключениеБезопасногоРежима(Истина);
КонецПроцедуры
"#;
        let diagnostics = check_dataflow_diagnostic(code, check);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 1);
    }

    #[test]
    fn test_set_disabled_false() {
        let code = r#"
Процедура Тест()
    УстановитьОтключениеБезопасногоРежима(Ложь);
КонецПроцедуры
"#;
        let diagnostics = check_dataflow_diagnostic(code, check);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 0);
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Процедура Тест()
    Модуль.УстановитьБезопасныйРежим(Ложь);
КонецПроцедуры
"#;
        let diagnostics = check_dataflow_diagnostic(code, check);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 0);
    }

    #[test]
    fn test_bilingual() {
        let code = r#"
Процедура Тест()
    SetSafeMode(False);
    SetSafeModeDisabled(True);
КонецПроцедуры
"#;
        let diagnostics = check_dataflow_diagnostic(code, check);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 2);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    УСТАНОВИТЬБЕЗОПАСНЫЙРЕЖИМ(ЛОЖЬ);
    установитьбезопасныйрежим(ложь);
КонецПроцедуры
"#;
        let diagnostics = check_dataflow_diagnostic(code, check);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 2);
    }

    /// Track 2 §1.6 Group C — Codex round-2 stop-hook regression
    /// guard: nested security calls (e.g. as a function argument)
    /// must still surface, matching legacy `lower_call_expr` behaviour
    /// which fired on every CALL_EXPR regardless of nesting depth.
    #[test]
    fn test_nested_call_in_argument_emits() {
        let code = r#"
Процедура Тест()
    Сообщить(УстановитьБезопасныйРежим(Ложь));
КонецПроцедуры
"#;
        let diagnostics = check_dataflow_diagnostic(code, check);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();
        assert_eq!(
            safe_mode_diags.len(),
            1,
            "nested SetSafeMode(Ложь) inside function arg must emit"
        );
    }

    #[test]
    fn test_all_four_patterns_in_procedure() {
        // Covers all 4 triggering patterns from the original fixture:
        // SetSafeMode(False), SetSafeMode(variable), SetSafeModeDisabled(True), SetSafeModeDisabled(variable)
        // SetSafeMode(True) and SetSafeModeDisabled(False) do NOT trigger.
        let code = r#"&НаСервере
Процедура Метод()
    УстановитьБезопасныйРежим(Ложь);

    Значение = Ложь;
    УстановитьБезопасныйРежим(Значение);

    УстановитьБезопасныйРежим(Истина);

    УстановитьОтключениеБезопасногоРежима(Истина);

    Значение = Истина;
    УстановитьОтключениеБезопасногоРежима(Значение);

    УстановитьОтключениеБезопасногоРежима(Ложь);
КонецПроцедуры
"#;
        let diagnostics = check_dataflow_diagnostic(code, check);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 4, "Expected 4 diagnostics");
    }
}
