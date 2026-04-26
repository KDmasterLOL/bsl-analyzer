//! Reports virtual table calls that omit required filtering parameters.

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

/// Single-pass dispatch for VirtualTableCallWithoutParameters.
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

/// Runs the VirtualTableCallWithoutParameters diagnostic (standalone, used in tests).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::VirtualTableCallWithoutParameters,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range_multiline, check_sdbl_diagnostic};
    use crate::DiagnosticCode;
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
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 4, "Expected 4 virtual table errors");

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::VirtualTableCallWithoutParameters);
        }

        let mut sorted = diagnostics.clone();
        sorted.sort_by_key(|d| d.range.start());

        // Тест1 строка 5: СрезПоследних без скобок
        assert_diagnostic_range_multiline(code, &sorted[0], 5, 8, 5, 44);
        // Тест5 строка 48: Остатки() пустые скобки
        assert_diagnostic_range_multiline(code, &sorted[1], 48, 8, 48, 42);
        // Тест6 строка 58: Остатки(, ) оба параметра пусты
        assert_diagnostic_range_multiline(code, &sorted[2], 58, 8, 58, 44);
        // Тест8 строка 78: Остатки(&Период, ) второй параметр пуст
        assert_diagnostic_range_multiline(code, &sorted[3], 78, 8, 78, 51);
    }

    #[test]
    fn test_virtual_table_with_params_ok() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки(Склад = &Параметр)";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Virtual table with params should be OK");
    }

    #[test]
    fn test_virtual_table_period_only_ok() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрСведений.Курсы.СрезПоследних(&Период)";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Period-only param should be OK for СрезПоследних");
    }

    #[test]
    fn test_virtual_table_empty_period_with_condition_ok() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки(, Склад = &Параметр)";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Empty period with condition should be OK");
    }

    #[test]
    fn test_virtual_table_without_parens() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрСведений.Курсы.СрезПоследних";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Virtual table without parens should trigger error");
    }

    #[test]
    fn test_virtual_table_empty_parens() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки()";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Empty parens should trigger error");
    }

    #[test]
    fn test_virtual_table_empty_second_param() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки(&Период, )";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Empty second param should trigger error");
    }

    #[test]
    fn test_virtual_table_both_empty() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки(, )";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Both empty params should trigger error");
    }
}
