//! CommonModuleNameCached diagnostic
//!
//! Cached CommonModules must contain "Cached" or "ПовтИсп" in their name.
//!
//! Ported from: CommonModuleNameCachedDiagnostic.java

use crate::common_module_helpers::check_common_module_name;
use crate::{Diagnostic, DiagnosticCode};
use bsl_metadata::ReturnValueReuse;
use hir::ModuleMetadata;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
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

pub fn from_metadata(
    metadata: &ModuleMetadata,
    ctx: &crate::DiagnosticsContext,
) -> Vec<Diagnostic> {
    check_common_module_name(
        metadata,
        ctx,
        DiagnosticCode::CommonModuleNameCached,
        |m, _oas| m.return_values_reuse() != ReturnValueReuse::DontUse,
        &["повторноеиспользование", "повтисп", "cached"],
        true,
        "Имя кэшируемого общего модуля должно содержать 'ПовтИсп' или 'Cached'",
    )
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
            http_service: None,
            web_service: None,
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
            http_service: None,
            web_service: None,
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
            http_service: None,
            web_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 0);
    }
}
