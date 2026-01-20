//! CommonModuleInvalidType diagnostic
//!
//! CommonModule must be one of four types: Server, ServerCall, Client, ClientServer.
//!
//! Ported from: CommonModuleInvalidTypeDiagnostic.java

use crate::{common_module_helpers, Diagnostic, DiagnosticCode, Severity};
use ide_db::hir_def::ModuleMetadata;
use ide_db::TextRange;

/// Check metadata-based diagnostics using ModuleMetadata.
///
/// This is the new metadata-driven version that uses HIR-collected metadata
/// instead of loading configuration for each file.
pub fn from_metadata(
    metadata: &ModuleMetadata,
    config: &crate::DiagnosticsConfig,
) -> Vec<Diagnostic> {
    // Only check CommonModules
    if !matches!(metadata.module_type, bsl_metadata::ModuleType::CommonModule) {
        return Vec::new();
    }

    let module = match &metadata.common_module {
        Some(m) => m.as_ref(),
        None => return Vec::new(),
    };

    let is_valid = common_module_helpers::is_server(module, config.ordinary_app_support)
        || common_module_helpers::is_server_call(module)
        || common_module_helpers::is_client(module, config.ordinary_app_support)
        || common_module_helpers::is_client_server(module, config.ordinary_app_support);

    if is_valid {
        return Vec::new();
    }

    vec![Diagnostic {
        code: DiagnosticCode::CommonModuleInvalidType,
        message:
            "Общий модуль должен быть одного из типов: Server, ServerCall, Client, ClientServer"
                .to_string(),
        severity: Severity::Warning,
        range: TextRange::empty(0.into()),
        tags: vec![],
        fixes: vec![],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;

    #[test]
    fn test_from_metadata_invalid_type() {
        let module = bsl_metadata::CommonModule::builder().name("InvalidModule").build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            form: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::CommonModuleInvalidType);
    }

    #[test]
    fn test_from_metadata_valid_server() {
        let module = bsl_metadata::CommonModule::builder()
            .name("ServerModule")
            .server(true)
            .external_connection(true)
            .client_ordinary_application(true)
            .client_managed_application(false)
            .build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: Some(ide_db::hir_def::ExecutionContext::Server),
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            form: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_from_metadata_non_common_module() {
        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::ManagerModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            form: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 0);
    }
}
