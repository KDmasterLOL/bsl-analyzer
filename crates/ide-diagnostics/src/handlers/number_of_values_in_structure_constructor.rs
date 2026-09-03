use crate::define_metadata;
use crate::metadata::*;
use crate::{BodyContext, Diagnostic, DiagnosticCode};
use hir::LocalRange;
use hir::{Body, BodySourceMap, Expr, Name};
use stdx::case::CaseExt;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_MAX_VALUES_COUNT: i64 = 3;

pub fn check_body(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    let code = DiagnosticCode::NumberOfValuesInStructureConstructor;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    let max_values_count = ctx
        .config
        .get_int(DiagnosticCode::NumberOfValuesInStructureConstructor, "maxValuesCount")
        .unwrap_or(DEFAULT_MAX_VALUES_COUNT) as usize;

    check_body_exprs(ctx.body(), ctx.source_map(), max_values_count, code, ctx, acc);
}

fn check_body_exprs(
    body: &Body,
    source_map: &BodySourceMap,
    max_values_count: usize,
    code: DiagnosticCode,
    ctx: &BodyContext,
    diagnostics: &mut Vec<Diagnostic<LocalRange>>,
) {
    for (expr_id, expr) in body.exprs_iter() {
        let Expr::New { type_name, args } = expr else {
            continue;
        };

        if !is_structure_or_fixed_structure(type_name) {
            continue;
        }

        if args.len() <= max_values_count + 1 {
            continue;
        }

        let Some(range) = source_map.expr_range(expr_id) else {
            continue;
        };

        diagnostics.push(Diagnostic {
            code,
            message: format!(
                "Слишком много значений в конструкторе Структура ({}, при допустимом {})",
                args.len() - 1,
                max_values_count
            ),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

fn is_structure_or_fixed_structure(type_name: &Option<Name>) -> bool {
    let Some(name) = type_name else {
        return false;
    };

    let text = name.as_str().fold_lower();
    matches!(text.as_str(), "структура" | "structure" | "фиксированнаяструктура" | "fixedstructure")
}

#[cfg(test)]
mod tests {
    use super::check_body;
    use crate::test_utils::{check_body_diagnostic, format_diags};
    use expect_test::expect;
    #[test]
    fn test_no_diagnostic_for_empty_structure() {
        let code = r#"
Результат = Новый Структура;
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_for_structure_with_only_keys() {
        let code = r#"
Результат = Новый Структура("Номенклатура, Характеристика, Количество, Стоимость");
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_for_three_values() {
        let code = r#"
Результат = Новый Структура("Номенклатура, Характеристика, Количество", Номенклатура, Характеристика, 5);
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_diagnostic_for_four_values() {
        let code = r#"
Результат = Новый Структура("Номенклатура, Характеристика, Количество, Стоимость", Номенклатура, Характеристика, 5, 10);
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            NumberOfValuesInStructureConstructor @ 2:13..2:120
              message: Слишком много значений в конструкторе Структура (4, при допустимом 3)
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_diagnostic_for_english_structure() {
        let code = r#"
Result = New Structure("Goods, Property, Count, Cost", Goods, Property, 5, 10);
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            NumberOfValuesInStructureConstructor @ 2:10..2:79
              message: Слишком много значений в конструкторе Структура (4, при допустимом 3)
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_for_other_constructors() {
        let code = r#"
Результат = Новый ОписаниеТипов(ИсходноеОписаниеТипов, ДобавляемыеТипы, ВычитаемыеТипы, КвалификаторыЧисла, КвалификаторыСтроки);
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_for_fixed_structure_with_three_values() {
        let code = r#"
Результат = Новый ФиксированнаяСтруктура("Номенклатура, Характеристика, Количество", Номенклатура, Характеристика, 5);
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_comprehensive() {
        let code = r#"
// Ru

// Структуры

// Pass
Результат = Новый Структура;

// Pass
Результат = Новый Структура();

// Pass
Результат = Новый Структура("Номенклатура, Характеристика, Количество", Номенклатура, Характеристика, 5);

// Pass
Результат = Новый Структура("Номенклатура, Характеристика, Количество, Стоимость");

// Warning
Результат = Новый Структура("Номенклатура, Характеристика, Количество, Стоимость", Номенклатура, Характеристика, 5, 10);

// Pass
Результат = Новый Структура("Номенклатура, Характеристика, Количество",
                            // Warning
                            Новый Структура("Наименование, Код, Производитель, Цена",,,,));

// Фиксированные структуры

// Pass
Результат = Новый ФиксированнаяСтруктура("Номенклатура, Характеристика, Количество, Стоимость");

// Pass
Результат = Новый ФиксированнаяСтруктура("Номенклатура, Характеристика, Количество", Номенклатура, Характеристика, 5 );

// Прочие конструкторы

// Pass
Результат = Новый ОписаниеТипов(ИсходноеОписаниеТипов, ДобавляемыеТипы, ВычитаемыеТипы, КвалификаторыЧисла);

// Pass
Результат = Новый Запрос("ВЫБРАТЬ
                         |	втТаблица.А,
                         |	втТаблица.Б,
                         |	втТаблица.В,
                         |	втТаблица.Г
                         |ИЗ
                         |	&Таблица КАК втТаблица");


// En

// Structure

// Pass
Result = New Structure;

// Pass
Result = New Structure();

// Pass
Result = New Structure("Goods, Property, Count", Goods, Property, 5);

// Pass
Result = New Structure("Goods, Property, Count, Cost");

// Warning
Result = New Structure("Goods, Property, Count, Cost", Goods, Property, 5, 10);

// Pass
Result = New Structure("Goods, Property, Count",
                            // Warning
                            New Structure("Name, Code, Manufacturer, Price", Name,,,100));

// FixedStructure

// Pass
Result = New FixedStructure("Goods, Property, Count, Cost");

// Pass
Result = New FixedStructure("Goods, Property, Count", Goods, Property, 5);

// Pass
Результат = Новый Массив;

// Pass
Результат = Новый ("КакойТоТип");
"#;
        let diagnostics = check_body_diagnostic(code, check_body);

        expect![[r#"
            NumberOfValuesInStructureConstructor @ 19:13..19:120
              message: Слишком много значений в конструкторе Структура (4, при допустимом 3)
              severity: Information
            NumberOfValuesInStructureConstructor @ 24:29..24:90
              message: Слишком много значений в конструкторе Структура (4, при допустимом 3)
              severity: Information
            NumberOfValuesInStructureConstructor @ 66:10..66:79
              message: Слишком много значений в конструкторе Структура (4, при допустимом 3)
              severity: Information
            NumberOfValuesInStructureConstructor @ 71:29..71:89
              message: Слишком много значений в конструкторе Структура (4, при допустимом 3)
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }
}
