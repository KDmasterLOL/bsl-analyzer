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
    use crate::test_utils::check_diagnostics_snapshot_for;
    use expect_test::expect;

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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingModalWindows,
            expect![[r#"
            UsingModalWindows @ 3:13..4:58
              message: Вместо модального метода "Вопрос" необходимо использовать "ПоказатьВопрос"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_modal_warning() {
        let code = r#"Процедура ТестПредупреждение()
    Предупреждение(НСтр("ru = 'Выберите документ!'; en = 'Select a document!'"), 10);
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingModalWindows,
            expect![[r#"
            UsingModalWindows @ 2:5..2:85
              message: Вместо модального метода "Предупреждение" необходимо использовать "ПоказатьПредупреждение"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_modal_open_value() {
        let code = r#"Процедура ТестОткрытьЗначение()
    Товар = Справочники.Номенклатура.НайтиПоКоду(КодТовара);
    ОткрытьЗначение(Товар);
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingModalWindows,
            expect![[r#"
            UsingModalWindows @ 3:5..3:27
              message: Вместо модального метода "ОткрытьЗначение" необходимо использовать "ПоказатьЗначение"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingModalWindows,
            expect![[r#"
            UsingModalWindows @ 5:10..5:59
              message: Вместо модального метода "ВвестиДату" необходимо использовать "ПоказатьВводДаты"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingModalWindows,
            expect![[r#"
            UsingModalWindows @ 11:10..11:68
              message: Вместо модального метода "ВвестиЗначение" необходимо использовать "ПоказатьВводЗначения"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingModalWindows,
            expect![[r#"
            UsingModalWindows @ 4:10..4:51
              message: Вместо модального метода "ВвестиСтроку" необходимо использовать "ПоказатьВводСтроки"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_modal_input_number() {
        let code = r#"Процедура ТестВвестиЧисло()
    Количество = 1;
    Если ВвестиЧисло(Количество, "Введите количество", 10, 2) Тогда
        // обработка введенного количества
    КонецЕсли;
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingModalWindows,
            expect![[r#"
            UsingModalWindows @ 3:10..3:62
              message: Вместо модального метода "ВвестиЧисло" необходимо использовать "ПоказатьВводЧисла"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_modal_install_addon() {
        let code = r#"Процедура ТестУстановитьВнешнююКомпоненту()
    УстановитьВнешнююКомпоненту("ПутьККомпоненте");
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingModalWindows,
            expect![[r#"
            UsingModalWindows @ 2:5..2:51
              message: Вместо модального метода "УстановитьВнешнююКомпоненту" необходимо использовать "НачатьУстановкуВнешнейКомпоненты"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_modal_open_form_modally() {
        let code = r#"Процедура ТестОткрытьФормуМодально()
    ОткрытьФормуМодально("Форма");
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingModalWindows,
            expect![[r#"
            UsingModalWindows @ 2:5..2:34
              message: Вместо модального метода "ОткрытьФормуМодально" необходимо использовать "ОткрытьФорму"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_modal_install_file_extension() {
        let code = r#"Процедура ТестУстановитьРасширениеРаботыСФайлами()
    #Если ВебКлиент Тогда
        Результат = УстановитьРасширениеРаботыСФайлами();
    #КонецЕсли
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingModalWindows,
            expect![[r#"
            UsingModalWindows @ 3:21..3:57
              message: Вместо модального метода "УстановитьРасширениеРаботыСФайлами" необходимо использовать "НачатьУстановкуРасширенияРаботыСФайлами"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_modal_install_crypto_extension() {
        let code = r#"Процедура ТестУстановитьРасширениеРаботыСКриптографией()
    #Если ВебКлиент Тогда
        Результат = УстановитьРасширениеРаботыСКриптографией();
    #КонецЕсли
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingModalWindows,
            expect![[r#"
            UsingModalWindows @ 3:21..3:63
              message: Вместо модального метода "УстановитьРасширениеРаботыСКриптографией" необходимо использовать "НачатьУстановкуРасширенияРаботыСКриптографией"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_modal_put_file() {
        let code = r#"Процедура ТестПоместитьФайл()
    Перем АдресХранилища;
    ПоместитьФайл(АдресХранилища, ПутьКФайлу, ПутьКФайлу, Ложь, УникальныйИдентификатор);
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingModalWindows,
            expect![[r#"
            UsingModalWindows @ 3:5..3:89
              message: Вместо модального метода "ПоместитьФайл" необходимо использовать "НачатьПомещениеФайла"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsingModalWindows, expect![[r#""#]]);
    }
}
