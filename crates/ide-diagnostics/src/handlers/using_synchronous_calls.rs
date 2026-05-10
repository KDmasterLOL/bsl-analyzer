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
    use crate::test_utils::check_diagnostics_snapshot_for;
    use expect_test::expect;
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:13..4:58
              message: Вместо синхронного вызова "Вопрос" необходимо использовать "ПоказатьВопрос"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_warning() {
        let code = r#"Процедура ТестПредупреждение()
    Предупреждение(НСтр("ru = 'Выберите документ!'; en = 'Select a document!'"), 10);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 2:5..2:85
              message: Вместо синхронного вызова "Предупреждение" необходимо использовать "ПоказатьПредупреждение"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_open_value() {
        let code = r#"Процедура ТестОткрытьЗначение()

    Товар = Справочники.Номенклатура.НайтиПоКоду(КодТовара);
    ОткрытьЗначение(Товар);

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 4:5..4:27
              message: Вместо синхронного вызова "ОткрытьЗначение" необходимо использовать "ПоказатьЗначение"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 6:10..6:59
              message: Вместо синхронного вызова "ВвестиДату" необходимо использовать "ПоказатьВводДаты"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 12:10..12:68
              message: Вместо синхронного вызова "ВвестиЗначение" необходимо использовать "ПоказатьВводЗначения"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 5:10..5:51
              message: Вместо синхронного вызова "ВвестиСтроку" необходимо использовать "ПоказатьВводСтроки"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 4:10..4:62
              message: Вместо синхронного вызова "ВвестиЧисло" необходимо использовать "ПоказатьВводЧисла"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_install_addon() {
        let code = r#"Процедура ТестУстановитьВнешнююКомпоненту()

    УстановитьВнешнююКомпоненту("ПутьККомпоненте");

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:5..3:51
              message: Вместо синхронного вызова "УстановитьВнешнююКомпоненту" необходимо использовать "НачатьУстановкуВнешнейКомпоненты"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_open_form_modally() {
        let code = r#"Процедура ТестОткрытьФормуМодально()

    ОткрытьФормуМодально("Форма");

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:5..3:34
              message: Вместо синхронного вызова "ОткрытьФормуМодально" необходимо использовать "ОткрытьФорму"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_install_file_ext() {
        let code = r#"Процедура ТестУстановитьРасширениеРаботыСФайлами()

    #Если ВебКлиент Тогда
        Результат = УстановитьРасширениеРаботыСФайлами();
    #КонецЕсли

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 4:21..4:57
              message: Вместо синхронного вызова "УстановитьРасширениеРаботыСФайлами" необходимо использовать "НачатьУстановкуРасширенияРаботыСФайлами"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_install_crypto_ext() {
        let code = r#"Процедура ТестУстановитьРасширениеРаботыСКриптографией()

    #Если ВебКлиент Тогда
        Результат = УстановитьРасширениеРаботыСКриптографией();
    #КонецЕсли

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 4:21..4:63
              message: Вместо синхронного вызова "УстановитьРасширениеРаботыСКриптографией" необходимо использовать "НачатьУстановкуРасширенияРаботыСКриптографией"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:13..3:55
              message: Вместо синхронного вызова "ПодключитьРасширениеРаботыСКриптографией" необходимо использовать "НачатьПодключениеРасширенияРаботыСКриптографией"
              severity: Warning
            UsingSynchronousCalls @ 4:9..4:130
              message: Вместо синхронного вызова "Предупреждение" необходимо использовать "ПоказатьПредупреждение"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:13..3:49
              message: Вместо синхронного вызова "ПодключитьРасширениеРаботыСФайлами" необходимо использовать "НачатьПодключениеРасширенияРаботыСФайлами"
              severity: Warning
            UsingSynchronousCalls @ 4:9..4:110
              message: Вместо синхронного вызова "Предупреждение" необходимо использовать "ПоказатьПредупреждение"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_put_file() {
        let code = r#"Процедура ТестПоместитьФайл()

    Перем АдресХранилища;

    ПоместитьФайл(АдресХранилища, ПутьКФайлу, ПутьКФайлу, Ложь, УникальныйИдентификатор);

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 5:5..5:89
              message: Вместо синхронного вызова "ПоместитьФайл" необходимо использовать "НачатьПомещениеФайла"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_copy_file() {
        let code = r#"Процедура ТестКопироватьФайл()

    КопироватьФайл("C:\Temp\Order.htm", "C:\My Documents\Order.htm");

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:5..3:69
              message: Вместо синхронного вызова "КопироватьФайл" необходимо использовать "НачатьКопированиеФайла"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_move_file() {
        let code = r#"Процедура ТестПереместитьФайл()

    ПереместитьФайл("C:\Temp\Order.htm", "C:\My Documents\Order.htm");

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:5..3:70
              message: Вместо синхронного вызова "ПереместитьФайл" необходимо использовать "НачатьПеремещениеФайла"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_find_files() {
        let code = r#"Процедура ТестНайтиФайлы()

    НайденныеФайлы = НайтиФайлы("C:\Temp", "*.cdx");

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:22..3:52
              message: Вместо синхронного вызова "НайтиФайлы" необходимо использовать "НачатьПоискФайлов"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 5:9..5:38
              message: Вместо синхронного вызова "УдалитьФайлы" необходимо использовать "НачатьУдалениеФайлов"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_create_directory() {
        let code = r#"Процедура ТестСоздатьКаталог()

    СоздатьКаталог("C:\Temp");

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:5..3:30
              message: Вместо синхронного вызова "СоздатьКаталог" необходимо использовать "НачатьСозданиеКаталога"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_temp_files_dir() {
        let code = r#"Процедура ТестКаталогВременныхФайлов()

    ГдеИскать = КаталогВременныхФайлов();

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:17..3:41
              message: Вместо синхронного вызова "КаталогВременныхФайлов" необходимо использовать "НачатьПолучениеКаталогаВременныхФайлов"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_documents_dir() {
        let code = r#"Процедура ТестКаталогДокументов()

    ГдеИскать = КаталогДокументов();

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:17..3:36
              message: Вместо синхронного вызова "КаталогДокументов" необходимо использовать "НачатьПолучениеКаталогаДокументов"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_user_data_dir() {
        let code = r#"Процедура ТестРабочийКаталогДанныхПользователя()

    ГдеИскать = РабочийКаталогДанныхПользователя();

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:17..3:51
              message: Вместо синхронного вызова "РабочийКаталогДанныхПользователя" необходимо использовать "НачатьПолучениеРабочегоКаталогаДанныхПользователя"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 3:17..3:90
              message: Вместо синхронного вызова "ПолучитьФайлы" необходимо использовать "НачатьПолучениеФайлов"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_put_files() {
        let code = r#"Процедура ТестПоместитьФайлы()

    МассивВнутреннихАдресовСервера = Новый Массив;
    Результат = ПоместитьФайлы(, МассивВнутреннихАдресовСервера);

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 4:17..4:65
              message: Вместо синхронного вызова "ПоместитьФайлы" необходимо использовать "НачатьПомещениеФайлов"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 13:13..13:60
              message: Вместо синхронного вызова "ЗапроситьРазрешениеПользователя" необходимо использовать "НачатьЗапросРазрешенияПользователя"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_sync_run_application() {
        let code = r#"Процедура ТестЗапуститьПриложение()

    // открытие файла MS Excel
    ЗапуститьПриложение("Таблица.xls");

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#"
            UsingSynchronousCalls @ 4:5..4:39
              message: Вместо синхронного вызова "ЗапуститьПриложение" необходимо использовать "НачатьЗапускПриложения"
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#""#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingSynchronousCalls,
            expect![[r#""#]],
        );
    }
}
