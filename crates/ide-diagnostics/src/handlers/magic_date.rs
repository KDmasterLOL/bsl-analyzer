use crate::define_metadata;
use crate::metadata::*;
use crate::utils::literal_context;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_AUTHORIZED_DATES: &str = "00010101,00010101000000,000101010000";

#[derive(Debug, Clone)]
struct Config {
    authorized_dates: HashSet<String>,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let authorized_str = ctx.config_string(
            DiagnosticCode::MagicDate,
            "authorizedDates",
            DEFAULT_AUTHORIZED_DATES,
        );
        let authorized_dates: HashSet<String> = authorized_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        tracing::debug!(
            count = authorized_dates.len(),
            "MagicDate config: authorized dates loaded"
        );

        Self { authorized_dates }
    }
}

fn is_date_literal(token: &SyntaxToken) -> bool {
    matches!(token.kind(), SyntaxKind::STRING | SyntaxKind::DATE)
}

fn extract_date_text(token: &SyntaxToken) -> Option<String> {
    let text = token.text();

    if text.len() < 3 {
        return None;
    }

    let inner = &text[1..text.len() - 1];

    if token.kind() == SyntaxKind::STRING {
        if inner.len() != 8 && inner.len() != 14 {
            return None;
        }
        if !inner.starts_with(|c: char| c.is_ascii_digit() && matches!(c, '0'..='3')) {
            return None;
        }
        if !inner.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
    } else if token.kind() == SyntaxKind::DATE {
        let digits_only: String = inner.chars().filter(|c| c.is_ascii_digit()).collect();

        if digits_only.len() < 8 {
            return None;
        }
        if !digits_only.starts_with(|c: char| matches!(c, '0'..='3')) {
            return None;
        }
    } else {
        return None;
    }

    Some(inner.to_string())
}

fn is_valid_date(date_str: &str) -> bool {
    if date_str.len() != 8 && date_str.len() != 14 {
        return false;
    }

    let year = date_str[0..4].parse::<u32>().ok();
    let month = date_str[4..6].parse::<u32>().ok();
    let day = date_str[6..8].parse::<u32>().ok();

    if let (Some(year), Some(month), Some(day)) = (year, month, day) {
        if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return false;
        }

        if date_str.len() == 14 {
            let hour = date_str[8..10].parse::<u32>().ok();
            let minute = date_str[10..12].parse::<u32>().ok();
            let second = date_str[12..14].parse::<u32>().ok();

            if let (Some(h), Some(m), Some(s)) = (hour, minute, second) {
                return h <= 23 && m <= 59 && s <= 59;
            }
            return false;
        }

        true
    } else {
        false
    }
}

fn is_authorized(date_str: &str, config: &Config) -> bool {
    let digits_only: String = date_str.chars().filter(|c| c.is_ascii_digit()).collect();

    config.authorized_dates.contains(&digits_only)
}

fn is_excluded_context(token: &SyntaxToken) -> bool {
    use tracing::debug;

    if is_in_return_statement(token) {
        debug!("Excluded: in return statement");
        return true;
    }
    if literal_context::is_in_default_value(token) {
        debug!("Excluded: in default value");
        return true;
    }
    if is_in_structure_or_correspondence_insert(token) {
        debug!("Excluded: in structure/correspondence insert");
        return true;
    }
    if literal_context::is_in_structure_constructor(token, &[]) {
        debug!("Excluded: in structure constructor");
        return true;
    }
    if literal_context::is_in_property_assignment(token) {
        debug!("Excluded: in property assignment");
        return true;
    }
    if is_in_date_function_simple_assignment(token) {
        debug!("Excluded: in date function simple assignment");
        return true;
    }
    if literal_context::is_in_simple_assignment(token) {
        debug!("Excluded: in simple assignment");
        return true;
    }
    false
}

fn is_in_return_statement(token: &SyntaxToken) -> bool {
    let mut node = token.parent();
    while let Some(current) = node {
        if current.kind() == SyntaxKind::RETURN_STMT {
            return true;
        }
        node = current.parent();
    }
    false
}

fn is_in_structure_or_correspondence_insert(token: &SyntaxToken) -> bool {
    let mut node = token.parent();

    while let Some(current) = node {
        if matches!(current.kind(), SyntaxKind::CALL_STMT | SyntaxKind::CALL_EXPR) {
            if let Some(method_name) = literal_context::find_method_name(&current) {
                let name = method_name.to_lowercase();
                if name == "вставить" || name == "insert" {
                    return true;
                }
            }
        }
        node = current.parent();
    }

    false
}

fn is_in_date_function_simple_assignment(token: &SyntaxToken) -> bool {
    let mut node = token.parent();
    let mut arg_list: Option<SyntaxNode> = None;

    while let Some(current) = node.clone() {
        if current.kind() == SyntaxKind::ARG_LIST {
            arg_list = Some(current.clone());
            break;
        }
        if matches!(current.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF) {
            return false;
        }
        node = current.parent();
    }

    let Some(arg_list) = arg_list else {
        return false;
    };

    let Some(parent_expr) = arg_list.parent() else {
        return false;
    };

    if !matches!(parent_expr.kind(), SyntaxKind::EXPR | SyntaxKind::CALL_EXPR) {
        return false;
    }

    let has_date_ident = parent_expr
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .any(|t| {
            let name = t.text().to_lowercase();
            name == "дата" || name == "date"
        });

    if !has_date_ident {
        return false;
    }

    let mut expr_node = Some(parent_expr);
    while let Some(current) = expr_node {
        if current.kind() == SyntaxKind::ASSIGN_STMT {
            let has_dot = current
                .descendants_with_tokens()
                .any(|e| e.as_token().is_some_and(|t| t.kind() == SyntaxKind::DOT));

            if has_dot {
                return false;
            }

            let has_binary = current.descendants().any(|d| d.kind() == SyntaxKind::BINARY_EXPR);

            return !has_binary;
        }
        expr_node = current.parent();
    }

    false
}

#[inline]
pub fn check_token(token: &SyntaxToken, acc: &mut Vec<Diagnostic>, ctx: &DiagnosticsContext) {
    let code = DiagnosticCode::MagicDate;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    if !is_date_literal(token) {
        return;
    }

    let Some(date_str) = extract_date_text(token) else {
        return;
    };

    if token.kind() == SyntaxKind::STRING && !is_valid_date(&date_str) {
        return;
    }

    let config = Config::from_context(ctx);

    if is_authorized(&date_str, &config) {
        return;
    }

    if is_excluded_context(token) {
        return;
    }

    acc.push(Diagnostic {
        code,
        message: format!(
            "Создайте переменную с понятным названием, присвойте ей значение \"{}\" и используйте эту константу вместо магической даты.",
            date_str
        ),
        severity: ctx.severity(code),
        range: token.text_range(),
        tags: ctx.tags(code),
        fixes: vec![],
    });
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("MagicDate::check").entered();

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            check_token(&token, &mut diagnostics, ctx);
        }
    }

    tracing::debug!(count = diagnostics.len(), "MagicDate diagnostics found");

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic, check_ast_diagnostic_with_config};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_line_31_detection() {
        let code = r#"Процедура Тест()
    ОтборЭлемента.ПравоеЗначение = Новый СтандартнаяДатаНачала(Дата('19800101000000'));
КонецПроцедуры"#;

        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1, "Should detect the date inside nested function calls");
    }

    #[test]
    fn test_date_in_expression_detected() {
        let code = r#"Процедура Тест()
	День = Дата("00020101") + Шаг;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Дата() in expression should be detected");
    }

    #[test]
    fn test_date_with_time_in_expression_detected() {
        let code = r#"Процедура Тест()
	День = Дата("00020101121314") + Шаг;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Дата() with time in expression should be detected");
    }

    #[test]
    fn test_single_quoted_date_in_expression_positions() {
        let code = r#"Процедура Тест()
	День = '00010102' + Шаг;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Single-quoted date in expression should be detected");
    }

    #[test]
    fn test_date_in_while_condition_detected() {
        let code = r#"Процедура Тест()
	Пока Сейчас < '12340101' Цикл
		Сейчас = Сейчас + Шаг;
	КонецЦикла;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Date in while condition should be detected");
    }

    #[test]
    fn test_date_in_method_calls_detected() {
        let code = r#"Процедура Тест()
	ИменаПараметров = СтроковыеФункции.РазложитьСтрокуВМассивПодстрок(ИмяПараметра, "00050101", "00050101");
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 2, "Both dates in method call should be detected");
    }

    #[test]
    fn test_date_in_custom_call_detected() {
        let code = r#"Процедура Тест()
	Настройки = Настройки('12350101');
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Date in function argument should be detected");
    }

    #[test]
    fn test_date_in_string_method_call_detected() {
        let code = r#"Процедура Тест()
	Настройки.Свойство("00020501121314", ЗначениеЕдиничногоПараметра);
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "String date in method call should be detected");
    }

    #[test]
    fn test_dates_in_execute_expression() {
        let code = r#"Процедура Тест()
	Выполнить("00020501121314" + '12350101');
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 2, "Both dates in Выполнить should be detected");
    }

    #[test]
    fn test_date_nested_in_constructor_call() {
        let code = r#"Процедура Тест()
	ОтборЭлемента.ПравоеЗначение = Новый СтандартнаяДатаНачала(Дата('19800101000000'));
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Date inside nested constructor call should be detected");
    }

    #[test]
    fn test_dates_in_ternary_expression() {
        let code = r#"Процедура Тест()
	Значение = ?(А = '39990202', '39991231235959', '39990101000000');
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 3, "All three dates in ternary should be detected");
    }

    #[test]
    fn test_date_in_if_condition() {
        let code = r#"Процедура Тест()
	Если Сейчас < Дата("12340101") Тогда
	КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Date in if condition should be detected");
    }

    #[test]
    fn test_configured_authorized_dates() {
        let code = "Процедура Тест()\n\tДата1 = '19800101' + Шаг;\n\tДата2 = '20250101' + Шаг;\nКонецПроцедуры";

        let diagnostics_default = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics_default.len(), 2, "Both dates should be detected by default");

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicDate,
            serde_json::json!({
                "authorizedDates": "00010101,00010101000000,000101010000,19800101"
            }),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "With 19800101 authorized, only 20250101 should be detected"
        );
        assert!(
            diagnostics[0].message.contains("20250101"),
            "Remaining diagnostic should be for 20250101"
        );
    }

    #[test]
    fn test_single_quoted_date_in_expression() {
        let code = r"День = '00010102' + Шаг;";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Single-quoted date in expression should be detected");
    }

    #[test]
    fn test_date_function_simple_assignment_excluded() {
        let code = r#"День = Дата("00020101");"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Simple Дата() assignment should be excluded");
    }

    #[test]
    fn test_date_function_expression_detected() {
        let code = r#"День = Дата("00020101") + Шаг;"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Дата() in expression should be detected");
    }

    #[test]
    fn test_authorized_date_excluded() {
        let code = r#"День = '00010101';"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Authorized date should be excluded");
    }

    #[test]
    fn test_return_statement_excluded() {
        let code = r"Возврат '39991231235959';";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Date in return statement should be excluded");
    }
}
