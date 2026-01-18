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

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir_def::item_tree::ModItem;
use ide_db::TextRange;

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
    if ctx.config.is_disabled(DiagnosticCode::NumberOfOptionalParams) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let item_tree = ctx.item_tree();

    let mut diagnostics = Vec::new();

    for item in item_tree.top_level_items() {
        match item {
            ModItem::Procedure(idx) => {
                let proc = item_tree.procedure(*idx);
                check_method(&proc.params, proc.param_list_range, &config, &mut diagnostics);
            }
            ModItem::Function(idx) => {
                let func = item_tree.function(*idx);
                check_method(&func.params, func.param_list_range, &config, &mut diagnostics);
            }
            ModItem::Variable(_) => {}
        }
    }

    diagnostics
}

fn check_method(
    params: &[hir_def::item_tree::Param],
    param_list_range: Option<TextRange>,
    config: &Config,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let optional_count = params.iter().filter(|p| p.has_default).count();

    if optional_count > config.max_optional_params {
        let Some(range) = param_list_range else {
            return;
        };

        diagnostics.push(Diagnostic {
            code: DiagnosticCode::NumberOfOptionalParams,
            message: format!(
                "Уменьшите количество необязательных параметров c {} до допустимого {}",
                optional_count, config.max_optional_params
            ),
            severity: Severity::Warning,
            range,
            tags: vec![],
            fixes: vec![],
        });
    }
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

        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, &diagnostics[0], 8, 45, 101);
        assert_eq!(diagnostics[0].code, DiagnosticCode::NumberOfOptionalParams);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
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
        assert_eq!(diagnostics.len(), 2);
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
}
