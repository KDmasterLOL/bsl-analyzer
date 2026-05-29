use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Standard, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::VirtualTableCallWithoutParameters { range, .. } = diag {
        crate::sdbl_utils::dispatch_simple(
            ctx,
            DiagnosticCode::VirtualTableCallWithoutParameters,
            "Не следует использовать виртуальные таблицы без параметров",
            *range,
            mapper,
            query_text,
            diagnostics,
        );
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::VirtualTableCallWithoutParameters,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_detects_virtual_table_calls_without_parameters() {
        let code = r#"Процедура Тест1()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "Выбрать Т.Ссылка
    |Из РегистрСведений.Курсы.СрезПоследних КАК Т //<-- ошибка
    |";

КонецПроцедуры

Процедура Тест2()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "Выбрать Т.Измерение Из Справочник.Справочник1 КАК Спр
    |Левое соединение
    |РегистрНакопления.Склады.Остатки(Склад = &Параметр) КАК Т
    |По Спр.Поле1 = Т.Местонахождение";

КонецПроцедуры

Процедура Тест3()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "Выбрать Т.Регистратор Из Справочник.Справочник1 КАК Спр
    |Правое соединение
    |РегистрНакопления.Склады.Остатки(, Склад = &Параметр) КАК Т
    |   По Спр.Поле1 = Т.Местонахождение";

КонецПроцедуры

Процедура Тест4()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "Выбрать Т.Измерение
    |Из РегистрСведений.Курсы.СрезПоследних(&Период) как Курсы //<-- не ошибка
    |Левое соединение РегистрНакопления.Склады.Остатки(Склад = &Параметр) КАК Т
    |По Курсы.Поле1 = Т.Измерение";

КонецПроцедуры

Процедура Тест5()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "Выбрать Т.Ссылка
    |Из РегистрНакопления.Склады.Остатки() как Т //<-- ошибка
    |";

КонецПроцедуры

Процедура Тест6()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "Выбрать Т.Ссылка
    |Из РегистрНакопления.Склады.Остатки(, ) как Т //<-- ошибка
    |";

КонецПроцедуры

Процедура Тест7()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "Выбрать Т.Ссылка
    |Из РегистрНакопления.Склады.Остатки(, Склад = &Параметр) как Т
    |";

КонецПроцедуры

Процедура Тест8()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "Выбрать Т.Ссылка
    |Из РегистрНакопления.Склады.Остатки(&Период, ) как Т //<-- считаем ошибкой
    |";

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::VirtualTableCallWithoutParameters,
            expect![[r#"
                VirtualTableCallWithoutParameters @ 6:9..6:45
                  message: Не следует использовать виртуальные таблицы без параметров
                  severity: Major
                VirtualTableCallWithoutParameters @ 49:9..49:43
                  message: Не следует использовать виртуальные таблицы без параметров
                  severity: Major
                VirtualTableCallWithoutParameters @ 59:9..59:45
                  message: Не следует использовать виртуальные таблицы без параметров
                  severity: Major
                VirtualTableCallWithoutParameters @ 79:9..79:52
                  message: Не следует использовать виртуальные таблицы без параметров
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_virtual_table_with_params_ok() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки(Склад = &Параметр)";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::VirtualTableCallWithoutParameters,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_virtual_table_period_only_ok() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрСведений.Курсы.СрезПоследних(&Период)";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::VirtualTableCallWithoutParameters,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_virtual_table_empty_period_with_condition_ok() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки(, Склад = &Параметр)";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::VirtualTableCallWithoutParameters,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_virtual_table_without_parens() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрСведений.Курсы.СрезПоследних";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::VirtualTableCallWithoutParameters,
            expect![[r#"
                VirtualTableCallWithoutParameters @ 3:28..3:63
                  message: Не следует использовать виртуальные таблицы без параметров
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_virtual_table_empty_parens() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки()";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::VirtualTableCallWithoutParameters,
            expect![[r#"
                VirtualTableCallWithoutParameters @ 3:28..3:62
                  message: Не следует использовать виртуальные таблицы без параметров
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_virtual_table_empty_second_param() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки(&Период, )";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::VirtualTableCallWithoutParameters,
            expect![[r#"
                VirtualTableCallWithoutParameters @ 3:28..3:71
                  message: Не следует использовать виртуальные таблицы без параметров
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_virtual_table_both_empty() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки(, )";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::VirtualTableCallWithoutParameters,
            expect![[r#"
                VirtualTableCallWithoutParameters @ 3:28..3:64
                  message: Не следует использовать виртуальные таблицы без параметров
                  severity: Major"#]],
        );
    }
}
