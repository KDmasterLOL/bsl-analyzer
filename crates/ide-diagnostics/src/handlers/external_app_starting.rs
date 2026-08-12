use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::ExternalAppStarting,
        "External application launch detected",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_global_methods_detected() {
        let code = r#"
Процедура Метод()
    СтрокаКоманды = "";
    ТекущийКаталог = "";
    ДождатьсяЗавершения = Истина;
    ОписаниеОповещения = Неопределено;

    КомандаСистемы(СтрокаКоманды, ТекущийКаталог);
    ЗапуститьПриложение(СтрокаКоманды, ТекущийКаталог);
    ЗапуститьПриложение(СтрокаКоманды, ТекущийКаталог, Истина);
    НачатьЗапускПриложения(ОписаниеОповещения, СтрокаКоманды, ТекущийКаталог, ДождатьсяЗавершения);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExternalAppStarting,
            expect![[r#"
                ExternalAppStarting @ 8:5..8:19
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 9:5..9:24
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 10:5..10:24
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 11:5..11:27
                  message: External application launch detected
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_run_program_methods_detected() {
        let code = r#"
Процедура Метод()
    СтрокаКоманды = "";
    ПараметрыКоманды = Новый Структура;

    ФайловаяСистемаКлиент.ЗапуститьПрограмму("ping 127.0.0.1 -n 5", ПараметрыКоманды);
    ФайловаяСистемаКлиент.ЗапуститьПрограмму(СтрокаКоманды, ПараметрыКоманды);
    ФайловаяСистема.ЗапуститьПрограмму(СтрокаКоманды);
    ФайловаяСистема.ЗапуститьПрограмму(СтрокаКоманды, ПараметрыКоманды);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExternalAppStarting,
            expect![[r#"
                ExternalAppStarting @ 6:27..6:45
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 7:27..7:45
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 8:21..8:39
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 9:21..9:39
                  message: External application launch detected
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_open_explorer_and_file_detected() {
        let code = r#"
Процедура Метод()
    СтрокаКоманды = "";
    ОписаниеОповещения = Неопределено;

    ФайловаяСистемаКлиент.ОткрытьПроводник("C:\Users");
    ФайловаяСистемаКлиент.ОткрытьФайл(СтрокаКоманды);
    ФайловаяСистемаКлиент.ОткрытьФайл(СтрокаКоманды, ОписаниеОповещения);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExternalAppStarting,
            expect![[r#"
                ExternalAppStarting @ 6:27..6:43
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 7:27..7:38
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 8:27..8:38
                  message: External application launch detected
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_run_app_async_detected() {
        let code = r#"
&НаКлиенте
Асинх Процедура Подключить()
    СтрокаКоманды = "";
    ТекущийКаталог = "";
    ДождатьсяЗавершения = Истина;

    Ждать ЗапуститьПриложениеАсинх(СтрокаКоманды, ТекущийКаталог, ДождатьсяЗавершения);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExternalAppStarting,
            expect![[r#"
                ExternalAppStarting @ 8:11..8:35
                  message: External application launch detected
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_zapustit_sistemu_variants_detected() {
        let code = r#"
&НаКлиенте
Процедура ПроверкаЗапуститьСистему()
    ДополнительныеПараметрыКоманднойСтроки = "";
    ДождатьсяЗавершения = Истина;
    КодВозврата = Неопределено;

    ЗапуститьСистему();
    ЗапуститьСистему(ДополнительныеПараметрыКоманднойСтроки);
    ЗапуститьСистему(ДополнительныеПараметрыКоманднойСтроки, ДождатьсяЗавершения);
    ЗапуститьСистему(ДополнительныеПараметрыКоманднойСтроки, ДождатьсяЗавершения, КодВозврата);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExternalAppStarting,
            expect![[r#"
                ExternalAppStarting @ 8:5..8:21
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 9:5..9:21
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 10:5..10:21
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 11:5..11:21
                  message: External application launch detected
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_global_call() {
        let code = r#"
Процедура Тест()
    КомандаСистемы("cmd.exe");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExternalAppStarting,
            expect![[r#"
                ExternalAppStarting @ 3:5..3:19
                  message: External application launch detected
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_object_method_call() {
        let code = r#"
Процедура Тест()
    ФайловаяСистемаКлиент.ЗапуститьПрограмму("calc.exe");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExternalAppStarting,
            expect![[r#"
                ExternalAppStarting @ 3:27..3:45
                  message: External application launch detected
                  severity: Warning"#]],
        );
    }

    /// `ОткрытьФайл` принадлежит общим модулям БСП, а не платформе; у девяти
    /// платформенных сериализаторов метод с тем же именем открывает поток на
    /// запись и никакого приложения не запускает.
    #[test]
    fn test_serializer_open_file_not_detected() {
        let code = r#"
Процедура Тест()
    ЗаписьXML = Новый ЗаписьXML;
    ЗаписьXML.ОткрытьФайл(Путь, "UTF-8");

    ЧтениеXML = Новый ЧтениеXML;
    ЧтениеXML.ОткрытьФайл(Путь);

    ЗаписьHTML = Новый ЗаписьHTML;
    ЗаписьHTML.ОткрытьФайл(Путь);

    ЧтениеHTML = Новый ЧтениеHTML;
    ЧтениеHTML.ОткрытьФайл(Путь);

    ЗаписьJSON = Новый ЗаписьJSON;
    ЗаписьJSON.ОткрытьФайл(Путь);

    ЧтениеJSON = Новый ЧтениеJSON;
    ЧтениеJSON.ОткрытьФайл(Путь);

    ЗаписьFastInfoset = Новый ЗаписьFastInfoset;
    ЗаписьFastInfoset.ОткрытьФайл(Путь);

    ЧтениеFastInfoset = Новый ЧтениеFastInfoset;
    ЧтениеFastInfoset.ОткрытьФайл(Путь);

    БазаDBF = Новый xBase;
    БазаDBF.ОткрытьФайл(Путь);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::ExternalAppStarting, expect![[r#""#]]);
    }

    #[test]
    fn test_foreign_receiver_not_detected() {
        let code = r#"
Процедура Тест()
    МойМодуль.ОткрытьФайл(Путь);
    Обработчик.ЗапуститьПрограмму(Команда);
    ПравилаОбмена.ОткрытьФайл(Путь);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::ExternalAppStarting, expect![[r#""#]]);
    }

    /// Устаревший модуль БСП, но вызов настоящий: файл открывается
    /// ассоциированным приложением.
    #[test]
    fn test_legacy_file_module_detected() {
        let code = r#"
Процедура Тест()
    РаботаСФайламиКлиент.ОткрытьФайл(Путь);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExternalAppStarting,
            expect![[r#"
                ExternalAppStarting @ 3:26..3:37
                  message: External application launch detected
                  severity: Warning"#]],
        );
    }

    /// Голое имя метода общего модуля не принадлежит платформе: без получателя
    /// это вызов чего-то своего.
    #[test]
    fn test_bare_module_method_not_detected() {
        let code = r#"
Процедура Тест()
    ОткрытьФайл(Путь);
    ЗапуститьПрограмму(Команда);
    ОткрытьПроводник(Путь);
КонецПроцедуры

Процедура ОткрытьФайл(Путь)
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::ExternalAppStarting, expect![[r#""#]]);
    }

    #[test]
    fn test_similar_name_ignored() {
        let code = r#"
Процедура Тест()
    МойМодуль.ЗапуститьВнешнееПриложение("cmd");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::ExternalAppStarting, expect![[r#""#]]);
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    System("cmd.exe");
    RunApp("calc.exe");
    RunSystem();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExternalAppStarting,
            expect![[r#"
                ExternalAppStarting @ 3:5..3:11
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 4:5..4:11
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 5:5..5:14
                  message: External application launch detected
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    КОМАНДАСИСТЕМЫ("cmd");
    ЗАПУСТИТЬПриложение("app");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ExternalAppStarting,
            expect![[r#"
                ExternalAppStarting @ 3:5..3:19
                  message: External application launch detected
                  severity: Warning
                ExternalAppStarting @ 4:5..4:24
                  message: External application launch detected
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_no_args_not_detected() {
        let code = r#"
Процедура Тест()
    Переменная = КомандаСистемы;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::ExternalAppStarting, expect![[r#""#]]);
    }
}
