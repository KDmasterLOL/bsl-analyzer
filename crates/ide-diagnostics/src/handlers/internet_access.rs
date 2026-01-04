//! InternetAccess diagnostic.
//!
//! Detects internet access operations for security review.
//!
//! ## Why?
//! Internet access creates security vulnerabilities:
//! - Potential for unauthorized data exfiltration
//! - May expose internal systems to external threats
//! - Creates attack vectors for command & control
//! - Uncontrolled HTTP/FTP/mail operations
//!
//! This diagnostic is a **security audit tool** - disabled by default.
//! Enable it for code review, especially when auditing third-party or contractor code.
//!
//! ## What is detected
//!
//! ### Constructor patterns (NEW_EXPRESSION):
//! - FTPСоединение/FTPConnection - FTP connections
//! - HTTPСоединение/HTTPConnection - HTTP connections
//! - WSОпределения/WSDefinitions - Web service definitions
//! - WSПрокси/WSProxy - Web service proxies
//! - ИнтернетПочтовыйПрофиль/InternetMailProfile - Internet mail profiles
//! - ИнтернетПочта/InternetMail - Internet mail
//! - Почта/Mail - Mail operations
//! - HTTPЗапрос/HTTPRequest - HTTP requests
//! - ИнтернетПрокси/InternetProxy - Internet proxy
//!
//! ## Bad practice
//! ```bsl
//! Процедура ОтправитьДанные()
//!     // Internet access without authorization check
//!     HTTPСоединение = Новый HTTPСоединение("external-server.com", 80);
//!     FTPСоединение = Новый FTPСоединение("ftp.example.com", 21);
//!     Почта = Новый ИнтернетПочта();
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Review and verify internet access is authorized
//! // Implement proper access control and validation
//! // Use secure protocols (HTTPS, FTPS)
//! // Validate destination URLs/addresses
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** No (security audit tool)
//! - **Severity:** Warning (MAJOR VULNERABILITY)
//! - **Type:** VULNERABILITY
//! - **Tags:** SUSPICIOUS
//! - **Minutes to fix:** 60
//!
//! ## Implementation
//! Ported from:
//! - InternetAccessDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//!
//! Adapted to use Rowan SyntaxNode.
//! Follows the pattern from file_system_access.rs.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use syntax::{SyntaxKind, SyntaxNode};

/// Constructor types that indicate internet access.
///
/// Case-insensitive patterns (stored in lowercase).
/// Supports both Russian and English keywords.
const NEW_EXPRESSION_PATTERNS: &[&str] = &[
    "ftpсоединение",
    "ftpconnection",
    "httpсоединение",
    "httpconnection",
    "wsопределения",
    "wsdefinitions",
    "wsпрокси",
    "wsproxy",
    "интернетпочтовыйпрофиль",
    "internetmailprofile",
    "интернетпочта",
    "internetmail",
    "почта",
    "mail",
    "httpзапрос",
    "httprequest",
    "интернетпрокси",
    "internetproxy",
];

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::InternetAccess) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();
    let mut seen_ranges = std::collections::HashSet::new();

    // ✅ OPTIMIZATION: Collect nodes ONCE instead of O(N²) nested tree traversal
    let all_nodes: Vec<_> = root.descendants().collect();

    // Check NEW_EXPR nodes for internet access types
    for node in all_nodes.iter() {
        if node.kind() == SyntaxKind::NEW_EXPR {
            if let Some(range) = extract_new_expr_range(node) {
                if seen_ranges.insert(range) {
                    diagnostics.push(create_diagnostic(range));
                }
            }
        }
    }

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

fn create_diagnostic(range: TextRange) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::InternetAccess,
        message: "Internet access detected (security review required)".to_string(),
        range,
        severity: Severity::Warning,
        tags: vec![],
        fixes: vec![],
    }
}

/// Extract range of internet access type from NEW_EXPR node.
///
/// Returns the range of the entire NEW_EXPR node if it matches internet access patterns.
/// This matches Java bsl-language-server behavior.
///
/// Examples:
/// - `Новый HTTPСоединение(...)` → range of entire "Новый HTTPСоединение(...)"
/// - `Новый FTPConnection` → range of entire "Новый FTPConnection"
/// - `Новый("InternetMail")` → range of entire "Новый("InternetMail")"
fn extract_new_expr_range(node: &SyntaxNode) -> Option<TextRange> {
    // NEW_EXPR pattern: KW_NEW IDENT [LPAREN ...]
    // or: KW_NEW LPAREN STRING RPAREN
    let mut found_new_kw = false;

    // Check immediate children
    for element in node.children_with_tokens() {
        if let Some(token) = element.as_token() {
            if token.kind() == SyntaxKind::KW_NEW {
                found_new_kw = true;
                continue;
            }

            // Pattern 1: Новый IDENT
            if found_new_kw && token.kind() == SyntaxKind::IDENT {
                let type_name = token.text().to_lowercase();

                if NEW_EXPRESSION_PATTERNS.contains(&type_name.as_str()) {
                    // Return range of entire NEW_EXPR node
                    return Some(node.text_range());
                }

                break;
            }
        }
    }

    // Pattern 2: Новый(STRING) - STRING may be inside EXPR child node
    // Check all descendant STRING tokens
    if found_new_kw {
        for token in node.descendants_with_tokens() {
            if let Some(token) = token.as_token() {
                if token.kind() == SyntaxKind::STRING {
                    let text = token.text();
                    if text.len() > 2 {
                        let type_name = text[1..text.len() - 1].to_lowercase();
                        if NEW_EXPRESSION_PATTERNS.contains(&type_name.as_str()) {
                            // Return range of entire NEW_EXPR node
                            return Some(node.text_range());
                        }
                    }
                }
            }
        }
    }

    None
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
        let code = include_str!("../../test_data/InternetAccessDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        // 13 internet access detections (matching Java test: hasSize(13))
        assert_eq!(diagnostics.len(), 13, "Expected 13 diagnostics");

        // All diagnostics match Java test expectations
        // Both Java and assert_diagnostic_range use 0-indexed lines

        // Java: .hasRange(1, 20, 75)
        assert_diagnostic_range(code, &diagnostics[0], 1, 20, 75);

        // Java: .hasRange(3, 18, 72)
        assert_diagnostic_range(code, &diagnostics[1], 3, 18, 72);

        // Java: .hasRange(5, 16, 80)
        assert_diagnostic_range(code, &diagnostics[2], 5, 16, 80);

        // Java: .hasRange(8, 8, 111)
        assert_diagnostic_range(code, &diagnostics[3], 8, 8, 111);

        // Java: .hasRange(13, 21, 65)
        assert_diagnostic_range(code, &diagnostics[4], 13, 21, 65);

        // Java: .hasRange(14, 17, 35)
        assert_diagnostic_range(code, &diagnostics[5], 14, 17, 35);

        // Java: .hasRange(15, 17, 47)
        assert_diagnostic_range(code, &diagnostics[6], 15, 17, 47);

        // Java: .hasRange(16, 17, 43)
        assert_diagnostic_range(code, &diagnostics[7], 16, 17, 43);

        // Java: .hasRange(17, 21, 51)
        assert_diagnostic_range(code, &diagnostics[8], 17, 21, 51);

        // Java: .hasRange(21, 14, 43)
        assert_diagnostic_range(code, &diagnostics[9], 21, 14, 43);

        // Java: .hasRange(27, 14, 32)
        assert_diagnostic_range(code, &diagnostics[10], 27, 14, 32);

        // Java: .hasRange(31, 14, 35)
        assert_diagnostic_range(code, &diagnostics[11], 31, 14, 35);

        // Java: .hasRange(34, 10, 21)
        assert_diagnostic_range(code, &diagnostics[12], 34, 10, 21);
    }

    #[test]
    fn test_new_expression_russian() {
        let code = r#"
Процедура Тест()
    HTTP = Новый HTTPСоединение("server", 80);
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::InternetAccess);
    }

    #[test]
    fn test_new_expression_english() {
        let code = r#"
Procedure Test()
    HTTP = New HTTPConnection("server", 80);
EndProcedure
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::InternetAccess);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    H1 = Новый httpсоединение("s", 80);      // lowercase
    H2 = Новый HTTPСОЕДИНЕНИЕ("s", 80);      // uppercase
    H3 = Новый HttpСоединение("s", 80);      // mixed
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 3);
    }

    #[test]
    fn test_all_constructor_types() {
        let code = r#"
Процедура Тест()
    F1 = Новый FTPСоединение();
    F2 = Новый FTPConnection();
    H1 = Новый HTTPСоединение();
    H2 = Новый HTTPConnection();
    W1 = Новый WSОпределения();
    W2 = Новый WSDefinitions();
    W3 = Новый WSПрокси();
    W4 = Новый WSProxy();
    M1 = Новый ИнтернетПочтовыйПрофиль();
    M2 = Новый InternetMailProfile();
    M3 = Новый ИнтернетПочта();
    M4 = Новый InternetMail();
    M5 = Новый Почта();
    M6 = Новый Mail();
    R1 = Новый HTTPЗапрос();
    R2 = Новый HTTPRequest();
    P1 = Новый ИнтернетПрокси();
    P2 = Новый InternetProxy();
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 18, "All constructor types detected");
    }

    #[test]
    fn test_standard_types_ignored() {
        let code = r#"
Процедура Тест()
    М = Новый Массив();
    С = Новый Структура();
    Т = Новый ТаблицаЗначений();
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Standard types should be ignored");
    }

    #[test]
    fn test_string_constructor() {
        let code = r#"
Процедура Тест()
    Профиль = Новый("InternetMail");
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "String constructor detected");
    }

    #[test]
    fn test_mixed_russian_english() {
        let code = r#"
Процедура Тест()
    // Mix of Russian and English
    F = Новый FTPConnection("server", 21);
    H = Новый HTTPСоединение("server", 80);
    M = Новый InternetMail();
    P = Новый ИнтернетПрокси();
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 4);
    }
}
