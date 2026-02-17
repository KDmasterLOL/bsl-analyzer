//! NumberOfValuesInStructureConstructor diagnostic.
//!
//! Detects when Structure/FixedStructure constructors have too many values.
//! The first argument is the key string, subsequent arguments are values.
//! If the number of values exceeds `maxValuesCount`, a warning is issued.
//!
//! ## Configuration
//! - **maxValuesCount**: Maximum number of values allowed (default: 3)
//!
//! ## Example
//! ```bsl
//! // Warning (4 values > 3)
//! Result = New Structure("A, B, C, D", 1, 2, 3, 4);
//!
//! // Pass (3 values <= 3)
//! Result = New Structure("A, B, C", 1, 2, 3);
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Body, BodySourceMap, Expr, Name};
use crate::define_metadata;
use crate::metadata::*;

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

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::NumberOfValuesInStructureConstructor;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let max_values_count = ctx
        .config
        .get_int(DiagnosticCode::NumberOfValuesInStructureConstructor, "maxValuesCount")
        .unwrap_or(DEFAULT_MAX_VALUES_COUNT) as usize;

    let mut diagnostics = Vec::new();

    let module_bodies = ctx.module_bodies();

    // Check module-level code
    if let Some(module_code) = module_bodies.module_code_result() {
        check_body(
            &module_code.body,
            &module_code.source_map,
            max_values_count,
            code,
            ctx,
            &mut diagnostics,
        );
    }

    // Check all method bodies
    for (_, body, source_map) in module_bodies.method_bodies() {
        check_body(body, source_map, max_values_count, code, ctx, &mut diagnostics);
    }

    // Sort by position (HIR expressions are stored in arena, not source order)
    diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));

    diagnostics
}

fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    max_values_count: usize,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (expr_id, expr) in body.exprs_iter() {
        let Expr::New { type_name, args } = expr else {
            continue;
        };

        if !is_structure_or_fixed_structure(type_name) {
            continue;
        }

        // First argument is the key string, rest are values
        // args.len() > maxValuesCount + 1 means too many values
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

    let text = name.as_str().to_lowercase();
    matches!(text.as_str(), "структура" | "structure" | "фиксированнаяструктура" | "fixedstructure")
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    #[test]
    fn test_no_diagnostic_for_empty_structure() {
        let code = r#"
Результат = Новый Структура;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_for_structure_with_only_keys() {
        let code = r#"
Результат = Новый Структура("Номенклатура, Характеристика, Количество, Стоимость");
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_for_three_values() {
        let code = r#"
Результат = Новый Структура("Номенклатура, Характеристика, Количество", Номенклатура, Характеристика, 5);
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_diagnostic_for_four_values() {
        let code = r#"
Результат = Новый Структура("Номенклатура, Характеристика, Количество, Стоимость", Номенклатура, Характеристика, 5, 10);
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("4"));
        assert!(diagnostics[0].message.contains("3"));
    }

    #[test]
    fn test_diagnostic_for_english_structure() {
        let code = r#"
Result = New Structure("Goods, Property, Count, Cost", Goods, Property, 5, 10);
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_diagnostic_for_other_constructors() {
        let code = r#"
Результат = Новый ОписаниеТипов(ИсходноеОписаниеТипов, ДобавляемыеТипы, ВычитаемыеТипы, КвалификаторыЧисла, КвалификаторыСтроки);
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_for_fixed_structure_with_three_values() {
        let code = r#"
Результат = Новый ФиксированнаяСтруктура("Номенклатура, Характеристика, Количество", Номенклатура, Характеристика, 5);
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_comprehensive() {
        let code =
            include_str!("../../test_data/NumberOfValuesInStructureConstructorDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            4,
            "Should find exactly 4 diagnostics (matching Java implementation)"
        );

        // Verify exact positions matching bsl-language-server (Java) implementation
        // Java uses 0-indexed lines
        assert_diagnostic_range(code, &diagnostics[0], 18, 12, 119);
        assert_diagnostic_range(code, &diagnostics[1], 23, 28, 89);
        assert_diagnostic_range(code, &diagnostics[2], 65, 9, 78);
        assert_diagnostic_range(code, &diagnostics[3], 70, 28, 88);
    }
}
