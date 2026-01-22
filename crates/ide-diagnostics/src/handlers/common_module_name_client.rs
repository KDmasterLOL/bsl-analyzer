//! CommonModuleNameClient diagnostic
//!
//! Client (non-global) CommonModules must contain "Client" or "Клиент" in their name.
//!
//! Ported from: CommonModuleNameClientDiagnostic.java

use crate::{common_module_helpers, Diagnostic, DiagnosticCode};
use bsl_metadata::traits::MdObject;
use ide_db::hir_def::ModuleMetadata;
use ide_db::TextRange;

/// Check metadata-based diagnostics using ModuleMetadata.
///
/// This is the new metadata-driven version that uses HIR-collected metadata
/// instead of loading configuration for each file.
pub fn from_metadata(
    metadata: &ModuleMetadata,
    ctx: &crate::DiagnosticsContext,
) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CommonModuleNameClient;

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

    if module.is_global()
        || !common_module_helpers::is_client(module, ctx.config.ordinary_app_support)
    {
        return Vec::new();
    }

    let name_lower = module.name().to_lowercase();
    if name_lower.contains("клиент") || name_lower.contains("client") {
        return Vec::new();
    }

    vec![Diagnostic {
        code,
        message: "Имя клиентского общего модуля должно содержать 'Клиент' или 'Client'".to_string(),
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
    fn test_from_metadata_client_without_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("ЧтоТо")
            .global(false)
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

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::CommonModuleNameClient);
    }

    #[test]
    fn test_from_metadata_client_with_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("ЧтоТоКлиент")
            .global(false)
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

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_from_metadata_global_module() {
        let module = bsl_metadata::CommonModule::builder()
            .name("ЧтоТо")
            .global(true)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

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

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 0);
    }
}
