//! Reports usage of synchronous calls that should be replaced with asynchronous alternatives.

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
    let code = DiagnosticCode::UsingSynchronousCalls;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let message = format!(
        "Вместо синхронного вызова \"{}\" необходимо использовать \"{}\"",
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
    fn test_sync_question() {
        let code = r#"Процедура ТестВопрос()
    Режим = РежимДиалогаВопрос.ДаНет;
    Ответ = Вопрос(НСтр("ru = 'Продолжить выполнение операции?';"
         + " en = 'Do you want to continue?'"), Режим, 0);
    Если Ответ = КодВозвратаДиалога.Нет Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        // Вопрос (multiline call): line 2 col 12 -> line 3 col 57
        assert_diagnostic_range_multiline(code, sync_diags[0], 2, 12, 3, 57);
    }

    #[test]
    fn test_sync_warning() {
        let code = r#"Процедура ТестПредупреждение()
    Предупреждение(НСтр("ru = 'Выберите документ!'; en = 'Select a document!'"), 10);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 1, 4, 84);
    }

    #[test]
    fn test_sync_open_value() {
        let code = r#"Процедура ТестОткрытьЗначение()

    Товар = Справочники.Номенклатура.НайтиПоКоду(КодТовара);
    ОткрытьЗначение(Товар);

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 3, 4, 26);
    }

    #[test]
    fn test_sync_input_date() {
        let code = r#"Процедура ТестВвестиДату()

    ДатаНапоминания = РабочаяДата;
    Подсказка = "Введите дату и время";
    ЧастьДаты = ЧастиДаты.ДатаВремя;
    Если ВвестиДату(ДатаНапоминания, Подсказка, ЧастьДаты) Тогда
        // запомнить дату напоминания
    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 5, 9, 58);
    }

    #[test]
    fn test_sync_input_value() {
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

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 11, 9, 67);
    }

    #[test]
    fn test_sync_input_string() {
        let code = r#"Процедура ТестВвестиСтроку()

    Текст = "";
    Подсказка = "Введите текст напоминания";
    Если ВвестиСтроку(Текст, Подсказка, 0, Истина) Тогда
        // запомнить текст напоминания
    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 4, 9, 50);
    }

    #[test]
    fn test_sync_input_number() {
        let code = r#"Процедура ТестВвестиЧисло()

    Количество = 1;
    Если ВвестиЧисло(Количество, "Введите количество", 10, 2) Тогда
        // обработка введенного количества
    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 3, 9, 61);
    }

    #[test]
    fn test_sync_install_addon() {
        let code = r#"Процедура ТестУстановитьВнешнююКомпоненту()

    УстановитьВнешнююКомпоненту("ПутьККомпоненте");

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 2, 4, 50);
    }

    #[test]
    fn test_sync_open_form_modally() {
        let code = r#"Процедура ТестОткрытьФормуМодально()

    ОткрытьФормуМодально("Форма");

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 2, 4, 33);
    }

    #[test]
    fn test_sync_install_file_ext() {
        let code = r#"Процедура ТестУстановитьРасширениеРаботыСФайлами()

    #Если ВебКлиент Тогда
        Результат = УстановитьРасширениеРаботыСФайлами();
    #КонецЕсли

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 3, 20, 56);
    }

    #[test]
    fn test_sync_install_crypto_ext() {
        let code = r#"Процедура ТестУстановитьРасширениеРаботыСКриптографией()

    #Если ВебКлиент Тогда
        Результат = УстановитьРасширениеРаботыСКриптографией();
    #КонецЕсли

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 3, 20, 62);
    }

    #[test]
    fn test_sync_connect_crypto_ext() {
        // Two diagnostics: ПодключитьРасширениеРаботыСКриптографией + Предупреждение inside
        let code = r#"Процедура ТестПодключитьРасширениеРаботыСКриптографией()

    Если НЕ ПодключитьРасширениеРаботыСКриптографией() Тогда
        Предупреждение(НСтр("ru='Для выполнения команды ""Подписать"" вам нужно установить расширение работы с криптографией.'"));
        Возврат;
    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 2);
        assert_diagnostic_range(code, sync_diags[0], 2, 12, 54);
        assert_diagnostic_range(code, sync_diags[1], 3, 8, 129);
    }

    #[test]
    fn test_sync_connect_file_ext() {
        // Two diagnostics: ПодключитьРасширениеРаботыСФайлами + Предупреждение inside
        let code = r#"Процедура ТестПодключитьРасширениеРаботыСФайлами()

    Если НЕ ПодключитьРасширениеРаботыСФайлами() Тогда
        Предупреждение(НСтр("ru='Для выполнения команды вам нужно установить расширение работы с файлами.'"));
        Возврат;
    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 2);
        assert_diagnostic_range(code, sync_diags[0], 2, 12, 48);
        assert_diagnostic_range(code, sync_diags[1], 3, 8, 109);
    }

    #[test]
    fn test_sync_put_file() {
        let code = r#"Процедура ТестПоместитьФайл()

    Перем АдресХранилища;

    ПоместитьФайл(АдресХранилища, ПутьКФайлу, ПутьКФайлу, Ложь, УникальныйИдентификатор);

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 4, 4, 88);
    }

    #[test]
    fn test_sync_copy_file() {
        let code = r#"Процедура ТестКопироватьФайл()

    КопироватьФайл("C:\Temp\Order.htm", "C:\My Documents\Order.htm");

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 2, 4, 68);
    }

    #[test]
    fn test_sync_move_file() {
        let code = r#"Процедура ТестПереместитьФайл()

    ПереместитьФайл("C:\Temp\Order.htm", "C:\My Documents\Order.htm");

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 2, 4, 69);
    }

    #[test]
    fn test_sync_find_files() {
        let code = r#"Процедура ТестНайтиФайлы()

    НайденныеФайлы = НайтиФайлы("C:\Temp", "*.cdx");

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 2, 21, 51);
    }

    #[test]
    fn test_sync_delete_files() {
        let code = r#"Процедура ТестУдалитьФайлы()

    // Удаление каталога и всех вложенных в него каталогов и файлов
    Попытка
        УдалитьФайлы("C:\temp\Works");
    Исключение
        Сообщить(ОписаниеОшибки());
    КонецПопытки;

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 4, 8, 37);
    }

    #[test]
    fn test_sync_create_directory() {
        let code = r#"Процедура ТестСоздатьКаталог()

    СоздатьКаталог("C:\Temp");

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 2, 4, 29);
    }

    #[test]
    fn test_sync_temp_files_dir() {
        let code = r#"Процедура ТестКаталогВременныхФайлов()

    ГдеИскать = КаталогВременныхФайлов();

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 2, 16, 40);
    }

    #[test]
    fn test_sync_documents_dir() {
        let code = r#"Процедура ТестКаталогДокументов()

    ГдеИскать = КаталогДокументов();

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 2, 16, 35);
    }

    #[test]
    fn test_sync_user_data_dir() {
        let code = r#"Процедура ТестРабочийКаталогДанныхПользователя()

    ГдеИскать = РабочийКаталогДанныхПользователя();

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 2, 16, 50);
    }

    #[test]
    fn test_sync_get_files() {
        let code = r#"Процедура ТестПолучитьФайлы()

    Результат = ПолучитьФайлы(МассивФайлов, ПолученныеФайлы, ПутьВыгружаемыхФайлов, Ложь);
    Если НЕ Результат Тогда
        Сообщение = Новый СообщениеПользователю;
        Сообщение.Текст = "Ошибка получения файлов!";
        Сообщение.Сообщить();
    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 2, 16, 89);
    }

    #[test]
    fn test_sync_put_files() {
        let code = r#"Процедура ТестПоместитьФайлы()

    МассивВнутреннихАдресовСервера = Новый Массив;
    Результат = ПоместитьФайлы(, МассивВнутреннихАдресовСервера);

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 3, 16, 64);
    }

    #[test]
    fn test_sync_request_user_permission() {
        let code = r#"Процедура ТестЗапроситьРазрешениеПользователя()

    ОписаниеВызова = Новый Массив;
    ОписаниеВызова.Добавить("ПоместитьФайлы");
    ПомещаемыеФайлы = Новый Массив;
    Описание = Новый ОписаниеПередаваемогоФайла(СтрокаСписка, "");
    ПомещаемыеФайлы.Добавить(Описание);
    ОписаниеВызова.Добавить(ПомещаемыеФайлы);
    ОписаниеВызова.Добавить(Неопределено); // не используется
    ОписаниеВызова.Добавить(Неопределено); // не используется
    ОписаниеВызова.Добавить(Ложь);            // Интерактивно = Ложь
    МассивОпераций.Добавить(ОписаниеВызова);
    Если НЕ ЗапроситьРазрешениеПользователя(МассивОпераций) Тогда
        // пользователь не дал разрешения
        Возврат;
    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 12, 12, 59);
    }

    #[test]
    fn test_sync_run_application() {
        let code = r#"Процедура ТестЗапуститьПриложение()

    // открытие файла MS Excel
    ЗапуститьПриложение("Таблица.xls");

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 1);
        assert_diagnostic_range(code, sync_diags[0], 3, 4, 38);
    }

    #[test]
    fn test_synchronous_calls_in_server_context() {
        let code = r#"
&НаСервере
Процедура СерверныйМетод()
    ЗапуститьПриложение("app.exe");
КонецПроцедуры

&НаСервереБезКонтекста
Процедура БезКонтекстаМетод()
    КопироватьФайл("source", "dest");
КонецПроцедуры

&AtServer
Procedure AtServerMethod()
    RunApp("app.exe");
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 0, "Server methods should not trigger UsingSynchronousCalls");
    }

    #[test]
    fn test_no_synchronous_calls() {
        let code = r#"
Процедура Тест()
    // Async methods should not trigger diagnostic
    ПоказатьВопрос(Оповещение, "Текст?", РежимДиалогаВопрос.ДаНет);
    ПоказатьПредупреждение(, "Текст");
    НачатьКопированиеФайла(Оповещение, "source", "dest");
    НачатьЗапускПриложения(Оповещение, "app.exe");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 0);
    }
}
