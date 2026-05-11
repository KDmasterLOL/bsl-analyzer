//! Checks that known external-resource objects are created with an explicit timeout.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Expr, ExprId, IdConversion, Literal, Name, Stmt};
use ide_db::TextRange;
use rustc_hash::FxHashMap;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Unpredictable, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_ANALYZE_INTERNET_MAIL_PROFILE: bool = true;

#[derive(Debug, Clone)]
struct Config {
    analyze_internet_mail_profile_zero_timeout: bool,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let code = DiagnosticCode::TimeoutsInExternalResources;

        let analyze_internet_mail_profile_zero_timeout = ctx
            .config
            .get_bool(code, "analyzeInternetMailProfileZeroTimeout")
            .unwrap_or(DEFAULT_ANALYZE_INTERNET_MAIL_PROFILE);

        Self { analyze_internet_mail_profile_zero_timeout }
    }
}

#[derive(Debug, Clone, Copy)]
enum ResourceType {
    FTPConnection,
    HTTPConnection,
    WSDefinitions,
    WSProxy,
    InternetMailProfile,
}

impl ResourceType {
    fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "ftpсоединение" | "ftpconnection" => Some(Self::FTPConnection),
            "httpсоединение" | "httpconnection" => Some(Self::HTTPConnection),
            "wsопределения" | "wsdefinitions" => Some(Self::WSDefinitions),
            "wsпрокси" | "wsproxy" => Some(Self::WSProxy),
            "интернетпочтовыйпрофиль" | "internetmailprofile" => {
                Some(Self::InternetMailProfile)
            }
            _ => None,
        }
    }

    fn timeout_param_position(self) -> usize {
        match self {
            Self::FTPConnection => 6,
            Self::HTTPConnection => 5,
            Self::WSDefinitions => 4,
            Self::WSProxy => 4,
            Self::InternetMailProfile => 5,
        }
    }
}

#[derive(Debug)]
struct NewExprWithoutTimeout {
    expr_id: ExprId,
    range: TextRange,
    target_var: Option<Name>,
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::TimeoutsInExternalResources;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);

    let mut diagnostics = crate::utils::for_each_body(ctx, |body, source_map, diags| {
        check_body_for_timeouts(body, source_map, code, ctx, &config, diags);
    });

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

fn check_body_for_timeouts(
    body: &hir::Body,
    source_map: &hir::BodySourceMap,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    config: &Config,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut candidates: Vec<NewExprWithoutTimeout> = Vec::new();

    for (expr_id, expr) in body.exprs_iter() {
        if let Expr::New { type_name, args } = expr {
            let resource_type = match type_name {
                Some(name) => ResourceType::from_name(name.as_str()),
                None => {
                    if !args.is_empty() {
                        if let Expr::Literal(Literal::String(s)) = body.expr_idx(args[0]) {
                            ResourceType::from_name(s.as_str())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            };

            if let Some(res_type) = resource_type {
                if res_type as u8 == ResourceType::InternetMailProfile as u8
                    && !config.analyze_internet_mail_profile_zero_timeout
                {
                    continue;
                }

                if !has_timeout_in_constructor(body, args, res_type) {
                    if let Some(range) = source_map.expr_range(expr_id) {
                        candidates.push(NewExprWithoutTimeout { expr_id, range, target_var: None });
                    }
                }
            }
        }
    }

    let timeout_assignments = find_timeout_assignments(body);

    for &stmt_idx in body.body_stmts_typed() {
        if let Stmt::Assign { target, value } = body.stmt_idx(stmt_idx) {
            let value_id = ExprId::from_idx(*value);
            if let Expr::New { .. } = body.expr_idx(*value) {
                if let Some(target_name) = extract_simple_path_idx(body, *target) {
                    for candidate in &mut candidates {
                        if value_id == candidate.expr_id {
                            candidate.target_var = Some(target_name.clone());
                            break;
                        }
                    }
                }
            }
        }
    }

    for candidate in candidates {
        let has_subsequent_timeout = candidate
            .target_var
            .as_ref()
            .map(|var_name| timeout_assignments.contains_key(var_name))
            .unwrap_or(false);

        if !has_subsequent_timeout {
            diagnostics.push(create_diagnostic(candidate.range, code, ctx));
        }
    }
}

fn has_timeout_in_constructor(
    body: &hir::Body,
    args: &[hir::ExprIdx],
    res_type: ResourceType,
) -> bool {
    let timeout_pos = res_type.timeout_param_position();

    if args.len() <= timeout_pos {
        return false;
    }

    let timeout_arg = body.expr_idx(args[timeout_pos]);

    match timeout_arg {
        Expr::Literal(Literal::Undefined) => {
            if args.len() > timeout_pos + 1 {
                for &arg_idx in args.iter().skip(timeout_pos + 1) {
                    let arg = body.expr_idx(arg_idx);
                    if !matches!(arg, Expr::Literal(Literal::Undefined) | Expr::Missing) {
                        return true;
                    }
                }
            }
            false
        }
        Expr::Missing => false,
        _ => true,
    }
}

fn find_timeout_assignments(body: &hir::Body) -> FxHashMap<Name, ()> {
    let mut result = FxHashMap::default();

    for (_stmt_id, stmt) in body.stmts_iter() {
        if let Stmt::Assign { target, .. } = stmt {
            if let Expr::Field { base, field } = body.expr(ExprId::from_idx(*target)) {
                if is_timeout_field(field) {
                    if let Some(var_name) = extract_simple_path_idx(body, *base) {
                        result.insert(var_name, ());
                    }
                }
            }
        }
    }

    result
}

fn is_timeout_field(field: &Name) -> bool {
    matches!(field.as_str().to_lowercase().as_str(), "таймаут" | "timeout")
}

fn extract_simple_path_idx(body: &hir::Body, expr_idx: hir::ExprIdx) -> Option<Name> {
    match body.expr_idx(expr_idx) {
        Expr::Path(name) => Some(name.clone()),
        _ => None,
    }
}

fn create_diagnostic(
    range: TextRange,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: "Timeout not specified when working with external resource".to_string(),
        range,
        severity: ctx.severity(code),
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_comprehensive() {
        let code = r#"
Процедура ПроверкаКейса()

    FTPСоединение = Новый FTPСоединение(Сервер, Порт, Пользователь, Пароль); // <<- ошибка
    Таймаут = 100;
    FTPСоединение = Новый FTPСоединение(Сервер, Порт, Пользователь, Пароль,,,, Неопределено); // <<- ошибка

    FTPСоединение = Новый FTPСоединение(Сервер, Порт, Пользователь, Пароль,,, 60);

    Определения = Новый WSОпределения("http://localhost/test.asmx?WSDL"); // <<- ошибка

    Прокси = Новый WSПрокси(Определения, "http://localhost/", "test", "test", Неопределено, 1);

    ПроксиДва = Новый WSПрокси(Определения, "http://localhost/", "test", "test"); // <<- ошибка

    Определения =
        Новый WSОпределения("http://localhost/test.asmx?WSDL", "Пользователь", "Пароль", Неопределено, Таймаут);

КонецПроцедуры

Процедура HTTPБезТаймАута()
    HTTPСоединение = Новый HTTPСоединение("zabbix.localhost", 80); // <<- ошибка
КонецПроцедуры

Процедура HTTPСТаймаутомВОбъявлении()
    HTTPСоединение = Новый HTTPСоединение("zabbix.localhost", 80,,,, Таймаут);
КонецПроцедуры

Процедура HTTPСТаймаутомВСвойстве()
    HTTPСоединение = Новый HTTPСоединение("zabbix.localhost", 80);
    HTTPСоединение.Таймаут = 1;
КонецПроцедуры

Функция НовыйИнтернетПочтовыйПрофильБезТаймАута()
    Профиль = Новый ИнтернетПочтовыйПрофиль; // <<- ошибка
    Профиль.Пользователь = "admin";
    Возврат Профиль;
КонецФункции

Функция НовыйИнтернетПочтовыйПрофильСТаймАутом()
    Профиль = Новый ИнтернетПочтовыйПрофиль;
    Профиль.Пользователь = "admin";
    Профиль.Таймаут = 5;
    Возврат Профиль;
КонецФункции

Функция НовыйИнтернетПочтовыйПрофильСТаймАутомИзФункции()
    Профиль = Новый ИнтернетПочтовыйПрофиль;
    Профиль.Пользователь = "admin";
    Профиль.Таймаут = ТаймАут();
    Возврат Профиль;
КонецФункции

Функция ТаймАут()
    Возврат 1;
КонецФункции

Процедура ТестУсловия()

    УсловиеИстина = Истина;

    HTTPСоединение = Новый HTTPСоединение("zabbix.localhost", 80);
    Если УсловиеИстина Тогда
        HTTPСоединение.Таймаут = 1;
    КонецЕсли;

КонецПроцедуры

Процедура ТестНаВложенныеБлокиКода()

    Попытка
    	Эластик_Соединение = Новый HTTPСоединение(Эластик_Сервер, Эластик_Порт, Эластик_Пользователь, Эластик_Пароль);
    Исключение
    	Эластик_Соединение = Неопределено;
    КонецПопытки;

КонецПроцедуры

Профиль = Новый ИнтернетПочтовыйПрофиль;

Структура.Вставить("ИнтернетПочтовыйПрофиль",  Новый ИнтернетПочтовыйПрофиль);
Структура.ТаймАут(ИнтернетПочтовыйПрофиль.ТаймАут); // Где то тут будет FP
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TimeoutsInExternalResources,
            expect![[r#"
                TimeoutsInExternalResources @ 4:21..4:76
                  message: Timeout not specified when working with external resource
                  severity: Critical
                TimeoutsInExternalResources @ 6:21..6:93
                  message: Timeout not specified when working with external resource
                  severity: Critical
                TimeoutsInExternalResources @ 10:19..10:73
                  message: Timeout not specified when working with external resource
                  severity: Critical
                TimeoutsInExternalResources @ 14:17..14:81
                  message: Timeout not specified when working with external resource
                  severity: Critical
                TimeoutsInExternalResources @ 22:22..22:66
                  message: Timeout not specified when working with external resource
                  severity: Critical
                TimeoutsInExternalResources @ 35:15..35:44
                  message: Timeout not specified when working with external resource
                  severity: Critical
                TimeoutsInExternalResources @ 72:27..72:115
                  message: Timeout not specified when working with external resource
                  severity: Critical
                TimeoutsInExternalResources @ 79:11..79:40
                  message: Timeout not specified when working with external resource
                  severity: Critical
                TimeoutsInExternalResources @ 81:48..81:77
                  message: Timeout not specified when working with external resource
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_ftp_without_timeout() {
        let code = r#"
Процедура Тест()
    FTPСоединение = Новый FTPСоединение(Сервер, Порт, Пользователь, Пароль);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TimeoutsInExternalResources,
            expect![[r#"
                TimeoutsInExternalResources @ 3:21..3:76
                  message: Timeout not specified when working with external resource
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_ftp_with_timeout() {
        let code = r#"
Процедура Тест()
    FTPСоединение = Новый FTPСоединение(Сервер, Порт, Пользователь, Пароль,,, 60);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TimeoutsInExternalResources,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_http_with_timeout_property() {
        let code = r#"
Процедура Тест()
    HTTPСоединение = Новый HTTPСоединение("zabbix.localhost", 80);
    HTTPСоединение.Таймаут = 1;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TimeoutsInExternalResources,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_internet_mail_profile() {
        let code = r#"
Функция Тест()
    Профиль = Новый ИнтернетПочтовыйПрофиль;
    Возврат Профиль;
КонецФункции
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TimeoutsInExternalResources,
            expect![[r#"
                TimeoutsInExternalResources @ 3:15..3:44
                  message: Timeout not specified when working with external resource
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_internet_mail_profile_with_timeout() {
        let code = r#"
Функция Тест()
    Профиль = Новый ИнтернетПочтовыйПрофиль;
    Профиль.Таймаут = 5;
    Возврат Профиль;
КонецФункции
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TimeoutsInExternalResources,
            expect![[r#""#]],
        );
    }
}
