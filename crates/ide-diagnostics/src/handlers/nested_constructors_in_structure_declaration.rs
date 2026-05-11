//! NestedConstructorsInStructureDeclaration diagnostic.
//!
//! Reports nested constructors with parameters inside structure declarations.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Body, BodySourceMap, Expr, ExprId, IdConversion, Name};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::NestedConstructorsInStructureDeclaration;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = crate::utils::for_each_body(ctx, |body, source_map, diags| {
        check_body(body, source_map, code, ctx, diags);
    });

    // Sort diagnostics by position (HIR expressions are stored in arena, not source order)
    diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));

    diagnostics
}

/// Check a single body for nested constructors in structure declarations.
fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Walk all expressions in the body
    for (expr_id, expr) in body.exprs_iter() {
        // Only check New expressions
        let Expr::New { type_name, args } = expr else {
            continue;
        };

        // Check if this is Structure or FixedStructure constructor
        if !is_structure_or_fixed_structure(type_name) {
            continue;
        }

        // Must have more than 1 argument to potentially have nested constructors
        if args.len() <= 1 {
            continue;
        }

        // Check if any argument is a New expression with non-empty parameters
        let has_nested_constructor_with_params = args.iter().any(|&arg_id| {
            matches!(
                body.expr(ExprId::from_idx(arg_id)),
                Expr::New { args: nested_args, .. } if !nested_args.is_empty()
            )
        });

        if !has_nested_constructor_with_params {
            continue;
        }

        // Get range from source map
        let Some(range) = source_map.expr_range(expr_id) else {
            continue;
        };

        diagnostics.push(Diagnostic {
            code,
            message: "Не используйте конструкторы с параметрами при объявлении структуры"
                .to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

/// Check if type name is Structure or FixedStructure (case-insensitive, bilingual).
fn is_structure_or_fixed_structure(type_name: &Option<Name>) -> bool {
    let Some(name) = type_name else {
        return false;
    };

    let text = name.as_str().to_lowercase();
    matches!(text.as_str(), "структура" | "structure" | "фиксированнаяструктура" | "fixedstructure")
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_no_diagnostic_for_empty_structure() {
        let code = r#"
Результат = Новый Структура;
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NestedConstructorsInStructureDeclaration,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_no_diagnostic_for_single_param() {
        let code = r#"
А = Новый Структура(Новый ФиксированнаяСтруктура(Мок_ПараметрыПроцедуры));
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NestedConstructorsInStructureDeclaration,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_no_diagnostic_for_nested_without_params() {
        let code = r#"
Результат = Новый Структура("МВТ, ТекстЗапроса, Параметры",
                             Новый МенеджерВременныхТаблиц,
                             ТекстЗапроса,
                             Новый Структура);
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NestedConstructorsInStructureDeclaration,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_diagnostic_for_nested_with_params() {
        let code = r#"
Результат = Новый Структура("ДанныеНоменклатуры, Количество",
                             Новый Структура("Код, Наименование"),
                             10);
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NestedConstructorsInStructureDeclaration,
            expect![[r#"
                NestedConstructorsInStructureDeclaration @ 2:13..4:33
                  message: Не используйте конструкторы с параметрами при объявлении структуры
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_diagnostic_for_english_keywords() {
        let code = r#"
Result = New Structure("GoodsData, Count",
                        New Structure("Code, Name"),
                        10);
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NestedConstructorsInStructureDeclaration,
            expect![[r#"
                NestedConstructorsInStructureDeclaration @ 2:10..4:28
                  message: Не используйте конструкторы с параметрами при объявлении структуры
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_no_diagnostic_for_non_structure() {
        let code = r#"
Result = New Structure("field1, field2, field3", New Array(), New Array(), New Array());
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NestedConstructorsInStructureDeclaration,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_comprehensive() {
        // Full fixture: RU + EN variants, nested constructors with/without params
        let code = r#"
    // RU

    // Pass
    Результат = Новый Структура("МВТ, ТекстЗапроса, Параметры",
                                 Новый МенеджерВременныхТаблиц,
                                 ТекстЗапроса,
                                 Новый Структура);

    // Warn
    Результат = Новый Структура("ДанныеНоменклатуры, Количество",
                                 Новый Структура("Код, Наименование"),
                                 10);

    Результат = Новый Структура("ЗаполнитьПризнакХарактеристикиИспользуются,                    // Warn
                                |ЗаполнитьПризнакТипНоменклатуры,
                                |ПустаяСтруктура,
                                |ЗаполнитьПризнакВариантОформленияПродажи,
                                |МВТ",
                                Новый Структура("Номенклатура", "ХарактеристикиИспользуются"),  // Warn
                                Новый Структура("Номенклатура", "ТипНоменклатуры"),             // Warn
                                Новый Структура,                                                // Pass
                                Новый Структура("Номенклатура", "ВариантОформленияПродажи"),    // Warn
                                Новый МенеджерВременныхТаблиц);                                 // Pass

    Результат = Новый Структура("Параметры",                                                        // Warn
                                Новый Структура("ФиксированнаяСтруктура",                           // Warn
                                                Новый ФиксированнаяСтруктура(Новый Струкутура)));   // Pass

    // EN

    // Pass
    Result = New Structure("TTM, Query, Params",
                            New TempTablesManager,
                            Query,
                            New Structure);

    // Warn
    Result = New Structure("GoodsData, Count",
                            New Structure("Code, Name"),
                            10);

    Result = New Structure("FillCharacter,                          // Warn
                            |FillType,
                            |EmptyStructure,
                            |FillDealType,
                            |TTM",
                            New Structure("Goods", "Character"),    // Warn
                            New Structure("Goods", "Type"),         // Warn
                            New Structure,                          // Pass
                            New Structure("Goods", "DealType"),     // Warn
                            New TempTablesManager);                 // Pass

    Result = New Structure("Params",                                                // Warn
                            New Structure("FixedStructure",                         // Warn
                                            New FixedStructure(New Structure)));    // Pass

    Result = New Structure("Params",                                              // Pass
                            FillStructure(New FixedStructure(New Structure)));    // Pass

    Result = New Structure("field1, field2, field3", New Array(), New Array(), New Array()); // Pass

    // FP
    А = Новый Структура(Новый ФиксированнаяСтруктура(Мок_ПараметрыПроцедуры));
    А = Новый ФиксированнаяСтруктура(Новый Структура("Источник, Данные"));"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NestedConstructorsInStructureDeclaration,
            expect![[r#"
                NestedConstructorsInStructureDeclaration @ 11:17..13:37
                  message: Не используйте конструкторы с параметрами при объявлении структуры
                  severity: Information
                NestedConstructorsInStructureDeclaration @ 15:17..24:63
                  message: Не используйте конструкторы с параметрами при объявлении структуры
                  severity: Information
                NestedConstructorsInStructureDeclaration @ 26:17..28:97
                  message: Не используйте конструкторы с параметрами при объявлении структуры
                  severity: Information
                NestedConstructorsInStructureDeclaration @ 27:33..28:96
                  message: Не используйте конструкторы с параметрами при объявлении структуры
                  severity: Information
                NestedConstructorsInStructureDeclaration @ 39:14..41:32
                  message: Не используйте конструкторы с параметрами при объявлении структуры
                  severity: Information
                NestedConstructorsInStructureDeclaration @ 43:14..52:51
                  message: Не используйте конструкторы с параметрами при объявлении структуры
                  severity: Information
                NestedConstructorsInStructureDeclaration @ 54:14..56:80
                  message: Не используйте конструкторы с параметрами при объявлении структуры
                  severity: Information
                NestedConstructorsInStructureDeclaration @ 55:29..56:79
                  message: Не используйте конструкторы с параметрами при объявлении структуры
                  severity: Information"#]],
        );
    }
}
