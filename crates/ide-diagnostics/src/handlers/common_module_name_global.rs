//! CommonModuleNameGlobal diagnostic
//!
//! Global CommonModules must contain "Global" or "Глобальный" in their name.
//!

use crate::define_common_module_name_check;
use crate::metadata::*;

define_common_module_name_check! {
    code: CommonModuleNameGlobal,
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Brainoverload],
    clean_code_attribute: CleanCodeAttribute::Consistent,
    predicate: |m: &bsl_metadata::CommonModule, _oas| m.is_global(),
    keywords: &["глобальный", "global"],
    name_should_contain: true,
    message: "Имя глобального общего модуля должно содержать 'Глобальный' или 'Global'",
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_global_without_keyword() {
        let module = bsl_metadata::CommonModule::builder().name("ЧтоТо").global(true).build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, crate::DiagnosticCode::CommonModuleNameGlobal);
    }

    #[test]
    fn test_global_with_russian_keyword() {
        let module =
            bsl_metadata::CommonModule::builder().name("ЧтоТоГлобальный").global(true).build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_global_with_english_keyword() {
        let module =
            bsl_metadata::CommonModule::builder().name("SomethingGlobal").global(true).build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_not_global() {
        let module = bsl_metadata::CommonModule::builder().name("ЧтоТо").global(false).build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }
}
