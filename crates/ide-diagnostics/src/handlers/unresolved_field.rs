use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Name, TypeId};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    receiver_ty: TypeId,
    field_name: &Name,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let message = format!(
        "Поле '{}' не найдено у типа '{}'",
        field_name.as_str(),
        ctx.kernel_type_display(receiver_ty, ctx.locale())
    );
    crate::simple_hir_diagnostic(DiagnosticCode::UnresolvedField, message, range, ctx)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_hir_diagnostic, check_hir_diagnostic_with_fixtures};
    use crate::DiagnosticCode;

    fn unresolved_fields(code: &str) -> Vec<crate::Diagnostic> {
        check_hir_diagnostic(code)
            .into_iter()
            .filter(|diag| diag.code == DiagnosticCode::UnresolvedField)
            .collect()
    }

    #[test]
    fn emits_on_module_level_code_not_only_inside_methods() {
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникСсылка.Справочник1
Функция Ссылка() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
С = ОбщийМодуль.Ссылка();
Х = С.НесуществующееПоле;
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let unresolved: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::UnresolvedField).collect();
        assert_eq!(
            unresolved.len(),
            1,
            "UnresolvedField must surface for module-level expressions too, got: {diags:?}"
        );
    }

    #[test]
    fn emits_on_missing_field_of_known_catalog_ref() {
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникСсылка.Справочник1
Функция Ссылка() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    С = ОбщийМодуль.Ссылка();
    Возврат С.НесуществующееПоле;
КонецФункции
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let unresolved: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::UnresolvedField).collect();
        assert_eq!(unresolved.len(), 1, "expected one UnresolvedField, got: {diags:?}");
        assert!(
            unresolved[0].message.contains("НесуществующееПоле"),
            "message must name the missing field, got: {}",
            unresolved[0].message
        );
    }

    #[test]
    fn emits_only_for_missing_fields_of_nonempty_closed_structure() {
        let code = r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Есть = С.СуществующееПоле;
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#;

        let unresolved = unresolved_fields(code);
        assert_eq!(unresolved.len(), 1, "expected one closed-shape miss: {unresolved:?}");
        assert!(unresolved[0].message.contains("НесуществующееПоле"));
        let start: usize = unresolved[0].range.start().into();
        let end: usize = unresolved[0].range.end().into();
        assert_eq!(&code[start..end], "С.НесуществующееПоле");
    }

    #[test]
    fn literal_insert_closes_keyed_structure_but_dynamic_shapes_stay_soft() {
        let closed = r#"Процедура Тест()
    С = Новый Структура();
    С.Вставить("СуществующееПоле", 5);
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#;
        assert_eq!(unresolved_fields(closed).len(), 1);

        let soft_cases = [
            r#"Процедура Тест()
    С = Новый Структура();
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Ключ = "ДинамическоеПоле";
    С.Вставить(Ключ, 1);
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Псевдоним = С;
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    С = ПолучитьСтруктуру();
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Изменить(С);
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    С.Очистить();
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Функция Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Нет = С.НесуществующееПоле;
    Возврат С;
КонецФункции"#,
            r#"// Параметры:
//   С - Структура:
//     * СуществующееПоле - Число
Процедура Тест(С)
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
        ];
        for code in soft_cases {
            assert!(
                unresolved_fields(code).is_empty(),
                "unsafe or keyless shape must stay soft:\n{code}"
            );
        }
    }

    #[test]
    fn only_proven_by_value_call_preserves_closed_shape() {
        let by_value = r#"Процедура Принять(Знач Параметр)
КонецПроцедуры

Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Принять(С);
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#;
        assert_eq!(unresolved_fields(by_value).len(), 1);

        let by_reference = by_value.replace("Знач Параметр", "Параметр");
        assert!(unresolved_fields(&by_reference).is_empty());
    }

    #[test]
    fn expression_context_escapes_keep_shape_soft() {
        let cases = [
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Если Проверить(С) Тогда КонецЕсли;
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Пока Проверить(С) Цикл Прервать; КонецЦикла;
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Для Индекс = Начало(С) По 1 Цикл КонецЦикла;
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Для Каждого Элемент Из Получить(С) Цикл КонецЦикла;
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Внешняя(Внутренняя(С));
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Выполнить С;
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    ДобавитьОбработчик С, Обработчик;
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    ВызватьИсключение С;
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Функция Тест(Условие)
    С = Новый Структура("СуществующееПоле", 5);
    Нет = С.НесуществующееПоле;
    Возврат ?(Условие, С, Неопределено);
КонецФункции"#,
            r#"Процедура Тест(Условие)
    С = Новый Структура("СуществующееПоле", 5);
    Псевдоним = ?(Условие, С, Неопределено);
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
            r#"Процедура Тест()
    С = Новый Структура("СуществующееПоле", 5);
    Контейнер = Новый Массив(С);
    Нет = С.НесуществующееПоле;
КонецПроцедуры"#,
        ];

        for code in cases {
            assert!(
                unresolved_fields(code).is_empty(),
                "expression escape must keep the shape soft:\n{code}"
            );
        }
    }

    #[test]
    fn nested_structure_completeness_is_independent() {
        let code = r#"Процедура Тест()
    С = Новый Структура("Вложенная, Корневое", Новый Структура("Известное", 1), 2);
    Псевдоним = С.Вложенная;
    НетВКорне = С.НесуществующееКорневое;
    НетВнутри = С.Вложенная.НесуществующееВложенное;
КонецПроцедуры"#;

        let unresolved = unresolved_fields(code);
        assert_eq!(
            unresolved.len(),
            1,
            "only the still-closed outer shape must report: {unresolved:?}"
        );
        assert!(unresolved[0].message.contains("НесуществующееКорневое"));
    }
}
