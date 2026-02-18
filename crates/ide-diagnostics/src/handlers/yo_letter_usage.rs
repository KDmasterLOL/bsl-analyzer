//! YoLetterUsage diagnostic
//!
//! Detects usage of Russian letter "ё" (yo) in identifiers.
//!
//! In module code it is prohibited to use the letter "ё".
//! Exception is interface texts displayed to user in messages, forms and help.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::{SyntaxKind, SyntaxToken};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

fn contains_yo_letter(text: &str) -> bool {
    text.chars().any(|c| c == 'ё' || c == 'Ё')
}

/// Single-pass token handler for YoLetterUsage diagnostic.
#[inline]
pub fn check_token(token: &SyntaxToken, acc: &mut Vec<Diagnostic>, ctx: &DiagnosticsContext) {
    let code = DiagnosticCode::YoLetterUsage;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    if token.kind() != SyntaxKind::IDENT {
        return;
    }

    if !contains_yo_letter(token.text()) {
        return;
    }

    acc.push(Diagnostic {
        code,
        message: "В текстах модулей не допускается использовать букву \"Ё\".".into(),
        severity: ctx.severity(code),
        range: token.text_range(),
        tags: ctx.tags(code),
        fixes: vec![],
    });
}

/// Legacy check function (delegates to single-pass).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            check_token(&token, &mut diagnostics, ctx);
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/YoLetterUsageDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 5, "Expected 5 diagnostics");

        // Java test positions:
        // .hasRange(0, 6, 0, 11)   - ёжики
        // .hasRange(2, 10, 2, 20)  - ЁлкиИголки
        // .hasRange(2, 21, 2, 25)  - Ёлки
        // .hasRange(3, 13, 3, 17)  - Ёлки
        // .hasRange(5, 39, 5, 43)  - Ёлки
        assert_diagnostic_range(code, &diagnostics[0], 0, 6, 11);
        assert_diagnostic_range(code, &diagnostics[1], 2, 10, 20);
        assert_diagnostic_range(code, &diagnostics[2], 2, 21, 25);
        assert_diagnostic_range(code, &diagnostics[3], 3, 13, 17);
        assert_diagnostic_range(code, &diagnostics[4], 5, 39, 43);
    }

    #[test]
    fn test_string_literal_not_flagged() {
        let code = r#"
Процедура Тест()
    Сообщить("Ёлка и ёжик в строке");
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "String literals should not be flagged");
    }

    #[test]
    fn test_pure_cyrillic_e_not_flagged() {
        let code = r#"
Перем Елка;
Процедура Ежик()
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Regular 'е' should not be flagged");
    }

    #[test]
    fn test_lowercase_yo() {
        let code = r#"Перем ёжик;"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, &diagnostics[0], 0, 6, 10);
    }

    #[test]
    fn test_uppercase_yo() {
        let code = r#"Перем Ёлка;"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, &diagnostics[0], 0, 6, 10);
    }
}
