//! Reports parameters whose names match a configured reserved-name list.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::ModItem;
use rustc_hash::FxHashSet;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Checks procedure and function parameters against the configured reserved-name list.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ReservedParameterNames;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let reserved_words = get_reserved_words(ctx);
    if reserved_words.is_empty() {
        return Vec::new();
    }

    let item_tree = ctx.item_tree();
    let mut diagnostics = Vec::new();

    for item in item_tree.top_level_items() {
        match item {
            ModItem::Procedure(idx) => {
                let proc = item_tree.procedure(*idx);
                check_params(&proc.params, &reserved_words, code, ctx, &mut diagnostics);
            }
            ModItem::Function(idx) => {
                let func = item_tree.function(*idx);
                check_params(&func.params, &reserved_words, code, ctx, &mut diagnostics);
            }
            ModItem::Variable(_) => {}
        }
    }

    diagnostics
}

fn get_reserved_words(ctx: &DiagnosticsContext) -> FxHashSet<String> {
    ctx.config
        .get_string_array(DiagnosticCode::ReservedParameterNames, "reservedWords")
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.to_lowercase())
        .collect()
}

fn check_params(
    params: &[hir::Param],
    reserved_words: &FxHashSet<String>,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for param in params {
        let name_lower = param.name.as_str().to_lowercase();
        if reserved_words.contains(&name_lower) {
            diagnostics.push(Diagnostic {
                code,
                message: format!(
                    "Переименуйте параметр '{}' так, чтобы он не совпадал с зарезервированным словом.",
                    param.name.as_str()
                ),
                severity: ctx.severity(code),
                range: param.name_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic_with_config, format_diags};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;

    fn check_reserved_parameter_names_snapshot(
        code: &str,
        diagnostics: Vec<crate::Diagnostic>,
        expected: expect_test::Expect,
    ) {
        let diagnostics = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::ReservedParameterNames)
            .collect::<Vec<_>>();
        expected.assert_eq(&format_diags(code, &diagnostics));
    }
    #[test]
    fn test_empty_list_no_diagnostics() {
        let code = r#"Процедура Тест(ВидГруппыФормы)
КонецПроцедуры"#;
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        check_reserved_parameter_names_snapshot(code, diagnostics, expect![[r#""#]]);
    }

    #[test]
    fn test_matching_word() {
        let code = r#"Процедура Тест(ВидГруппыФормы)
КонецПроцедуры"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::ReservedParameterNames,
            serde_json::json!({ "reservedWords": ["ВидГруппыФормы"] }),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        check_reserved_parameter_names_snapshot(
            code,
            diagnostics,
            expect![[r#"
            ReservedParameterNames @ 1:16..1:30
              message: Переименуйте параметр 'ВидГруппыФормы' так, чтобы он не совпадал с зарезервированным словом.
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"Процедура Тест(видгруппыформы)
КонецПроцедуры"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::ReservedParameterNames,
            serde_json::json!({ "reservedWords": ["ВидГруппыФормы"] }),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        check_reserved_parameter_names_snapshot(
            code,
            diagnostics,
            expect![[r#"
            ReservedParameterNames @ 1:16..1:30
              message: Переименуйте параметр 'видгруппыформы' так, чтобы он не совпадал с зарезервированным словом.
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_multiple_reserved_words() {
        let code = r#"Процедура Тест(ВидГруппыФормы, СтрокаТаблицы)
КонецПроцедуры"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::ReservedParameterNames,
            serde_json::json!({ "reservedWords": ["ВидГруппыФормы", "СтрокаТаблицы"] }),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        check_reserved_parameter_names_snapshot(
            code,
            diagnostics,
            expect![[r#"
            ReservedParameterNames @ 1:16..1:30
              message: Переименуйте параметр 'ВидГруппыФормы' так, чтобы он не совпадал с зарезервированным словом.
              severity: Warning
            ReservedParameterNames @ 1:32..1:45
              message: Переименуйте параметр 'СтрокаТаблицы' так, чтобы он не совпадал с зарезервированным словом.
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_partial_match_not_detected() {
        let code = r#"Процедура Тест(ВидГруппыФормыРасширенный)
КонецПроцедуры"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::ReservedParameterNames,
            serde_json::json!({ "reservedWords": ["ВидГруппыФормы"] }),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        check_reserved_parameter_names_snapshot(code, diagnostics, expect![[r#""#]]);
    }

    #[test]
    fn test_function_params() {
        let code = r#"Функция Тест(ВидГруппыФормы)
    Возврат 1;
КонецФункции"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::ReservedParameterNames,
            serde_json::json!({ "reservedWords": ["ВидГруппыФормы"] }),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        check_reserved_parameter_names_snapshot(
            code,
            diagnostics,
            expect![[r#"
            ReservedParameterNames @ 1:14..1:28
              message: Переименуйте параметр 'ВидГруппыФормы' так, чтобы он не совпадал с зарезервированным словом.
              severity: Warning"#]],
        );
    }
}
