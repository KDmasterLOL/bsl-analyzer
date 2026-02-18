//! NumberOfParams diagnostic.
//!
//! Detects functions and procedures with too many parameters.
//!
//! ## Why?
//! Too many parameters make methods hard to understand and use correctly.
//! They often indicate that the method is trying to do too much.
//!
//! ## Bad practice
//! ```bsl
//! Процедура СОченьМногоПараметров(А, Б, В, Г, Д, Е, Ж, З)
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
//! - **maxParamsCount** (default: 7) - Maximum allowed parameters
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

const DEFAULT_MAX_PARAMS: i64 = 7;

struct Config {
    max_params: usize,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let max_params = ctx
            .config
            .get_int(DiagnosticCode::NumberOfParams, "maxParamsCount")
            .unwrap_or(DEFAULT_MAX_PARAMS) as usize;
        Self { max_params }
    }
}

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::NumberOfParams` is encountered.
/// Applies configuration filtering (maxParamsCount).
pub fn from_hir(
    _method_name: &str,
    count: u32,
    _is_function: bool,
    range: ide_db::TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::NumberOfParams;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let config = Config::from_context(ctx);
    if (count as usize) <= config.max_params {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Уменьшите количество параметров c {} до допустимого {}",
            count, config.max_params
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
        let code = include_str!("../../test_data/NumberOfParamsDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NumberOfParams).collect();

        // Only "СработкаПоКоличеству" has 8 params (exceeds 7)
        // HIR produces 1 diagnostic per method at method name range
        assert_eq!(diagnostics.len(), 1);
        // "Функция " = 8 chars → name at col 8, "СработкаПоКоличеству" = 20 chars → end 28
        assert_diagnostic_range(code, diagnostics[0], 14, 8, 28);
        assert_eq!(diagnostics[0].code, DiagnosticCode::NumberOfParams);
        // CodeSmell + Minor → Information (per metadata mapping)
        assert_eq!(diagnostics[0].severity, Severity::Information);
    }

    #[test]
    fn test_custom_threshold() {
        let code = include_str!("../../test_data/NumberOfParamsDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::NumberOfParams, serde_json::json!({ "maxParamsCount": 1 }));
        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NumberOfParams).collect();
        // HIR produces 1 diagnostic per method:
        // МимоДва, МимоТри, СработкаПоКоличеству,
        // СработкаПоКоличествуНеобязательных, СработкаПоНеобязательныйПередОбязательным
        assert_eq!(diagnostics.len(), 5);
    }

    #[test]
    fn test_at_threshold() {
        let code = r#"Функция Тест(А, Б, В, Г, Д, Е, Ж) Возврат 0; КонецФункции"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NumberOfParams).collect();
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_params() {
        let code = r#"Процедура Тест() КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NumberOfParams).collect();
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_multiple_excess_params() {
        // NOTE: Using 2-letter names because single "И" is lexed as KwAnd (logical AND),
        // not as Ident. This is expected BSL behavior - keywords can't be used as identifiers.
        let code = r#"Процедура Тест(Аа, Бб, Вв, Гг, Дд, Ее, Жж, Зз, Ии)
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NumberOfParams).collect();
        // HIR produces 1 diagnostic per method at method name range
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, diagnostics[0], 0, 10, 14); // Тест
    }
}
