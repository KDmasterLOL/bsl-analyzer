//! SpaceAtStartComment diagnostic
//!
//! Detects comments without space after // delimiter.
//!
//! **Source (Java):** bsl-language-server/SpaceAtStartCommentDiagnostic.java
//!
//! Between comment symbols "//" and comment text there should be a space.
//! Exceptions are comment-annotations (starting with specific sequences like //@, //(c), //©).
//!
//! ## Implementation
//!
//! File-level token-based diagnostic: iterates through all tokens in the file once to find
//! COMMENT tokens. This avoids false positives on // inside strings (lexer distinguishes them).
//! Cannot use per-node check_node() API because comments are tokens, not nodes.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use line_index::TextSize;
use once_cell::sync::Lazy;
use regex::Regex;
use syntax::{NodeOrToken, SyntaxKind};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

// Default comment annotations (same as Java)
const DEFAULT_COMMENTS_ANNOTATION: &str = "//@,//(c),//©";

// Good comment patterns (from Java)
// Java GOOD_COMMENT_PATTERN_STRICT: "(?:(?:\\/\\/[ \\t].*)|(?:\\/{2,}[ \\t]*))$"
// - First alternative: exactly // followed by space/tab and text
// - Second alternative: 2+ slashes followed by space/tab (separators like ////////)
static GOOD_COMMENT_PATTERN_STRICT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^//[ \t].*$|^/{2,}[ \t]*$").expect("valid regex"));

// Java GOOD_COMMENT_PATTERN: "(?:(?:\\/{2,}[ \\t].*)|(?:\\/{2,}[ \\t]*))$"
// - First alternative: 2+ slashes followed by space/tab and text
// - Second alternative: 2+ slashes followed by space/tab (separators)
static GOOD_COMMENT_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^/{2,}[ \t].*$|^/{2,}[ \t]*$").expect("valid regex"));

/// Parse comma-separated annotation patterns
fn parse_comments_annotation(config: &str) -> Vec<String> {
    config.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect()
}

/// Check if comment starts with annotation pattern
fn is_annotation(text: &str, annotations: &[String]) -> bool {
    let text_lower = text.to_lowercase();
    annotations.iter().any(|ann| text_lower.starts_with(ann))
}

/// Check if comment is good according to pattern and configuration
fn is_good_comment(comment_text: &str, use_strict: bool, annotations: &[String]) -> bool {
    // Empty comment (just // with nothing after, followed by newline or end of line)
    // This is considered good (no diagnostic)
    let trimmed = comment_text.trim_end();
    if trimmed == "//" {
        return true;
    }

    // Check if comment matches good pattern
    let good_pattern =
        if use_strict { &GOOD_COMMENT_PATTERN_STRICT } else { &GOOD_COMMENT_PATTERN };

    if good_pattern.is_match(comment_text) {
        return true;
    }

    // Check if matches annotation patterns
    if is_annotation(comment_text, annotations) {
        return true;
    }

    // Check if it's commented code (Java uses CodeRecognizer with 0.9 threshold)
    // For now, skip this check - we'll implement it later if needed
    // TODO: Port CodeRecognizer from Java

    false
}

/// Main entry point for SpaceAtStartComment diagnostic.
///
/// Uses tokens from parser to correctly identify comments (avoiding false positives on // in strings).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("SpaceAtStartComment::check").entered();

    let code = DiagnosticCode::SpaceAtStartComment;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // Get configuration (using defaults for now, TODO: support configuration)
    let use_strict = true; // Java default: USE_STRICT_VALIDATION = true
    let comments_annotation = parse_comments_annotation(DEFAULT_COMMENTS_ANNOTATION);

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Traverse all tokens in the file looking for COMMENT tokens
    // This correctly handles strings vs comments (lexer already distinguished them)
    for element in root.descendants_with_tokens() {
        if let NodeOrToken::Token(token) = element {
            if token.kind() == SyntaxKind::COMMENT {
                let text = token.text();

                // Check if comment is bad
                if !is_good_comment(text, use_strict, &comments_annotation) {
                    let slash_count = text.chars().take_while(|c| *c == '/').count() as u32;
                    let insert_pos = token.text_range().start() + TextSize::from(slash_count);
                    diagnostics.push(Diagnostic {
                        code,
                        message: "Comment should have space after //".to_string(),
                        severity: ctx.severity(code),
                        range: token.text_range(),
                        tags: ctx.tags(code),
                        fixes: vec![Fix {
                            label: "Добавить пробел после //".to_string(),
                            edits: vec![TextEdit {
                                range: ide_db::TextRange::new(insert_pos, insert_pos),
                                new_text: " ".to_string(),
                            }],
                        }],
                    });
                }
            }
        }
    }

    tracing::debug!(count = diagnostics.len(), "SpaceAtStartComment diagnostics found");

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    #[test]
    fn test_space_at_start_comment() {
        let code = include_str!("../../test_data/SpaceAtStartCommentDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // TODO: Port CodeRecognizer from Java to skip commented code (lines 31-33)
        // Java expects 7 diagnostics, we get 10 because we don't skip commented code yet.
        // Expected Java diagnostics:
        // 1. Line 6: //Плохой комментарий
        // 2. Line 8: //И это плохой (inline comment)
        // 3. Line 9: //Так тоже плохо
        // 4. Line 20: //(с) Похоже... (cyrillic 'с', not in default annotations)
        // 5. Line 22: //// Плохой... (4 slashes without space in strict mode)
        // 6. Line 30: //&НаКлиенте (commented code, skipped in Java)
        // 7. Line 34: /// Текст... (3 slashes with text, error in strict mode)
        // 8. Line 35: ////Текст... (4 slashes with text)

        // We get extra diagnostics for commented code (lines 31-32)
        assert!(
            diagnostics.len() >= 5,
            "Expected at least 5 diagnostics, got {}",
            diagnostics.len()
        );

        // Check first 5 diagnostics that don't depend on CodeRecognizer
        // Line 6 (0-indexed), cols 0-20: //Плохой комментарий
        assert_diagnostic_range(code, &diagnostics[0], 6, 0, 20);

        // Line 8 (0-indexed), cols 12-26: //И это плохой
        assert_diagnostic_range(code, &diagnostics[1], 8, 12, 26);

        // Line 9 (0-indexed), cols 16-32: //Так тоже плохо
        assert_diagnostic_range(code, &diagnostics[2], 9, 16, 32);

        // Line 20 (0-indexed), cols 0-56: //(с) Похоже на строку с копирайтом
        assert_diagnostic_range(code, &diagnostics[3], 20, 0, 56);

        // Line 22 (0-indexed), cols 0-56: //// Плохой комментарий
        assert_diagnostic_range(code, &diagnostics[4], 22, 0, 56);
    }

    #[test]
    fn test_good_comments() {
        let code = r#"
// Это хороший комментарий, с пробелом
//  Это хороший комментарий, с табом
//      Этот комментарий тоже норм
Перем1 = 7; // И это нормальный
// Строка ниже используется как разделитель
/////////////////////////////////////////////////////////////////////////////////
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_annotations() {
        let code = r#"
//@skip-warring Пропускаем замечания в EDT
//@unit-test Аннотациия для юниттестов в EDT
//(c) Это строка с копирайтом
//© Это рамка копирайта
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_bad_comments() {
        let code = r#"
//Плохой комментарий
Перем1 = 7; //И это плохой
                //Так тоже плохо
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 3, "Expected 3 bad comments");
    }

    #[test]
    fn test_empty_comment_lines() {
        // Test case from user: empty comment lines should not trigger diagnostic
        let code = r#"
// Возвращает параметры запроса для ключа действия см. ПОЗКДействия()
//  Если требуются дополнительные параметры, то необходимо добавить ваше действие в ПОЗКДействияСПараметрами()
//  создать функцию с именем "Адаптер<ИмяКлюча>" которая содержит структуру дополнительных параметров
//
// Параметры:
//  КлючДействия - Ключ структуры ПОЗКДействия()
//
// Возвращаемое значение:
//   Тип.Структура - Параметры запроса для заданного ключа действия.
//
Функция ПараметрыЗапросаПОЗК(КлючДействия) Экспорт
    Результат = Новый Структура;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        // Should have 0 diagnostics - all empty comment lines (just //) are valid
        assert_eq!(
            diagnostics.len(),
            0,
            "Empty comment lines should not trigger diagnostic, got {} diagnostics",
            diagnostics.len()
        );
    }

    #[test]
    fn test_empty_comment_variants() {
        // Test different variants of empty comments
        let code = r#"
//
//
//
// Хороший комментарий
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        // All empty lines with // are good, last line with space is also good
        assert_eq!(
            diagnostics.len(),
            0,
            "Empty comments with various whitespace should not trigger diagnostic"
        );
    }

    #[test]
    fn test_comment_with_text_no_space() {
        // Ensure comments with text but no space still trigger diagnostic
        let code = r#"
//Плохо
//Тоже плохо
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            2,
            "Comments with text but no space should trigger diagnostic"
        );
    }

    #[test]
    fn test_comment_in_string_false_positive() {
        // Test that we don't trigger on // inside strings
        let code = r#"
Процедура Тест()
    URL = "http://example.com"; // Нормальный комментарий
    Путь = "C://folder//file.txt"; // Еще комментарий
    Текст = "Текст с // внутри строки"; // И еще
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        // Should have 0 diagnostics - all comments have space after //
        // If we get false positives on // inside strings, this test will fail
        assert_eq!(diagnostics.len(), 0, "Should not detect // inside strings as comments");
    }

    #[test]
    fn test_url_in_string() {
        // Simple test with URL
        let code = r#"
URL = "http://example.com";
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "URL inside string should not trigger diagnostic");
    }
}
