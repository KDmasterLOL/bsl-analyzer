use crate::define_metadata;
use crate::metadata::*;
use crate::AnalysisContext;
use crate::{Diagnostic, DiagnosticCode};
use hir::LocalRange;
use hir::Name;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Clumsy],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    iterator_name: &str,
    range: LocalRange,
    ctx: &AnalysisContext,
) -> Option<Diagnostic<LocalRange>> {
    let code = DiagnosticCode::UseLessForEach;
    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    if ctx.interface_variable_named(&Name::new(iterator_name)).is_some() {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Итератор не используется в теле цикла".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_unused_iterator() {
        let code = r#"
Процедура Тест()
    Для Каждого Итератор Из Коллекция Цикл
        Итератор();
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UseLessForEach,
            expect![[r#"
            UseLessForEach @ 3:17..3:25
              message: Итератор не используется в теле цикла
              severity: Critical"#]],
        );
    }

    #[test]
    fn test_used_in_method_call() {
        let code = r#"
Процедура Тест()
    Для Каждого А Из Б Цикл
        КакойТОМетод(а);
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UseLessForEach, expect![[r#""#]]);
    }

    #[test]
    fn test_used_in_assignment() {
        let code = r#"
Процедура Тест()
    Для Каждого А Из Б Цикл
        В = А;
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UseLessForEach, expect![[r#""#]]);
    }

    #[test]
    fn test_iterator_assigned() {
        let code = r#"
Процедура Тест()
    Для Каждого А Из Б Цикл
        А = Истина;
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UseLessForEach, expect![[r#""#]]);
    }

    #[test]
    fn test_property_access() {
        let code = r#"
Процедура Тест()
    Для Каждого А Из Б Цикл
        А.Свойство = 1;
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UseLessForEach, expect![[r#""#]]);
    }

    #[test]
    fn test_in_condition() {
        let code = r#"
Процедура Тест()
    Для Каждого А Из Б Цикл
        Если А Тогда
        КонецЕсли;
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UseLessForEach, expect![[r#""#]]);
    }

    #[test]
    fn test_method_call_on_iterator() {
        let code = r#"
Процедура Тест()
    Для Каждого Объект Из Б Цикл
        Объект.Метод();
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UseLessForEach, expect![[r#""#]]);
    }

    #[test]
    fn test_chained_method_call() {
        let code = r#"
Процедура Тест()
    Для Каждого АСтруктура Из Б Цикл
        АСтруктура.Ключ.Метод();
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UseLessForEach, expect![[r#""#]]);
    }

    #[test]
    fn test_comprehensive() {
        let code = r#"Перем ПолеМодуля;

Для Каждого Итератор Из Коллекция Цикл // Сработать Итератор неиспользуется в теле цикла
	Итератор();
КонецЦикла;

Для Каждого А Из Б Цикл
	КакойТОМетод(а);
КонецЦикла;

Для Каждого А Из Б Цикл
	В = А;
КонецЦикла;

Для Каждого А Из Б Цикл
	А = Истина;
КонецЦикла;

Для Каждого А Из Б Цикл
	 А.Свойство = 1;
КонецЦикла;

Для Каждого А Из Б Цикл
	 Если А Тогда
	 КонецЕсли;
КонецЦикла;

Для Каждого Объект Из Б Цикл
    Объект.Метод();
КонецЦикла;

Для Каждого АСтруктура Из Б Цикл
    АСтруктура.Ключ.Метод();
КонецЦикла;

Процедура А()

    Перем ПолеМетода;

    Для Каждого ПолеМетода Из Б Цикл // Тут ловить
        КакойтоМетод();
    КонецЦикла;

    Для Каждого ПолеМодуля Из Б Цикл // Тут не ловить
            КакойтоМетод();
    КонецЦикла;

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UseLessForEach,
            expect![[r#"
            UseLessForEach @ 3:13..3:21
              message: Итератор не используется в теле цикла
              severity: Critical
            UseLessForEach @ 40:17..40:27
              message: Итератор не используется в теле цикла
              severity: Critical"#]],
        );
    }

    #[test]
    fn test_used_iterator() {
        let code = r#"
Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Результат = Элемент.Свойство;
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UseLessForEach, expect![[r#""#]]);
    }
}
