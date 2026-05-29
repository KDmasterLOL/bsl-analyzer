use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
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
    if let sdbl_hir::SdblDiagnostic::JoinWithVirtualTable { range, .. } = diag {
        crate::sdbl_utils::dispatch_simple(
            ctx,
            DiagnosticCode::JoinWithVirtualTable,
            "Не следует использовать соединения с виртуальными таблицами",
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
        DiagnosticCode::JoinWithVirtualTable,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_join_with_virtual_table_single_line() {
        let code = r#"Процедура Тест1()
    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Ссылка Из Справочник.Справочник1 СПр Левое соединение РегистрСведений.Курсы.СрезПоследних КАК Т По СПр.Поле1 = Т.Валюта";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::JoinWithVirtualTable,
            expect![[r#"
                JoinWithVirtualTable @ 3:85..3:121
                  message: Не следует использовать соединения с виртуальными таблицами
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_join_with_virtual_table_multiline_left() {
        let code = r#"Процедура Тест2()
    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Измерение Из Справочник.Справочник1
    |СПр Левое соединение
    |РегистрНакопления.Склады.Остатки(Склад = &Параметр) КАК Т
    |По СПр.Поле1 = Т.Местонахождение";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::JoinWithVirtualTable,
            expect![[r#"
                JoinWithVirtualTable @ 5:6..5:57
                  message: Не следует использовать соединения с виртуальными таблицами
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_join_with_virtual_table_multiline_right() {
        let code = r#"Процедура Тест3()
    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Регистратор Из Справочник.Справочник1
    |СПр Правое соединение
    |РегистрНакопления.Склады.Остатки(Склад = &Параметр) КАК Т
    |По СПр.Поле1 = Т.Местонахождение";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::JoinWithVirtualTable,
            expect![[r#"
                JoinWithVirtualTable @ 5:6..5:57
                  message: Не следует использовать соединения с виртуальными таблицами
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_join_with_two_virtual_tables() {
        let code = r#"Процедура Тест4()
    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Измерение
    | Из РегистрСведений.Курсы.СрезПоследних(&Период) как Курсы Левое соединение
    |РегистрНакопления.Склады.Остатки(Склад = &Параметр) КАК Т
    |По Курсы.Поле1 = Т.Измерение";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::JoinWithVirtualTable,
            expect![[r#"
                JoinWithVirtualTable @ 4:10..4:54
                  message: Не следует использовать соединения с виртуальными таблицами
                  severity: Warning
                JoinWithVirtualTable @ 5:6..5:57
                  message: Не следует использовать соединения с виртуальными таблицами
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_virtual_table_in_from_no_join_no_trigger() {
        let code = r#"Процедура Тест7()
    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Ссылка
    | Из РегистрНакопления.Склады.Остатки(Склад = &Параметр) как Р,
    |(Выбрать СС.Ссылка Из Справочник.Справочник2 КАК СС Где СС.Ссылка = &Параметр) КАК Т";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::JoinWithVirtualTable,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_simple_join_with_virtual_table() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ЛЕВОЕ СОЕДИНЕНИЕ РегистрСведений.Курсы.СрезПоследних КАК Т ПО ID";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::JoinWithVirtualTable,
            expect![[r#"
                JoinWithVirtualTable @ 3:48..3:84
                  message: Не следует использовать соединения с виртуальными таблицами
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_no_false_positive_regular_table() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Справочник.Товары ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Цены ПО ID";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::JoinWithVirtualTable,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_no_false_positive_virtual_table_without_join() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки(Склад = &Параметр) КАК Р";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::JoinWithVirtualTable,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_virtual_table_in_from_with_join() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрСведений.Курсы.СрезПоследних(&Период) КАК К ЛЕВОЕ СОЕДИНЕНИЕ Т2 ПО ID";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::JoinWithVirtualTable,
            expect![[r#"
                JoinWithVirtualTable @ 3:28..3:72
                  message: Не следует использовать соединения с виртуальными таблицами
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_multiple_virtual_tables_in_joins() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ *
    |ИЗ Справочник.Товары
    |ЛЕВОЕ СОЕДИНЕНИЕ РегистрСведений.Курсы.СрезПоследних КАК К ПО ID
    |ЛЕВОЕ СОЕДИНЕНИЕ РегистрНакопления.Склады.Остатки КАК О ПО ID";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::JoinWithVirtualTable,
            expect![[r#"
                JoinWithVirtualTable @ 5:23..5:59
                  message: Не следует использовать соединения с виртуальными таблицами
                  severity: Warning
                JoinWithVirtualTable @ 6:23..6:56
                  message: Не следует использовать соединения с виртуальными таблицами
                  severity: Warning"#]],
        );
    }
}
