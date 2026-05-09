//! IfConditionComplexity diagnostic.
//!
//! Detects overly complex if conditions with too many boolean operations.
//!
//! ## Why?
//! Complex if conditions are hard to understand:
//! - Reduced readability
//! - Difficult to debug
//! - Error-prone
//! - Should be extracted to variables
//!
//! ## Bad practice
//! ```bsl
//! Если А И Б ИЛИ В И Г Тогда  // Too complex!
//!     ВыполнитьДействие();
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! ```bsl
//! УсловиеВыполнено = (А И Б) ИЛИ (В И Г);
//! Если УсловиеВыполнено Тогда
//!     ВыполнитьДействие();
//! КонецЕсли;
//! ```
//!
//! ## Track 2 Phase B §6.4 migration
//! Pre-migration the legacy `from_hir` adapter consumed
//! `BodyDiagnostic::IfConditionComplexity`, which was emitted from
//! `lower::stmt::check_condition_complexity` once per `Если`/`ИначеЕсли`
//! condition that exceeded a hardcoded default threshold of 3. That
//! per-condition emit pattern is preserved here: the `compute_hir_metrics`
//! visitor records a `ConditionMetrics { condition: ExprId, logical_op_count }`
//! entry for every `If`/`Elsif` condition, and this handler replays the
//! threshold filter directly against the cached `module_hir_metrics_query`
//! data — one diagnostic per over-budget condition, attached to the
//! condition's source range trimmed of trailing whitespace.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use text_size::TextSize;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Default maximum if condition complexity (legacy `DEFAULT_MAX_COMPLEXITY`).
const DEFAULT_MAX_IF_CONDITION_COMPLEXITY: i64 = 3;

/// Track 2 Phase B §6.4 — handler-side detection consuming the cached
/// `HirMethodMetrics::if_conditions` via `ctx.module_hir_metrics()`.
/// Emits one diagnostic per `Если`/`ИначеЕсли` condition whose
/// complexity (`logical_op_count + 1`) exceeds `maxIfConditionComplexity`
/// (mirrors the legacy behaviour the retired
/// `lower::stmt::check_condition_complexity` produced).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::IfConditionComplexity;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let max_complexity =
        ctx.config_int(code, "maxIfConditionComplexity", DEFAULT_MAX_IF_CONDITION_COMPLEXITY)
            as u32;

    let module_metrics = ctx.module_hir_metrics();
    if module_metrics.is_empty() {
        return Vec::new();
    }
    let module_bodies = ctx.module_bodies();
    let file_text = ctx.file_text();

    let mut out = Vec::new();
    for (local_id, _body) in module_bodies.iter_bodies() {
        let Some(metrics) = module_metrics.get(local_id) else { continue };
        if metrics.if_conditions.is_empty() {
            continue;
        }
        let Some(source_map) = module_bodies.source_map(local_id) else { continue };
        emit_conditions(ctx, code, &metrics, source_map, &file_text, max_complexity, &mut out);
    }
    // Module-level code: top-level `Если`/`ИначеЕсли` outside any
    // method body. The legacy lowering-time emit ran for these the same
    // way it ran for method bodies — preserve that coverage.
    if let Some(metrics) = module_metrics.module_code() {
        if !metrics.if_conditions.is_empty() {
            if let Some(lower_result) = module_bodies.module_code_result() {
                emit_conditions(
                    ctx,
                    code,
                    &metrics,
                    &lower_result.source_map,
                    &file_text,
                    max_complexity,
                    &mut out,
                );
            }
        }
    }
    out
}

fn emit_conditions(
    ctx: &DiagnosticsContext,
    code: DiagnosticCode,
    metrics: &hir::metrics::HirMethodMetrics,
    source_map: &hir::BodySourceMap,
    file_text: &str,
    max_complexity: u32,
    out: &mut Vec<Diagnostic>,
) {
    for cond in metrics.if_conditions.iter() {
        let complexity = cond.logical_op_count + 1;
        if complexity <= max_complexity {
            continue;
        }
        let Some(raw_range) = source_map.expr_range(cond.condition) else { continue };
        let range = trim_trailing_whitespace(file_text, raw_range);
        out.push(Diagnostic {
            code,
            message: format!(
                "Условие имеет сложность {} (максимум {}). Упростите условие или вынесите части в переменные.",
                complexity, max_complexity
            ),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

/// Mirror of the retired `lower::stmt::get_condition_range`: Rowan
/// records expression node ranges with trailing trivia included; the
/// user-visible diagnostic range should not.
fn trim_trailing_whitespace(file_text: &str, range: TextRange) -> TextRange {
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    if end > file_text.len() || start >= end {
        return range;
    }
    let slice = &file_text[start..end];
    let trimmed_len = slice.trim_end().len();
    if trimmed_len == slice.len() || trimmed_len == 0 {
        // No trailing trivia, or the entire slice is trivia (unreachable
        // for real expressions but stay defensive — never produce a
        // zero-length diagnostic range).
        return range;
    }
    let new_end = range.start() + TextSize::from(trimmed_len as u32);
    TextRange::new(range.start(), new_end)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::{DiagnosticCode, DiagnosticsConfig, Severity};
    /// Test simple condition (should pass)
    #[test]
    fn test_simple_condition() {
        let code = r#"Процедура Тест()
    Если А И Б Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should NOT detect - complexity = 2 (1 AND + 1 = 2)
        assert_eq!(if_diags.len(), 0);
    }

    /// Test at threshold (should pass)
    #[test]
    fn test_at_threshold() {
        let code = r#"Процедура Тест()
    Если А И Б ИЛИ В Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should NOT detect - complexity = 3 (2 ops: AND + OR = 2, complexity = 2+1 = 3)
        assert_eq!(if_diags.len(), 0);
    }

    /// Test complex condition (should fail)
    #[test]
    fn test_complex_condition() {
        let code = r#"Процедура Тест()
    Если А И Б ИЛИ В И Г Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should detect - complexity = 4 (3 ops: AND, OR, AND = 3, complexity = 3+1 = 4)
        assert_eq!(if_diags.len(), 1);
        assert_eq!(if_diags[0].code, DiagnosticCode::IfConditionComplexity);
        assert_eq!(if_diags[0].severity, Severity::Information); // CodeSmell + Minor -> Information
        assert!(if_diags[0].message.contains("сложность 4"));
        assert!(if_diags[0].message.contains("максимум 3"));
    }

    /// Test elsif clause
    #[test]
    fn test_elseif_complex() {
        let code = r#"Процедура Тест()
    Если А Тогда
        Сообщить("1");
    ИначеЕсли Б И В ИЛИ Г И Д Тогда
        Сообщить("2");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should detect in elseif - complexity = 4
        assert_eq!(if_diags.len(), 1);
        assert_eq!(if_diags[0].code, DiagnosticCode::IfConditionComplexity);
    }

    /// Test English keywords
    #[test]
    fn test_english_condition() {
        let code = r#"Procedure Test()
    If A And B Or C And D Then
        Message("OK");
    EndIf;
EndProcedure"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should detect - complexity = 4
        assert_eq!(if_diags.len(), 1);
    }

    /// Large multiline condition (9 OR ops) - should warn
    #[test]
    fn test_large_multiline_condition() {
        let code = r#"Процедура Тест()
    Если ИдентификаторОбъекта = "АнализСубконто"
        ИЛИ ИдентификаторОбъекта = "АнализСчета"
        ИЛИ ИдентификаторОбъекта = "ОборотноСальдоваяВедомость"
        ИЛИ ИдентификаторОбъекта = "ОборотноСальдоваяВедомостьПоСчету"
        ИЛИ ИдентификаторОбъекта = "ОборотыМеждуСубконто"
        ИЛИ ИдентификаторОбъекта = "ОборотыСчета"
        ИЛИ ИдентификаторОбъекта = "СводныеПроводки"
        ИЛИ ИдентификаторОбъекта = "ГлавнаяКнига"
        ИЛИ ИдентификаторОбъекта = "ШахматнаяВедомость" Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        assert_eq!(if_diags.len(), 1, "Should warn on 9-OR condition");
    }

    /// Simple outer condition (2 OR ops) should pass; nested condition (3 OR ops) should warn
    #[test]
    fn test_nested_outer_pass_inner_warn() {
        let code = r#"Процедура Тест()
    Если ИдентификаторОбъекта = "АнализСубконто"
        ИЛИ ИдентификаторОбъекта = "АнализСчета" Тогда
        Если ИдентификаторОбъекта = "ОборотыМеждуСубконто"
            ИЛИ ИдентификаторОбъекта = "ОборотыСчета"
            ИЛИ ИдентификаторОбъекта = "СводныеПроводки"
            ИЛИ ИдентификаторОбъекта = "ШахматнаяВедомость" Тогда
            Возврат;
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        assert_eq!(if_diags.len(), 1, "Only inner nested condition should warn");
    }

    /// Codex round-A regression guard: sub-default `maxIfConditionComplexity`.
    /// Legacy lowering hard-gated emission at `complexity > 3` before the
    /// per-config check, so users who set the threshold below 3 silently
    /// got no diagnostics. The migrated handler applies the config max
    /// directly, so a stricter threshold now fires as expected.
    #[test]
    fn test_sub_default_threshold_emits() {
        let code = r#"Процедура Тест()
    Если А И Б Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::IfConditionComplexity,
            serde_json::json!({ "maxIfConditionComplexity": 1 }),
        );

        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // complexity = 1 AND + 1 = 2; with max=1, fires.
        assert_eq!(if_diags.len(), 1, "complexity=2 must fire when maxIfConditionComplexity=1");
        assert!(if_diags[0].message.contains("сложность 2"));
        assert!(if_diags[0].message.contains("максимум 1"));
    }

    /// If branch (4 OR) and ElseIf branch (6 OR) both exceed threshold
    #[test]
    fn test_if_and_elseif_both_complex() {
        let code = r#"Процедура Тест()
    Если ИдентификаторОбъекта = "ИД1"
        ИЛИ ИдентификаторОбъекта = "ИД2"
        ИЛИ ИдентификаторОбъекта = "ИД3"
        ИЛИ ИдентификаторОбъекта = "ИД4" Тогда
        Возврат;
    ИначеЕсли ИдентификаторОбъекта = "ИД5"
        ИЛИ ИдентификаторОбъекта = "ИД6"
        ИЛИ ИдентификаторОбъекта = "ИД7"
        ИЛИ ИдентификаторОбъекта = "ИД8"
        ИЛИ ИдентификаторОбъекта = "ИД9"
        ИЛИ ИдентификаторОбъекта = "ИД10"
        ИЛИ ИдентификаторОбъекта = "ИД10" Тогда
        Возврат;
    Иначе
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        assert_eq!(if_diags.len(), 2, "Both If and ElseIf branches should warn");
    }
}
