//! CommonModuleNameFullAccess diagnostic
//!
//! Privileged CommonModules must contain "FullAccess" or "ПолныеПрава" in their name.
//!
//! Ported from: CommonModuleNameFullAccessDiagnostic.java
//! Type: SECURITY_HOTSPOT

use crate::define_common_module_name_check;
use crate::metadata::*;

define_common_module_name_check! {
    code: CommonModuleNameFullAccess,
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Major,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    predicate: |m: &bsl_metadata::CommonModule, _oas| m.is_privileged(),
    keywords: &["полныеправа", "fullaccess"],
    name_should_contain: true,
    message: "Имя привилегированного общего модуля должно содержать 'ПолныеПрава' или 'FullAccess'",
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_privileged_without_keyword() {
        let module =
            bsl_metadata::CommonModule::builder().name("Something").privileged(true).build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, crate::DiagnosticCode::CommonModuleNameFullAccess);
    }

    #[test]
    fn test_privileged_with_fullaccess() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingFullAccess")
            .privileged(true)
            .build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_privileged_with_russian_keyword() {
        let module =
            bsl_metadata::CommonModule::builder().name("НечтоПолныеПрава").privileged(true).build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }
}
