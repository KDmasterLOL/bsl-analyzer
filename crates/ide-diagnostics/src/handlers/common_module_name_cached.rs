//! CommonModuleNameCached diagnostic
//!
//! Cached CommonModules must contain "Cached" or "ПовтИсп" in their name.
//!
//! Ported from: CommonModuleNameCachedDiagnostic.java

use crate::{Diagnostic, DiagnosticCode};
use bsl_metadata::traits::MdObject;
use bsl_metadata::ReturnValueReuse;
use ide_db::hir_def::ModuleMetadata;
use ide_db::TextRange;

/// Check metadata-based diagnostics using ModuleMetadata.
pub fn from_metadata(
    metadata: &ModuleMetadata,
    ctx: &crate::DiagnosticsContext,
) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CommonModuleNameCached;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    if !matches!(metadata.module_type, bsl_metadata::ModuleType::CommonModule) {
        return Vec::new();
    }

    let module = match &metadata.common_module {
        Some(m) => m.as_ref(),
        None => return Vec::new(),
    };

    if module.return_values_reuse() == ReturnValueReuse::DontUse {
        return Vec::new();
    }

    let name_lower = module.name().to_lowercase();
    if name_lower.contains("повторноеиспользование")
        || name_lower.contains("повтисп")
        || name_lower.contains("cached")
    {
        return Vec::new();
    }

    vec![Diagnostic {
        code,
        message: "Имя кэшируемого общего модуля должно содержать 'ПовтИсп' или 'Cached'"
            .to_string(),
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
    fn test_from_metadata_cached_without_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .return_values_reuse(ReturnValueReuse::DuringRequest)
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

        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_from_metadata_cached_with_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingCached")
            .return_values_reuse(ReturnValueReuse::DuringSession)
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
    fn test_from_metadata_not_cached() {
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .return_values_reuse(ReturnValueReuse::DontUse)
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
}
