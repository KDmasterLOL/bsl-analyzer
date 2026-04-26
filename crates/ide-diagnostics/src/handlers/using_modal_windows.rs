//! Reports usage of modal window methods.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_3,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    method_name: &str,
    replacement: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::UsingModalWindows;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let message = format!(
        "Вместо модального метода \"{}\" необходимо использовать \"{}\"",
        method_name, replacement
    );

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_modal_question() {
        let code = r#"Процедура ТестВопрос()
    Режим = РежимДиалогаВопрос.ДаНет;
    Ответ = Вопрос(НСтр("ru = 'Продолжить выполнение операции?';"
         + " en = 'Do you want to continue?'"), Режим, 0);
    Если Ответ = КодВозвратаДиалога.Нет Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range_multiline(code, diags[0], 2, 12, 3, 57);
    }

    #[test]
    fn test_modal_warning() {
        let code = r#"Процедура ТестПредупреждение()
    Предупреждение(НСтр("ru = 'Выберите документ!'; en = 'Select a document!'"), 10);
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 1, 4, 84);
    }

    #[test]
    fn test_modal_open_value() {
        let code = r#"Процедура ТестОткрытьЗначение()
    Товар = Справочники.Номенклатура.НайтиПоКоду(КодТовара);
    ОткрытьЗначение(Товар);
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 2, 4, 26);
    }

    #[test]
    fn test_modal_input_date() {
        let code = r#"Процедура ТестВвестиДату()
    ДатаНапоминания = РабочаяДата;
    Подсказка = "Введите дату и время";
    ЧастьДаты = ЧастиДаты.ДатаВремя;
    Если ВвестиДату(ДатаНапоминания, Подсказка, ЧастьДаты) Тогда
        // запомнить дату напоминания
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 4, 9, 58);
    }

    #[test]
    fn test_modal_input_value() {
        let code = r#"Процедура ТестВвестиЗначение()
    Перем ВыбЗнач;
    Массив = Новый Массив;
    Массив.Добавить(Тип("Число"));
    Массив.Добавить(Тип("Строка"));
    Массив.Добавить(Тип("Дата"));
    КЧ = Новый КвалификаторыЧисла(12,2);
    КС = Новый КвалификаторыСтроки(20);
    КД = Новый КвалификаторыДаты(ЧастиДаты.Дата);
    ОписаниеТипов = Новый ОписаниеТипов(Массив, КЧ, КС, КД);
    Если ВвестиЗначение(ВыбЗнач, "Введите значение", ОписаниеТипов) Тогда
        Сообщить("Введенное значение: "+ВыбЗнач);
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 10, 9, 67);
    }

    #[test]
    fn test_modal_input_string() {
        let code = r#"Процедура ТестВвестиСтроку()
    Текст = "";
    Подсказка = "Введите текст напоминания";
    Если ВвестиСтроку(Текст, Подсказка, 0, Истина) Тогда
        // запомнить текст напоминания
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 3, 9, 50);
    }

    #[test]
    fn test_modal_input_number() {
        let code = r#"Процедура ТестВвестиЧисло()
    Количество = 1;
    Если ВвестиЧисло(Количество, "Введите количество", 10, 2) Тогда
        // обработка введенного количества
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 2, 9, 61);
    }

    #[test]
    fn test_modal_install_addon() {
        let code = r#"Процедура ТестУстановитьВнешнююКомпоненту()
    УстановитьВнешнююКомпоненту("ПутьККомпоненте");
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 1, 4, 50);
    }

    #[test]
    fn test_modal_open_form_modally() {
        let code = r#"Процедура ТестОткрытьФормуМодально()
    ОткрытьФормуМодально("Форма");
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 1, 4, 33);
    }

    #[test]
    fn test_modal_install_file_extension() {
        let code = r#"Процедура ТестУстановитьРасширениеРаботыСФайлами()
    #Если ВебКлиент Тогда
        Результат = УстановитьРасширениеРаботыСФайлами();
    #КонецЕсли
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 2, 20, 56);
    }

    #[test]
    fn test_modal_install_crypto_extension() {
        let code = r#"Процедура ТестУстановитьРасширениеРаботыСКриптографией()
    #Если ВебКлиент Тогда
        Результат = УстановитьРасширениеРаботыСКриптографией();
    #КонецЕсли
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 2, 20, 62);
    }

    #[test]
    fn test_modal_put_file() {
        let code = r#"Процедура ТестПоместитьФайл()
    Перем АдресХранилища;
    ПоместитьФайл(АдресХранилища, ПутьКФайлу, ПутьКФайлу, Ложь, УникальныйИдентификатор);
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 2, 4, 88);
    }

    #[test]
    fn test_no_modal_windows() {
        let code = r#"
Процедура Тест()
    // Non-modal methods should not trigger diagnostic
    ПоказатьВопрос(Оповещение, "Текст?", РежимДиалогаВопрос.ДаНет);
    ПоказатьПредупреждение(, "Текст");
    ПоказатьЗначение(, Значение);
    ПоказатьВводДаты(Оповещение, Дата, "Подсказка");
    ОткрытьФорму("Форма");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let modal_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(modal_diags.len(), 0);
    }
}
