//! CommonModuleNameFullAccess diagnostic
//!
//! Privileged CommonModules must contain "FullAccess" or "ПолныеПрава" in their name.
//!
//! Ported from: CommonModuleNameFullAccessDiagnostic.java
//! Type: SECURITY_HOTSPOT

use crate::{Diagnostic, DiagnosticCode, Severity};
use bsl_metadata::traits::MdObject;
use ide_db::hir_def::ModuleMetadata;
use ide_db::TextRange;

/// Check metadata-based diagnostics using ModuleMetadata.
///
/// This is the new metadata-driven version that uses HIR-collected metadata
/// instead of loading configuration for each file.
pub fn from_metadata(
    metadata: &ModuleMetadata,
    _config: &crate::DiagnosticsConfig,
) -> Vec<Diagnostic> {
    // Only check CommonModules
    if !matches!(metadata.module_type, bsl_metadata::ModuleType::CommonModule) {
        return Vec::new();
    }

    let module = match &metadata.common_module {
        Some(m) => m.as_ref(),
        None => return Vec::new(),
    };

    if !module.is_privileged() {
        return Vec::new();
    }

    let name_lower = module.name().to_lowercase();
    if name_lower.contains("полныеправа") || name_lower.contains("fullaccess") {
        return Vec::new();
    }

    vec![Diagnostic {
        code: DiagnosticCode::CommonModuleNameFullAccess,
        message:
            "Имя привилегированного общего модуля должно содержать 'ПолныеПрава' или 'FullAccess'"
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
    fn test_from_metadata_privileged_without_keyword() {
        let module =
            bsl_metadata::CommonModule::builder().name("Something").privileged(true).build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::CommonModuleNameFullAccess);
    }

    #[test]
    fn test_from_metadata_privileged_with_fullaccess() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingFullAccess")
            .privileged(true)
            .build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_from_metadata_privileged_with_russian_keyword() {
        let module =
            bsl_metadata::CommonModule::builder().name("НечтоПолныеПрава").privileged(true).build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 0);
    }
}
