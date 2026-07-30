use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Body, BodySourceMap, Expr, ExprId, ExprIdx, IdConversion, Literal, Stmt};
use regex::Regex;
use stdx::case::CaseExt;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_SEARCH_WORDS: &str = "Пароль|Password";

fn is_structure_or_map(type_name: &Option<hir::Name>) -> bool {
    let Some(name) = type_name else {
        return false;
    };
    let text = name.as_str().fold_lower();
    matches!(text.as_str(), "структура" | "structure" | "соответствие" | "map")
}

fn is_connection(type_name: &Option<hir::Name>) -> bool {
    let Some(name) = type_name else {
        return false;
    };
    let text = name.as_str().fold_lower();
    matches!(text.as_str(), "httpсоединение" | "httpconnection" | "ftpсоединение" | "ftpconnection")
}

fn is_insert_method(method_name: &hir::Name) -> bool {
    let text = method_name.as_str().fold_lower();
    matches!(text.as_str(), "вставить" | "insert")
}

fn is_not_empty_string(s: &str) -> bool {
    !s.is_empty() && !s.bytes().all(|b| b == b'*')
}

fn get_string_literal(body: &Body, expr_idx: ExprIdx) -> Option<&str> {
    let expr = body.expr_idx(expr_idx);
    if let Expr::Literal(Literal::String(s)) = expr {
        Some(s.as_str())
    } else {
        None
    }
}

fn extract_string_content(s: &str) -> String {
    s.replace(['"', ' '], "")
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UsingHardcodeSecretInformation;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let search_words_str =
        ctx.config.get_string(code, "searchWords").unwrap_or(DEFAULT_SEARCH_WORDS);

    let search_words = Regex::new(&format!("(?i)^({})$", search_words_str))
        .unwrap_or_else(|_| Regex::new(&format!("(?i)^({})$", DEFAULT_SEARCH_WORDS)).unwrap());

    let mut diagnostics = crate::utils::for_each_body(ctx, |body, source_map, diags| {
        check_body(body, source_map, &search_words, code, ctx, diags);
    });

    diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));
    diagnostics
}

fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    search_words: &Regex,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (stmt_id, stmt) in body.stmts_iter() {
        let Stmt::Assign { target, value } = stmt else {
            continue;
        };

        let target_expr = body.expr_idx(*target);
        let value_expr_idx = *value;

        if check_assignment(body, target_expr, value_expr_idx, search_words) {
            if let Some(stmt_range) = source_map.stmt_range(stmt_id) {
                diagnostics.push(Diagnostic {
                    code,
                    message: "Используется хранение конфиденциальной информации в коде".to_string(),
                    severity: ctx.severity(code),
                    range: stmt_range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
            }
        }
    }

    for (expr_id, expr) in body.exprs_iter() {
        match expr {
            Expr::MethodCall { receiver: _, method, args } => {
                if !is_insert_method(method) {
                    continue;
                }
                check_insert_call(
                    body,
                    source_map,
                    args,
                    expr_id,
                    search_words,
                    code,
                    ctx,
                    diagnostics,
                );
            }
            Expr::Call { callee, args } => {
                let callee_expr = body.expr_idx(*callee);
                let method_name = match callee_expr {
                    Expr::Field { field, .. } => Some(field),
                    _ => None,
                };
                if let Some(field) = method_name {
                    if !is_insert_method(field) {
                        continue;
                    }
                    check_insert_call(
                        body,
                        source_map,
                        args,
                        expr_id,
                        search_words,
                        code,
                        ctx,
                        diagnostics,
                    );
                }
            }
            Expr::New { type_name, args } => {
                if is_connection(type_name) {
                    if args.len() >= 4 {
                        let password_idx = args[3];
                        if let Some(password_str) = get_string_literal(body, password_idx) {
                            if is_not_empty_string(password_str) {
                                if let Some(range) =
                                    find_containing_statement_range(body, source_map, expr_id)
                                {
                                    diagnostics.push(Diagnostic {
                                        code,
                                        message: "Используется хранение конфиденциальной информации в коде".to_string(),
                                        severity: ctx.severity(code),
                                        range,
                                        tags: ctx.tags(code),
                                        fixes: vec![],
                                    });
                                }
                            }
                        }
                    }
                } else if is_structure_or_map(type_name) && !args.is_empty() {
                    let keys_idx = args[0];
                    if let Some(keys_str) = get_string_literal(body, keys_idx) {
                        let keys: Vec<&str> = keys_str.split(',').collect();
                        for (i, key) in keys.iter().enumerate() {
                            let clean_key = extract_string_content(key);
                            if search_words.is_match(&clean_key) {
                                let value_index = i + 1;
                                if value_index < args.len() {
                                    let value_idx = args[value_index];
                                    if let Some(value_str) = get_string_literal(body, value_idx) {
                                        if is_not_empty_string(value_str) {
                                            if let Some(range) = find_containing_statement_range(
                                                body, source_map, expr_id,
                                            ) {
                                                diagnostics.push(Diagnostic {
                                                    code,
                                                    message: "Используется хранение конфиденциальной информации в коде".to_string(),
                                                    severity: ctx.severity(code),
                                                    range,
                                                    tags: ctx.tags(code),
                                                    fixes: vec![],
                                                });
                                            }
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_insert_call(
    body: &Body,
    source_map: &BodySourceMap,
    args: &[ExprIdx],
    expr_id: ExprId,
    search_words: &Regex,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if args.len() < 2 {
        return;
    }
    let key_expr_idx = args[0];
    let value_expr_idx = args[1];

    if let Some(key_str) = get_string_literal(body, key_expr_idx) {
        let clean_key = extract_string_content(key_str);
        if search_words.is_match(&clean_key) {
            if let Some(value_str) = get_string_literal(body, value_expr_idx) {
                if is_not_empty_string(value_str) {
                    let range = find_containing_statement_range(body, source_map, expr_id)
                        .or_else(|| source_map.expr_range(expr_id));
                    if let Some(range) = range {
                        diagnostics.push(Diagnostic {
                            code,
                            message: "Используется хранение конфиденциальной информации в коде"
                                .to_string(),
                            severity: ctx.severity(code),
                            range,
                            tags: ctx.tags(code),
                            fixes: vec![],
                        });
                    }
                }
            }
        }
    }
}

fn check_assignment(
    body: &Body,
    target_expr: &Expr,
    value_expr_idx: ExprIdx,
    search_words: &Regex,
) -> bool {
    let Some(value_str) = get_string_literal(body, value_expr_idx) else {
        return false;
    };
    if !is_not_empty_string(value_str) {
        return false;
    }

    match target_expr {
        Expr::Path(name) => search_words.is_match(name.as_str()),
        Expr::Field { base: _, field } => search_words.is_match(field.as_str()),
        Expr::Index { base: _, index } => {
            if let Some(index_str) = get_string_literal(body, *index) {
                let clean_index = extract_string_content(index_str);
                search_words.is_match(&clean_index)
            } else {
                false
            }
        }
        _ => false,
    }
}

fn find_containing_statement_range(
    body: &Body,
    source_map: &BodySourceMap,
    expr_id: ExprId,
) -> Option<ide_db::TextRange> {
    let target_idx: ExprIdx = expr_id.to_idx();

    for (stmt_id, stmt) in body.stmts_iter() {
        if stmt_contains_expr_idx(stmt, target_idx) {
            if let Some(range) = source_map.stmt_range(stmt_id) {
                return Some(range);
            }
        }
    }
    source_map.expr_range(expr_id)
}

fn stmt_contains_expr_idx(stmt: &Stmt, target_idx: ExprIdx) -> bool {
    match stmt {
        Stmt::Expr(e) => *e == target_idx,
        Stmt::Assign { target, value } => *target == target_idx || *value == target_idx,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic_with_config, check_diagnostics_snapshot_for};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;
    #[test]
    fn test_comprehensive() {
        let code = r#"
Перем Пароль;
Перем Password;

Процедура ХардкодимПароль()

    КакаяТоСтруктура = Новый Структура;
    КакаяТоСтруктура.Вставить("Логин", "Петька");
    КакаяТоСтруктура.Вставить("Пароль", "12345"); // <-- Уязвимость
    КакаяТоСтруктура.Вставить("Пароль", "");
    КакаяТоСтруктура.Вставить("Пароль", Неопределено);

    ВтораяСтруктура = Новый Структура("Первое, Пароль, Компот", 1, "qwerty", ""); // <-- Уязвимость

    ТретьяСтруктура = Новый Структура("Пароль", Неопределено);

    Пароль = "12345670"; // <-- Уязвимость
    Password = "qwerty"; // <-- Уязвимость

    Пароль = Неопределено;
    Password = Undefined;
    // Пароль = "123";
    // Это называется Пароль?
    НеПароль = "123";
    Пороль = "Истина";

    Map = New Map();
    Map.Insert("Password", "1234"); // <-- Уязвимость

    Данные = Новый Структура("Логин, Пароль");
    Данные.Пароль = Неопределено;
    Данные.Пароль = "";
    Данные.Пароль = "12345"; // <-- Уязвимость
    Данные["Пароль"] = "qwerty"; // <-- Уязвимость

    Пароль = "" + ВернутьПароль(); // пока не сработает
    Пароль = "134" + ВернутьПароль(); // пока не сработает

    НоваяСтрока = ВтораяСтруктура.Первое + "%" + ВтораяСтруктура.Пароль + "@";

    ДанныеУровень2 = Новый Структура("Пароль");
    Данные = Новый Структура("Аутентификация", ДанныеУровень2);
    Данные["Аутентификация"]["Пароль"] = ВтораяСтруктура.Пароль + "@";

    СоединениеHTTP = Новый HTTPСоединение("Сервер", 8080, "Пользователь", "12345"); // <-- Уязвимость
    СоединениеFTP = Новый FTPСоединение("Сервер", 80, "Пользователь", "dfdfdf"); // <-- Уязвимость
    СоединениеHTTP = Новый HTTPСоединение("Сервер", 8080);

    Пароль = "*12345*"; // <-- Уязвимость
    Пароль = "*12345"; // <-- Уязвимость
    Пароль = "12345*"; // <-- Уязвимость
    Пароль = "******";
    Пароль = "*";

КонецПроцедуры

Функция ВернутьПароль()
    Возврат "Пароль?";
КонецФункции

Пароль = "";
Password = "";
ХардкодимПароль();

Элементы["Пароль"]["Заголовок"] = "Пароль";
Элементы.Пароль.Заголовок = "Пароль";"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#"
                UsingHardcodeSecretInformation @ 9:5..9:49
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical
                UsingHardcodeSecretInformation @ 13:5..13:81
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical
                UsingHardcodeSecretInformation @ 17:5..17:24
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical
                UsingHardcodeSecretInformation @ 18:5..18:24
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical
                UsingHardcodeSecretInformation @ 28:5..28:35
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical
                UsingHardcodeSecretInformation @ 33:5..33:28
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical
                UsingHardcodeSecretInformation @ 34:5..34:32
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical
                UsingHardcodeSecretInformation @ 45:5..45:83
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical
                UsingHardcodeSecretInformation @ 46:5..46:80
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical
                UsingHardcodeSecretInformation @ 49:5..49:23
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical
                UsingHardcodeSecretInformation @ 50:5..50:22
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical
                UsingHardcodeSecretInformation @ 51:5..51:22
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_simple_assignment() {
        let code = r#"
Процедура Тест()
    Пароль = "12345";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#"
                UsingHardcodeSecretInformation @ 3:5..3:21
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_assignment_empty_string() {
        let code = r#"
Процедура Тест()
    Пароль = "";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_assignment_asterisks() {
        let code = r#"
Процедура Тест()
    Пароль = "******";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_structure_property_access() {
        let code = r#"
Процедура Тест()
    Данные.Пароль = "12345";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#"
                UsingHardcodeSecretInformation @ 3:5..3:28
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_structure_index_access() {
        let code = r#"
Процедура Тест()
    Данные["Пароль"] = "qwerty";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#"
                UsingHardcodeSecretInformation @ 3:5..3:32
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_insert_method() {
        let code = r#"
Процедура Тест()
    Структура.Вставить("Пароль", "12345");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#"
                UsingHardcodeSecretInformation @ 3:5..3:42
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_insert_method_empty_value() {
        let code = r#"
Процедура Тест()
    Структура.Вставить("Пароль", "");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_structure_constructor() {
        let code = r#"
Процедура Тест()
    Структура = Новый Структура("Первое, Пароль, Третье", 1, "qwerty", 3);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#"
                UsingHardcodeSecretInformation @ 3:5..3:74
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_http_connection() {
        let code = r#"
Процедура Тест()
    Соединение = Новый HTTPСоединение("Сервер", 8080, "Пользователь", "12345");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#"
                UsingHardcodeSecretInformation @ 3:5..3:79
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_ftp_connection() {
        let code = r#"
Процедура Тест()
    Соединение = Новый FTPСоединение("Сервер", 21, "Пользователь", "password");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#"
                UsingHardcodeSecretInformation @ 3:5..3:79
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_connection_no_password() {
        let code = r#"
Процедура Тест()
    Соединение = Новый HTTPСоединение("Сервер", 8080);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_custom_search_words() {
        let code = r#"
Процедура Тест()
    Password = "qwerty";
    Пароль = "12345";
КонецПроцедуры
"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::UsingHardcodeSecretInformation,
            serde_json::json!({
                "searchWords": "Password"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 1, "Should only match 'Password' with custom config");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    пАрОлЬ = "12345";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingHardcodeSecretInformation,
            expect![[r#"
                UsingHardcodeSecretInformation @ 3:5..3:21
                  message: Используется хранение конфиденциальной информации в коде
                  severity: Critical"#]],
        );
    }
}
