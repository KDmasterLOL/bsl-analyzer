use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{is_dotted_type_reference, ModItem};
use ide_db::TextRange;
use std::collections::HashMap;
use stdx::case::CaseExt;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MissingParameterDescription;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let module_data = ctx.module_data();

    let allow_short = ctx.config.get_bool(code, "allowShortDescriptionParameters").unwrap_or(true);

    for method_id in &module_data.procedures {
        diagnostics.extend(check_method(ctx, *method_id, code, false, allow_short));
    }

    for method_id in &module_data.functions {
        diagnostics.extend(check_method(ctx, *method_id, code, true, allow_short));
    }

    diagnostics
}

fn check_method(
    ctx: &DiagnosticsContext,
    method_id: hir::MethodId,
    code: DiagnosticCode,
    is_function: bool,
    allow_short: bool,
) -> Vec<Diagnostic> {
    let tree = ctx.item_tree();

    let method_info = tree.item_of(method_id.local_id).and_then(|item| match item {
        ModItem::Function(func_idx) if is_function => {
            let func = tree.function(*func_idx);
            Some((func.name_range, &func.params[..], func.is_export))
        }
        ModItem::Procedure(proc_idx) if !is_function => {
            let proc = tree.procedure(*proc_idx);
            Some((proc.name_range, &proc.params[..], proc.is_export))
        }
        _ => None,
    });

    let (name_range, params, is_export) = match method_info {
        Some(info) => info,
        None => return Vec::new(),
    };

    let docs = match ctx.method_docs(method_id) {
        Some(d) => d,
        None => return Vec::new(),
    };

    if docs.is_hyperlink() {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let param_docs = &docs.parameters;

    if params.is_empty() && param_docs.is_empty() {
        return Vec::new();
    }

    if params.is_empty() && !param_docs.is_empty() {
        let extra_names: Vec<_> = param_docs.iter().map(|p| p.name.as_str()).collect();
        let message = format!(
            "Необходимо удалить описания параметров \"{}\", отсутствующих в сигнатуре метода",
            extra_names.join(", ")
        );
        diagnostics.push(create_diagnostic(name_range, &message, code, ctx));
        return diagnostics;
    }

    if !params.is_empty() && param_docs.is_empty() {
        // A `См.`/`See` cross-reference documents the method as a whole; bsl-ls does not
        // require a parameter section in that case.
        if is_export && !docs.has_see_reference() {
            diagnostics.push(create_diagnostic(
                name_range,
                "Необходимо добавить описание всех параметров метода",
                code,
                ctx,
            ));
        }
        return diagnostics;
    }

    check_parameter_descriptions(
        ctx,
        params,
        param_docs,
        name_range,
        code,
        allow_short,
        &mut diagnostics,
    );

    diagnostics
}

fn check_parameter_descriptions(
    ctx: &DiagnosticsContext,
    params: &[hir::Param],
    param_docs: &[hir::ParameterDoc],
    name_range: TextRange,
    code: DiagnosticCode,
    allow_short: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if is_single_parameter_legacy_type_only_doc(params, param_docs) {
        return;
    }

    let mut doc_map: HashMap<String, &hir::ParameterDoc> = HashMap::new();
    let mut doc_order: Vec<String> = Vec::new();
    let mut duplicate_docs: Vec<&str> = Vec::new();

    for doc in param_docs {
        let lower_name = doc.name.fold_lower();
        if doc_map.contains_key(&lower_name) {
            duplicate_docs.push(&doc.name);
        } else {
            doc_map.insert(lower_name.clone(), doc);
            doc_order.push(lower_name);
        }
    }

    let mut has_missing_description = false;
    let mut matched_docs: Vec<String> = Vec::new();

    for param in params {
        let param_name = param.name.to_string();
        let lower_name = param_name.fold_lower();

        if doc_map.contains_key(&lower_name) {
            if !allow_short {
                let doc = doc_map[&lower_name];
                if !param_doc_has_description(doc) {
                    let message =
                        format!("Необходимо добавить пояснение к параметру \"{}\"", param_name);
                    diagnostics.push(create_diagnostic(param.name_range, &message, code, ctx));
                }
            }
            matched_docs.push(lower_name);
        } else {
            let message = format!("Необходимо добавить описание параметра \"{}\"", param_name);
            diagnostics.push(create_diagnostic(param.name_range, &message, code, ctx));
            has_missing_description = true;
        }
    }

    let mut extra_docs: Vec<_> = param_docs
        .iter()
        .filter(|doc| !matched_docs.contains(&doc.name.fold_lower()))
        .map(|doc| doc.name.as_str())
        .collect();

    extra_docs.extend(duplicate_docs);

    if !extra_docs.is_empty() {
        has_missing_description = true;
        let unique_extra: Vec<_> = extra_docs.into_iter().collect();
        let message = format!(
            "Необходимо удалить описания параметров \"{}\", отсутствующих в сигнатуре метода",
            unique_extra.join(", ")
        );
        diagnostics.push(create_diagnostic(name_range, &message, code, ctx));
    }

    if !has_missing_description {
        let signature_order: Vec<String> =
            params.iter().map(|p| p.name.to_string().fold_lower()).collect();

        let doc_matched_order: Vec<_> =
            doc_order.iter().filter(|n| matched_docs.contains(n)).cloned().collect();

        if signature_order != doc_matched_order {
            diagnostics.push(create_diagnostic(
                name_range,
                "Необходимо исправить порядок описаний параметров",
                code,
                ctx,
            ));
        }
    }
}

fn param_doc_has_description(doc: &hir::ParameterDoc) -> bool {
    if doc.types.is_empty() {
        return false;
    }
    doc.types.iter().any(|type_doc| {
        type_doc.description.as_ref().is_some_and(|d| !d.trim().is_empty())
            || !type_doc.parameters.is_empty()
    })
}

fn is_single_parameter_legacy_type_only_doc(
    params: &[hir::Param],
    param_docs: &[hir::ParameterDoc],
) -> bool {
    if params.len() != 1 || param_docs.len() != 1 {
        return false;
    }

    let param_name = params[0].name.to_string();
    let doc_name = param_docs[0].name.as_str();

    !param_name.eq_ignore_ascii_case(doc_name) && is_dotted_type_reference(doc_name)
}

fn create_diagnostic(
    range: TextRange,
    message: &str,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: message.to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        check_ast_diagnostic_with_config, check_diagnostics_snapshot_for, format_diags,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;
    const FIXTURE: &str = "Функция БезПараметровИОписания()\nКонецФункции\n\nФункция БезОписания(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть, но нет параметров\nФункция Пример1(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример2()\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример3(Параметр1)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример4(Параметр2, Параметр3)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр2 - Строка - Описание параметра 2\n// Параметр1 - Строка - Описание параметра 1\nФункция Пример5(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка\n// Параметр2\nФункция Пример6(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример7(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр3 - Строка - Описание параметра 3\n// Параметр4 - Строка - Описание параметра 4\n// Параметр5\nФункция Пример8(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\n// Параметр3 - Строка - Описание параметра 3\n// Параметр4 - Строка - Описание параметра 4\n// Параметр5 - тип\nФункция Пример9(Параметр1, Знач Параметр4)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример10(параметр1, ПаРамЕтр2)\nКонецФункции\n\n// См. Пример10()\nФункция Пример11(параметр1, ПаРамЕтр2)\nКонецФункции\n\n// Загружает настройку из хранилища общих настроек, как метод платформы Загрузить,\n// объектов СтандартноеХранилищеНастроекМенеджер или ХранилищеНастроекМенеджер.<Имя хранилища>,\n// но с поддержкой длины ключа настроек более 128 символов путем хеширования части,\n// которая превышает 96 символов.\n// Кроме того, возвращает указанное значение по умолчанию, если настройки не существуют.\n// Если нет права СохранениеДанныхПользователя, возвращается значение по умолчанию без ошибки.\n//\n// В возвращаемом значении очищаются ссылки на несуществующий объект в базе данных, а именно\n// - возвращаемая ссылка заменяется на указанное значение по умолчанию;\n// - из данных типа Массив ссылки удаляются;\n// - у данных типа Структура и Соответствие ключ не меняется, а значение устанавливается Неопределено;\n// - анализ значений в данных типа Массив, Структура, Соответствие выполняется рекурсивно.\n//\n// Параметры:\n//   КлючОбъекта          - Строка           - см. синтакс-помощник платформы.\n//   КлючНастроек         - Строка           - см. синтакс-помощник платформы.\n//   ЗначениеПоУмолчанию  - Произвольный     - значение, которое возвращается, если настройки не существуют.\n//                                             Если не указано, возвращается значение Неопределено.\n//   ОписаниеНастроек     - ОписаниеНастроек - см. синтакс-помощник платформы.\n//   ИмяПользователя      - Строка           - см. синтакс-помощник платформы.\n//\n// Возвращаемое значение:\n//   Произвольный - см. синтакс-помощник платформы.\n//\nФункция BUG_1490(КлючОбъекта, КлючНастроек, ЗначениеПоУмолчанию = Неопределено,\n\t\t\tОписаниеНастроек = Неопределено, ИмяПользователя = Неопределено) Экспорт\nКонецФункции\n\n// Делает некоторые вещи с массивом строк\n//\n// Параметры:\n//  МассивСтрок - Массив из Строка - Массив строк\nФункция BUG_1620(МассивСтрок)\nКонецФункции";

    #[test]
    fn test_java_fixture_compatibility() {
        check_diagnostics_snapshot_for(
            FIXTURE,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#"
                MissingParameterDescription @ 15:9..15:16
                  message: Необходимо удалить описания параметров "Параметр1, Параметр2", отсутствующих в сигнатуре метода
                  severity: Warning
                MissingParameterDescription @ 22:9..22:16
                  message: Необходимо удалить описания параметров "Параметр2", отсутствующих в сигнатуре метода
                  severity: Warning
                MissingParameterDescription @ 29:9..29:16
                  message: Необходимо удалить описания параметров "Параметр1", отсутствующих в сигнатуре метода
                  severity: Warning
                MissingParameterDescription @ 29:28..29:37
                  message: Необходимо добавить описание параметра "Параметр3"
                  severity: Warning
                MissingParameterDescription @ 36:9..36:16
                  message: Необходимо исправить порядок описаний параметров
                  severity: Warning
                MissingParameterDescription @ 43:28..43:37
                  message: Необходимо добавить описание параметра "Параметр2"
                  severity: Warning
                MissingParameterDescription @ 51:9..51:16
                  message: Необходимо удалить описания параметров "Параметр2", отсутствующих в сигнатуре метода
                  severity: Warning
                MissingParameterDescription @ 59:9..59:16
                  message: Необходимо удалить описания параметров "Параметр3, Параметр4", отсутствующих в сигнатуре метода
                  severity: Warning
                MissingParameterDescription @ 59:17..59:26
                  message: Необходимо добавить описание параметра "Параметр1"
                  severity: Warning
                MissingParameterDescription @ 59:28..59:37
                  message: Необходимо добавить описание параметра "Параметр2"
                  severity: Warning
                MissingParameterDescription @ 69:9..69:16
                  message: Необходимо удалить описания параметров "Параметр2, Параметр3, Параметр5", отсутствующих в сигнатуре метода
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_no_description() {
        let code = "Функция БезОписания(Параметр1)\nКонецФункции";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_hyperlink_reference() {
        let code = "// См. ДругойМетод()\nФункция Пример(Параметр1)\nКонецФункции";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_non_export_purpose_only_comment_does_not_require_parameters() {
        let code =
            "// Межотчетный период\nПроцедура УстановитьУточнениеПериода(Проводки)\nКонецПроцедуры";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_export_purpose_only_comment_still_requires_parameters() {
        let code =
            "// Межотчетный период\nПроцедура УстановитьУточнениеПериода(Проводки) Экспорт\nКонецПроцедуры";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#"
                MissingParameterDescription @ 2:11..2:37
                  message: Необходимо добавить описание всех параметров метода
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_export_method_with_inline_see_reference_does_not_require_parameters() {
        let code = "// Продолжение процедуры (см. выше).\nПроцедура СохранитьЗавершение(Результат, Параметры) Экспорт\nКонецПроцедуры";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_non_export_with_parameter_section_still_checks_missing_parameter() {
        let code = r#"// Описание.
//
// Параметры:
//   Первый - Строка - первый параметр.
Процедура Пример(Первый, Второй)
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#"
                MissingParameterDescription @ 5:26..5:32
                  message: Необходимо добавить описание параметра "Второй"
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_hyperlink_reference_after_service_prefix() {
        let code = r#"// СтандартныеПодсистемы.УправлениеДоступом
//
// См. УправлениеДоступомПереопределяемый.ПриЗаполненииСписковСОграничениемДоступа.
Процедура ПриЗаполненииОграниченияДоступа(Ограничение) Экспорт
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_parameter_description_continuation_not_extra_parameter() {
        let code = r#"// Получает оформленное накладными по заказам количество.
//
// Параметры:
//   ТаблицаОтбора - ТаблицаЗначений - таблица отбора.
//   ОтборПоИзмерениям - Структура - Ключ структуры определяет имя измерения,
//                       а значение структуры - искомое значение.
//   ИсключитьЗаказ - Булево - признак исключения заказа.
Функция ТаблицаОформлено(ТаблицаОтбора, ОтборПоИзмерениям = Неопределено, ИсключитьЗаказ = Ложь) Экспорт
КонецФункции"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_result_section_after_parameters_not_extra_parameter() {
        let code = r#"// Для переданной организации определяет, является ли она юридическим лицом.
//
// Параметры:
//   Организация - СправочникСсылка.Организации - организация.
//
// Результат:
//   Булево - Истина, если организация - юридическое лицо.
Функция ЭтоЮрЛицо(Организация) Экспорт
КонецФункции"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_single_parameter_legacy_type_only_description_ok() {
        let code = r#"// Для переданной организации определяет, является ли она юридическим лицом.
//
// Параметры:
//   СправочникСсылка.Организации - организация.
//
// Результат:
//   Булево - Истина, если организация - юридическое лицо.
Функция ЭтоЮрЛицо(Организация) Экспорт
КонецФункции"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_correct_documentation() {
        let code = r#"// Описание
// Параметры:
//   Параметр1 - Строка - описание
Функция Пример(Параметр1)
КонецФункции"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"// Описание
// Параметры:
//   Параметр1 - Строка - описание
Функция Пример(параметр1)
КонецФункции"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_strict_mode_flags_type_only_param_doc() {
        let code = r#"// Описание
// Параметры:
//   Параметр1 - Строка
Функция Пример(Параметр1)
КонецФункции"#;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingParameterDescription,
            serde_json::json!({"allowShortDescriptionParameters": false}),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#"
            MissingParameterDescription @ 4:16..4:25
              message: Необходимо добавить пояснение к параметру "Параметр1"
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_strict_mode_passes_param_with_description() {
        let code = r#"// Описание
// Параметры:
//   Параметр1 - Строка - первое слагаемое
Функция Пример(Параметр1)
КонецФункции"#;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingParameterDescription,
            serde_json::json!({"allowShortDescriptionParameters": false}),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_strict_mode_passes_structured_param_doc() {
        let code = r#"// Описание
// Параметры:
//   Параметр1 - Структура:
//     * Поле1 - Строка - первое поле
//     * Поле2 - Число - второе поле
Функция Пример(Параметр1)
КонецФункции"#;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingParameterDescription,
            serde_json::json!({"allowShortDescriptionParameters": false}),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_strict_mode_content_does_not_suppress_order_check() {
        let code = r#"// Описание
// Параметры:
//   Параметр2 - Строка - second
//   Параметр1 - Строка
Функция Пример(Параметр1, Параметр2)
КонецФункции"#;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingParameterDescription,
            serde_json::json!({"allowShortDescriptionParameters": false}),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#"
            MissingParameterDescription @ 5:9..5:15
              message: Необходимо исправить порядок описаний параметров
              severity: Warning
            MissingParameterDescription @ 5:16..5:25
              message: Необходимо добавить пояснение к параметру "Параметр1"
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_default_mode_accepts_type_only_param_doc() {
        let code = r#"// Описание
// Параметры:
//   Параметр1 - Строка
Функция Пример(Параметр1)
КонецФункции"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingParameterDescription,
            expect![[r#""#]],
        );
    }
}
