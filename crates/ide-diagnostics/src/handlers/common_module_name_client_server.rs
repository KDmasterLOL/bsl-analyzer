//! CommonModuleNameClientServer diagnostic
//!
//! ClientServer CommonModules must contain "ClientServer" or "КлиентСервер" in their name.
//!
//! Ported from: CommonModuleNameClientServerDiagnostic.java

use crate::{common_module_helpers, Diagnostic, DiagnosticCode, Severity};
use bsl_metadata::traits::MdObject;
use ide_db::hir_def::ModuleMetadata;
use ide_db::TextRange;

pub fn from_metadata(
    metadata: &ModuleMetadata,
    config: &crate::DiagnosticsConfig,
) -> Vec<Diagnostic> {
    if !matches!(metadata.module_type, bsl_metadata::ModuleType::CommonModule) {
        return Vec::new();
    }

    let module = match &metadata.common_module {
        Some(m) => m.as_ref(),
        None => return Vec::new(),
    };

    if !common_module_helpers::is_client_server(module, config.ordinary_app_support) {
        return Vec::new();
    }

    let name_lower = module.name().to_lowercase();
    if name_lower.contains("клиентсервер") || name_lower.contains("clientserver") {
        return Vec::new();
    }

    vec![Diagnostic {
        code: DiagnosticCode::CommonModuleNameClientServer,
        message:
            "Имя клиент-серверного общего модуля должно содержать 'КлиентСервер' или 'ClientServer'"
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
    fn test_from_metadata_without_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .server(true)
            .external_connection(true)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: Some(ide_db::hir_def::ExecutionContext::ClientServer),
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            form: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_from_metadata_with_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingClientServer")
            .server(true)
            .external_connection(true)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: Some(ide_db::hir_def::ExecutionContext::ClientServer),
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            form: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);
        assert_eq!(diagnostics.len(), 0);
    }
}
