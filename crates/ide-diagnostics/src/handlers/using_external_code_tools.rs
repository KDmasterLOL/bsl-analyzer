use crate::define_metadata;
use crate::metadata::*;
use crate::AnalysisContext;
use crate::{Diagnostic, DiagnosticCode};
use hir::LocalRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: LocalRange, ctx: &AnalysisContext) -> Option<Diagnostic<LocalRange>> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::UsingExternalCodeTools,
        "Potentially unsafe use of external code tools",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_diagnostics_snapshot_for;
    use expect_test::expect;
    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура Тест()
    ИмяОбработки = ВнешниеОбработки.Подключить("ПутьКОбработке", ЛОЖЬ); // <-- Ошибка
    Обработка = ВнешниеОбработки.Создать(ИмяОбработки); // <-- Ошибка

    ИмяОтчета = ExternalReports.Connect("Path", true); // <-- Ошибка
    Отчет = ExternalReports.Create(ИмяОтчета); // <-- Ошибка

    Расширение = РасширенияКонфигурации.Создать("ИмяРасширения"); // <-- Ошибка
    СписокРасширений = Новый СписокЗначений;
    СписокРасширений.Добавить(РасширенияКонфигурации.Создать("ИмяРасширения2")); // <-- Ошибка
КонецПроцедуры

Процедура Тест2()
    Справочники.ВнешниеОбработки.Подключить("ПутьКОбработке", ЛОЖЬ); // <-- Не ошибка
    Обработка.ExternalReports.Connect("Path", true); // <-- не ошибка
    ExternalReports.Connect("Path", true).Create("name"); // <-- Ошибка
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingExternalCodeTools,
            expect![[r#"
            UsingExternalCodeTools @ 2:20..2:71
              message: Potentially unsafe use of external code tools
              severity: Warning
            UsingExternalCodeTools @ 3:17..3:55
              message: Potentially unsafe use of external code tools
              severity: Warning
            UsingExternalCodeTools @ 5:17..5:54
              message: Potentially unsafe use of external code tools
              severity: Warning
            UsingExternalCodeTools @ 6:13..6:46
              message: Potentially unsafe use of external code tools
              severity: Warning
            UsingExternalCodeTools @ 8:18..8:65
              message: Potentially unsafe use of external code tools
              severity: Warning
            UsingExternalCodeTools @ 10:31..10:79
              message: Potentially unsafe use of external code tools
              severity: Warning
            UsingExternalCodeTools @ 16:5..16:42
              message: Potentially unsafe use of external code tools
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_not_triggered_on_qualified_access() {
        let code = r#"
Процедура Тест()
    Справочники.ВнешниеОбработки.Подключить("ПутьКОбработке", ЛОЖЬ);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingExternalCodeTools,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_not_triggered_on_variable_access() {
        let code = r#"
Процедура Тест()
    Обработка.ExternalReports.Connect("Path", true);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingExternalCodeTools,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_russian_names() {
        let code = r#"
Процедура Тест()
    ВнешниеОбработки.Создать("Имя");
    ВнешниеОтчеты.Подключить("Путь");
    РасширенияКонфигурации.Создать("Расширение");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingExternalCodeTools,
            expect![[r#"
            UsingExternalCodeTools @ 3:5..3:36
              message: Potentially unsafe use of external code tools
              severity: Warning
            UsingExternalCodeTools @ 4:5..4:37
              message: Potentially unsafe use of external code tools
              severity: Warning
            UsingExternalCodeTools @ 5:5..5:49
              message: Potentially unsafe use of external code tools
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_english_names() {
        let code = r#"
Procedure Test()
    ExternalDataProcessors.Create("Name");
    ExternalReports.Connect("Path");
    ConfigurationExtensions.Create("Extension");
EndProcedure
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingExternalCodeTools,
            expect![[r#"
            UsingExternalCodeTools @ 3:5..3:42
              message: Potentially unsafe use of external code tools
              severity: Warning
            UsingExternalCodeTools @ 4:5..4:36
              message: Potentially unsafe use of external code tools
              severity: Warning
            UsingExternalCodeTools @ 5:5..5:48
              message: Potentially unsafe use of external code tools
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    ВНЕШНИЕОБРАБОТКИ.СОЗДАТЬ("Имя");
    externaldataprocessors.create("Name");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingExternalCodeTools,
            expect![[r#"
            UsingExternalCodeTools @ 3:5..3:36
              message: Potentially unsafe use of external code tools
              severity: Warning
            UsingExternalCodeTools @ 4:5..4:42
              message: Potentially unsafe use of external code tools
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_local_variable_exclusion() {
        let code = r#"
Процедура Тест()
    ВнешниеОбработки = Новый Структура;
    ВнешниеОбработки.Создать("Имя");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingExternalCodeTools,
            expect![[r#""#]],
        );
    }
}
