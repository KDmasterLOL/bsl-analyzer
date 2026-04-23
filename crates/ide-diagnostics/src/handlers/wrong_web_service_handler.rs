//! Reports missing or unresolved Web service operation handlers.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode};
use hir::{ModuleMetadata, Name};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::WebServiceModule],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_metadata(
    metadata: &ModuleMetadata,
    ctx: &crate::DiagnosticsContext,
) -> Vec<Diagnostic> {
    let code = DiagnosticCode::WrongWebServiceHandler;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    if metadata.module_type != bsl_metadata::ModuleType::WebServiceModule {
        return Vec::new();
    }

    let Some(ref web_service) = metadata.web_service else {
        return Vec::new();
    };

    let mut diagnostics = Vec::new();
    let symbol_tree = ctx.symbol_tree();
    let file_text = ctx.file_text();
    let file_len = file_text.len();

    for operation in web_service.operations() {
        let handler_name = operation.procedure_name();
        let operation_name = operation.name();
        let service_name = web_service.name();

        if handler_name.is_empty() {
            let message = format!(
                "Задайте обработчик операции \"{}\" web-сервиса \"{}\"",
                operation_name, service_name
            );
            let end_offset = std::cmp::min(1, file_len);
            let range = TextRange::new(0.into(), (end_offset as u32).into());

            diagnostics.push(Diagnostic {
                code,
                message,
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
            continue;
        }

        let name = Name::new(handler_name);
        let method_symbol = symbol_tree.find_method(&name);

        if method_symbol.is_none() {
            let message = format!(
                "Создайте функцию-обработчик \"{}\" или исправьте некорректный обработчик операции \"{}\" web-сервиса \"{}\"",
                handler_name, operation_name, service_name
            );
            let end_offset = std::cmp::min(1, file_len);
            let range = TextRange::new(0.into(), (end_offset as u32).into());

            diagnostics.push(Diagnostic {
                code,
                message,
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn make_web_service_metadata(web_service: bsl_metadata::WebService) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::WebServiceModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            http_service: None,
            form: None,
            web_service: Some(Arc::new(web_service)),
        }
    }

    #[test]
    fn test_missing_handler() {
        let operation = bsl_metadata::WebServiceOperationBuilder::new()
            .name("ОперацияБезОбработчика")
            .procedure_name("")
            .build();

        let web_service = bsl_metadata::WebServiceBuilder::new()
            .name("WebСервис1")
            .namespace("http://example.com")
            .add_operation(operation)
            .build();

        let metadata = make_web_service_metadata(web_service);
        let file_text = "Функция Операция1()\n\tВозврат Неопределено;\nКонецФункции";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Задайте обработчик операции"));
        assert!(diagnostics[0].message.contains("ОперацияБезОбработчика"));
    }

    #[test]
    fn test_handler_not_found() {
        let operation = bsl_metadata::WebServiceOperationBuilder::new()
            .name("ОперацияСНесуществующимОбработчиком")
            .procedure_name("НесуществующийОбработчик1")
            .build();

        let web_service = bsl_metadata::WebServiceBuilder::new()
            .name("WebСервис1")
            .namespace("http://example.com")
            .add_operation(operation)
            .build();

        let metadata = make_web_service_metadata(web_service);
        let file_text = "Функция Операция1()\n\tВозврат Неопределено;\nКонецФункции";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Создайте функцию-обработчик"));
        assert!(diagnostics[0].message.contains("НесуществующийОбработчик1"));
    }

    #[test]
    fn test_valid_handler() {
        let operation = bsl_metadata::WebServiceOperationBuilder::new()
            .name("Операция1")
            .procedure_name("Операция1")
            .build();

        let web_service = bsl_metadata::WebServiceBuilder::new()
            .name("WebСервис1")
            .namespace("http://example.com")
            .add_operation(operation)
            .build();

        let metadata = make_web_service_metadata(web_service);
        let file_text = "Функция Операция1()\n\tВозврат Неопределено;\nКонецФункции";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_not_web_service_module() {
        let metadata = ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            http_service: None,
            form: None,
            web_service: None,
        };

        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_multiple_operations() {
        let operation1 = bsl_metadata::WebServiceOperationBuilder::new()
            .name("Операция1")
            .procedure_name("Операция1")
            .build();

        let operation2 = bsl_metadata::WebServiceOperationBuilder::new()
            .name("ОперацияБезОбработчика")
            .procedure_name("")
            .build();

        let operation3 = bsl_metadata::WebServiceOperationBuilder::new()
            .name("ОперацияСНесуществующимОбработчиком")
            .procedure_name("НесуществующийОбработчик1")
            .build();

        let web_service = bsl_metadata::WebServiceBuilder::new()
            .name("WebСервис1")
            .namespace("http://example.com")
            .add_operation(operation1)
            .add_operation(operation2)
            .add_operation(operation3)
            .build();

        let metadata = make_web_service_metadata(web_service);
        let file_text = "Функция Операция1()\n\tВозврат Неопределено;\nКонецФункции";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert_eq!(diagnostics.len(), 2);
    }
}
