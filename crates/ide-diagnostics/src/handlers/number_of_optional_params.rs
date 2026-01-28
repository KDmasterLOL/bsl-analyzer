//! NumberOfOptionalParams diagnostic.
//!
//! Detects functions and procedures with too many optional parameters.
//!
//! ## Why?
//! Too many optional parameters make methods hard to understand and use correctly.
//! They often indicate that the method is trying to do too much.
//!
//! ## Bad practice
//! ```bsl
//! Процедура СОченьМногоПараметров(А = 1, Б = 2, В = 3, Г = 4, Д = 5)
//!     // ...
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! Use parameter structures or split into multiple methods:
//! ```bsl
//! Процедура ВыполнитьОперацию(Параметры)
//!     // Параметры - структура с полями
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **maxOptionalParamsCount** (default: 3) - Maximum allowed optional parameters
//! - **Enabled by default:** Yes
//! - **Severity:** MINOR → Warning
//!
//! ## Implementation
//! Uses ItemTree for efficiency (cached by Salsa).

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir_def::item_tree::ModItem;

const DEFAULT_MAX_OPTIONAL_PARAMS: i64 = 3;

struct Config {
    max_optional_params: usize,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let max_optional_params = ctx
            .config
            .get_int(DiagnosticCode::NumberOfOptionalParams, "maxOptionalParamsCount")
            .unwrap_or(DEFAULT_MAX_OPTIONAL_PARAMS) as usize;
        Self { max_optional_params }
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::NumberOfOptionalParams;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let item_tree = ctx.item_tree();

    let mut diagnostics = Vec::new();

    for item in item_tree.top_level_items() {
        match item {
            ModItem::Procedure(idx) => {
                let proc = item_tree.procedure(*idx);
                check_method(&proc.params, &config, code, ctx, &mut diagnostics);
            }
            ModItem::Function(idx) => {
                let func = item_tree.function(*idx);
                check_method(&func.params, &config, code, ctx, &mut diagnostics);
            }
            ModItem::Variable(_) => {}
        }
    }

    diagnostics
}

fn check_method(
    params: &[hir_def::item_tree::Param],
    config: &Config,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let optional_params: Vec<_> = params.iter().filter(|p| p.has_default).collect();
    let optional_count = optional_params.len();

    if optional_count > config.max_optional_params {
        // Report each excess optional parameter individually
        for param in optional_params.iter().skip(config.max_optional_params) {
            diagnostics.push(Diagnostic {
                code,
                message: format!(
                    "Уменьшите количество необязательных параметров c {} до допустимого {}",
                    optional_count, config.max_optional_params
                ),
                severity: ctx.severity(code),
                range: param.name_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }
}

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::NumberOfOptionalParams` is encountered.
/// Applies configuration filtering (maxOptionalParamsCount).
pub fn from_hir(
    _method_name: &str,
    count: u32,
    _is_function: bool,
    range: ide_db::TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::NumberOfOptionalParams;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let config = Config::from_context(ctx);
    if (count as usize) <= config.max_optional_params {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Уменьшите количество необязательных параметров c {} до допустимого {}",
            count, config.max_optional_params
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig, Severity};

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/NumberOfOptionalParamsDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // "СработкаПоКоличествуНеобязательных" has 4 optional params (Раз, Четыре, Пять, Шесть)
        // With max=3, only "Шесть" is excess (the 4th optional)
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::NumberOfOptionalParams);
        // CodeSmell + Minor → Information (per metadata mapping)
        assert_eq!(diagnostics[0].severity, Severity::Information);
        // Шесть = 6 is on line 8 (0-indexed)
        assert_diagnostic_range(code, &diagnostics[0], 8, 86, 91);
    }

    #[test]
    fn test_custom_threshold() {
        let code = include_str!("../../test_data/NumberOfOptionalParamsDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::NumberOfOptionalParams,
            serde_json::json!({ "maxOptionalParamsCount": 1 }),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        // МимоТри: 3 optional (Пять, Шесть, Семь) → 2 excess
        // СработкаПоКоличествуНеобязательных: 4 optional → 3 excess
        // Total: 2 + 3 = 5
        assert_eq!(diagnostics.len(), 5);
    }

    #[test]
    fn test_at_threshold() {
        let code = r#"Функция Тест(А = 1, Б = 2, В = 3) Возврат 0; КонецФункции"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_optional_params() {
        let code = r#"Процедура Тест(А, Б, В) КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_multiple_excess_optional() {
        let code = r#"Процедура Тест(А = 1, Б = 2, В = 3, Г = 4, Д = 5)
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        // 5 optional, max 3, so 2 excess: Г, Д
        assert_eq!(diagnostics.len(), 2);
        assert_diagnostic_range(code, &diagnostics[0], 0, 36, 37); // Г
        assert_diagnostic_range(code, &diagnostics[1], 0, 43, 44); // Д
    }
}
