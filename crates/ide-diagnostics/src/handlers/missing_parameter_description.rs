//! Reports missing, extra, duplicated, or misordered parameter descriptions.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{is_dotted_type_reference, ModItem};
use ide_db::TextRange;
use std::collections::HashMap;

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

    // Strict mode rejects "Параметр - Тип" docs that omit the trailing
    // "— описание" prose. Default `true` keeps existing fixtures (and
    // BSL-style "Тип alone" parameters) compatible; opt-in via config to
    // mirror MissingReturnedValueDescription's `allowShortDescriptionReturnValues`.
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

    let method_info =
        tree.top_level_items().get(method_id.local_id as usize).and_then(|item| match item {
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
        if is_export {
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
        let lower_name = doc.name.to_lowercase();
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
        let lower_name = param_name.to_lowercase();

        if doc_map.contains_key(&lower_name) {
            if !allow_short {
                let doc = doc_map[&lower_name];
                if !param_doc_has_description(doc) {
                    let message =
                        format!("Необходимо добавить пояснение к параметру \"{}\"", param_name);
                    diagnostics.push(create_diagnostic(param.name_range, &message, code, ctx));
                    // Intentionally NOT setting `has_missing_description`:
                    // content-quality issues are orthogonal to structural
                    // (missing/extra/order) issues and should not suppress
                    // the order-correctness check below.
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
        .filter(|doc| !matched_docs.contains(&doc.name.to_lowercase()))
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
            params.iter().map(|p| p.name.to_string().to_lowercase()).collect();

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

/// True iff the parameter doc carries any prose description (a `- описание`
/// tail on at least one type, or a structured `Структура:` block with sub-fields).
/// "Type alone" docs (`Параметр - Строка`) and bare-name docs (`Параметр`)
/// return false — these are what strict mode wants to flag.
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
        assert_diagnostic_message_at_line, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    const FIXTURE: &str = "Функция БезПараметровИОписания()\nКонецФункции\n\nФункция БезОписания(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть, но нет параметров\nФункция Пример1(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример2()\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример3(Параметр1)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример4(Параметр2, Параметр3)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр2 - Строка - Описание параметра 2\n// Параметр1 - Строка - Описание параметра 1\nФункция Пример5(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка\n// Параметр2\nФункция Пример6(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример7(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр3 - Строка - Описание параметра 3\n// Параметр4 - Строка - Описание параметра 4\n// Параметр5\nФункция Пример8(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\n// Параметр3 - Строка - Описание параметра 3\n// Параметр4 - Строка - Описание параметра 4\n// Параметр5 - тип\nФункция Пример9(Параметр1, Знач Параметр4)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример10(параметр1, ПаРамЕтр2)\nКонецФункции\n\n// См. Пример10()\nФункция Пример11(параметр1, ПаРамЕтр2)\nКонецФункции\n\n// Загружает настройку из хранилища общих настроек, как метод платформы Загрузить,\n// объектов СтандартноеХранилищеНастроекМенеджер или ХранилищеНастроекМенеджер.<Имя хранилища>,\n// но с поддержкой длины ключа настроек более 128 символов путем хеширования части,\n// которая превышает 96 символов.\n// Кроме того, возвращает указанное значение по умолчанию, если настройки не существуют.\n// Если нет права СохранениеДанныхПользователя, возвращается значение по умолчанию без ошибки.\n//\n// В возвращаемом значении очищаются ссылки на несуществующий объект в базе данных, а именно\n// - возвращаемая ссылка заменяется на указанное значение по умолчанию;\n// - из данных типа Массив ссылки удаляются;\n// - у данных типа Структура и Соответствие ключ не меняется, а значение устанавливается Неопределено;\n// - анализ значений в данных типа Массив, Структура, Соответствие выполняется рекурсивно.\n//\n// Параметры:\n//   КлючОбъекта          - Строка           - см. синтакс-помощник платформы.\n//   КлючНастроек         - Строка           - см. синтакс-помощник платформы.\n//   ЗначениеПоУмолчанию  - Произвольный     - значение, которое возвращается, если настройки не существуют.\n//                                             Если не указано, возвращается значение Неопределено.\n//   ОписаниеНастроек     - ОписаниеНастроек - см. синтакс-помощник платформы.\n//   ИмяПользователя      - Строка           - см. синтакс-помощник платформы.\n//\n// Возвращаемое значение:\n//   Произвольный - см. синтакс-помощник платформы.\n//\nФункция BUG_1490(КлючОбъекта, КлючНастроек, ЗначениеПоУмолчанию = Неопределено,\n\t\t\tОписаниеНастроек = Неопределено, ИмяПользователя = Неопределено) Экспорт\nКонецФункции\n\n// Делает некоторые вещи с массивом строк\n//\n// Параметры:\n//  МассивСтрок - Массив из Строка - Массив строк\nФункция BUG_1620(МассивСтрок)\nКонецФункции";

    #[test]
    fn test_java_fixture_compatibility() {
        let diagnostics = check_ast_diagnostic(FIXTURE, check);

        let mpd: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::MissingParameterDescription)
            .collect();

        // Was 12 before PR #3: a non-export purpose-only comment (line 7,
        // `// Описание есть, но нет параметров`) used to trigger
        // "Необходимо добавить описание всех параметров метода". Non-export
        // methods no longer require a Параметры section unless one is
        // already present.
        assert_eq!(mpd.len(), 11, "Expected 11 diagnostics");

        assert_diagnostic_message_at_line(
            FIXTURE,
            &mpd,
            14,
            "Необходимо удалить описания параметров \"Параметр1, Параметр2\", отсутствующих в сигнатуре метода",
        );

        assert_diagnostic_message_at_line(
            FIXTURE,
            &mpd,
            21,
            "Необходимо удалить описания параметров \"Параметр2\", отсутствующих в сигнатуре метода",
        );

        let line28_diags: Vec<_> = mpd
            .iter()
            .filter(|d| {
                let start: u32 = d.range.start().into();
                let line = FIXTURE[..start as usize].matches('\n').count();
                line == 28
            })
            .collect();
        assert_eq!(line28_diags.len(), 2, "Line 28 should have 2 diagnostics");

        assert_diagnostic_message_at_line(
            FIXTURE,
            &mpd,
            35,
            "Необходимо исправить порядок описаний параметров",
        );

        assert_diagnostic_message_at_line(
            FIXTURE,
            &mpd,
            42,
            "Необходимо добавить описание параметра \"Параметр2\"",
        );

        assert_diagnostic_message_at_line(
            FIXTURE,
            &mpd,
            50,
            "Необходимо удалить описания параметров \"Параметр2\", отсутствующих в сигнатуре метода",
        );

        let line58_diags: Vec<_> = mpd
            .iter()
            .filter(|d| {
                let start: u32 = d.range.start().into();
                let line = FIXTURE[..start as usize].matches('\n').count();
                line == 58
            })
            .collect();
        assert_eq!(line58_diags.len(), 3, "Line 58 should have 3 diagnostics");

        assert_diagnostic_message_at_line(
            FIXTURE,
            &mpd,
            68,
            "Необходимо удалить описания параметров",
        );
    }

    #[test]
    fn test_no_description() {
        let code = "Функция БезОписания(Параметр1)\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_hyperlink_reference() {
        let code = "// См. ДругойМетод()\nФункция Пример(Параметр1)\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_non_export_purpose_only_comment_does_not_require_parameters() {
        let code =
            "// Межотчетный период\nПроцедура УстановитьУточнениеПериода(Проводки)\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_export_purpose_only_comment_still_requires_parameters() {
        let code =
            "// Межотчетный период\nПроцедура УстановитьУточнениеПериода(Проводки) Экспорт\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingParameterDescription);
        assert!(diagnostics[0]
            .message
            .contains("Необходимо добавить описание всех параметров метода"));
    }

    #[test]
    fn test_non_export_with_parameter_section_still_checks_missing_parameter() {
        let code = r#"// Описание.
//
// Параметры:
//   Первый - Строка - первый параметр.
Процедура Пример(Первый, Второй)
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingParameterDescription);
        assert!(diagnostics[0]
            .message
            .contains("Необходимо добавить описание параметра \"Второй\""));
    }

    #[test]
    fn test_hyperlink_reference_after_service_prefix() {
        let code = r#"// СтандартныеПодсистемы.УправлениеДоступом
//
// См. УправлениеДоступомПереопределяемый.ПриЗаполненииСписковСОграничениемДоступа.
Процедура ПриЗаполненииОграниченияДоступа(Ограничение) Экспорт
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
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
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
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
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
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
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_correct_documentation() {
        let code = r#"// Описание
// Параметры:
//   Параметр1 - Строка - описание
Функция Пример(Параметр1)
КонецФункции"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"// Описание
// Параметры:
//   Параметр1 - Строка - описание
Функция Пример(параметр1)
КонецФункции"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_strict_mode_flags_type_only_param_doc() {
        // `Параметр1 - Строка` matches the signature but has no description
        // tail. Default mode accepts this (BSL idiom). Strict mode
        // (`allowShortDescription=false`) should emit.
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
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingParameterDescription);
        assert!(diagnostics[0].message.contains("Необходимо добавить пояснение к параметру"));
        assert!(diagnostics[0].message.contains("Параметр1"));
    }

    #[test]
    fn test_strict_mode_passes_param_with_description() {
        // Param with full prose (`- описание`) is acceptable even in strict mode.
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
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_strict_mode_passes_structured_param_doc() {
        // Structured Структура: docs carry semantic content via sub-fields.
        // Strict mode treats this as adequate.
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
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_strict_mode_content_does_not_suppress_order_check() {
        // Codex pair-mode regression guard: a content-quality emission
        // (param doc lacking prose description) must not mask the
        // structural order-mismatch emission for the same method.
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
        assert_eq!(diagnostics.len(), 2);
        let messages: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(
            messages.iter().any(|m| m.contains("Необходимо добавить пояснение к параметру")
                && m.contains("Параметр1")),
            "missing strict-mode content emission for Параметр1: {messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("Необходимо исправить порядок описаний параметров")),
            "missing order-mismatch emission: {messages:?}"
        );
    }

    #[test]
    fn test_default_mode_accepts_type_only_param_doc() {
        // Regression guard: default mode (`allowShortDescription=true`)
        // must keep accepting `Параметр - Тип` shorthand.
        let code = r#"// Описание
// Параметры:
//   Параметр1 - Строка
Функция Пример(Параметр1)
КонецФункции"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }
}
