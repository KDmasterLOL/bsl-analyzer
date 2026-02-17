//! CommonModuleNameFullAccess diagnostic
//!
//! Privileged CommonModules must contain "FullAccess" or "ПолныеПрава" in their name.
//!
//! Ported from: CommonModuleNameFullAccessDiagnostic.java
//! Type: SECURITY_HOTSPOT

use crate::{Diagnostic, DiagnosticCode};
use bsl_metadata::traits::MdObject;
use ide_db::hir_def::ModuleMetadata;
use ide_db::TextRange;
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

/// Check metadata-based diagnostics using ModuleMetadata.
///
/// This is the new metadata-driven version that uses HIR-collected metadata
/// instead of loading configuration for each file.
pub fn from_metadata(
    metadata: &ModuleMetadata,
    ctx: &crate::DiagnosticsContext,
) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CommonModuleNameFullAccess;

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

    if !module.is_privileged() {
        return Vec::new();
    }

    let name_lower = module.name().to_lowercase();
    if name_lower.contains("полныеправа") || name_lower.contains("fullaccess") {
        return Vec::new();
    }

    vec![Diagnostic {
        code,
        message:
            "Имя привилегированного общего модуля должно содержать 'ПолныеПрава' или 'FullAccess'"
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
