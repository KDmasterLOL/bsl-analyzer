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

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_MAX_OPTIONAL_PARAMS: i64 = 3;

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

    let max_optional_params =
        ctx.config_int(code, "maxOptionalParamsCount", DEFAULT_MAX_OPTIONAL_PARAMS) as usize;
    if (count as usize) <= max_optional_params {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Уменьшите количество необязательных параметров c {} до допустимого {}",
            count, max_optional_params
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        assert_diagnostic_range, check_hir_diagnostic, check_hir_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig, Severity};
    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура МимоРаз()

КонецПроцедуры

Функция МимоТри(Раз, Два, Три, Четыре, Пять = 5, Шесть = 6, Семь = 7)
    Возврат;
КонецФункции

Процедура СработкаПоКоличествуНеобязательных(Раз = 1, Два, Три, Четыре = 4, Пять = 5, Шесть = 6, Семь)

КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::NumberOfOptionalParams)
            .collect();

        // "СработкаПоКоличествуНеобязательных" has 4 optional params
        // HIR produces 1 diagnostic per method at method name range
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::NumberOfOptionalParams);
        // CodeSmell + Minor → Information (per metadata mapping)
        assert_eq!(diagnostics[0].severity, Severity::Information);
        // Method name at line 8 (0-indexed), col 10-44
        assert_diagnostic_range(code, diagnostics[0], 8, 10, 44);
    }

    #[test]
    fn test_custom_threshold() {
        let code = r#"Процедура МимоРаз()

КонецПроцедуры

Функция МимоТри(Раз, Два, Три, Четыре, Пять = 5, Шесть = 6, Семь = 7)
    Возврат;
КонецФункции

Процедура СработкаПоКоличествуНеобязательных(Раз = 1, Два, Три, Четыре = 4, Пять = 5, Шесть = 6, Семь)

КонецПроцедуры"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::NumberOfOptionalParams,
            serde_json::json!({ "maxOptionalParamsCount": 1 }),
        );
        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diagnostics: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::NumberOfOptionalParams)
            .collect();
        // HIR produces 1 diagnostic per method:
        // МимоТри: 3 optional → exceeds 1 → 1 diagnostic
        // СработкаПоКоличествуНеобязательных: 4 optional → exceeds 1 → 1 diagnostic
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_at_threshold() {
        let code = r#"Функция Тест(А = 1, Б = 2, В = 3) Возврат 0; КонецФункции"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::NumberOfOptionalParams)
            .collect();
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_optional_params() {
        let code = r#"Процедура Тест(А, Б, В) КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::NumberOfOptionalParams)
            .collect();
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_multiple_excess_optional() {
        let code = r#"Процедура Тест(А = 1, Б = 2, В = 3, Г = 4, Д = 5)
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::NumberOfOptionalParams)
            .collect();
        // HIR produces 1 diagnostic per method at method name range
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, diagnostics[0], 0, 10, 14); // Тест
    }
}
