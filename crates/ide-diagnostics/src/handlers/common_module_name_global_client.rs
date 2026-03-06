//! CommonModuleNameGlobalClient diagnostic
//!
//! Global client CommonModules should NOT contain "Client" or "Клиент" in their name.
//! Only "Global" should be used, not "GlobalClient".
//!

use crate::define_common_module_name_check;
use crate::metadata::*;

define_common_module_name_check! {
    code: CommonModuleNameGlobalClient,
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    tags: &[MetadataTag::Standard],
    clean_code_attribute: CleanCodeAttribute::Consistent,
    predicate: |m: &bsl_metadata::CommonModule, oas| {
        m.is_global() && crate::common_module_helpers::is_client(m, oas)
    },
    keywords: &["клиент", "client"],
    name_should_contain: false,
    message: "Имя глобального клиентского модуля не должно содержать 'Клиент' или 'Client'",
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_global_client_with_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingGlobalClient")
            .global(true)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();
        let metadata =
            make_common_module_metadata_with_ctx(module, hir_def::ExecutionContext::Client);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, crate::DiagnosticCode::CommonModuleNameGlobalClient);
    }

    #[test]
    fn test_global_client_without_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingGlobal")
            .global(true)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();
        let metadata =
            make_common_module_metadata_with_ctx(module, hir_def::ExecutionContext::Client);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_global_client_russian_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("НечтоГлобальноеКлиент")
            .global(true)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();
        let metadata =
            make_common_module_metadata_with_ctx(module, hir_def::ExecutionContext::Client);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
    }
}
