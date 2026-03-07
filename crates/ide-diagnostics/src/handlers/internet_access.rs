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
//!
//! **Architecture:** HIR-based diagnostic (migrated from AST).
//!
//! ### HIR approach
//! - Scans `Expr::New { type_name, args }` in method bodies and module-level code
//! - Supports both named constructors (`Новый HTTPСоединение(...)`) and string constructors (`Новый("HTTPСоединение")`)
//! - Case-insensitive matching against 18 internet access patterns
//! - Uses `ModuleBodies` and `BodySourceMap` for accurate source locations
//!
//! ### Advantages over AST
//! - Semantic analysis - operates on lowered HIR representation
//! - Salsa caching - benefits from automatic invalidation
//! - Consistent with other diagnostics - same pattern as identical_expressions, incorrect_use_of_str_template
//! - Better error recovery - HIR handles parse errors gracefully

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Expr, ExprId, IdConversion, Literal};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 60,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

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

/// HIR-based check for internet access operations.
///
/// Scans all method bodies and module-level code for NEW expressions
/// that create internet access types (HTTP, FTP, Mail, etc.).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::InternetAccess;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let module_bodies = ctx.module_bodies();

    // Check method bodies
    for (_local_id, body, source_map) in module_bodies.method_bodies() {
        check_body_for_internet_access(body, source_map, code, ctx, &mut diagnostics);
    }

    // Check module-level code
    if let Some(lower_result) = module_bodies.module_code_result() {
        check_body_for_internet_access(
            &lower_result.body,
            &lower_result.source_map,
            code,
            ctx,
            &mut diagnostics,
        );
    }

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

fn create_diagnostic(
    range: TextRange,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: "Internet access detected (security review required)".to_string(),
        range,
        severity: ctx.severity(code),
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

/// Check a single body (method or module-level code) for internet access operations.
///
/// Scans all expressions in the body looking for NEW expressions that match
/// internet access patterns.
fn check_body_for_internet_access(
    body: &hir::Body,
    source_map: &hir::BodySourceMap,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (expr_id, expr) in body.exprs_iter() {
        if let Expr::New { type_name, args } = expr {
            let mut detected = false;

            // Pattern 1: Новый HTTPСоединение(...)
            if let Some(name) = type_name {
                let type_text = name.as_str().to_lowercase();
                if NEW_EXPRESSION_PATTERNS.contains(&type_text.as_str()) {
                    detected = true;
                }
            } else {
                // Pattern 2: Новый("HTTPСоединение")
                if !args.is_empty() {
                    if let Expr::Literal(Literal::String(s)) = body.expr(ExprId::from_idx(args[0]))
                    {
                        let type_text = s.to_lowercase();
                        if NEW_EXPRESSION_PATTERNS.contains(&type_text.as_str()) {
                            detected = true;
                        }
                    }
                }
            }

            if detected {
                if let Some(range) = source_map.expr_range(expr_id) {
                    diagnostics.push(create_diagnostic(range, code, ctx));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    use crate::DiagnosticCode;

    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура Тест1()
    FTPСоединение = Новый FTPСоединение(Сервер, Порт, Пользователь, Пароль); // ошибка

    Определения = Новый WSОпределения("http://localhost/test.asmx?WSDL"); // ошибка

    ПроксиДва = Новый WSПрокси(Определения, "http://localhost/", "test", "test"); // ошибка

    Определения =
        Новый WSОпределения("http://localhost/test.asmx?WSDL", "Пользователь", "Пароль", Неопределено, Таймаут); // ошибка

КонецПроцедуры

Процедура HTTP()
    HTTPСоединение = Новый HTTPСоединение("zabbix.localhost", 80); // ошибка
    HTTPЗапрос = Новый HTTPЗапрос(); // ошибка
    HTTPЗапрос = Новый HTTPЗапрос("zabbix", 80); // ошибка
    HTTPЗапрос = Новый HTTPЗапрос("zabbix"); // ошибка
    ИнтернетПрокси = Новый ИнтернетПрокси("zabbix"); // ошибка
КонецПроцедуры

Функция НовыйИнтернетПочтовыйПрофильБезТаймАута()
    Профиль = Новый ИнтернетПочтовыйПрофиль; // ошибка
    Профиль.Пользователь = "admin";
    Возврат Профиль;
КонецФункции

Функция InternetMail()
    Профиль = Новый InternetMail; // ошибка
КонецФункции

Функция InternetMail_НовыйИмя()
    Профиль = Новый("InternetMail"); // ошибка
КонецФункции

Профиль = Новый Почта; // ошибка
"#;
        let diagnostics = check_ast_diagnostic(code, check);

        // 13 internet access detections (matching reference test: hasSize(13))
        assert_eq!(diagnostics.len(), 13, "Expected 13 diagnostics");

        // assert_diagnostic_range uses 0-indexed lines

        assert_diagnostic_range(code, &diagnostics[0], 1, 20, 75);

        assert_diagnostic_range(code, &diagnostics[1], 3, 18, 72);

        assert_diagnostic_range(code, &diagnostics[2], 5, 16, 80);

        assert_diagnostic_range(code, &diagnostics[3], 8, 8, 111);

        assert_diagnostic_range(code, &diagnostics[4], 13, 21, 65);

        assert_diagnostic_range(code, &diagnostics[5], 14, 17, 35);

        assert_diagnostic_range(code, &diagnostics[6], 15, 17, 47);

        assert_diagnostic_range(code, &diagnostics[7], 16, 17, 43);

        assert_diagnostic_range(code, &diagnostics[8], 17, 21, 51);

        assert_diagnostic_range(code, &diagnostics[9], 21, 14, 43);

        assert_diagnostic_range(code, &diagnostics[10], 27, 14, 32);

        assert_diagnostic_range(code, &diagnostics[11], 31, 14, 35);

        assert_diagnostic_range(code, &diagnostics[12], 34, 10, 21);
    }

    #[test]
    fn test_new_expression_russian() {
        let code = r#"
Процедура Тест()
    HTTP = Новый HTTPСоединение("server", 80);
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
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
        let diagnostics = check_ast_diagnostic(code, check);
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
        let diagnostics = check_ast_diagnostic(code, check);
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
        let diagnostics = check_ast_diagnostic(code, check);
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
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Standard types should be ignored");
    }

    #[test]
    fn test_string_constructor() {
        let code = r#"
Процедура Тест()
    Профиль = Новый("InternetMail");
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
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
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 4);
    }
}
