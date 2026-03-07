//! CommonModuleNameClient diagnostic
//!
//! Client (non-global) CommonModules must contain "Client" or "Клиент" in their name.
//!

use crate::define_common_module_name_check;
use crate::metadata::*;

define_common_module_name_check! {
    code: CommonModuleNameClient,
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    clean_code_attribute: CleanCodeAttribute::Consistent,
    predicate: |m: &bsl_metadata::CommonModule, oas| {
        !m.is_global() && crate::common_module_helpers::is_client(m, oas)
    },
    keywords: &["клиент", "client"],
    name_should_contain: true,
    message: "Имя клиентского общего модуля должно содержать 'Клиент' или 'Client'",
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_client_without_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("ЧтоТо")
            .global(false)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();
        let metadata = make_common_module_metadata_with_ctx(module, hir::ExecutionContext::Client);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, crate::DiagnosticCode::CommonModuleNameClient);
    }

    #[test]
    fn test_client_with_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("ЧтоТоКлиент")
            .global(false)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();
        let metadata = make_common_module_metadata_with_ctx(module, hir::ExecutionContext::Client);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_global_module() {
        let module = bsl_metadata::CommonModule::builder()
            .name("ЧтоТо")
            .global(true)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_non_common_module() {
        let metadata = make_non_common_module_metadata(bsl_metadata::ModuleType::ManagerModule);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }
}
