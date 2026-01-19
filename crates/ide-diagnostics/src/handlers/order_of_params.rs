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
use ide_db::TextRange;

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
                check_method(&proc.params, proc.param_list_range, &mut diagnostics);
            }
            ModItem::Function(idx) => {
                let func = item_tree.function(*idx);
                check_method(&func.params, func.param_list_range, &mut diagnostics);
            }
            ModItem::Variable(_) => {}
        }
    }

    diagnostics
}

fn check_method(
    params: &[hir_def::item_tree::Param],
    param_list_range: Option<TextRange>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Java logic: dropWhile(Objects::isNull).anyMatch(Objects::isNull)
    // Skip required params until first optional, then check if any required follow
    let has_required_after_optional = params
        .iter()
        .map(|p| p.has_default)
        .skip_while(|&has_default| !has_default)
        .any(|has_default| !has_default);

    if has_required_after_optional {
        let Some(range) = param_list_range else {
            return;
        };

        diagnostics.push(Diagnostic {
            code: DiagnosticCode::OrderOfParams,
            message: "Переместите необязательные параметры после обязательных".into(),
            severity: Severity::Major,
            range,
            tags: vec![],
            fixes: vec![],
        });
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

        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, &diagnostics[0], 14, 52, 102);
        assert_eq!(diagnostics[0].code, DiagnosticCode::OrderOfParams);
        assert_eq!(diagnostics[0].severity, Severity::Major);
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
    fn test_wrong_order() {
        let code = r#"Процедура Тест(А, Б = 2, В) КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }
}
