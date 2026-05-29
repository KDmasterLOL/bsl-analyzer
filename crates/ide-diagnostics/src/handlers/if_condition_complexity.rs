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

const DEFAULT_MAX_IF_CONDITION_COMPLEXITY: i64 = 3;

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

    let mut local_ids: Vec<u32> = module_bodies.iter_bodies().map(|(id, _)| id).collect();
    local_ids.sort_unstable();

    let mut out = Vec::new();
    for local_id in local_ids {
        let Some(metrics) = module_metrics.get(local_id) else { continue };
        if metrics.if_conditions.is_empty() {
            continue;
        }
        let Some(source_map) = module_bodies.source_map(local_id) else { continue };
        emit_conditions(ctx, code, &metrics, source_map, &file_text, max_complexity, &mut out);
    }
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

fn trim_trailing_whitespace(file_text: &str, range: TextRange) -> TextRange {
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    if end > file_text.len() || start >= end {
        return range;
    }
    let slice = &file_text[start..end];
    let trimmed_len = slice.trim_end().len();
    if trimmed_len == slice.len() || trimmed_len == 0 {
        return range;
    }
    let new_end = range.start() + TextSize::from(trimmed_len as u32);
    TextRange::new(range.start(), new_end)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;
    #[test]
    fn test_simple_condition() {
        let code = r#"Процедура Тест()
    Если А И Б Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfConditionComplexity,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_at_threshold() {
        let code = r#"Процедура Тест()
    Если А И Б ИЛИ В Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfConditionComplexity,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_complex_condition() {
        let code = r#"Процедура Тест()
    Если А И Б ИЛИ В И Г Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfConditionComplexity,
            expect![[r#"
                IfConditionComplexity @ 2:10..2:25
                  message: Условие имеет сложность 4 (максимум 3). Упростите условие или вынесите части в переменные.
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_elseif_complex() {
        let code = r#"Процедура Тест()
    Если А Тогда
        Сообщить("1");
    ИначеЕсли Б И В ИЛИ Г И Д Тогда
        Сообщить("2");
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfConditionComplexity,
            expect![[r#"
                IfConditionComplexity @ 4:15..4:30
                  message: Условие имеет сложность 4 (максимум 3). Упростите условие или вынесите части в переменные.
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_english_condition() {
        let code = r#"Procedure Test()
    If A And B Or C And D Then
        Message("OK");
    EndIf;
EndProcedure"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfConditionComplexity,
            expect![[r#"
                IfConditionComplexity @ 2:8..2:26
                  message: Условие имеет сложность 4 (максимум 3). Упростите условие или вынесите части в переменные.
                  severity: Information"#]],
        );
    }

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

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfConditionComplexity,
            expect![[r#"
                IfConditionComplexity @ 2:10..10:56
                  message: Условие имеет сложность 9 (максимум 3). Упростите условие или вынесите части в переменные.
                  severity: Information"#]],
        );
    }

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

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfConditionComplexity,
            expect![[r#"
                IfConditionComplexity @ 4:14..7:60
                  message: Условие имеет сложность 4 (максимум 3). Упростите условие или вынесите части в переменные.
                  severity: Information"#]],
        );
    }

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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();
        expect![[r#"
            IfConditionComplexity @ 2:10..2:15
              message: Условие имеет сложность 2 (максимум 1). Упростите условие или вынесите части в переменные.
              severity: Information"#]].assert_eq(&format_diags(code, &if_diags));
    }

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

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfConditionComplexity,
            expect![[r#"
                IfConditionComplexity @ 2:10..5:41
                  message: Условие имеет сложность 4 (максимум 3). Упростите условие или вынесите части в переменные.
                  severity: Information
                IfConditionComplexity @ 7:15..13:42
                  message: Условие имеет сложность 7 (максимум 3). Упростите условие или вынесите части в переменные.
                  severity: Information"#]],
        );
    }
}
