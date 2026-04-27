//! MismatchedArgCount diagnostic.
//!
//! Emitted from `hir-ty::infer` when a call is routed to a resolved callee
//! (qualified `Module.Method` or platform built-in) and the argument count
//! doesn't match the signature.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from `InferenceDiagnostic::MismatchedArgCount`.
///
/// `required_count` is the minimum number of arguments the caller must
/// supply (last index without a default + 1, so non-standard mixed
/// orders like `(А, Б = ..., В)` correctly require 3 arguments). When
/// `required_count == total_count` the signature has no optional
/// parameters and the message renders a single number; otherwise it
/// renders the inclusive range `от {required} до {total}`.
pub fn from_hir(
    required_count: usize,
    total_count: usize,
    found: usize,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let message = if required_count == total_count {
        format!("Неверное количество аргументов: ожидалось {required_count}, передано {found}")
    } else {
        format!(
            "Неверное количество аргументов: ожидалось от {required_count} до {total_count}, передано {found}"
        )
    };
    crate::simple_hir_diagnostic(DiagnosticCode::MismatchedArgCount, message, range, ctx)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic_with_fixtures;
    use crate::DiagnosticCode;

    #[test]
    fn emits_when_arg_count_differs_from_signature() {
        // Local fixture: resolved common-module call with too few arguments.
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
Процедура Сложение(Левый, Правый) Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ОбщийМодуль.Сложение(1);
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let mismatched: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::MismatchedArgCount).collect();
        assert_eq!(mismatched.len(), 1, "expected one MismatchedArgCount, got: {diags:?}");
        assert!(mismatched[0].message.contains("2") && mismatched[0].message.contains("1"));
    }

    /// Regression: a call that supplies fewer args than the total parameter
    /// count is fine when the missing parameters all have default values.
    /// Mirror of `СтроковыеФункцииКлиентСервер.ПодставитьПараметрыВСтроку`
    /// (10 params total, 2 required) which previously reported a spurious
    /// `expected 10, found 4` against legitimate user code.
    #[test]
    fn does_not_fire_when_optional_args_are_omitted() {
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
Функция ПодставитьПараметры(Шаблон, П1, П2 = Неопределено, П3 = Неопределено,
    П4 = Неопределено, П5 = Неопределено, П6 = Неопределено,
    П7 = Неопределено, П8 = Неопределено, П9 = Неопределено) Экспорт
    Возврат Шаблон;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщийМодуль.ПодставитьПараметры("шаблон", 1, 2, 3);
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let mismatched: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::MismatchedArgCount).collect();
        assert!(
            mismatched.is_empty(),
            "expected no MismatchedArgCount when call has 4 args within [2, 10] range, got: {diags:?}"
        );
    }

    /// Too few args (fewer than the required count) must still fire, and the
    /// message must use the inclusive-range form when required != total.
    #[test]
    fn emits_range_message_when_fewer_than_required() {
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
Функция Метод(А, Б, В = Неопределено) Экспорт
    Возврат А;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщийМодуль.Метод(1);
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let mismatched: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::MismatchedArgCount).collect();
        assert_eq!(mismatched.len(), 1, "expected one MismatchedArgCount, got: {diags:?}");
        assert!(
            mismatched[0].message.contains("от 2 до 3")
                && mismatched[0].message.contains("передано 1"),
            "expected range form 'от 2 до 3, передано 1', got: {}",
            mismatched[0].message
        );
    }

    /// Too many args (more than the total parameter count) must fire even
    /// when there are optional params — the upper bound is exact.
    #[test]
    fn emits_when_more_than_total_args() {
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
Функция Метод(А, Б = Неопределено) Экспорт
    Возврат А;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщийМодуль.Метод(1, 2, 3);
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let mismatched: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::MismatchedArgCount).collect();
        assert_eq!(mismatched.len(), 1, "expected one MismatchedArgCount, got: {diags:?}");
        assert!(
            mismatched[0].message.contains("от 1 до 2")
                && mismatched[0].message.contains("передано 3"),
            "expected 'от 1 до 2, передано 3', got: {}",
            mismatched[0].message
        );
    }

    /// Positive case for the non-standard order `(А, Б = ..., В)`: a 3-arg
    /// call satisfies `required_count = 3` and must not fire the diagnostic.
    /// Locks the `rposition`-based `required_count` semantics: a single
    /// non-default parameter after an optional one bumps the requirement to
    /// the full arity, but supplying that arity is still legal.
    #[test]
    fn non_standard_optional_in_middle_accepts_full_arity_call() {
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
Функция Метод(А, Б = Неопределено, В) Экспорт
    Возврат А;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщийМодуль.Метод(1, 2, 3);
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let mismatched: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::MismatchedArgCount).collect();
        assert!(
            mismatched.is_empty(),
            "non-standard order with all 3 args supplied must not fire MismatchedArgCount, got: {diags:?}"
        );
    }

    /// `Foo(1,,3)` against a method whose middle parameter is optional:
    /// the parser drops the empty slot, but HIR lowering inserts an
    /// `Expr::Missing` placeholder so `args.len()` stays at 3, matching
    /// `total_count`. The arity check must therefore accept the call —
    /// the empty-slot policy belongs to `MissedRequiredParameter`, not
    /// here. Locks both the parser/HIR contract and the range-based
    /// arity check against future regressions.
    #[test]
    fn skipped_args_in_optional_slot_pass_arity_check() {
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
Функция Метод(А, Б = Неопределено, В = Неопределено) Экспорт
    Возврат А;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщийМодуль.Метод(1, , 3);
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let mismatched: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::MismatchedArgCount).collect();
        assert!(
            mismatched.is_empty(),
            "Foo(1,,3) against (А, Б = ..., В = ...) must not fire — Expr::Missing fills slot 2, args.len()=3=total. Got: {diags:?}"
        );
    }

    /// BSL standards prefer required-first ordering, but the language allows
    /// `Функция Foo(А, Б = ..., В)`. The required-count must be the index of
    /// the LAST non-default parameter + 1 — here `В` at index 2, so required
    /// = 3 = total. A 2-arg call must still fire the diagnostic.
    #[test]
    fn non_standard_optional_in_middle_requires_all_args() {
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
Функция Метод(А, Б = Неопределено, В) Экспорт
    Возврат А;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщийМодуль.Метод(1, 2);
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let mismatched: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::MismatchedArgCount).collect();
        assert_eq!(
            mismatched.len(),
            1,
            "non-standard optional-in-middle: 2 args must trigger diagnostic, got: {diags:?}"
        );
        assert!(
            mismatched[0].message.contains("ожидалось 3"),
            "expected single-number form 'ожидалось 3' (required==total), got: {}",
            mismatched[0].message
        );
    }
}
