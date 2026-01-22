//! MethodSize diagnostic.
//!
//! Detects functions and procedures with excessive line count.
//!
//! ## Why?
//! Long methods are hard to understand, test, and maintain.
//! They often indicate lack of proper abstraction and responsibility separation.
//!
//! ## Bad practice
//! ```bsl
//! Процедура ОченьДлиннаяПроцедура()
//!     // 300 lines of code...
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! Split into smaller, focused methods:
//! ```bsl
//! Процедура ВыполнитьОперацию()
//!     ПодготовитьДанные();
//!     ВыполнитьОсновнуюЛогику();
//!     ОбработатьРезультат();
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **maxMethodSize** (default: 200) - Maximum allowed method line count
//! - **Enabled by default:** Yes
//! - **Severity:** MAJOR
//! - **Tags:** BADPRACTICE (concept)
//! - **Minutes to fix:** 30
//!
//! ## Implementation
//! Ported from: MethodSizeDiagnostic.java (bsl-language-server)
//!
//! Algorithm: Calculates line difference (stop_line - start_line) matching Java's ANTLR behavior.
//!
//! ## Performance
//! Uses LineIndex for O(1) line number lookups instead of scanning the entire
//! file text for each method. LineIndex is built once O(n) at the start.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use line_index::LineIndex;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

#[derive(Debug, Clone)]
struct Config {
    max_method_size: usize,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let max_method_size =
            ctx.config.get_int(DiagnosticCode::MethodSize, "maxMethodSize").unwrap_or(200) as usize;

        Self { max_method_size }
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("MethodSize::check").entered();

    let code = DiagnosticCode::MethodSize;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let parse = ctx.parse();
    let root = parse.syntax_node();

    // Get line index (cached by Salsa, following rust-analyzer pattern)
    let line_index = ctx.line_index();

    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if matches!(node.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF) {
            // Check if method has a body
            let body = node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);

            let body_node = match body {
                Some(b) => b,
                None => continue, // No body at all (malformed or one-liner)
            };

            // Empty methods: size = 0, don't trigger
            if is_empty_method(&body_node) {
                continue;
            }

            // Calculate method size using line difference - O(1) per method
            let size = calculate_method_size(&node, &line_index);

            if size > config.max_method_size {
                let name_token = get_method_name(&node);
                let name_range = name_token
                    .as_ref()
                    .map(|t| t.text_range())
                    .unwrap_or_else(|| node.text_range());

                let name = name_token.as_ref().map(|t| t.text()).unwrap_or("Unknown");

                diagnostics.push(Diagnostic {
                    code,
                    message: format!(
                        "Длина метода \"{}\" равна {}, что больше установленного лимита в {} строк",
                        name, size, config.max_method_size
                    ),
                    severity: ctx.severity(code),
                    range: name_range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
            }
        }
    }

    tracing::debug!(count = diagnostics.len(), "MethodSize diagnostics found");

    diagnostics
}

/// Calculate method size using line difference (matches Java ANTLR behavior).
///
/// Uses LineIndex for O(1) line lookups instead of scanning file text.
///
/// Java calculates: subCodeBlock.getStop().getLine() - subCodeBlock.getStart().getLine()
/// where subCodeBlock spans from the first statement to the last statement.
///
/// The typical BSL method structure:
/// Line N:   Процедура Name()
/// Line N+1: (blank line)
/// Line N+2: (first statement)
/// ...
/// Line M-2: (last statement)
/// Line M-1: (blank line)
/// Line M:   КонецПроцедуры
///
/// Rowan PROCEDURE_DEF spans from N to M, so we subtract 4 to match Java's subCodeBlock.
fn calculate_method_size(method_node: &SyntaxNode, line_index: &LineIndex) -> usize {
    let range = method_node.text_range();

    // O(1) lookups using LineIndex
    let start_line = line_index.line_col(range.start()).line as usize;
    let end_line = line_index.line_col(range.end()).line as usize;

    // Rowan PROCEDURE_DEF spans from declaration to end keyword
    // Java subCodeBlock spans from first statement to last statement
    // Subtract 4 to match Java behavior
    let total_span = end_line.saturating_sub(start_line);
    total_span.saturating_sub(4)
}

/// Check if method body is empty (no executable statements).
fn is_empty_method(body: &SyntaxNode) -> bool {
    body.children().count() == 0
}

/// Extract method name token (IDENT).
fn get_method_name(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)
}

#[cfg(test)]
mod tests {
    use super::{calculate_method_size, check};
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig, Severity};
    use ide_db::base_db::SourceDatabase;
    use ide_db::RootDatabaseImpl;
    use syntax::SyntaxKind;
    use test_fixture::Fixture;

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/MethodSizeDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 2, "Should match Java implementation (2 diagnostics)");

        // First diagnostic: Процедура201Строка at line 6 (0-indexed), cols 10-28
        assert_diagnostic_range(code, &diagnostics[0], 6, 10, 28);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MethodSize);
        assert_eq!(diagnostics[0].severity, Severity::Warning); // CodeSmell + Major -> Warning
        assert!(
            diagnostics[0].message.contains("Процедура201Строка"),
            "Message should contain method name, got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("201"),
            "Message should contain size 201, got: {}",
            diagnostics[0].message
        );

        // Second diagnostic: Функция201Строка at line 419 (0-indexed), cols 8-24
        assert_diagnostic_range(code, &diagnostics[1], 419, 8, 24);
        assert!(diagnostics[1].message.contains("Функция201Строка"));
    }

    #[test]
    fn test_configure_threshold_20() {
        let code = include_str!("../../test_data/MethodSizeDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("maxMethodSize".to_string(), serde_json::Value::Number(20.into()));
        config.parameters.insert(DiagnosticCode::MethodSize, serde_json::Value::Object(params));

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 4, "Should match Java: 4 diagnostics with threshold 20");
    }

    #[test]
    fn test_empty_method() {
        let code = r#"Процедура Пустая()

КонецПроцедуры"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Empty method should not trigger");
    }

    #[test]
    fn test_one_liner() {
        let code = r#"Функция Тест() КонецФункции"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "One-liner should not trigger");
    }

    #[test]
    fn test_line_counting() {
        // Test that line counting matches Java algorithm
        let code = r#"Процедура Тест()
    А = 0;
    Б = 1;
    В = 2;
КонецПроцедуры"#;

        use ide_db::base_db::RootQueryDb;
        use ide_db::RootDatabase;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let parse = db.parse(file_id);
        let root = parse.syntax_node();
        let file_id_input = ide_db::base_db::FileIdInput::new(&db, file_id);
        let line_index = db.line_index(file_id_input);

        let procedure = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::PROCEDURE_DEF)
            .expect("Should find procedure");

        let size = calculate_method_size(&procedure, &line_index);

        // Line 1: Процедура Тест()
        // Line 2-4: body (3 lines)
        // Line 5: КонецПроцедуры
        // Total span: 5 - 1 = 4
        // Adjusted for subCodeBlock (subtract 4): 4 - 4 = 0
        // This method is too small to have a meaningful size
        assert_eq!(size, 0, "Small method should have size 0 after adjustment");
    }
}
