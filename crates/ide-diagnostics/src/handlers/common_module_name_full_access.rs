//! CommonModuleNameFullAccess diagnostic
//!
//! Privileged CommonModules must contain "FullAccess" or "ПолныеПрава" in their name.
//!
//! Ported from: CommonModuleNameFullAccessDiagnostic.java
//! Type: SECURITY_HOTSPOT

use crate::common_module_helpers::check_common_module_name;
use crate::{Diagnostic, DiagnosticCode};
use hir::ModuleMetadata;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::SecurityHotspot,
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
};

pub fn from_metadata(
    metadata: &ModuleMetadata,
    ctx: &crate::DiagnosticsContext,
) -> Vec<Diagnostic> {
    check_common_module_name(
        metadata,
        ctx,
        DiagnosticCode::CommonModuleNameFullAccess,
        |m, _oas| m.is_privileged(),
        &["полныеправа", "fullaccess"],
        true,
        "Имя привилегированного общего модуля должно содержать 'ПолныеПрава' или 'FullAccess'",
    )
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
            http_service: None,
            web_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

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
            http_service: None,
            web_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

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
            http_service: None,
            web_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 0);
    }
}
