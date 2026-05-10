//! MissingReturn diagnostic (AllFunctionPathMustHaveReturn).
//!
//! Checks that ALL execution paths in a function return a value using CFG analysis.
//! This is the HIR-based version that replaces the AST-based AllFunctionPathMustHaveReturn.
//!
//! ## Why?
//! Functions should ensure that every possible execution path returns a value.
//! Without this, some code paths may return undefined, leading to subtle bugs.
//!
//! ## Bad practice
//! ```bsl
//! Функция Сумма(А, Б)
//!     Если А > 0 Тогда
//!         Возврат А + Б;
//!     КонецЕсли;
//!     // Missing return in the Else path!
//! КонецФункции
//!
//! Функция ПроверитьХ(Х)
//!     Попытка
//!         Возврат Х / 2;
//!     Исключение
//!         // Missing return in exception handler!
//!     КонецПопытки;
//! КонецФункции
//! ```
//!
//! ## Good practice
//! ```bsl
//! Функция Сумма(А, Б)
//!     Если А > 0 Тогда
//!         Возврат А + Б;
//!     Иначе
//!         Возврат 0;
//!     КонецЕсли;
//! КонецФункции
//!
//! Функция ПроверитьХ(Х)
//!     Попытка
//!         Возврат Х / 2;
//!     Исключение
//!         Возврат -1;
//!     КонецПопытки;
//! КонецФункции
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Warning (Major)
//! - **Tags:** DESIGN, CONFUSING
//!
//! ## Implementation
//!
//! Lowering emits a [`BodyDiagnostic::MissingReturn`] candidate per
//! function whose body could conceivably miss a `Return` (procedures
//! are excluded at the lowering layer because they cannot return a
//! value). The handler here re-validates the candidate against the
//! `module_path_terminates` Salsa query (Track 1 Step E): if the
//! analyser proves every path from the entry block reaches `Return`
//! / `Raise` (i.e. `IN[entry] = MayFallthrough(false)`), the candidate
//! is suppressed; otherwise it is materialised into a user-visible
//! diagnostic.
//!
//! This replaces the previous local CFG walker that inspected every
//! incoming edge of the exit vertex case-by-case (`BasicBlock` ending
//! in `Return`, `WhileLoop` false-branch, `Conditional` false-branch,
//! …). The dataflow framework gives the same answer with a single
//! lattice transfer rule, and centralises the loop / dead-code edge
//! semantics in one place (`crates/dataflow/src/path_terminates.rs`).

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::MethodId;
use ide_db::TextRange;

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

/// Validate a `MissingReturn` candidate emitted by lowering.
///
/// Suppresses the diagnostic when the path-terminates dataflow proves
/// every execution path from the function's entry reaches a `Return`
/// or `Raise` (i.e. `IN[entry] = MayFallthrough(false)`); otherwise
/// emits the candidate as a user-visible diagnostic.
///
/// If either the CFG or the path-terminates result is missing for
/// this method (e.g. malformed lowering, no entry vertex), the
/// handler conservatively trusts the lowering candidate and emits —
/// the same posture the legacy walker took for unreachable cases.
pub fn from_hir(
    range: TextRange,
    method_id: &MethodId,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    // AllFunctionPathMustHaveReturn is the diagnostic code; MissingReturn is the internal HIR name
    let code = DiagnosticCode::AllFunctionPathMustHaveReturn;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let local_id = method_id.local_id;

    // If the analyser proves every path from entry reaches Return/Raise,
    // suppress the candidate. Any missing piece (CFG, entry vertex,
    // path-terminates result) leaves `every_path_returns = false` so the
    // candidate is materialised — the conservative direction.
    let every_path_returns = ctx
        .module_cfgs()
        .get(local_id)
        .and_then(|cfg| cfg.entry_point())
        .and_then(|entry| {
            ctx.module_path_terminates().get(local_id).map(|pt| !pt.may_fallthrough_at_block(entry))
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

    /// Function with ElseIf chain but no final Else - missing return on fallthrough path.
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

    /// Function with explicit Неопределено return - no diagnostic.
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

    /// ElseIf branch body has no return (only a call, not a return statement).
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

    /// ForEach loop with `Возврат` only inside the body and no return after
    /// the loop emits a diagnostic: the empty-collection path through the
    /// `Для Каждого … КонецЦикла` reaches the function's end without
    /// hitting any `Возврат`. Per plan §1.6 + §7 risk #3 the dataflow runs
    /// with `loops_executed_at_least_once = false` and treats every loop as
    /// potentially-skippable, so the user is expected to add an explicit
    /// fallback return after the loop (see
    /// [`test_while_with_break_and_return_after_loop`] for the fixed shape).
    /// This was a known edge case for the legacy walker that "accept[ed]
    /// whatever the current behavior is"; Step E + I lock in the principled
    /// answer.
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

    /// `Пока Истина` with `Возврат` only inside the body emits a
    /// diagnostic: without constant-propagation the analyser cannot
    /// prove the loop is provably-infinite, so the loop's
    /// "didn't execute" / "exited normally" path is treated as a real
    /// runtime path that reaches the function's end without `Возврат`.
    /// Plan §7 risk #3 makes this an explicit known-limitation; the fix
    /// for the user is the same fallback-return idiom.
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

    /// Function with while loop containing Прервать and explicit return after loop.
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

    /// Test simple case with missing else branch
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

    /// Test that functions with returns on all paths don't trigger diagnostic
    #[test]
    fn test_no_diagnostic_when_all_paths_return() {
        // NOTE: In BSL, even when if-else both have returns, control flow continues after the block.
        // This is because BSL's if-else is a statement, not an expression.
        // The idiomatic pattern is to have a fallback return after conditional blocks.
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

    /// If/Else where both branches have Return should not trigger diagnostic
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

    /// Simple If/Else where both branches return
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

    /// Test that raise exception counts as exit
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

    /// Function with If (no Else) followed by TryExcept followed by Return should not trigger.
    ///
    /// Bug: the false branch of Если without Иначе goes to merge block.
    /// If the next statement after КонецЕсли is a Попытка block, the checker
    /// was incorrectly flagging the merge block as "missing return" because it
    /// saw it had a FalseBranch incoming edge.
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

    /// If (no Else) + TryExcept where BOTH try and except have Return — no diagnostic.
    ///
    /// Bug: try/except builder used `is_block_reachable` instead of `block_has_live_incoming`,
    /// so dead blocks after Return were connected to merge block with Direct edges,
    /// causing false positive.
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

    /// Standalone TryExcept where both branches return — no diagnostic.
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

    /// Test procedure (not function) doesn't trigger diagnostic
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
