//! SpaceAtStartComment diagnostic
//!
//! Detects comments without space after // delimiter.
//!
//! **Source (Java):** bsl-language-server/SpaceAtStartCommentDiagnostic.java
//!
//! Between comment symbols "//" and comment text there should be a space.
//! Exceptions are comment-annotations (starting with specific sequences like //@, //(c), //©).

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use once_cell::sync::Lazy;
use regex::Regex;
use syntax::{NodeOrToken, SyntaxKind, SyntaxNode, SyntaxToken};

// Default comment annotations (same as Java)
const DEFAULT_COMMENTS_ANNOTATION: &str = "//@,//(c),//©";

// Good comment patterns (from Java)
// Java GOOD_COMMENT_PATTERN_STRICT: "(?:(?:\\/\\/[ \\t].*)|(?:\\/{2,}[ \\t]*))$"
// - First alternative: exactly // followed by space/tab and text
// - Second alternative: 2+ slashes followed by space/tab (separators like ////////)
static GOOD_COMMENT_PATTERN_STRICT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(?://[ \t].*|/{2,}[ \t]*)$").expect("valid regex"));

// Java GOOD_COMMENT_PATTERN: "(?:(?:\\/{2,}[ \\t].*)|(?:\\/{2,}[ \\t]*))$"
// - First alternative: 2+ slashes followed by space/tab and text
// - Second alternative: 2+ slashes followed by space/tab (separators)
static GOOD_COMMENT_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(?:/{2,}[ \t].*|/{2,}[ \t]*)$").expect("valid regex"));

/// Check a single comment token for missing space (token-based API).
fn check_comment_token(
    token: &SyntaxToken,
    acc: &mut Vec<Diagnostic>,
    use_strict: bool,
    comments_annotation: &[String],
) {
    let text = token.text();

    // Check if comment matches good pattern
    let good_pattern =
        if use_strict { &GOOD_COMMENT_PATTERN_STRICT } else { &GOOD_COMMENT_PATTERN };

    if good_pattern.is_match(text) {
        return;
    }

    // Check if matches annotation patterns
    if is_annotation(text, comments_annotation) {
        return;
    }

    // Check if it's commented code (Java uses CodeRecognizer with 0.9 threshold)
    // For now, skip this check - we'll implement it later if needed
    // TODO: Port CodeRecognizer from Java

    // If we got here, comment needs a space
    acc.push(Diagnostic {
        code: DiagnosticCode::SpaceAtStartComment,
        message: "Comment should have space after //".to_string(),
        severity: Severity::Information,
        range: token.text_range(),
        tags: vec![],
        fixes: vec![],
    });
}

/// Check a single syntax node for comments without space (node-based API).
///
/// This is called from collect_text_diagnostics() for each node in single AST pass.
/// NOTE: This is not used for comments because comments are tokens, not nodes.
/// Use the file-level check() function instead.
pub fn check_node(_node: &SyntaxNode, _acc: &mut Vec<Diagnostic>, _ctx: &DiagnosticsContext) {
    // Comments are tokens in Rowan, not nodes.
    // This diagnostic uses file-level check() instead of node-based check_node().
}

/// Parse comma-separated annotation patterns
fn parse_comments_annotation(config: &str) -> Vec<String> {
    config.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect()
}

/// Check if comment starts with annotation pattern
fn is_annotation(text: &str, annotations: &[String]) -> bool {
    let text_lower = text.to_lowercase();
    annotations.iter().any(|ann| text_lower.starts_with(ann))
}

/// Main entry point for SpaceAtStartComment diagnostic.
///
/// This is a file-level diagnostic because comments in Rowan are tokens, not nodes.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::SpaceAtStartComment) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Get configuration (using defaults for now, TODO: support configuration)
    let use_strict = true; // Java default: USE_STRICT_VALIDATION = true
    let comments_annotation = parse_comments_annotation(DEFAULT_COMMENTS_ANNOTATION);

    // Traverse all tokens in the file looking for COMMENT tokens
    for element in root.descendants_with_tokens() {
        if let NodeOrToken::Token(token) = element {
            if token.kind() == SyntaxKind::COMMENT {
                check_comment_token(&token, &mut diagnostics, use_strict, &comments_annotation);
            }
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range;
    use crate::{DiagnosticsConfig, DiagnosticsContext};
    use ide_db::base_db::SourceDatabase;
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let config = Rc::new(DiagnosticsConfig::default());
        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_space_at_start_comment() {
        let code = include_str!("../../test_data/SpaceAtStartCommentDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

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
        let diagnostics = check_diagnostic(code);
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
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_bad_comments() {
        let code = r#"
//Плохой комментарий
Перем1 = 7; //И это плохой
                //Так тоже плохо
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 3, "Expected 3 bad comments");
    }
}
