use crate::define_common_module_name_check;
use crate::metadata::*;

define_common_module_name_check! {
    code: CommonModuleNameCached,
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    clean_code_attribute: CleanCodeAttribute::Consistent,
    predicate: |m: &bsl_metadata::CommonModule, _oas| {
        m.return_values_reuse() != bsl_metadata::ReturnValueReuse::DontUse
    },
    keywords: &["повторноеиспользование", "повтисп", "cached"],
    name_should_contain: true,
    message: "Имя кэшируемого общего модуля должно содержать 'ПовтИсп' или 'Cached'",
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_cached_without_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .return_values_reuse(bsl_metadata::ReturnValueReuse::DuringRequest)
            .build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_cached_with_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingCached")
            .return_values_reuse(bsl_metadata::ReturnValueReuse::DuringSession)
            .build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_not_cached() {
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .return_values_reuse(bsl_metadata::ReturnValueReuse::DontUse)
            .build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }
}
