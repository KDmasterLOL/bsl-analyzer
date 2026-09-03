use crate::define_metadata;
use crate::metadata::*;
use crate::AnalysisContext;
use crate::{Diagnostic, DiagnosticCode};
use hir::LocalRange;
use hir::MethodId;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Unpredictable, MetadataTag::Badpractice, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    range: LocalRange,
    method_id: &MethodId,
    ctx: &AnalysisContext,
) -> Option<Diagnostic<LocalRange>> {
    let code = DiagnosticCode::AllFunctionPathMustHaveReturn;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let every_path_returns = ctx
        .method_cfg(*method_id)
        .entry_point()
        .and_then(|entry| {
            ctx.method_path_terminates(*method_id).map(|pt| !pt.may_fallthrough_at_block(entry))
        })
        .unwrap_or(false);

    if every_path_returns {
        return None;
    }

    Some(Diagnostic {
        code,
        message: message_ru(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

fn message_ru() -> String {
    "Не все пути выполнения функции возвращают значение".to_string()
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_hir_diagnostic, format_diags};
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_missing_return_elseif_no_else() {
        let code = r#"Функция РассчитатьСкидку(Знач КатегорияКлиента)
    Если КатегорияКлиента = "VIP" Тогда
        Возврат 0.15;
    ИначеЕсли КатегорияКлиента = "Постоянный" Тогда
        Возврат 0.10;
    ИначеЕсли КатегорияКлиента = "Новый" Тогда
        Возврат 0.05;
    КонецЕсли;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let count = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
            .count();

        assert_eq!(count, 1, "Expected 1 diagnostic: fallthrough path has no return");
    }

    #[test]
    fn test_no_diagnostic_explicit_undefined_return() {
        let code = r#"Функция РассчитатьСкидку(Знач КатегорияКлиента)
    Если КатегорияКлиента = "VIP" Тогда
        Возврат 0.15;
    ИначеЕсли КатегорияКлиента = "Постоянный" Тогда
        Возврат 0.10;
    ИначеЕсли КатегорияКлиента = "Новый" Тогда
        Возврат 0.05;
    КонецЕсли;
    Возврат Неопределено;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "Explicit return Неопределено should suppress diagnostic"
        );
    }

    #[test]
    fn test_missing_return_in_elseif_branch() {
        let code = r#"Функция ОпределитьТариф(Знач Клиент)
    Если Клиент.Премиум Тогда
        Возврат "Максимальный";
    ИначеЕсли Клиент.Льготный Тогда
        ЗаписатьЛьготныйТарифВЖурнал(Клиент);
    Иначе
        Возврат "Базовый";
    КонецЕсли;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let count = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
            .count();

        assert_eq!(count, 1, "Expected 1 diagnostic: ElseIf branch missing return");
    }

    #[test]
    fn test_foreach_loop_no_return_after_loop_emits_diagnostic() {
        let code = r#"Функция ЦиклДляПроверки(Коллекция, Поиск)
    Для Каждого Элемент Из Коллекция Цикл
        Если Элемент = Поиск Тогда
            Возврат 1;
        КонецЕсли;
    КонецЦикла;
КонецФункции"#;

        let count = check_hir_diagnostic(code)
            .iter()
            .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
            .count();
        assert_eq!(
            count, 1,
            "empty-collection path through ForEach reaches function end without Возврат"
        );
    }

    #[test]
    fn test_while_true_no_fallback_return_emits_diagnostic() {
        let code = r#"Функция НайтиСледующееСовпадение(ТекущиеДанные)
    Пока Истина Цикл
        Если ТекущиеДанные = Неопределено Тогда
            Возврат Неопределено;
        КонецЕсли;
        ТекущиеДанные = СледующийЭлемент(ТекущиеДанные);
    КонецЦикла;
КонецФункции"#;

        let count = check_hir_diagnostic(code)
            .iter()
            .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
            .count();
        assert_eq!(
            count, 1,
            "without constant propagation, `Пока Истина` is treated as potentially-skippable"
        );
    }

    #[test]
    fn test_while_with_break_and_return_after_loop() {
        let code = r#"Функция ПроверкаПрерыванийИПродолжений()
    А = 1;
    Пока Выборка.Следующий() Цикл
        Если РезультатыОтбора.Количество() >= МаксКоличествоВыбранных Тогда
            Прервать;
        КонецЕсли;
        Б = 2;
        С = 3
    КонецЦикла;
    Возврат 1;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "Explicit return after loop should suppress diagnostic"
        );
    }

    #[test]
    fn test_simple_missing_else() {
        let code = r#"
Функция Тест(Х)
    Если Х > 0 Тогда
        Возврат 1;
    КонецЕсли;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let missing_return_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
            .collect();

        expect![[r#"
            AllFunctionPathMustHaveReturn @ 2:9..2:13
              message: Не все пути выполнения функции возвращают значение
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &missing_return_diags));
    }

    #[test]
    fn test_no_diagnostic_when_all_paths_return() {
        let code = r#"
Функция Тест(Х)
    Если Х > 0 Тогда
        Возврат 1;
    ИначеЕсли Х < 0 Тогда
        Возврат -1;
    КонецЕсли;
    Возврат 0; // Fallback return
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);

        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "No diagnostic when all paths return"
        );
    }

    #[test]
    fn test_no_diagnostic_if_else_both_return() {
        let code = r#"
Функция НайтиНазначение(ТекущееНазначение)

    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ
    |    СпрНазначения.Ссылка КАК Назначение
    |ИЗ
    |    Справочник.Назначения КАК СпрНазначения
    |ГДЕ
    |    СпрНазначения.НазначениеНаПроверке = &НазначениеНаПроверке";

    Запрос.УстановитьПараметр("НазначениеНаПроверке", ТекущееНазначение);
    РезультатЗапроса = Запрос.Выполнить();

    Выборка = РезультатЗапроса.Выбрать();

    Если Выборка.Следующий() Тогда
        Возврат Выборка.Назначение;
    Иначе
        Возврат Справочники.Назначения.ПустаяСсылка();
    КонецЕсли;

КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "If/Else with Return in both branches should not trigger diagnostic"
        );
    }

    #[test]
    fn test_no_diagnostic_simple_if_else_both_return() {
        let code = r#"
Функция Тест(Х)
    Если Х > 0 Тогда
        Возврат 1;
    Иначе
        Возврат 0;
    КонецЕсли;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "Simple if/else with both branches returning should not trigger"
        );
    }

    #[test]
    fn test_no_diagnostic_preproc_both_branches_return() {
        let code = r#"Функция F()
    #Если Сервер Тогда
        Возврат 1;
    #Иначе
        Возврат 2;
    #КонецЕсли
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let count = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
            .count();

        assert_eq!(count, 0, "Expected 0 diagnostics: both preprocessor branches return");
    }

    #[test]
    fn test_missing_return_preproc_else_no_return() {
        let code = r#"Функция F()
    #Если Сервер Тогда
        Возврат 1;
    #Иначе
        // no return
    #КонецЕсли
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let count = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
            .count();

        assert_eq!(count, 1, "Expected 1 diagnostic: preprocessor else branch has no return");
    }

    #[test]
    fn test_missing_return_preproc_no_else() {
        let code = r#"Функция F()
    #Если Сервер Тогда
        Возврат 1;
    #КонецЕсли
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let count = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
            .count();

        assert_eq!(count, 1, "Expected 1 diagnostic: preprocessor condition can fall through");
    }

    #[test]
    fn test_no_diagnostic_preproc_nested_in_semantic_if() {
        let code = r#"Функция F(Cond)
    Если Cond Тогда
        #Если Сервер Тогда
            Возврат 1;
        #Иначе
            Возврат 2;
        #КонецЕсли
    Иначе
        Возврат 3;
    КонецЕсли
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let count = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
            .count();

        assert_eq!(
            count, 0,
            "Expected 0 diagnostics: all semantic and preprocessor branches return"
        );
    }

    #[test]
    fn test_raise_counts_as_exit() {
        let code = r#"
Функция Тест()
    ВызватьИсключение "Ошибка";
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "Raise should count as exit"
        );
    }

    #[test]
    fn test_no_diagnostic_if_no_else_then_try_except_then_return() {
        let code = r#"Функция Тест(Запрос)
    Если Не Запрос.Свойство("code") Тогда
        Возврат "error";
    КонецЕсли;
    Результат = Новый Структура;
    Попытка
        Результат.Вставить("success", Истина);
    Исключение
        Результат.Вставить("success", Ложь);
    КонецПопытки;
    Возврат Результат;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "If (no Else) + TryExcept + Return at end should not trigger diagnostic"
        );
    }

    #[test]
    fn test_no_diagnostic_if_then_try_except_both_return() {
        let code = r#"Функция ИндексДняПоИмениКолонки(Знач ИмяКолонки)
    Если НЕ СтрНачинаетсяС(ИмяКолонки, "ПланРаботДень") Тогда
        Возврат -1;
    КонецЕсли;
    Попытка
        Возврат Число(Сред(ИмяКолонки, СтрДлина("ПланРаботДень") + 1));
    Исключение
        Возврат -1;
    КонецПопытки;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "If + TryExcept where all branches return should not trigger diagnostic"
        );
    }

    #[test]
    fn test_no_diagnostic_try_except_both_return() {
        let code = r#"Функция Тест(Х)
    Попытка
        Возврат Х / 2;
    Исключение
        Возврат -1;
    КонецПопытки;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "TryExcept where both branches return should not trigger diagnostic"
        );
    }

    #[test]
    fn test_procedure_not_checked() {
        let code = r#"
Процедура Тест(Х)
    Если Х > 0 Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "Procedures should not be checked"
        );
    }
}
