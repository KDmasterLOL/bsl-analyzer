//! Reports `НСтр` / `NStr` calls with missing languages when they are used as templates.

use crate::define_metadata;
use crate::metadata::*;
use crate::utils::nstr::{
    extract_language_keys, get_assigned_variable_name, has_template_in_parents, is_nstr_call,
    is_variable_used_in_template, NstrConfig,
};
use crate::{sdbl_utils, Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::SyntaxKind;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Localize],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Checks multilingual `НСтр` literals used by `СтрШаблон` / `StrTemplate`.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("MultilingualStringUsingWithTemplate::check").entered();

    let code = DiagnosticCode::MultilingualStringUsingWithTemplate;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = NstrConfig::from_context(ctx, code);
    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    // Find all NStr calls by finding IDENT tokens with НСтр/NStr text
    for token in root.descendants_with_tokens() {
        let tok = match token {
            syntax::NodeOrToken::Token(t) => t,
            _ => continue,
        };

        if tok.kind() != SyntaxKind::IDENT || !is_nstr_call(tok.text()) {
            continue;
        }

        // AST structure: CALL_EXPR > IDENT(node) > IDENT(token) > ARG_LIST
        // Or for qualified: CALL_EXPR > FIELD_EXPR > IDENT(node) > IDENT(token)
        // tok.parent() returns IDENT node, we need to find CALL_EXPR ancestor
        let call_expr = match tok
            .parent()
            .and_then(|p| p.ancestors().find(|n| n.kind() == SyntaxKind::CALL_EXPR))
        {
            Some(ce) => ce,
            None => continue,
        };

        // Check if NStr is inside StrTemplate call OR assigned to variable used in StrTemplate
        let in_template = has_template_in_parents(&call_expr);
        let used_in_template = get_assigned_variable_name(&call_expr)
            .map(|var| is_variable_used_in_template(&var, &call_expr))
            .unwrap_or(false);

        // Skip if NOT in template context - this is the opposite of MultilingualStringHasAllDeclaredLanguages
        if !in_template && !used_in_template {
            continue;
        }

        // Find ARG_LIST sibling
        let arg_list = call_expr.children().find(|n| n.kind() == SyntaxKind::ARG_LIST);
        let arg_list = match arg_list {
            Some(al) => al,
            None => {
                // НСтр() with empty arguments in template context - error
                diagnostics.push(Diagnostic {
                    code,
                    message: format!(
                        "Добавьте строки для языков: [{}]",
                        config.declared_languages.iter().cloned().collect::<Vec<_>>().join(", ")
                    ),
                    severity: ctx.severity(code),
                    range: call_expr.text_range(),
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
                continue;
            }
        };

        // Get first argument from ARG_LIST
        let first_arg = arg_list.children().find(|n| n.kind() == SyntaxKind::EXPR);
        let first_arg = match first_arg {
            Some(a) => a,
            None => {
                // Empty arguments in template context
                diagnostics.push(Diagnostic {
                    code,
                    message: format!(
                        "Добавьте строки для языков: [{}]",
                        config.declared_languages.iter().cloned().collect::<Vec<_>>().join(", ")
                    ),
                    severity: ctx.severity(code),
                    range: call_expr.text_range(),
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
                continue;
            }
        };

        // Find the LITERAL node containing the string
        let literal = first_arg.descendants().find(|n| n.kind() == SyntaxKind::LITERAL);
        let literal = match literal {
            Some(l) => l,
            None => continue,
        };

        // Extract the string content
        let string_content = match sdbl_utils::extract_string_content(&literal) {
            Some(s) => s,
            None => continue,
        };

        // Extract language keys from the string
        let found_languages = extract_language_keys(&string_content);

        // Find missing languages
        let missing: Vec<&String> = config
            .declared_languages
            .iter()
            .filter(|lang| !found_languages.contains(*lang))
            .collect();

        if !missing.is_empty() {
            // Format missing languages for message
            let missing_str = missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");

            diagnostics.push(Diagnostic {
                code,
                message: format!("Добавьте строки для языков: [{}]", missing_str),
                severity: ctx.severity(code),
                range: call_expr.text_range(),
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    tracing::debug!(
        count = diagnostics.len(),
        "MultilingualStringUsingWithTemplate diagnostics found"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assert_diagnostic_range_multiline, check_ast_diagnostic_with_config};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_only_ru() {
        // Default config: declaredLanguages = "ru"
        // Only NStr in StrTemplate context with missing ru triggers diagnostic
        let code = r#"// Считаем, что в конфигурации два языка ru и en
Процедура БезОшибок()

    Приветствие = НСтр("ru='Привет, я простая строка';
        |en='Hi, i'm a simple string'");

    ПолеЗапроса = СтрШаблон("%1.%2 КАК %2", "Документ", "Автомобиль");

КонецПроцедуры

Функция СОшибками(Строка)

    БезТекста = НСтр();
    СНевернымФорматомСтроки = НСтр("Тут текст который не относиться к ниодному языку");

    ТекстТолькоНаРусском = НСтр("ru='Привет, я простая строка''");
    ТекстТолькоНаАнглийском = НСтр("en='Hi, i'm a simple string'");

    СообщениеПользователю = СтрШаблон(НСтр("ru='В строке №%1 не заполнена номенклатура'"), Строка.Номер);
    СообщениеПользователю = СтрШаблон(НСтр("en='In line №%1 nomenclature is not filled'"), Строка.Номер);

    ТекстТолькоНаРусском2 = НСтр("ru='В строке №%1 не заполнена номенклатура'");
    СообщениеПользователю = СтрШаблон(ТекстТолькоНаРусском2, Строка.Номер);

    ТекстТолькоНаАнглийском2 = НСтр("en='In line №%1 nomenclature is not filled'");
    СообщениеПользователю = СтрШаблон(ТекстТолькоНаАнглийском2, Строка.Номер);
    Возврат КонструкторАдресов();

    Порция = Сервис.Autocomplete(ИдентификаторАдресногоОбъекта, 0, НСтр("ru = 'ДОМ='"), 1, КодЯзыка, Метаданные.Имя);

    ВычисляемоеПоле.Оформление.УстановитьЗначениеПараметра("Формат", НСтр("ru = 'ДФ=''д ММММ'''"));

    Возврат НСтр("en=""You must specify the user's extension number to the PBX."";ru='Необходимо указать внутренний номер пользователя АТС.'");

КонецФункции
"#;
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Expected 2 diagnostics with default config (declaredLanguages = "ru")
        assert_eq!(diagnostics.len(), 2, "Should find 2 diagnostics for ru only");

        // Verify exact positions (0-indexed)
        assert_diagnostic_range_multiline(code, &diagnostics[0], 19, 38, 19, 89);
        assert_diagnostic_range_multiline(code, &diagnostics[1], 24, 31, 24, 82);
    }

    #[test]
    fn test_ru_and_en() {
        // declaredLanguages = "ru,en": NStr in StrTemplate context missing either language triggers
        let code = r#"// Считаем, что в конфигурации два языка ru и en
Процедура БезОшибок()

    Приветствие = НСтр("ru='Привет, я простая строка';
        |en='Hi, i'm a simple string'");

    ПолеЗапроса = СтрШаблон("%1.%2 КАК %2", "Документ", "Автомобиль");

КонецПроцедуры

Функция СОшибками(Строка)

    БезТекста = НСтр();
    СНевернымФорматомСтроки = НСтр("Тут текст который не относиться к ниодному языку");

    ТекстТолькоНаРусском = НСтр("ru='Привет, я простая строка''");
    ТекстТолькоНаАнглийском = НСтр("en='Hi, i'm a simple string'");

    СообщениеПользователю = СтрШаблон(НСтр("ru='В строке №%1 не заполнена номенклатура'"), Строка.Номер);
    СообщениеПользователю = СтрШаблон(НСтр("en='In line №%1 nomenclature is not filled'"), Строка.Номер);

    ТекстТолькоНаРусском2 = НСтр("ru='В строке №%1 не заполнена номенклатура'");
    СообщениеПользователю = СтрШаблон(ТекстТолькоНаРусском2, Строка.Номер);

    ТекстТолькоНаАнглийском2 = НСтр("en='In line №%1 nomenclature is not filled'");
    СообщениеПользователю = СтрШаблон(ТекстТолькоНаАнглийском2, Строка.Номер);
    Возврат КонструкторАдресов();

    Порция = Сервис.Autocomplete(ИдентификаторАдресногоОбъекта, 0, НСтр("ru = 'ДОМ='"), 1, КодЯзыка, Метаданные.Имя);

    ВычисляемоеПоле.Оформление.УстановитьЗначениеПараметра("Формат", НСтр("ru = 'ДФ=''д ММММ'''"));

    Возврат НСтр("en=""You must specify the user's extension number to the PBX."";ru='Необходимо указать внутренний номер пользователя АТС.'");

КонецФункции
"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MultilingualStringUsingWithTemplate,
            serde_json::json!({
                "declaredLanguages": "ru,en"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Expected 4 diagnostics with declaredLanguages = "ru,en"
        assert_eq!(diagnostics.len(), 4, "Should find 4 diagnostics for ru,en");

        assert_diagnostic_range_multiline(code, &diagnostics[0], 18, 38, 18, 89);
        assert_diagnostic_range_multiline(code, &diagnostics[1], 19, 38, 19, 89);
        assert_diagnostic_range_multiline(code, &diagnostics[2], 21, 28, 21, 79);
        assert_diagnostic_range_multiline(code, &diagnostics[3], 24, 31, 24, 82);
    }

    #[test]
    fn test_no_error_when_all_languages_present() {
        let code = r#"
Процедура Тест()
    Сообщение = СтрШаблон(НСтр("ru='Значение: %1'; en='Value: %1'"), Значение);
КонецПроцедуры
"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MultilingualStringUsingWithTemplate,
            serde_json::json!({
                "declaredLanguages": "ru,en"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 0, "Should not detect when all languages present");
    }

    #[test]
    fn test_nstr_outside_template_not_detected() {
        let code = r#"
Процедура Тест()
    // This should NOT fire - NStr is not in StrTemplate
    Текст = НСтр("ru='Привет'");
КонецПроцедуры
"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MultilingualStringUsingWithTemplate,
            serde_json::json!({
                "declaredLanguages": "ru,en"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 0, "Should not fire when NStr is not in StrTemplate");
    }
}
