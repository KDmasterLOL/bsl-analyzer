use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::SyntaxKind;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::ParseError) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();

    root.descendants()
        .filter(|node| node.kind() == SyntaxKind::ERROR && !node.text_range().is_empty())
        .map(|node| Diagnostic {
            code: DiagnosticCode::ParseError,
            message: "Ошибка разбора исходного кода".to_string(),
            severity: Severity::Critical,
            range: node.text_range(),
            tags: vec![],
            fixes: vec![],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_parse_error_basic() {
        let code = include_str!("../../test_data/ParseErrorDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, super::check);

        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();

        assert!(!parse_errors.is_empty(), "Expected at least one parse error");
    }

    #[test]
    fn test_no_parse_errors_in_valid_code() {
        let code = r#"
Процедура Тест()
    А = 1;
    Б = 2;
    Возврат А + Б;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert_eq!(parse_errors.len(), 0, "Valid code should have no parse errors");
    }

    #[test]
    fn test_parse_error_if_without_condition() {
        let code = r#"
Процедура Тест()
    Если НЕ Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(!parse_errors.is_empty(), "Expected parse error for 'Если НЕ Тогда'");
    }

    #[test]
    fn test_parse_error_unclosed_string() {
        let code = r#"
Процедура Тест()
    А = "незакрытая строка
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(!parse_errors.is_empty(), "Expected parse error for unclosed string");
    }

    #[test]
    fn test_parse_error_bare_identifier() {
        let code = r#"
Процедура Тест()
КонецПроцедуры
HHH
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(!parse_errors.is_empty(), "Expected parse error for bare identifier 'HHH'");
    }

    #[test]
    fn test_parse_error_eof_fixture() {
        let code = include_str!("../../test_data/ParseErrorDiagnosticEOF.bsl");
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(!parse_errors.is_empty(), "Expected parse error for EOF fixture with 'HHH'");
    }
}
