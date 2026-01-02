//! GlobalContextMethodCollision8312 diagnostic.
//!
//! Checks for method name collisions with platform 8.3.12 global context methods.
//!
//! ## Why?
//! Starting from platform version 8.3.12, new bitwise operation methods were added
//! to the global context. User-defined methods with these names will conflict with
//! platform methods, leading to unexpected behavior.
//!
//! ## Conflicting methods (Russian/English)
//! - ПроверитьБит / CheckBit
//! - ПроверитьПоБитовойМаске / CheckByBitMask
//! - УстановитьБит / SetBit
//! - ПобитовоеИ / BitwiseAnd
//! - ПобитовоеИли / BitwiseOr
//! - ПобитовоеНе / BitwiseNot
//! - ПобитовоеИНе / BitwiseAndNot
//! - ПобитовоеИсключительноеИли / BitwiseXor
//! - ПобитовыйСдвигВлево / BitwiseShiftLeft
//! - ПобитовыйСдвигВправо / BitwiseShiftRight
//!
//! ## Bad practice
//! ```bsl
//! Функция ПроверитьБит(Число, Позиция)
//!     // Custom implementation conflicts with platform method
//!     Возврат (Число % 2) = 1;
//! КонецФункции
//! ```
//!
//! ## Good practice
//! ```bsl
//! Функция ПроверитьБитПользовательский(Число, Позиция)
//!     Возврат (Число % 2) = 1;
//! КонецФункции
//! ```
//!
//! ## Implementation
//!
//! Ported from:
//! - GlobalContextMethodCollision8312Diagnostic.java (bsl-language-server)
//!
//! ### Algorithm:
//! - O(n) complexity: single pass through function definitions
//! - Case-insensitive matching (BSL is case-insensitive)
//! - Checks both Russian and English method names
//!
//! ### Diagnostic range:
//! - Java: `diagnosticStorage.addDiagnostic(method.getSubNameRange())`
//! - Rust: First IDENT token before PARAM_LIST (function name)
//!
//! ## References
//! - Source: https://its.1c.ru/db/metod8dev#content:5293:hdoc:pereimenovaniya_metodov_i_svojstv

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode};

/// List of platform 8.3.12 global context methods that can conflict with user-defined methods.
/// Both Russian and English variants are included.
///
/// Reference: GlobalContextMethodCollision8312Diagnostic.java, line 46-49
const COLLISION_METHODS: &[&str] = &[
    // Russian variants
    "проверитьбит",
    "проверитьпобитовоймаске",
    "установитьбит",
    "побитовоеи",
    "побитовоеили",
    "побитовоене",
    "побитовоеине",
    "побитовоеисключительноеили",
    "побитовыйсдвигвлево",
    "побитовыйсдвигвправо",
    // English variants
    "checkbit",
    "checkbybitmask",
    "setbit",
    "bitwiseand",
    "bitwiseor",
    "bitwisenot",
    "bitwiseandnot",
    "bitwisexor",
    "bitwiseshiftleft",
    "bitwiseshiftright",
];

/// Runs the GlobalContextMethodCollision8312 diagnostic.
///
/// Complexity: O(n) where n is the number of AST nodes.
/// Only one pass through the syntax tree to find all function definitions.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Check if diagnostic is disabled
    if ctx.config.is_disabled(DiagnosticCode::GlobalContextMethodCollision8312) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    // O(n) complexity: single pass to find all function definitions
    for node in root.descendants() {
        if node.kind() == SyntaxKind::FUNCTION_DEF {
            if let Some(diag) = check_function(&node) {
                diagnostics.push(diag);
            }
        }
    }

    diagnostics
}

/// Check a single function for name collision with platform methods.
///
/// Returns Some(Diagnostic) if the function name matches a platform method.
fn check_function(func_node: &SyntaxNode) -> Option<Diagnostic> {
    // Get function name token (first IDENT before PARAM_LIST)
    let name_token = func_node
        .children_with_tokens()
        .take_while(|el| !matches!(el.kind(), SyntaxKind::PARAM_LIST))
        .filter_map(|el| el.into_token())
        .filter(|tok| !tok.kind().is_trivia())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)?;

    let name = name_token.text();
    let name_lowercase = name.to_lowercase();

    // Check if name matches any collision method (case-insensitive)
    // This matches Java's CaseInsensitivePattern.matcher(method.getName()).matches()
    if COLLISION_METHODS.contains(&name_lowercase.as_str()) {
        let range = name_token.text_range();

        return Some(Diagnostic {
            code: DiagnosticCode::GlobalContextMethodCollision8312,
            message: format!(
                "Имя метода \"{}\" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12",
                name
            ),
            severity: Severity::Blocker,
            range,
            tags: vec![],
            fixes: vec![],
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::assert_diagnostic_range, DiagnosticsConfig};
    use ide_db::RootDatabase;
    use std::sync::Arc;

    /// Helper to run diagnostic on test code
    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        use ide_db::base_db::SourceDatabase;
        use ide_db::RootDatabaseImpl;
        use test_fixture::Fixture;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();

        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    /// Integration test matching Java test structure.
    ///
    /// Based on GlobalContextMethodCollision8312DiagnosticTest.java
    /// Uses the same test file: GlobalContextMethodCollision8312Diagnostic.bsl
    ///
    /// Expected: 20 diagnostics (all conflicting method names)
    /// Lines 0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 30, 33, 36, 39, 42, 45, 48, 51, 54, 57
    #[test]
    fn test_8312() {
        let code = include_str!(
            "global_context_method_collision8312/GlobalContextMethodCollision8312Diagnostic.bsl"
        );

        let (diagnostics, file_content) = check_diagnostic(code);

        // Java test expects 20 diagnostics
        assert_eq!(diagnostics.len(), 20, "Expected 20 diagnostics");

        // Verify all diagnostics match Java ranges exactly
        // Format: line, start_col, end_col
        let expected_ranges = [
            (0, 8, 20),  // ПроверитьБит
            (3, 8, 31),  // ПроверитьПоБитовойМаске
            (6, 8, 21),  // УстановитьБит
            (9, 8, 18),  // ПобитовоеИ
            (12, 8, 20), // ПобитовоеИли
            (15, 8, 19), // ПобитовоеНе
            (18, 8, 20), // ПобитовоеИНе
            (21, 8, 34), // ПобитовоеИсключительноеИли
            (24, 8, 27), // ПобитовыйСдвигВлево
            (27, 8, 28), // ПобитовыйСдвигВправо
            (30, 8, 16), // CheckBit
            (33, 8, 22), // CheckByBitMask
            (36, 8, 14), // SetBit
            (39, 8, 18), // BitwiseAnd
            (42, 8, 17), // BitwiseOr
            (45, 8, 18), // BitwiseNot
            (48, 8, 21), // BitwiseAndNot
            (51, 8, 18), // BitwiseXor
            (54, 8, 24), // BitwiseShiftLeft
            (57, 8, 25), // BitwiseShiftRight
        ];

        for (i, (line, start_col, end_col)) in expected_ranges.iter().enumerate() {
            assert_eq!(
                diagnostics[i].code,
                DiagnosticCode::GlobalContextMethodCollision8312,
                "Diagnostic {} should have correct code",
                i
            );
            assert_eq!(
                diagnostics[i].severity,
                Severity::Blocker,
                "Diagnostic {} should have Blocker severity",
                i
            );
            assert_diagnostic_range(&file_content, &diagnostics[i], *line, *start_col, *end_col);
        }
    }

    /// Test that methods with prefixes/suffixes don't trigger
    #[test]
    fn test_no_collision_with_prefix_suffix() {
        let code = r#"Функция _ПроверитьБит()
КонецФункции

Функция ПроверитьПоБитовойМаске_()
КонецФункции

Функция БИТУстановитьБит()
КонецФункции"#;

        let (diagnostics, _file_content) = check_diagnostic(code);

        // These methods have prefixes/suffixes so they don't conflict
        assert_eq!(diagnostics.len(), 0, "Methods with prefix/suffix should not trigger");
    }

    /// Test case-insensitive matching (Russian uppercase)
    #[test]
    fn test_case_insensitive_russian() {
        let code = r#"Функция ПРОВЕРИТЬБИТ()
КонецФункции"#;

        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(&file_content, &diagnostics[0], 0, 8, 20);
    }

    /// Test case-insensitive matching (English mixed case)
    #[test]
    fn test_case_insensitive_english() {
        let code = r#"Function CheckBit()
EndFunction"#;

        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(&file_content, &diagnostics[0], 0, 9, 17);
    }

    /// Test multiple conflicting functions
    #[test]
    fn test_multiple_collisions() {
        let code = r#"Функция ПроверитьБит()
КонецФункции

Функция CheckBit()
КонецФункции

Функция ПобитовоеИ()
КонецФункции"#;

        let (diagnostics, _file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 3, "Should detect all 3 collisions");
    }

    /// Test non-conflicting function names
    #[test]
    fn test_no_collision() {
        let code = r#"Функция МояФункция()
КонецФункции

Функция ВычислитьСумму()
КонецФункции"#;

        let (diagnostics, _file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 0, "Non-conflicting names should not trigger");
    }
}
