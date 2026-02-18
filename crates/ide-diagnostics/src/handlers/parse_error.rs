use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::SyntaxKind;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ParseError;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();

    root.descendants()
        .filter(|node| node.kind() == SyntaxKind::ERROR && !node.text_range().is_empty())
        .map(|node| Diagnostic {
            code,
            message: "Ошибка разбора исходного кода".to_string(),
            severity: ctx.severity(code),
            range: node.text_range(),
            tags: ctx.tags(code),
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

    #[test]
    fn test_no_parse_error_for_bom() {
        // UTF-8 BOM at start of file should not trigger ParseError
        let code = "\u{FEFF}Процедура Тест()\n    А = 1;\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(parse_errors.is_empty(), "BOM should not trigger parse error");
    }

    #[test]
    fn test_no_parse_error_for_bom_with_region() {
        // UTF-8 BOM + CRLF + #Область (common in 1C exports)
        let code =
            "\u{FEFF}\r\n#Область Test\r\nПроцедура Тест()\r\nКонецПроцедуры\r\n#КонецОбласти";
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(parse_errors.is_empty(), "BOM with region should not trigger parse error");
    }
}
