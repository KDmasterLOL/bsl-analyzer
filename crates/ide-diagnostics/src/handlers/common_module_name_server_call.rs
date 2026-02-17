//! CommonModuleNameServerCall diagnostic
//!
//! ServerCall CommonModules must contain "ServerCall" or "ВызовСервера" in their name.
//!
//! Ported from: CommonModuleNameServerCallDiagnostic.java

use crate::{common_module_helpers, Diagnostic, DiagnosticCode};
use bsl_metadata::traits::MdObject;
use ide_db::hir_def::ModuleMetadata;
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Consistent,
};

/// Check metadata-based diagnostics using ModuleMetadata.
///
/// This is the new metadata-driven version that uses HIR-collected metadata
/// instead of loading configuration for each file.
pub fn from_metadata(
    metadata: &ModuleMetadata,
    ctx: &crate::DiagnosticsContext,
) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CommonModuleNameServerCall;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // Only check CommonModules
    if !matches!(metadata.module_type, bsl_metadata::ModuleType::CommonModule) {
        return Vec::new();
    }

    let module = match &metadata.common_module {
        Some(m) => m.as_ref(),
        None => return Vec::new(),
    };

    if !common_module_helpers::is_server_call(module) {
        return Vec::new();
    }

    let name_lower = module.name().to_lowercase();
    if name_lower.contains("вызовсервера") || name_lower.contains("servercall") {
        return Vec::new();
    }

    vec![Diagnostic {
        code,
        message: "Имя серверного модуля для вызова с клиента должно содержать 'ВызовСервера' или 'ServerCall'".to_string(),
        severity: ctx.severity(code),
        range: TextRange::empty(0.into()),
        tags: ctx.tags(code),
        fixes: vec![],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    #[test]
    fn test_from_metadata_server_call_without_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .server_call(true)
            .server(true)
            .external_connection(false)
            .client_ordinary_application(false)
            .client_managed_application(false)
            .build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            http_service: None,
            web_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::CommonModuleNameServerCall);
    }

    #[test]
    fn test_from_metadata_server_call_with_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingServerCall")
            .server_call(true)
            .server(true)
            .external_connection(false)
            .client_ordinary_application(false)
            .client_managed_application(false)
            .build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            http_service: None,
            web_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_from_metadata_server_call_russian_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("НечтоВызовСервера")
            .server_call(true)
            .server(true)
            .external_connection(false)
            .client_ordinary_application(false)
            .client_managed_application(false)
            .build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            http_service: None,
            web_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 0);
    }
}
