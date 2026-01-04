//! DeprecatedCurrentDate diagnostic.
//!
//! Detects usage of deprecated ТекущаяДата() / CurrentDate() methods.
//!
//! ## Why?
//! The `ТекущаяДата()` / `CurrentDate()` method returns server date/time but with unpredictable timezone behavior.
//! - On server: returns server's local time
//! - On client: may return incorrect time due to timezone discrepancies
//! - Causes bugs in multi-timezone deployments
//!
//! ## Bad practice
//! ```bsl
//! Процедура ПолучитьДату()
//!     Возврат ТекущаяДата(); // ❌ Unpredictable timezone!
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! // On server:
//! Процедура ПолучитьДату()
//!     Возврат ТекущаяДатаСеанса(); // ✅ Session date
//! КонецПроцедуры
//!
//! // On client:
//! Процедура ПолучитьДату()
//!     Возврат ОбщегоНазначенияКлиент.ДатаСеанса(); // ✅ From StandardLibrary
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Error (MAJOR)
//! - **Tags:** STANDARD, DEPRECATED, UNPREDICTABLE
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! Ported from:
//! - DeprecatedCurrentDateDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - deprecated_current_date.rs (bsl-language-server-rust) - Rust reference
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxToken};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::DeprecatedCurrentDate) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();
    let mut seen_ranges = std::collections::HashSet::new();

    // ✅ OPTIMIZATION: Collect tokens ONCE instead of O(N²) nested tree traversal
    let tokens: Vec<_> = root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    // Find all global method calls (IDENT + LPAREN without preceding DOT)
    for (i, token) in tokens.iter().enumerate() {
        if token.kind() == SyntaxKind::IDENT {
            let next_is_lparen =
                tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

            if next_is_lparen {
                let prev_is_dot = i
                    .checked_sub(1)
                    .and_then(|idx| tokens.get(idx))
                    .map(|t| t.kind() == SyntaxKind::DOT)
                    .unwrap_or(false);

                if !prev_is_dot {
                    let method_name = token.text().to_string();
                    if is_deprecated_current_date(&method_name) {
                        let diagnostic = create_diagnostic(token, &method_name);
                        if seen_ranges.insert(diagnostic.range) {
                            diagnostics.push(diagnostic);
                        }
                    }
                }
            }
        }
    }

    diagnostics
}

fn is_deprecated_current_date(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "текущаядата" || lower == "currentdate"
}

fn create_diagnostic(token: &SyntaxToken, method_name: &str) -> Diagnostic {
    let message = get_message(method_name);
    let range = token.text_range();

    Diagnostic {
        code: DiagnosticCode::DeprecatedCurrentDate,
        message,
        severity: Severity::Error,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

fn get_message(method_name: &str) -> String {
    let lower = method_name.to_lowercase();
    if lower == "текущаядата" {
        "Используйте \"ТекущаяДатаСеанса\" вместо устаревшего \"ТекущаяДата\"".to_string()
    } else {
        "Use \"CurrentSessionDate\" instead of deprecated \"CurrentDate\"".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
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

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_deprecated_russian() {
        let code = r#"
Процедура Тест()
    Дата = ТекущаяДата();
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedCurrentDate);
        assert_eq!(diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn test_deprecated_english() {
        let code = r#"
Procedure Test()
    Date = CurrentDate();
EndProcedure
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedCurrentDate);
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Процедура Тест()
    Дата = Модуль.ТекущаяДата();
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Дата1 = ТЕКУЩАЯДАТА();
    Дата2 = текущаядата();
    Дата3 = ТекущаяДата();
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 3);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DeprecatedCurrentDateDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(input);

        assert_eq!(diagnostics.len(), 2, "Expected 2 diagnostics");

        assert_diagnostic_range(&file_content, &diagnostics[0], 2, 19, 30);

        assert_diagnostic_range(&file_content, &diagnostics[1], 11, 16, 27);
    }
}
