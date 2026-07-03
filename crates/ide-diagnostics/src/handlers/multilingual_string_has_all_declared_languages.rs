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
    severity: DiagnosticSeverityLevel::Minor,
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

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("MultilingualStringHasAllDeclaredLanguages::check").entered();

    let code = DiagnosticCode::MultilingualStringHasAllDeclaredLanguages;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = NstrConfig::from_context(ctx, code);
    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    for token in root.descendants_with_tokens() {
        let tok = match token {
            syntax::NodeOrToken::Token(t) => t,
            _ => continue,
        };

        if tok.kind() != SyntaxKind::IDENT || !is_nstr_call(tok.text()) {
            continue;
        }

        let call_expr = match tok
            .parent()
            .and_then(|p| p.ancestors().find(|n| n.kind() == SyntaxKind::CALL_EXPR))
        {
            Some(ce) => ce,
            None => continue,
        };

        let arg_list = call_expr.children().find(|n| n.kind() == SyntaxKind::ARG_LIST);
        let arg_list = match arg_list {
            Some(al) => al,
            None => {
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

        if has_template_in_parents(&call_expr) {
            continue;
        }

        if let Some(var_name) = get_assigned_variable_name(&call_expr) {
            if is_variable_used_in_template(&var_name, &call_expr) {
                continue;
            }
        }

        let first_arg = arg_list.children().find(|n| n.kind() == SyntaxKind::EXPR);
        let first_arg = match first_arg {
            Some(a) => a,
            None => {
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

        let literal = first_arg.descendants().find(|n| n.kind() == SyntaxKind::LITERAL);
        let literal = match literal {
            Some(l) => l,
            None => continue,
        };

        let string_content = match sdbl_utils::extract_string_content(&literal) {
            Some(s) => s,
            None => continue,
        };

        let found_languages = extract_language_keys(&string_content);

        let missing: Vec<&String> = config
            .declared_languages
            .iter()
            .filter(|lang| !found_languages.contains(*lang))
            .collect();

        if !missing.is_empty() {
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
        "MultilingualStringHasAllDeclaredLanguages diagnostics found"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{check_ast_diagnostic_with_config, format_diags};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;
    #[test]
    fn test_extract_language_keys() {
        let keys = extract_language_keys("ru='Привет'; en='Hello'");
        assert!(keys.contains("ru"));
        assert!(keys.contains("en"));
        assert_eq!(keys.len(), 2);

        let keys2 = extract_language_keys("ru = 'Привет' ; en = \"Hello\"");
        assert!(keys2.contains("ru"));
        assert!(keys2.contains("en"));

        let keys3 = extract_language_keys("Просто текст без языков");
        assert!(keys3.is_empty());
    }

    #[test]
    fn test_only_ru() {
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

    СообщениеПользователю = СтрШаблон(НСтр("ru='В строке №%1 не заполнена номенклатура'"), Строка.Номер); // <-- для ru и en ошибка
    СообщениеПользователю = СтрШаблон(НСтр("en='In line №%1 nomenclature is not filled'"), Строка.Номер); // <-- для ru, ru и en ошибка

    ТекстТолькоНаРусском2 = НСтр("ru='В строке №%1 не заполнена номенклатура'");
    СообщениеПользователю = СтрШаблон(ТекстТолькоНаРусском2, Строка.Номер);

    ТекстТолькоНаАнглийском2 = НСтр("en='In line №%1 nomenclature is not filled'");
    СообщениеПользователю = СтрШаблон(ТекстТолькоНаАнглийском2, Строка.Номер);

    ТекстТолькоНаРусскомСПробелами = НСтр("ru='Привет, я простая строка''");
    ПриветствиеСПробелами = НСтр("ru = 'Привет, я простая строка'; en = 'Hi, i'm a simple string'");
    Возврат КонструкторАдресов();

    Порция = Сервис.Autocomplete(ИдентификаторАдресногоОбъекта, 0, НСтр("ru = 'ДОМ='"), 1, КодЯзыка, Метаданные.Имя);

    ВычисляемоеПоле.Оформление.УстановитьЗначениеПараметра("Формат", НСтр("ru = 'ДФ=''д ММММ'''"));

    Возврат НСтр("en=""You must specify the user's extension number to the PBX."";ru='Необходимо указать внутренний номер пользователя АТС.'");

КонецФункции

Процедура СноваБезОшибок()

    БыстрыйДоступПредставление = СтроковыеФункцииКлиентСервер.СтрокаСЧисломДляЛюбогоЯзыка(
        НСтр("ru = ';%1 пользователь;;%1 пользователя;%1 пользователей;%1 пользователя'"), КоличествоПользователей);

    Для каждого КодЯзыка Из КодыЯзыков Цикл

    	Элемент[ИмяЛокализуемогоРеквизита(ИмяРеквизита, КодЯзыка)] = НСтр(ИсходнаяСтрока, КодЯзыка);

    КонецЦикла;

КонецПроцелуры
"#;
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        expect![[r#"
            MultilingualStringHasAllDeclaredLanguages @ 13:17..13:23
              message: Добавьте строки для языков: [ru]
              severity: Error
            MultilingualStringHasAllDeclaredLanguages @ 14:31..14:87
              message: Добавьте строки для языков: [ru]
              severity: Error
            MultilingualStringHasAllDeclaredLanguages @ 17:31..17:67
              message: Добавьте строки для языков: [ru]
              severity: Error"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_ru_and_en() {
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

    СообщениеПользователю = СтрШаблон(НСтр("ru='В строке №%1 не заполнена номенклатура'"), Строка.Номер); // <-- для ru и en ошибка
    СообщениеПользователю = СтрШаблон(НСтр("en='In line №%1 nomenclature is not filled'"), Строка.Номер); // <-- для ru, ru и en ошибка

    ТекстТолькоНаРусском2 = НСтр("ru='В строке №%1 не заполнена номенклатура'");
    СообщениеПользователю = СтрШаблон(ТекстТолькоНаРусском2, Строка.Номер);

    ТекстТолькоНаАнглийском2 = НСтр("en='In line №%1 nomenclature is not filled'");
    СообщениеПользователю = СтрШаблон(ТекстТолькоНаАнглийском2, Строка.Номер);

    ТекстТолькоНаРусскомСПробелами = НСтр("ru='Привет, я простая строка''");
    ПриветствиеСПробелами = НСтр("ru = 'Привет, я простая строка'; en = 'Hi, i'm a simple string'");
    Возврат КонструкторАдресов();

    Порция = Сервис.Autocomplete(ИдентификаторАдресногоОбъекта, 0, НСтр("ru = 'ДОМ='"), 1, КодЯзыка, Метаданные.Имя);

    ВычисляемоеПоле.Оформление.УстановитьЗначениеПараметра("Формат", НСтр("ru = 'ДФ=''д ММММ'''"));

    Возврат НСтр("en=""You must specify the user's extension number to the PBX."";ru='Необходимо указать внутренний номер пользователя АТС.'");

КонецФункции

Процедура СноваБезОшибок()

    БыстрыйДоступПредставление = СтроковыеФункцииКлиентСервер.СтрокаСЧисломДляЛюбогоЯзыка(
        НСтр("ru = ';%1 пользователь;;%1 пользователя;%1 пользователей;%1 пользователя'"), КоличествоПользователей);

    Для каждого КодЯзыка Из КодыЯзыков Цикл

    	Элемент[ИмяЛокализуемогоРеквизита(ИмяРеквизита, КодЯзыка)] = НСтр(ИсходнаяСтрока, КодЯзыка);

    КонецЦикла;

КонецПроцелуры
"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MultilingualStringHasAllDeclaredLanguages,
            serde_json::json!({
                "declaredLanguages": "ru,en"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        let snapshot = format_diags(code, &diagnostics);
        expect![[r#"
            MultilingualStringHasAllDeclaredLanguages @ 13:17..13:23
              message: Добавьте строки для языков: [en, ru]
              severity: Error
            MultilingualStringHasAllDeclaredLanguages @ 14:31..14:87
              message: Добавьте строки для языков: [en, ru]
              severity: Error
            MultilingualStringHasAllDeclaredLanguages @ 16:28..16:66
              message: Добавьте строки для языков: [en]
              severity: Error
            MultilingualStringHasAllDeclaredLanguages @ 17:31..17:67
              message: Добавьте строки для языков: [ru]
              severity: Error
            MultilingualStringHasAllDeclaredLanguages @ 28:38..28:76
              message: Добавьте строки для языков: [en]
              severity: Error
            MultilingualStringHasAllDeclaredLanguages @ 32:68..32:87
              message: Добавьте строки для языков: [en]
              severity: Error
            MultilingualStringHasAllDeclaredLanguages @ 34:70..34:98
              message: Добавьте строки для языков: [en]
              severity: Error
            MultilingualStringHasAllDeclaredLanguages @ 43:9..43:90
              message: Добавьте строки для языков: [en]
              severity: Error"#]]
        .assert_eq(&snapshot);
    }

    #[test]
    fn test_no_error_when_all_languages_present() {
        let code = r#"
Процедура Тест()
    Приветствие = НСтр("ru='Привет'; en='Hello'");
КонецПроцедуры
"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MultilingualStringHasAllDeclaredLanguages,
            serde_json::json!({
                "declaredLanguages": "ru,en"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_skip_nstr_in_str_template() {
        let code = r#"
Процедура Тест()
    Сообщение = СтрШаблон(НСтр("ru='Значение: %1'"), Значение);
КонецПроцедуры
"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MultilingualStringHasAllDeclaredLanguages,
            serde_json::json!({
                "declaredLanguages": "ru,en"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }
}
