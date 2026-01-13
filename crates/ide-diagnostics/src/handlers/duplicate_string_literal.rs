//! DuplicateStringLiteral diagnostic.
//!
//! Detects duplicate string literals that should be replaced with named constants.
//!
//! ## Why?
//! Multiple uses of identical string literals complicate maintenance:
//! - Risk of missing updates when changing string values
//! - Can indicate copy-paste errors
//! - Hard to track all occurrences across the codebase
//!
//! ## Bad practice
//! ```bsl
//! Процедура ПримерПлохойПрактики()
//!     Сообщить("Ошибка валидации");
//!     Если Условие Тогда
//!         ЗаписьЖурнала("Ошибка валидации");
//!     КонецЕсли;
//!     ВызватьИсключение "Ошибка валидации";  // Same string repeated 3 times!
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура ПримерХорошейПрактики()
//!     СообщениеОшибки = "Ошибка валидации";  // Define once
//!
//!     Сообщить(СообщениеОшибки);
//!     Если Условие Тогда
//!         ЗаписьЖурнала(СообщениеОшибки);
//!     КонецЕсли;
//!     ВызватьИсключение СообщениеОшибки;
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **allowedNumberCopies** (default: 2) - Number of occurrences allowed before reporting (≥ 1)
//! - **analyzeFile** (default: false) - If false: per-method scope; if true: whole-file scope
//! - **caseSensitive** (default: false) - If false: case-insensitive matching; if true: case matters
//! - **minTextLength** (default: 5) - Minimum string length INCLUDING quotes (≥ 5)
//! - **Enabled by default:** No
//! - **Severity:** Information (MINOR)
//! - **Tags:** BADPRACTICE
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! Ported from:
//! - DuplicateStringLiteralDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - duplicate_string_literal.rs (bsl-language-server-rust) - Rust reference (tree-sitter)
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use std::collections::HashMap;
use syntax::{SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("DuplicateStringLiteral::check").entered();

    if ctx.config.is_disabled(DiagnosticCode::DuplicateStringLiteral) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);

    let parse = ctx.parse();
    let root = parse.syntax_node();

    let scopes = find_scopes(&root, config.analyze_file);
    let mut diagnostics = Vec::new();

    for scope in scopes {
        let groups = collect_strings(&scope, &config);
        diagnostics.extend(report_duplicates(groups, &config));
    }

    tracing::debug!(count = diagnostics.len(), "DuplicateStringLiteral diagnostics found");
    diagnostics
}

#[derive(Debug, Clone)]
struct Config {
    allowed_number_copies: usize,
    analyze_file: bool,
    case_sensitive: bool,
    min_text_length: usize,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let code = DiagnosticCode::DuplicateStringLiteral;

        let mut allowed = ctx.config.get_int(code, "allowedNumberCopies").unwrap_or(2) as usize;
        if allowed < 1 {
            tracing::warn!("allowedNumberCopies < 1 ({}), resetting to default (2)", allowed);
            allowed = 2;
        }

        let analyze_file = ctx.config.get_bool(code, "analyzeFile").unwrap_or(false);

        let case_sensitive = ctx.config.get_bool(code, "caseSensitive").unwrap_or(false);

        let min_length = ctx.config.get_int(code, "minTextLength").unwrap_or(5) as usize;
        let min_text_length = min_length.max(5);

        tracing::debug!(
            allowed_number_copies = allowed,
            analyze_file = analyze_file,
            case_sensitive = case_sensitive,
            min_text_length = min_text_length,
            "Config loaded"
        );

        Self { allowed_number_copies: allowed, analyze_file, case_sensitive, min_text_length }
    }
}

fn find_scopes(root: &SyntaxNode, analyze_file: bool) -> Vec<SyntaxNode> {
    if analyze_file {
        return vec![root.clone()];
    }

    root.descendants()
        .filter(|n| matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF))
        .collect()
}

fn collect_strings(
    scope: &SyntaxNode,
    config: &Config,
) -> HashMap<String, Vec<(String, TextRange)>> {
    let mut groups: HashMap<String, Vec<(String, TextRange)>> = HashMap::new();
    let mut string_count = 0;

    for node in scope.descendants() {
        if node.kind() == SyntaxKind::LITERAL {
            // Check if this LITERAL contains a STRING token
            let has_string = node.children_with_tokens().any(|elem| {
                elem.as_token()
                    .map(|t| {
                        matches!(
                            t.kind(),
                            SyntaxKind::STRING
                                | SyntaxKind::STRING_START
                                | SyntaxKind::STRING_TAIL
                                | SyntaxKind::STRING_PART
                        )
                    })
                    .unwrap_or(false)
            });

            if !has_string {
                continue;
            }

            string_count += 1;
            let text = node.text().to_string();

            tracing::trace!(
                text = %text,
                len = text.len(),
                min_len = config.min_text_length,
                "Found string literal"
            );

            if text.len() < config.min_text_length {
                tracing::trace!("Filtered by min_text_length");
                continue;
            }

            let key = if config.case_sensitive { text.clone() } else { text.to_lowercase() };

            groups.entry(key).or_default().push((text, node.text_range()));
        }
    }

    tracing::debug!(string_count = string_count, groups = groups.len(), "Collected strings");

    groups
}

fn report_duplicates(
    groups: HashMap<String, Vec<(String, TextRange)>>,
    config: &Config,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (_, occurrences) in groups {
        if occurrences.len() > config.allowed_number_copies {
            let (first_text, first_range) = &occurrences[0];

            let message = format!(
                "Необходимо избавиться от многократного использования строкового литерала \"{}\"",
                first_text
            );

            diagnostics.push(Diagnostic {
                code: DiagnosticCode::DuplicateStringLiteral,
                message,
                severity: Severity::Information,
                range: *first_range,
                tags: vec![],
                fixes: vec![],
            });
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/DuplicateStringLiteralDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 2, "Should match Java: 2 diagnostics (default config)");

        // Метод1: "Строка2" at line 2 (0-indexed: line 1), col 8-17
        // Note: Col 17 is the position AFTER the closing quote (half-open range)
        assert_diagnostic_range(code, &diagnostics[0], 1, 8, 17);

        // Метод2: "Строка22" at line 11 (0-indexed: line 10), col 9-19
        // Note: Col 19 is the position AFTER the closing quote (half-open range)
        assert_diagnostic_range(code, &diagnostics[1], 10, 9, 19);

        assert!(
            diagnostics[0].message.contains("Строка2"),
            "Message should contain original string"
        );
        assert!(
            diagnostics[1].message.contains("Строка22"),
            "Message should contain original string"
        );
    }

    #[test]
    fn test_debug_positions() {
        let code = r#"Процедура Метод1()
    Ц = "Строка2";
КонецПроцедуры"#;

        use ide_db::base_db::{RootQueryDb, SourceDatabase};
        use ide_db::RootDatabaseImpl;
        use test_fixture::Fixture;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        let parse = db.parse(file_id);
        let root = parse.syntax_node();

        // Find first string LITERAL
        let literal = root
            .descendants()
            .find(|n| {
                n.kind() == SyntaxKind::LITERAL
                    && n.children_with_tokens().any(|elem| {
                        elem.as_token().map(|t| t.kind() == SyntaxKind::STRING).unwrap_or(false)
                    })
            })
            .unwrap();

        println!("LITERAL text: {:?}", literal.text());
        println!("LITERAL range: {:?}", literal.text_range());

        // Get STRING token range
        let string_range = literal
            .children_with_tokens()
            .find_map(|elem| {
                elem.as_token().filter(|t| t.kind() == SyntaxKind::STRING).map(|t| t.text_range())
            })
            .unwrap();

        println!("STRING token range: {:?}", string_range);

        // Convert to line/col
        let (start_line, start_col, _end_line, end_col) =
            range_to_line_col(&file_content, literal.text_range());
        println!("LITERAL position: line {}, col {}-{}", start_line, start_col, end_col);

        let (start_line, start_col, _end_line, end_col) =
            range_to_line_col(&file_content, string_range);
        println!("STRING token position: line {}, col {}-{}", start_line, start_col, end_col);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    А = "Ошибка";
    Б = "ошибка";
    В = "ОШИБКА";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        // caseSensitive=false (default): groups 3 together (3 > 2) → 1 diagnostic
        assert_eq!(diagnostics.len(), 1, "Should group case-insensitive strings");
    }

    #[test]
    fn test_min_length_filter() {
        let code = r#"
Процедура Тест()
    А = "OK";
    Б = "OK";
    В = "OK";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        // minTextLength=5 (including quotes), "OK" with quotes is 4 chars → filtered
        assert_eq!(diagnostics.len(), 0, "Should filter short strings");
    }

    #[test]
    fn test_threshold() {
        let code = r#"
Процедура Тест()
    А = "Текст1";
    Б = "Текст1";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        // allowedNumberCopies=2 (default): 2 occurrences is allowed, need > 2
        assert_eq!(diagnostics.len(), 0, "Should not report at threshold");
    }

    #[test]
    fn test_exceeds_threshold() {
        let code = r#"
Процедура Тест()
    А = "Текст1";
    Б = "Текст1";
    В = "Текст1";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        // allowedNumberCopies=2: 3 occurrences > 2 → 1 diagnostic
        assert_eq!(diagnostics.len(), 1, "Should report when exceeding threshold");
    }

    #[test]
    fn test_separate_scopes() {
        let code = r#"
Процедура Метод1()
    А = "Текст1";
    Б = "Текст1";
КонецПроцедуры

Процедура Метод2()
    В = "Текст1";
    Г = "Текст1";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        // analyzeFile=false (default): each method is separate scope
        // Each method has 2 occurrences, threshold is >2 → 0 diagnostics
        assert_eq!(diagnostics.len(), 0, "Should not report across method scopes");
    }
}
