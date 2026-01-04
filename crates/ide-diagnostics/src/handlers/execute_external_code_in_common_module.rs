//! ExecuteExternalCodeInCommonModule diagnostic.
//!
//! Detects usage of Execute() statements and Eval()/Вычислить() method calls
//! in CommonModules, which creates security vulnerabilities.
//!
//! ## Severity
//! CRITICAL (SECURITY_HOTSPOT)
//!
//! ## Tags
//! BADPRACTICE, STANDARD
//!
//! ## Examples
//!
//! ```bsl
//! // CommonModule file
//!
//! // ❌ Bad: Execute statement in CommonModule
//! Процедура ВыполнитьПроизвольныйКод(Строка)
//!     Выполнить(Строка); // CRITICAL: Arbitrary code execution in CommonModule
//! КонецПроцедуры
//!
//! // ❌ Bad: Eval method call in CommonModule
//! Функция ВычислитьЗначение(Строка)
//!     Возврат Вычислить(Строка); // CRITICAL: Arbitrary code execution
//! КонецФункции
//! ```
//!
//! ## Metadata Filtering (Future)
//!
//! When metadata infrastructure is ready (Iteration 11), this diagnostic will
//! only check CommonModules with specific flags:
//! - isServer() = true
//! - isClientOrdinaryApplication() = true
//! - isExternalConnection() = true
//!
//! For now, all CommonModules are checked.
//!
//! ## References
//! - 1C Standard: https://its.1c.ru/db/v8std#content:770:hdoc
//! - Java implementation: bsl-language-server/diagnostics/ExecuteExternalCodeInCommonModuleDiagnostic.java

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use syntax::{SyntaxKind, TextSize};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::ExecuteExternalCodeInCommonModule) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();
    let mut seen_ranges = std::collections::HashSet::new();

    // TODO: Add metadata filtering when metadata infrastructure is ready (Iteration 11)
    // Only check if: isServer() || isClientOrdinaryApplication() || isExternalConnection()
    // For now, check all files (simplified implementation)

    // Optimized: single traversal O(n) instead of O(n²)
    // 1. Collect all EXECUTE_STMT nodes
    for node in root.descendants() {
        if node.kind() == SyntaxKind::EXECUTE_STMT {
            let mut range = node.text_range();
            if node.text().to_string().ends_with(';') {
                range = TextRange::new(range.start(), range.end() - TextSize::from(1));
            }
            if seen_ranges.insert(range) {
                diagnostics.push(create_diagnostic(range));
            }
        }
    }

    // 2. Build token stream once for Eval detection
    let tokens: Vec<_> = root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    // 3. Search for Eval/Вычислить calls (global, not Object.Eval)
    for (i, token) in tokens.iter().enumerate() {
        if token.kind() != SyntaxKind::IDENT {
            continue;
        }

        // Check if this is Eval/Вычислить
        let text_lower = token.text().to_lowercase();
        if text_lower != "вычислить" && text_lower != "eval" {
            continue;
        }

        // Check pattern: IDENT ( but not .IDENT(
        let next_is_lparen =
            tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

        if !next_is_lparen {
            continue;
        }

        let prev_is_dot = i
            .checked_sub(1)
            .and_then(|idx| tokens.get(idx))
            .map(|t| t.kind() == SyntaxKind::DOT)
            .unwrap_or(false);

        if prev_is_dot {
            continue;
        }

        // Extract range: from method name to closing )
        let start = token.text_range().start();
        let mut end = token.text_range().end();

        for next_token in tokens.iter().skip(i + 1) {
            end = next_token.text_range().end();
            if next_token.kind() == SyntaxKind::R_PAREN {
                break;
            }
        }

        let range = TextRange::new(start, end);
        if seen_ranges.insert(range) {
            diagnostics.push(create_diagnostic(range));
        }
    }

    diagnostics
}

fn create_diagnostic(range: TextRange) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::ExecuteExternalCodeInCommonModule,
        message: "It is forbidden to execute external code in common modules".to_string(),
        range,
        severity: Severity::Critical,
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;
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
    fn test_comprehensive() {
        let code = include_str!("../../test_data/ExecuteExternalCodeInCommonModuleDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 2, "Expected 2 diagnostics");

        assert_diagnostic_range(code, &diagnostics[0], 2, 4, 21);
        assert_diagnostic_range(code, &diagnostics[1], 6, 12, 29);
    }

    #[test]
    fn test_execute_statement() {
        let code = r#"
Процедура ВыполнитьНаСервере(Строка)
    Выполнить(Строка);
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Execute statement should be detected");
    }

    #[test]
    fn test_eval_call() {
        let code = r#"
Функция ВычислитьЗначение(Строка)
    Возврат Вычислить(Строка);
КонецФункции
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Eval call should be detected");
    }
}
