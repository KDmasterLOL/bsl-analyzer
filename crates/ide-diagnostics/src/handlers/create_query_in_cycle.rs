use crate::define_metadata;
use crate::metadata::*;
use crate::AnalysisContext;
use crate::{Diagnostic, DiagnosticCode};
use hir::LocalRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 20,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: LocalRange, ctx: &AnalysisContext) -> Option<Diagnostic<LocalRange>> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::CreateQueryInCycle,
        "Выполнение запроса в цикле приводит к деградации производительности. \
         Создайте запрос один раз до цикла и изменяйте только параметры внутри цикла",
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
    fn test_query_in_for_loop() {
        let code = r#"
Процедура Тест()
Запрос = Новый Запрос();
Для Каждого ИД Из МассивИД Цикл
    Запрос.Выполнить();
КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CreateQueryInCycle,
            expect![[r#"
            CreateQueryInCycle @ 5:5..5:23
              message: Выполнение запроса в цикле приводит к деградации производительности. Создайте запрос один раз до цикла и изменяйте только параметры внутри цикла
              severity: Critical"#]],
        );
    }

    #[test]
    fn test_query_created_outside_loop_but_executed_inside_loop() {
        let code = r#"
Процедура Тест(МассивИД)
    Запрос = Новый Запрос;

    Для Каждого ИД Из МассивИД Цикл
        Запрос.УстановитьПараметр("Код", ИД);
        Результат = Запрос.Выполнить();
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CreateQueryInCycle,
            expect![[r#"
            CreateQueryInCycle @ 7:21..7:39
              message: Выполнение запроса в цикле приводит к деградации производительности. Создайте запрос один раз до цикла и изменяйте только параметры внутри цикла
              severity: Critical"#]],
        );
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    For Each Item In Collection Do
        Query = New Query;
        Query.Execute();
    EndDo;
EndProcedure
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CreateQueryInCycle,
            expect![[r#"
            CreateQueryInCycle @ 5:9..5:24
              message: Выполнение запроса в цикле приводит к деградации производительности. Создайте запрос один раз до цикла и изменяйте только параметры внутри цикла
              severity: Critical"#]],
        );
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Для инт = 1 По 10 Цикл
        Запрос = Новый ЗАПРОС;
        Запрос.ВЫПОЛНИТЬ();
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CreateQueryInCycle,
            expect![[r#"
            CreateQueryInCycle @ 5:9..5:27
              message: Выполнение запроса в цикле приводит к деградации производительности. Создайте запрос один раз до цикла и изменяйте только параметры внутри цикла
              severity: Critical"#]],
        );
    }

    #[test]
    fn test_query_builder() {
        let code = r#"
Процедура Тест()
ПЗ = Новый ПостроительЗапроса;
Для инт = 1 По 10 Цикл
    ПЗ.Выполнить();
КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CreateQueryInCycle,
            expect![[r#"
            CreateQueryInCycle @ 5:5..5:19
              message: Выполнение запроса в цикле приводит к деградации производительности. Создайте запрос один раз до цикла и изменяйте только параметры внутри цикла
              severity: Critical"#]],
        );
    }
}
