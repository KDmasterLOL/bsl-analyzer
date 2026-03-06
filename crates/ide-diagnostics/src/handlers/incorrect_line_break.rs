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

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use line_index::LineIndex;
use syntax::SyntaxKind;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Consistent,
};

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

    // bsl-language-server processes in this order: all line-end checks first, then all line-start checks
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
    use crate::test_utils::check_ast_diagnostic;
    /// Two operators (+) at line end in simple concatenation - 2 warnings
    #[test]
    fn test_operator_at_end_two_lines() {
        let code = r#"СуммаДокумента = СуммаБезСкидки +
                 СуммаРучнойСкидки +
                 СуммаАвтоматическойСкидки;"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 2, "Should detect 2 operators at line end");
    }

    /// Operator at end with comment before next line - still warns
    #[test]
    fn test_operator_at_end_before_comment() {
        let code = r#"ПоляОтбора = "Номенклатура,Характеристика,Склад" + // Дополнительный комментарий
   ДополнительныеПоляОтбора;"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect + at line end even before comment");
    }

    /// Operator at end when next line starts with string literal - should pass
    #[test]
    fn test_operator_before_string_continuation_passes() {
        let code = r#"ТекстЗапроса = ТекстЗапроса +
"ВЫБРАТЬ
| Номенклатура.Ссылка КАК Ссылка
|ИЗ
| Справочник. Номенклатура КАК Номенклатура";"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "String continuation after + should not warn");
    }

    /// ИЛИ at end of line - warns
    #[test]
    fn test_or_keyword_at_line_end() {
        let code = r#"Если (ВидОперации = Перечисления.ВидыОперацийПоступлениеМПЗ.ПоступлениеРозница) ИЛИ
  (ВидОперации = Перечисления.ВидыОперацийПоступлениеМПЗ.ПоступлениеРозницаКомиссия) Тогда
  Возврат Истина;
КонецЕсли;"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect ИЛИ at line end");
    }

    /// Comma at line start with meaningful content - warns; lone comma placeholder - passes
    #[test]
    fn test_comma_at_line_start_with_content() {
        let code = r#"ИменаДокументов.Добавить(Метаданные.Документы.СтрокаВыпискиРасход.Имя
    ,Метаданные.Документы.СтрокаВыпискиРасход.Синоним);"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Comma at line start with content should warn");
    }

    /// Lone comma (empty parameter placeholder) at line start - should not warn
    #[test]
    fn test_lone_comma_placeholder_passes() {
        let code = r#"ЗафиксироватьОшибку(
    ИмяСобытияЖР(),
    УровеньЖурналаРегистрации.Ошибка,
    , // не ошибка
    ТекстОшибки);"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Lone comma placeholder should not warn");
    }

    /// Closing paren at line start - warns
    #[test]
    fn test_closing_paren_at_line_start_warns() {
        let code = r#"Результат = Функция(Аргумент1
    , Аргумент2
    );"#;
        let diagnostics = check_ast_diagnostic(code, check);
        // ) at start and , at start with content
        assert!(!diagnostics.is_empty(), "Should detect ) at line start");
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
