//! ReservedParameterNames diagnostic.
//!
//! Detects parameter names that match configured reserved words list.
//!
//! ## Configuration
//! - **reservedWords** (default: []) - List of reserved words
//! - **Enabled by default:** Yes (but does nothing without configuration)
//! - **Severity:** MAJOR → Warning

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir_def::item_tree::ModItem;
use rustc_hash::FxHashSet;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::ReservedParameterNames) {
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
                check_params(&proc.params, &reserved_words, &mut diagnostics);
            }
            ModItem::Function(idx) => {
                let func = item_tree.function(*idx);
                check_params(&func.params, &reserved_words, &mut diagnostics);
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
    params: &[hir_def::item_tree::Param],
    reserved_words: &FxHashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for param in params {
        let name_lower = param.name.as_str().to_lowercase();
        if reserved_words.contains(&name_lower) {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::ReservedParameterNames,
                message: format!(
                    "Переименуйте параметр '{}' так, чтобы он не совпадал с зарезервированным словом.",
                    param.name.as_str()
                ),
                severity: Severity::Warning,
                range: param.name_range,
                tags: vec![],
                fixes: vec![],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic_with_config};
    use crate::{DiagnosticCode, DiagnosticsConfig};

    #[test]
    fn test_empty_list_no_diagnostics() {
        let code = r#"Процедура Тест(ВидГруппыФормы)
КонецПроцедуры"#;
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ReservedParameterNames)
            .collect();
        assert_eq!(diags.len(), 0);
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
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ReservedParameterNames)
            .collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 0, 15, 29);
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
        assert_eq!(
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ReservedParameterNames).count(),
            1
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
        assert_eq!(
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ReservedParameterNames).count(),
            2
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
        assert_eq!(
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ReservedParameterNames).count(),
            0
        );
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
        assert_eq!(
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ReservedParameterNames).count(),
            1
        );
    }
}
