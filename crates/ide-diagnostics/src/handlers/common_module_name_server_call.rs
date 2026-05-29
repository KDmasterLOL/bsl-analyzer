use crate::define_common_module_name_check;
use crate::metadata::*;

define_common_module_name_check! {
    code: CommonModuleNameServerCall,
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    clean_code_attribute: CleanCodeAttribute::Consistent,
    predicate: |m: &bsl_metadata::CommonModule, _oas| {
        crate::common_module_helpers::is_server_call(m)
    },
    keywords: &["вызовсервера", "servercall"],
    name_should_contain: true,
    message: "Имя серверного модуля для вызова с клиента должно содержать 'ВызовСервера' или 'ServerCall'",
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_server_call_without_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .server_call(true)
            .server(true)
            .external_connection(false)
            .client_ordinary_application(false)
            .client_managed_application(false)
            .build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, crate::DiagnosticCode::CommonModuleNameServerCall);
    }

    #[test]
    fn test_server_call_with_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingServerCall")
            .server_call(true)
            .server(true)
            .external_connection(false)
            .client_ordinary_application(false)
            .client_managed_application(false)
            .build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_server_call_russian_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("НечтоВызовСервера")
            .server_call(true)
            .server(true)
            .external_connection(false)
            .client_ordinary_application(false)
            .client_managed_application(false)
            .build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }
}
