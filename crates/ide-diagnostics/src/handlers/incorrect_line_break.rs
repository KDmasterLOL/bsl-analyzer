//! IncorrectLineBreak diagnostic
//!
//! Detects incorrect line breaks (forbidden characters at line start/end).
//!
//! ## Why?
//! Incorrect line breaks reduce code readability:
//! - Closing parenthesis at line start is hard to read
//! - Logical operators at line end make code flow unclear
//! - Proper line breaks improve code formatting
//!
//! ## Bad practice
//! ```bsl
//! Результат = Value1 +    // Operator at end - bad!
//!     Value2;
//!
//! Если (Условие1 ИЛИ     // "ИЛИ" at end - bad!
//!     Условие2) Тогда
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! ```bsl
//! Результат = Value1
//!     + Value2;          // Operator at start - good!
//!
//! Если (Условие1
//!     ИЛИ Условие2) Тогда  // "ИЛИ" at start - good!
//! КонецЕсли;
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use line_index::LineIndex;
use syntax::SyntaxKind;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::IncorrectLineBreak;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let text = root.text().to_string();
    let line_index = LineIndex::new(&text);
    let lines: Vec<&str> = text.lines().collect();

    let mut diagnostics = Vec::new();

    // Collect tokens grouped by line for efficient processing
    let mut line_tokens: Vec<Vec<(SyntaxKind, TextRange, String)>> = vec![Vec::new(); lines.len()];

    for element in root.descendants_with_tokens() {
        let Some(token) = element.as_token() else { continue };
        let kind = token.kind();

        // Skip whitespace and trivia
        if matches!(kind, SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE) {
            continue;
        }

        let range = token.text_range();
        let line = line_index.line_col(range.start()).line as usize;

        if line < lines.len() {
            line_tokens[line].push((kind, range, token.text().to_string()));
        }
    }

    // Java processes in this order: all line-end checks first, then all line-start checks
    // This order is required for test compatibility

    // First pass: check line END (operators/keywords at the end)
    for (line_idx, tokens) in line_tokens.iter().enumerate() {
        if tokens.is_empty() {
            continue;
        }
        if let Some(diag) = check_line_end(tokens, line_idx, &lines, code, ctx) {
            diagnostics.push(diag);
        }
    }

    // Second pass: check line START (forbidden symbols at the beginning)
    for tokens in line_tokens.iter() {
        if tokens.is_empty() {
            continue;
        }
        if let Some(diag) = check_line_start(tokens, code, ctx) {
            diagnostics.push(diag);
        }
    }

    diagnostics
}

/// Operators that should not appear at line end
fn is_forbidden_at_line_end(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PLUS
            | SyntaxKind::MINUS
            | SyntaxKind::STAR
            | SyntaxKind::SLASH
            | SyntaxKind::PERCENT
            | SyntaxKind::KW_AND
            | SyntaxKind::KW_OR
    )
}

/// Symbols that should not appear at line start
fn is_forbidden_at_line_start(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::R_PAREN | SyntaxKind::SEMICOLON)
}

/// Check for forbidden operators at line end
fn check_line_end(
    tokens: &[(SyntaxKind, TextRange, String)],
    line_idx: usize,
    lines: &[&str],
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    // Skip if next line starts with string literal (multiline string concatenation is OK)
    if let Some(next_line) = lines.get(line_idx + 1) {
        let trimmed = next_line.trim_start();
        if trimmed.starts_with('"') || trimmed.starts_with('|') {
            return None;
        }
    }

    // Find last meaningful token (skip comments)
    let last_meaningful = tokens.iter().rev().find(|(kind, _, _)| *kind != SyntaxKind::COMMENT)?;

    let (kind, range, text) = last_meaningful;

    if is_forbidden_at_line_end(*kind) {
        return Some(Diagnostic {
            code,
            message: format!("Incorrect line break: '{}' at line end", text.trim()),
            severity: ctx.severity(code),
            range: *range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    None
}

/// Check for forbidden symbols at line start
fn check_line_start(
    tokens: &[(SyntaxKind, TextRange, String)],
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    // Get first token on line
    let (kind, range, text) = tokens.first()?;

    if is_forbidden_at_line_start(*kind) {
        return Some(Diagnostic {
            code,
            message: format!("Incorrect line break: '{}' at line start", text.trim()),
            severity: ctx.severity(code),
            range: *range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    // Special case: comma followed by non-whitespace content on same logical position
    // Pattern: ,\s*\S+ at line start (comma with meaningful content after it)
    // A lone comma (empty parameter placeholder) is OK
    if *kind == SyntaxKind::COMMA {
        // Find non-comment tokens after comma
        let meaningful_after: Vec<_> =
            tokens.iter().skip(1).filter(|(k, _, _)| *k != SyntaxKind::COMMENT).collect();

        if !meaningful_after.is_empty() {
            // Comma at start with meaningful content - report the whole expression
            let last_range = meaningful_after.last().map(|(_, r, _)| *r)?;
            let combined_range = TextRange::new(range.start(), last_range.end());
            return Some(Diagnostic {
                code,
                message: "Incorrect line break: ',' at line start".to_string(),
                severity: ctx.severity(code),
                range: combined_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range_multiline, check_ast_diagnostic};

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/IncorrectLineBreakDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // Java expects 14 diagnostics with specific positions
        assert_eq!(diagnostics.len(), 14, "Should match Java implementation: 14 diagnostics");

        // Verify exact positions match Java test expectations (0-indexed lines)
        // Line 7: + at end
        assert_diagnostic_range_multiline(code, &diagnostics[0], 6, 32, 6, 33);
        // Line 8: + at end
        assert_diagnostic_range_multiline(code, &diagnostics[1], 7, 35, 7, 36);
        // Line 16: + at end
        assert_diagnostic_range_multiline(code, &diagnostics[2], 15, 32, 15, 33);
        // Line 17: + at end
        assert_diagnostic_range_multiline(code, &diagnostics[3], 16, 22, 16, 23);
        // Line 21: + at end (before comment)
        assert_diagnostic_range_multiline(code, &diagnostics[4], 20, 49, 20, 50);
        // Line 70: ИЛИ at end
        assert_diagnostic_range_multiline(code, &diagnostics[5], 69, 80, 69, 83);
        // Line 83: ИЛИ at end
        assert_diagnostic_range_multiline(code, &diagnostics[6], 82, 89, 82, 92);
        // Line 45: , at start with content
        assert_diagnostic_range_multiline(code, &diagnostics[7], 44, 25, 44, 76);
        // Line 47: , at start with content
        assert_diagnostic_range_multiline(code, &diagnostics[8], 46, 25, 46, 79);
        // Line 59: , at start with content
        assert_diagnostic_range_multiline(code, &diagnostics[9], 58, 4, 58, 55);
        // Line 61: , at start with content
        assert_diagnostic_range_multiline(code, &diagnostics[10], 60, 4, 60, 58);
        // Line 102: ) at start
        assert_diagnostic_range_multiline(code, &diagnostics[11], 101, 2, 101, 3);
        // Line 106: ) at start
        assert_diagnostic_range_multiline(code, &diagnostics[12], 105, 2, 105, 3);
        // Line 110: ) at start
        assert_diagnostic_range_multiline(code, &diagnostics[13], 109, 2, 109, 3);
    }

    #[test]
    fn test_correct_line_breaks() {
        let code = r#"
Функция Тест()
    Результат = Value1
        + Value2
        + Value3;
    Возврат Результат;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not detect correct line breaks");
    }

    #[test]
    fn test_operator_at_end() {
        let code = r#"
Функция Тест()
    Результат = Value1 +
        Value2;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert!(!diagnostics.is_empty(), "Should detect '+' at line end");
    }

    #[test]
    fn test_logical_operator_at_end() {
        let code = r#"
Процедура Тест()
    Если Условие1 ИЛИ
        Условие2 Тогда
        Сообщить("Да");
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert!(!diagnostics.is_empty(), "Should detect 'ИЛИ' at line end");
    }

    #[test]
    fn test_closing_paren_at_start() {
        let code = r#"
Функция Тест()
    Результат = Функция(Аргумент1
    );
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert!(!diagnostics.is_empty(), "Should detect ')' at line start");
    }

    #[test]
    fn test_multiline_string_ok() {
        // Operator at end is OK when next line is string continuation
        let code = r#"
Функция Тест()
    Текст = "Строка1" +
        "Строка2";
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Multiline string concatenation should be OK");
    }
}
