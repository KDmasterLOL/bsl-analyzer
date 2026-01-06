//! CommonModuleNameGlobal diagnostic
//!
//! Global CommonModules must contain "Global" or "Глобальный" in their name.
//!
//! Ported from: CommonModuleNameGlobalDiagnostic.java (bsl-language-server)
//!
//! ## Severity
//! MAJOR (Warning)
//!
//! ## Tags
//! STANDARD, BADPRACTICE, UNPREDICTABLE

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

    if !module.is_global() {
        return Vec::new();
    }

    let name_lower = module.name().to_lowercase();
    if name_lower.contains("глобальный") || name_lower.contains("global") {
        return Vec::new();
    }

    vec![Diagnostic {
        code: DiagnosticCode::CommonModuleNameGlobal,
        message: "Имя глобального общего модуля должно содержать 'Глобальный' или 'Global'"
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
    fn test_from_metadata_global_without_keyword() {
        let module = bsl_metadata::CommonModule::builder().name("ЧтоТо").global(true).build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::CommonModuleNameGlobal);
    }

    #[test]
    fn test_from_metadata_global_with_russian_keyword() {
        let module =
            bsl_metadata::CommonModule::builder().name("ЧтоТоГлобальный").global(true).build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_from_metadata_global_with_english_keyword() {
        let module =
            bsl_metadata::CommonModule::builder().name("SomethingGlobal").global(true).build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_from_metadata_not_global() {
        let module = bsl_metadata::CommonModule::builder().name("ЧтоТо").global(false).build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 0);
    }
}
