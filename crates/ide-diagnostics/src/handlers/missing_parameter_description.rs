//! Reports missing, extra, duplicated, or misordered parameter descriptions.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::ModItem;
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

    for method_id in &module_data.procedures {
        diagnostics.extend(check_method(ctx, *method_id, code, false));
    }

    for method_id in &module_data.functions {
        diagnostics.extend(check_method(ctx, *method_id, code, true));
    }

    diagnostics
}

fn check_method(
    ctx: &DiagnosticsContext,
    method_id: hir::MethodId,
    code: DiagnosticCode,
    is_function: bool,
) -> Vec<Diagnostic> {
    let tree = ctx.item_tree();

    let method_info =
        tree.top_level_items().get(method_id.local_id as usize).and_then(|item| match item {
            ModItem::Function(func_idx) if is_function => {
                let func = tree.function(*func_idx);
                Some((func.name_range, &func.params[..]))
            }
            ModItem::Procedure(proc_idx) if !is_function => {
                let proc = tree.procedure(*proc_idx);
                Some((proc.name_range, &proc.params[..]))
            }
            _ => None,
        });

    let (name_range, params) = match method_info {
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
        diagnostics.push(create_diagnostic(
            name_range,
            "Необходимо добавить описание всех параметров метода",
            code,
            ctx,
        ));
        return diagnostics;
    }

    check_parameter_descriptions(ctx, params, param_docs, name_range, code, &mut diagnostics);

    diagnostics
}

fn check_parameter_descriptions(
    ctx: &DiagnosticsContext,
    params: &[hir::Param],
    param_docs: &[hir::ParameterDoc],
    name_range: TextRange,
    code: DiagnosticCode,
    diagnostics: &mut Vec<Diagnostic>,
) {
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
    use crate::test_utils::{assert_diagnostic_message_at_line, check_ast_diagnostic};
    use crate::DiagnosticCode;
    const FIXTURE: &str = "Функция БезПараметровИОписания()\nКонецФункции\n\nФункция БезОписания(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть, но нет параметров\nФункция Пример1(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример2()\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример3(Параметр1)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример4(Параметр2, Параметр3)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр2 - Строка - Описание параметра 2\n// Параметр1 - Строка - Описание параметра 1\nФункция Пример5(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка\n// Параметр2\nФункция Пример6(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример7(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр3 - Строка - Описание параметра 3\n// Параметр4 - Строка - Описание параметра 4\n// Параметр5\nФункция Пример8(Параметр1, Параметр2)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\n// Параметр3 - Строка - Описание параметра 3\n// Параметр4 - Строка - Описание параметра 4\n// Параметр5 - тип\nФункция Пример9(Параметр1, Знач Параметр4)\nКонецФункции\n\n// Описание есть,\n// Параметры:\n// Параметр1 - Строка - Описание параметра 1\n// Параметр2 - Строка - Описание параметра 2\nФункция Пример10(параметр1, ПаРамЕтр2)\nКонецФункции\n\n// См. Пример10()\nФункция Пример11(параметр1, ПаРамЕтр2)\nКонецФункции\n\n// Загружает настройку из хранилища общих настроек, как метод платформы Загрузить,\n// объектов СтандартноеХранилищеНастроекМенеджер или ХранилищеНастроекМенеджер.<Имя хранилища>,\n// но с поддержкой длины ключа настроек более 128 символов путем хеширования части,\n// которая превышает 96 символов.\n// Кроме того, возвращает указанное значение по умолчанию, если настройки не существуют.\n// Если нет права СохранениеДанныхПользователя, возвращается значение по умолчанию без ошибки.\n//\n// В возвращаемом значении очищаются ссылки на несуществующий объект в базе данных, а именно\n// - возвращаемая ссылка заменяется на указанное значение по умолчанию;\n// - из данных типа Массив ссылки удаляются;\n// - у данных типа Структура и Соответствие ключ не меняется, а значение устанавливается Неопределено;\n// - анализ значений в данных типа Массив, Структура, Соответствие выполняется рекурсивно.\n//\n// Параметры:\n//   КлючОбъекта          - Строка           - см. синтакс-помощник платформы.\n//   КлючНастроек         - Строка           - см. синтакс-помощник платформы.\n//   ЗначениеПоУмолчанию  - Произвольный     - значение, которое возвращается, если настройки не существуют.\n//                                             Если не указано, возвращается значение Неопределено.\n//   ОписаниеНастроек     - ОписаниеНастроек - см. синтакс-помощник платформы.\n//   ИмяПользователя      - Строка           - см. синтакс-помощник платформы.\n//\n// Возвращаемое значение:\n//   Произвольный - см. синтакс-помощник платформы.\n//\nФункция BUG_1490(КлючОбъекта, КлючНастроек, ЗначениеПоУмолчанию = Неопределено,\n\t\t\tОписаниеНастроек = Неопределено, ИмяПользователя = Неопределено) Экспорт\nКонецФункции\n\n// Делает некоторые вещи с массивом строк\n//\n// Параметры:\n//  МассивСтрок - Массив из Строка - Массив строк\nФункция BUG_1620(МассивСтрок)\nКонецФункции";

    #[test]
    fn test_java_fixture_compatibility() {
        let diagnostics = check_ast_diagnostic(FIXTURE, check);

        let mpd: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::MissingParameterDescription)
            .collect();

        assert_eq!(mpd.len(), 12, "Expected 12 diagnostics");

        assert_diagnostic_message_at_line(
            FIXTURE,
            &mpd,
            7,
            "Необходимо добавить описание всех параметров метода",
        );

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
}
