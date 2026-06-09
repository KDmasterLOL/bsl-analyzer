use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Suspicious, MetadataTag::Unpredictable],
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
    if let sdbl_hir::SdblDiagnostic::FieldsFromJoinWithoutNullCheck {
        join_type,
        unprotected_fields,
        ..
    } = diag
    {
        let code = DiagnosticCode::FieldsFromJoinsWithoutIsNull;
        let join_type_str = match join_type {
            sdbl_hir::JoinType::Left => "ЛЕВОГО СОЕДИНЕНИЯ",
            sdbl_hir::JoinType::Right => "ПРАВОГО СОЕДИНЕНИЯ",
            sdbl_hir::JoinType::Full => "ПОЛНОГО СОЕДИНЕНИЯ",
            _ => "СОЕДИНЕНИЯ",
        };
        let message = format!(
            "Для полей из {} добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ",
            join_type_str
        );
        for field_ref in unprotected_fields {
            let bsl_range = mapper.map_range(field_ref.range, query_text);
            diagnostics.push(Diagnostic {
                code,
                message: message.clone(),
                severity: ctx.severity(code),
                range: bsl_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::FieldsFromJoinsWithoutIsNull,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_left_join_unprotected_field() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Сотрудники.Ссылка
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 4:14..5:6
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_left_join_with_isnull_protected() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Сотрудники3.Ссылка,
    |ЕСТЬNULL(Сотрудники3.Ссылка, 0)
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники3
    |ПО Склады.Кладовщик = Сотрудники3.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 4:14..4:32
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_left_join_where_clause_unprotected() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Склады.Ссылка
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники4
    |ПО Склады.Кладовщик = Сотрудники4.Ссылка
    |ГДЕ Сотрудники4.Флаг
    |И ЕСТЬNULL(Сотрудники4.Флаг, Истина)
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 8:10..9:6
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_right_join_unprotected_field() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Склады5.Ссылка,
    |ЕСТЬNULL(Склады5.Ссылка, 0)
    |ИЗ Справочник.Склады КАК Склады5
    |ПРАВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники5
    |ПО Склады5.Кладовщик = Сотрудники5.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 4:14..4:28
                  message: Для полей из ПРАВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_inner_join_no_diagnostic() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Склады6.Ссылка
    |ИЗ Справочник.Склады КАК Склады6
    |ВНУТРЕННЕЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники6
    |ПО Склады6.Кладовщик = Сотрудники6.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_full_join_multiple_unprotected_fields() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Сотрудники8.Ссылка,
    |Склады8.Ссылка,
    |Сотрудники8.Организация,
    |ЕСТЬNULL(Сотрудники8.Ссылка, 0),
    |ЕСТЬNULL(Склады8.Ссылка, 0)
    |ИЗ Справочник.Склады КАК Склады8
    |ПОЛНОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники8
    |ПО Склады8.Кладовщик = Сотрудники8.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 4:14..4:32
                  message: Для полей из ПОЛНОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical
                FieldsFromJoinsWithoutIsNull @ 5:6..5:20
                  message: Для полей из ПОЛНОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical
                FieldsFromJoinsWithoutIsNull @ 6:6..6:29
                  message: Для полей из ПОЛНОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_left_join_is_not_null_in_where_exempts() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Сотрудники9.Ссылка
    |ИЗ Справочник.Склады КАК Склады9
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники9
    |ПО Склады9.Кладовщик = Сотрудники9.Ссылка
    |ГДЕ (Сотрудники9.Реквизит ЕСТЬ НЕ NULL)
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_is_null_in_where_does_not_exempt_select() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Сотрудники13.Ссылка
    |ИЗ Справочник.Склады КАК Склады13
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники13
    |ПО Склады13.Кладовщик = Сотрудники13.Реквизит
    |ГДЕ Сотрудники13.Реквизит ЕСТЬ NULL
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 4:14..5:6
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_no_diagnostic_for_fields_in_join_conditions() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ РАЗЛИЧНЫЕ
    |    ПартнерыКИ.Ссылка КАК Партнер,
    |    ЕСТЬNULL(КИПочта.АдресЭП, """") КАК email,
    |    ВЫБОР
    |        КОГДА КартыЛояльности.Ссылка ЕСТЬ NULL
    |            ТОГДА ЛОЖЬ
    |        ИНАЧЕ ИСТИНА
    |    КОНЕЦ КАК ЕстьДействующаяКарта
    |ПОМЕСТИТЬ ИнформацияКлиент
    |ИЗ
    |    Справочник.Партнеры.КонтактнаяИнформация КАК ПартнерыКИ
    |        ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Партнеры.КонтактнаяИнформация КАК КИПочта
    |        ПО ПартнерыКИ.Ссылка = КИПочта.Ссылка
    |        ЛЕВОЕ СОЕДИНЕНИЕ РегистрСведений.СостояниеАдресовЭлектроннойПочты КАК СостояниеАдресов
    |        ПО (КИПочта.Ссылка = СостояниеАдресов.Партнер)
    |            И (КИПочта.АдресЭП = СостояниеАдресов.АдресЭП)
    |        ЛЕВОЕ СОЕДИНЕНИЕ Справочник.КартыЛояльности КАК КартыЛояльности
    |        ПО ПартнерыКИ.Ссылка = КартыЛояльности.Партнер";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_no_diagnostic_for_inner_joined_table_with_nested_left_join() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |    ЧекККМ.Ссылка КАК Документ,
    |    ЕСТЬNULL(ДокЗаказ.НомерДокумента, """") КАК НомерЗаказа
    |ИЗ
    |    Документ.ЧекККМ.Товары КАК ЧекККМТовары
    |        ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ЧекККМ КАК ЧекККМ
    |            ЛЕВОЕ СОЕДИНЕНИЕ Документ.ЗаказКлиента КАК ДокЗаказ
    |            ПО ЧекККМ.ЗаказКлиента = ДокЗаказ.Ссылка
    |        ПО ЧекККМТовары.Ссылка = ЧекККМ.Ссылка";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_diagnostic_highlights_field_not_join() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос("ВЫБРАТЬ
        |    ЗадачиИсполнителей.Исполнитель,
        |    ИсполнениеРезультатыПроверки.Комментарий КАК Комментарий
        |ИЗ
        |    БизнесПроцесс.Исполнение.РезультатыПроверки КАК ИсполнениеРезультатыПроверки
        |        ЛЕВОЕ СОЕДИНЕНИЕ Задача.ЗадачаИсполнителя КАК ЗадачиИсполнителей
        |        ПО ИсполнениеРезультатыПроверки.ЗадачаИсполнителя = ЗадачиИсполнителей.Ссылка
        |ГДЕ
        |    ИсполнениеРезультатыПроверки.ЗадачаПроверяющего = &ЗадачаПроверяющего");
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 3:14..3:44
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn case_else_guarded_by_when_is_null_silent() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР
    |    КОГДА Сотрудники.Ссылка ЕСТЬ NULL
    |        ТОГДА 0
    |    ИНАЧЕ Сотрудники.Ссылка
    |КОНЕЦ КАК Поле
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn case_then_guarded_by_is_not_null_silent() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР
    |    КОГДА Сотрудники.Ссылка ЕСТЬ НЕ NULL
    |        ТОГДА Сотрудники.Ссылка
    |    ИНАЧЕ 0
    |КОНЕЦ КАК Поле
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn case_then_guarded_by_isnull_comparison_silent() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР
    |    КОГДА ЕСТЬNULL(Сотрудники.Флаг, ЛОЖЬ) <> ЛОЖЬ
    |        ТОГДА Сотрудники.Ссылка
    |    ИНАЧЕ 0
    |КОНЕЦ КАК Поле
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn case_following_branches_guarded_by_first_is_null_silent() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР
    |    КОГДА Сотрудники.Ссылка ЕСТЬ NULL
    |        ТОГДА 0
    |    КОГДА Сотрудники.Флаг
    |        ТОГДА Сотрудники.Ссылка
    |    ИНАЧЕ Сотрудники.Организация
    |КОНЕЦ КАК Поле
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn where_disjunction_with_is_null_guards_other_operand() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Склады.Ссылка
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |ГДЕ (Сотрудники.Ссылка ЕСТЬ NULL ИЛИ Сотрудники.Наименование <> Склады.Наименование)
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn case_else_after_is_null_conjunction_still_fires() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР
    |    КОГДА Сотрудники.Ссылка ЕСТЬ NULL И Склады.Флаг
    |        ТОГДА 0
    |    ИНАЧЕ Сотрудники.Ссылка
    |КОНЕЦ КАК Поле
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 7:16..8:6
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn case_guard_for_other_table_still_fires() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР
    |    КОГДА Склады.Ссылка ЕСТЬ NULL
    |        ТОГДА 0
    |    ИНАЧЕ Сотрудники.Ссылка
    |КОНЕЦ КАК Поле
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 7:16..8:6
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn case_then_after_is_not_null_disjunction_still_fires() {
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР
    |    КОГДА Сотрудники.Ссылка ЕСТЬ НЕ NULL ИЛИ Склады.Флаг
    |        ТОГДА Сотрудники.Ссылка
    |    ИНАЧЕ 0
    |КОНЕЦ КАК Поле
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 6:20..7:10
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn case_then_after_isnull_equality_still_fires() {
        // ЕСТЬNULL(Поле, ЛОЖЬ) = ЛОЖЬ selects ТОГДА exactly when the row is
        // absent (the fallback satisfies the equality), so the raw field in
        // ТОГДА is a genuine NULL hazard.
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР
    |    КОГДА ЕСТЬNULL(Сотрудники.Флаг, ЛОЖЬ) = ЛОЖЬ
    |        ТОГДА Сотрудники.Ссылка
    |    ИНАЧЕ 0
    |КОНЕЦ КАК Поле
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 6:20..7:10
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn case_then_after_isnull_inequality_with_other_literal_still_fires() {
        // ЕСТЬNULL(Поле, ЛОЖЬ) <> ИСТИНА is TRUE on an absent row (fallback
        // ЛОЖЬ differs from ИСТИНА), so ТОГДА executes with NULL fields.
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР
    |    КОГДА ЕСТЬNULL(Сотрудники.Флаг, ЛОЖЬ) <> ИСТИНА
    |        ТОГДА Сотрудники.Ссылка
    |    ИНАЧЕ 0
    |КОНЕЦ КАК Поле
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 6:20..7:10
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn case_else_after_wrapped_is_null_still_fires() {
        // ЕСТЬNULL(…) never yields NULL, so `ЕСТЬNULL(…) ЕСТЬ NULL` is
        // constant-false: ИНАЧЕ always executes, including on absent rows.
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР
    |    КОГДА ЕСТЬNULL(Сотрудники.Код, 0) ЕСТЬ NULL
    |        ТОГДА 0
    |    ИНАЧЕ Сотрудники.Ссылка
    |КОНЕЦ КАК Поле
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 7:16..8:6
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn case_then_after_wrapped_is_not_null_still_fires() {
        // ЕСТЬNULL(…) never yields NULL, so `ЕСТЬNULL(…) ЕСТЬ НЕ NULL` is
        // constant-true: ТОГДА executes on absent rows too.
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР
    |    КОГДА ЕСТЬNULL(Сотрудники.Код, 0) ЕСТЬ НЕ NULL
    |        ТОГДА Сотрудники.Ссылка
    |    ИНАЧЕ 0
    |КОНЕЦ КАК Поле
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 6:20..7:10
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn track3_full_outer_join_classification_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
        "SELECT Employees.Ref AS EmployeeRef,
        |       Warehouses.Ref AS WarehouseRef
        |FROM Catalog.Warehouses AS Warehouses
        |FULL OUTER JOIN Catalog.Employees AS Employees
        |ON Warehouses.Manager = Employees.Ref";
КонецПроцедуры"#,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 4:17..4:31
                  message: Для полей из ПОЛНОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical
                FieldsFromJoinsWithoutIsNull @ 5:17..5:32
                  message: Для полей из ПОЛНОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn track3_left_outer_join_isnull_wrapped_field_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
        "ВЫБРАТЬ
        |   ЕСТЬNULL(Сотрудники.Ссылка, ЗНАЧЕНИЕ(Справочник.Сотрудники.ПустаяСсылка)) КАК Сотрудник
        |ИЗ
        |   Справочник.Склады КАК Склады
        |   ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
        |   ПО Склады.Кладовщик = Сотрудники.Ссылка";
КонецПроцедуры"#,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn track3_right_outer_join_classification_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
        "ВЫБРАТЬ
        |   Склады.Ссылка КАК Склад
        |ИЗ
        |   Справочник.Склады КАК Склады
        |   ПРАВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
        |   ПО Склады.Кладовщик = Сотрудники.Ссылка";
КонецПроцедуры"#,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 5:13..5:27
                  message: Для полей из ПРАВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }
}
