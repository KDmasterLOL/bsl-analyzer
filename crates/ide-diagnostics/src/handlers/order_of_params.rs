//! OrderOfParams diagnostic.
//!
//! Detects methods where optional parameters (with default values) appear before required ones.
//!
//! ## Why?
//! Optional parameters should come after required parameters for clarity and consistency.
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест(Раз, Два = 2, Три, Четыре)
//!     // необязательный Два перед обязательными Три и Четыре
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Тест(Раз, Три, Четыре, Два = 2)
//!     // все обязательные параметры идут первыми
//! КонецПроцедуры
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir_def::item_tree::ModItem;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::OrderOfParams) {
        return Vec::new();
    }

    let item_tree = ctx.item_tree();
    let mut diagnostics = Vec::new();

    for item in item_tree.top_level_items() {
        match item {
            ModItem::Procedure(idx) => {
                let proc = item_tree.procedure(*idx);
                check_method(&proc.params, &mut diagnostics);
            }
            ModItem::Function(idx) => {
                let func = item_tree.function(*idx);
                check_method(&func.params, &mut diagnostics);
            }
            ModItem::Variable(_) => {}
        }
    }

    diagnostics
}

fn check_method(params: &[hir_def::item_tree::Param], diagnostics: &mut Vec<Diagnostic>) {
    // Find first optional parameter
    let first_optional_idx = params.iter().position(|p| p.has_default);
    let Some(first_optional_idx) = first_optional_idx else {
        return; // No optional params - nothing to check
    };

    // Report each required parameter that comes after an optional one
    for param in params.iter().skip(first_optional_idx + 1) {
        if !param.has_default {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::OrderOfParams,
                message: format!(
                    "Переместите обязательный параметр '{}' перед необязательными",
                    param.name.as_str()
                ),
                severity: Severity::Major,
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
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    use crate::{DiagnosticCode, Severity};

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/OrderOfParamsDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // "СработкаПоНеобязательныйПередОбязательным(Раз, Два = 2, Три = 3, Четыре, Пять, Шесть, Семь=7)"
        // Required after optional: Четыре, Пять, Шесть
        assert_eq!(diagnostics.len(), 3);
        assert_eq!(diagnostics[0].code, DiagnosticCode::OrderOfParams);
        assert_eq!(diagnostics[0].severity, Severity::Major);
        assert!(diagnostics[0].message.contains("Четыре"));
        assert!(diagnostics[1].message.contains("Пять"));
        assert!(diagnostics[2].message.contains("Шесть"));
    }

    #[test]
    fn test_no_params() {
        let code = r#"Процедура Тест() КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_all_required() {
        let code = r#"Процедура Тест(А, Б, В) КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_all_optional() {
        let code = r#"Процедура Тест(А = 1, Б = 2, В = 3) КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_correct_order() {
        let code = r#"Процедура Тест(А, Б, В = 3, Г = 4) КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_wrong_order_single() {
        let code = r#"Процедура Тест(А, Б = 2, В)
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        // В is required after optional Б
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, &diagnostics[0], 0, 25, 26); // В
    }

    #[test]
    fn test_wrong_order_multiple() {
        let code = r#"Процедура Тест(А = 1, Б, В)
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        // Б and В are required after optional А
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics[0].message.contains("Б"));
        assert!(diagnostics[1].message.contains("В"));
    }
}
