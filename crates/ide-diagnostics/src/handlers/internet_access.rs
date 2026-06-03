use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_platform::security::{registry, Category};
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

fn is_internet_constructor(name: &str) -> bool {
    registry().lookup_constructor(name).is_some_and(|e| matches!(e.category, Category::Internet))
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::InternetAccess;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = crate::utils::for_each_body(ctx, |body, source_map, diags| {
        check_body_for_internet_access(body, source_map, code, ctx, diags);
    });

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

            if let Some(name) = type_name {
                if is_internet_constructor(name.as_str()) {
                    detected = true;
                }
            } else if !args.is_empty() {
                if let Expr::Literal(Literal::String(s)) = body.expr(ExprId::from_idx(args[0])) {
                    if is_internet_constructor(s) {
                        detected = true;
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
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InternetAccess,
            expect![[r#"
            InternetAccess @ 2:21..2:76
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 4:19..4:73
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 6:17..6:81
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 9:9..9:112
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 14:22..14:66
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 15:18..15:36
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 16:18..16:48
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 17:18..17:44
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 18:22..18:52
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 22:15..22:44
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 28:15..28:33
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 32:15..32:36
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 35:11..35:22
              message: Internet access detected (security review required)
              severity: Major"#]],
        );
    }

    #[test]
    fn test_new_expression_russian() {
        let code = r#"
Процедура Тест()
    HTTP = Новый HTTPСоединение("server", 80);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InternetAccess,
            expect![[r#"
            InternetAccess @ 3:12..3:46
              message: Internet access detected (security review required)
              severity: Major"#]],
        );
    }

    #[test]
    fn test_new_expression_english() {
        let code = r#"
Procedure Test()
    HTTP = New HTTPConnection("server", 80);
EndProcedure
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InternetAccess,
            expect![[r#"
            InternetAccess @ 3:12..3:44
              message: Internet access detected (security review required)
              severity: Major"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InternetAccess,
            expect![[r#"
            InternetAccess @ 3:10..3:39
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 4:10..4:39
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 5:10..5:39
              message: Internet access detected (security review required)
              severity: Major"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InternetAccess,
            expect![[r#"
            InternetAccess @ 3:10..3:31
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 4:10..4:31
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 5:10..5:32
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 6:10..6:32
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 7:10..7:31
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 8:10..8:31
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 9:10..9:26
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 10:10..10:25
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 11:10..11:41
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 12:10..12:37
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 13:10..13:31
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 14:10..14:30
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 15:10..15:23
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 16:10..16:22
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 17:10..17:28
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 18:10..18:29
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 19:10..19:32
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 20:10..20:31
              message: Internet access detected (security review required)
              severity: Major"#]],
        );
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
        check_diagnostics_snapshot_for(code, DiagnosticCode::InternetAccess, expect![[r#""#]]);
    }

    #[test]
    fn test_string_constructor() {
        let code = r#"
Процедура Тест()
    Профиль = Новый("InternetMail");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InternetAccess,
            expect![[r#"
            InternetAccess @ 3:15..3:36
              message: Internet access detected (security review required)
              severity: Major"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::InternetAccess,
            expect![[r#"
            InternetAccess @ 4:9..4:42
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 5:9..5:43
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 6:9..6:29
              message: Internet access detected (security review required)
              severity: Major
            InternetAccess @ 7:9..7:31
              message: Internet access detected (security review required)
              severity: Major"#]],
        );
    }
}
