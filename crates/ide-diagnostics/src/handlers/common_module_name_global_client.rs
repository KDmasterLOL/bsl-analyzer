//! CommonModuleNameGlobalClient diagnostic
//!
//! Global client CommonModules should NOT contain "Client" or "Клиент" in their name.
//! Only "Global" should be used, not "GlobalClient".
//!
//! Ported from: CommonModuleNameGlobalClientDiagnostic.java

use crate::{common_module_helpers, Diagnostic, DiagnosticCode, Severity};
use bsl_metadata::traits::MdObject;
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

    if !module.is_global() || !common_module_helpers::is_client(module, config.ordinary_app_support)
    {
        return Vec::new();
    }

    let name_lower = module.name().to_lowercase();
    if !name_lower.contains("клиент") && !name_lower.contains("client") {
        return Vec::new();
    }

    vec![Diagnostic {
        code: DiagnosticCode::CommonModuleNameGlobalClient,
        message: "Имя глобального клиентского модуля не должно содержать 'Клиент' или 'Client'"
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
    fn test_from_metadata_global_client_with_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingGlobalClient")
            .global(true)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: Some(ide_db::hir_def::ExecutionContext::Client),
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            form: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::CommonModuleNameGlobalClient);
    }

    #[test]
    fn test_from_metadata_global_client_without_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingGlobal")
            .global(true)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: Some(ide_db::hir_def::ExecutionContext::Client),
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
    fn test_from_metadata_global_client_russian_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("НечтоГлобальноеКлиент")
            .global(true)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: Some(ide_db::hir_def::ExecutionContext::Client),
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            form: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 1);
    }
}
