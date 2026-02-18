//! CommonModuleNameClientServer diagnostic
//!
//! ClientServer CommonModules must contain "ClientServer" or "КлиентСервер" in their name.
//!
//! Ported from: CommonModuleNameClientServerDiagnostic.java

use crate::define_common_module_name_check;
use crate::metadata::*;

define_common_module_name_check! {
    code: CommonModuleNameClientServer,
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    clean_code_attribute: CleanCodeAttribute::Consistent,
    predicate: crate::common_module_helpers::is_client_server,
    keywords: &["клиентсервер", "clientserver"],
    name_should_contain: true,
    message: "Имя клиент-серверного общего модуля должно содержать 'КлиентСервер' или 'ClientServer'",
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_without_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .server(true)
            .external_connection(true)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();
        let metadata =
            make_common_module_metadata_with_ctx(module, hir_def::ExecutionContext::ClientServer);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_with_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingClientServer")
            .server(true)
            .external_connection(true)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();
        let metadata =
            make_common_module_metadata_with_ctx(module, hir_def::ExecutionContext::ClientServer);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }
}
