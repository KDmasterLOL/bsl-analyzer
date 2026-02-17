//! Diagnostic: ServerSideExportFormMethod
//!
//! Forbids creating server-side export methods in managed forms.
//!
//! Export methods in managed forms must have only `&НаКлиенте` annotation.
//! Any other annotation or missing annotation is an error.
//!
//! ## Severity
//! BLOCKER
//!
//! ## Conditions
//! - Module is FormModule
//! - Form is Managed (not Ordinary)
//! - Method is exported (`Экспорт`)
//! - Method does NOT have `&НаКлиенте` annotation

use bsl_metadata::{FormType, ModuleType};
use hir::AnnotationKind;
use ide_db::TextRange;

use crate::{Diagnostic, DiagnosticCode, DiagnosticsConfig, DiagnosticsContext};
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::FormModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Unpredictable, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Check for server-side export methods in managed forms.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ServerSideExportFormMethod;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let metadata = ctx.module_metadata();

    // Only FormModule
    if metadata.module_type != ModuleType::FormModule {
        return Vec::new();
    }

    // Only managed forms
    match &metadata.form {
        Some(form) if form.form_type == FormType::Managed => {}
        _ => return Vec::new(),
    }

    let item_tree = ctx.item_tree();
    let mut diagnostics = Vec::new();

    // Check procedures
    for (_, proc) in item_tree.procedures() {
        if proc.is_export && !has_client_annotation(&proc.annotations) {
            diagnostics.push(make_diagnostic(proc.name_range, code, ctx));
        }
    }

    // Check functions
    for (_, func) in item_tree.functions() {
        if func.is_export && !has_client_annotation(&func.annotations) {
            diagnostics.push(make_diagnostic(func.name_range, code, ctx));
        }
    }

    diagnostics
}

/// Check if method has `&НаКлиенте` annotation.
fn has_client_annotation(annotations: &[hir_def::item_tree::Annotation]) -> bool {
    annotations.iter().any(|a| a.kind == AnnotationKind::AtClient)
}

/// Create diagnostic for export method without client annotation.
fn make_diagnostic(range: TextRange, code: DiagnosticCode, ctx: &DiagnosticsContext) -> Diagnostic {
    Diagnostic {
        code,
        message: "Запрещено создавать серверные экспортные методы в форме".to_string(),
        range,
        severity: ctx.severity(code),
        tags: ctx.tags(code),
        fixes: Vec::new(),
    }
}

/// Create diagnostics from metadata (for metadata-dispatch).
pub fn from_metadata(
    metadata: &hir_def::ModuleMetadata,
    _config: &DiagnosticsConfig,
) -> Vec<Diagnostic> {
    // This diagnostic requires ItemTree (not just metadata)
    // Return empty - actual check is done in check() function
    let _ = metadata;
    Vec::new()
}

#[cfg(test)]
#[allow(unexpected_cfgs)]
mod tests {
    #[cfg(feature = "disabled-form-test-helper")]
    use bsl_metadata::FormType;

    #[cfg(feature = "disabled-form-test-helper")]
    use crate::test_utils::check_diagnostics_in_form;
    #[cfg(feature = "disabled-form-test-helper")]
    #[test]
    fn test_export_without_annotation() {
        // Line 0, columns 10-22: БезАннотации
        check_diagnostics_in_form(
            FormType::Managed,
            r#"Процедура БезАннотации() Экспорт
КонецПроцедуры"#,
            &[(0, 10, 22)],
        );
    }

    #[allow(unexpected_cfgs)]
    #[cfg(feature = "disabled-form-test-helper")]
    #[test]
    fn test_export_with_client_annotation() {
        // No diagnostic - client annotation is allowed
        check_diagnostics_in_form(
            FormType::Managed,
            r#"&НаКлиенте
Процедура НаКлиенте() Экспорт
КонецПроцедуры"#,
            &[],
        );
    }

    #[allow(unexpected_cfgs)]
    #[cfg(feature = "disabled-form-test-helper")]
    #[test]
    fn test_export_with_server_annotation() {
        // Line 1, columns 10-19: НаСервере
        check_diagnostics_in_form(
            FormType::Managed,
            r#"&НаСервере
Процедура НаСервере() Экспорт
КонецПроцедуры"#,
            &[(1, 10, 19)],
        );
    }

    #[allow(unexpected_cfgs)]
    #[cfg(feature = "disabled-form-test-helper")]
    #[test]
    fn test_export_with_server_no_context_annotation() {
        // Line 1, columns 10-31: НаСервереБезКонтекста
        check_diagnostics_in_form(
            FormType::Managed,
            r#"&НаСервереБезКонтекста
Процедура НаСервереБезКонтекста() Экспорт
КонецПроцедуры"#,
            &[(1, 10, 31)],
        );
    }

    #[allow(unexpected_cfgs)]
    #[cfg(feature = "disabled-form-test-helper")]
    #[test]
    fn test_ordinary_form_no_diagnostics() {
        // Ordinary forms should have no diagnostics
        check_diagnostics_in_form(
            FormType::Ordinary,
            r#"Процедура БезАннотации() Экспорт
КонецПроцедуры"#,
            &[],
        );
    }

    #[allow(unexpected_cfgs)]
    #[cfg(feature = "disabled-form-test-helper")]
    #[test]
    fn test_non_export_method_no_diagnostics() {
        // Non-export methods should have no diagnostics
        check_diagnostics_in_form(
            FormType::Managed,
            r#"&НаСервере
Процедура НаСервере()
КонецПроцедуры"#,
            &[],
        );
    }

    #[allow(unexpected_cfgs)]
    #[cfg(feature = "disabled-form-test-helper")]
    #[test]
    fn test_full_fixture() {
        // Full test matching Java fixture (0-indexed positions):
        // Line 0, columns 10-22: БезАннотации
        // Line 8, columns 10-19: НаСервере
        // Line 12, columns 10-31: НаСервереБезКонтекста
        check_diagnostics_in_form(
            FormType::Managed,
            r#"Процедура БезАннотации() Экспорт
КонецПроцедуры

&НаКлиенте
Процедура НаКлиенте() Экспорт
КонецПроцедуры

&НаСервере
Процедура НаСервере() Экспорт
КонецПроцедуры

&НаСервереБезКонтекста
Процедура НаСервереБезКонтекста() Экспорт
КонецПроцедуры
"#,
            &[(0, 10, 22), (8, 10, 19), (12, 10, 31)],
        );
    }
}
