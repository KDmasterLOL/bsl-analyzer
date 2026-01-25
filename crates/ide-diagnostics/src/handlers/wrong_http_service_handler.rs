//! WrongHttpServiceHandler diagnostic.
//!
//! Checks HTTP service handlers for:
//! - Missing handler (empty handler name)
//! - Handler not found in module
//! - Incorrect handler (wrong number of parameters)

use crate::{Diagnostic, DiagnosticCode};
use hir_def::{item_tree::ModItem, ModuleMetadata, Name};
use ide_db::TextRange;

pub fn from_metadata(
    metadata: &ModuleMetadata,
    ctx: &crate::DiagnosticsContext,
) -> Vec<Diagnostic> {
    let code = DiagnosticCode::WrongHttpServiceHandler;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    if metadata.module_type != bsl_metadata::ModuleType::HTTPServiceModule {
        return Vec::new();
    }

    let Some(ref http_service) = metadata.http_service else {
        return Vec::new();
    };

    let mut diagnostics = Vec::new();
    let symbol_tree = ctx.symbol_tree();
    let item_tree = ctx.item_tree();
    let file_text = ctx.file_text();
    let file_len = file_text.len();

    for (url_template, method) in http_service.all_methods() {
        let handler_name = method.handler();
        let service_path = format!(
            "HTTPService.{}.URLTemplate.{}.Method.{}",
            http_service.name(),
            url_template.name(),
            method.name()
        );

        if handler_name.is_empty() {
            let message = format!("Задайте обработчик http-сервиса \"{}\"", service_path);
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

        match method_symbol {
            None => {
                let message = format!(
                    "Создайте функцию-обработчик \"{}\" или исправьте некорректный обработчик http-сервиса \"{}\"",
                    handler_name, service_path
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
            Some(method_sym) => {
                let param_count = method_sym.params.len();
                if param_count != 1 {
                    let message = format!(
                        "Задайте всего один параметр у обработчика \"{}\" для http-сервиса \"{}\"",
                        handler_name, service_path
                    );

                    let range = get_method_name_range(&item_tree, method_sym.id.local_id)
                        .unwrap_or_else(|| {
                            let end_offset = std::cmp::min(1, file_len);
                            TextRange::new(0.into(), (end_offset as u32).into())
                        });

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
        }
    }

    diagnostics
}

fn get_method_name_range(item_tree: &hir_def::ItemTree, local_id: u32) -> Option<TextRange> {
    let item = item_tree.top_level_items().get(local_id as usize)?;
    match item {
        ModItem::Function(func_idx) => Some(item_tree.function(*func_idx).name_range),
        ModItem::Procedure(proc_idx) => Some(item_tree.procedure(*proc_idx).name_range),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use std::sync::Arc;

    fn make_http_service_metadata(http_service: bsl_metadata::HTTPService) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::HTTPServiceModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            http_service: Some(Arc::new(http_service)),
            form: None,
        }
    }

    #[test]
    fn test_missing_handler() {
        let method = bsl_metadata::HTTPServiceMethodBuilder::new()
            .name("GET")
            .http_method("GET")
            .handler("")
            .build();

        let template = bsl_metadata::HTTPServiceURLTemplateBuilder::new()
            .name("URLTemplate1")
            .template("/test")
            .add_method(method)
            .build();

        let http_service = bsl_metadata::HTTPServiceBuilder::new()
            .name("HTTPСервис1")
            .root_url("/api")
            .add_url_template(template)
            .build();

        let metadata = make_http_service_metadata(http_service);
        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Задайте обработчик"));
    }

    #[test]
    fn test_handler_not_found() {
        let method = bsl_metadata::HTTPServiceMethodBuilder::new()
            .name("GET")
            .http_method("GET")
            .handler("НесуществующийОбработчик")
            .build();

        let template = bsl_metadata::HTTPServiceURLTemplateBuilder::new()
            .name("URLTemplate1")
            .template("/test")
            .add_method(method)
            .build();

        let http_service = bsl_metadata::HTTPServiceBuilder::new()
            .name("HTTPСервис1")
            .root_url("/api")
            .add_url_template(template)
            .build();

        let metadata = make_http_service_metadata(http_service);
        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Создайте функцию-обработчик"));
    }

    #[test]
    fn test_incorrect_handler_params() {
        let method = bsl_metadata::HTTPServiceMethodBuilder::new()
            .name("POST")
            .http_method("POST")
            .handler("НеверныйОбработчик")
            .build();

        let template = bsl_metadata::HTTPServiceURLTemplateBuilder::new()
            .name("URLTemplate1")
            .template("/test")
            .add_method(method)
            .build();

        let http_service = bsl_metadata::HTTPServiceBuilder::new()
            .name("HTTPСервис1")
            .root_url("/api")
            .add_url_template(template)
            .build();

        let metadata = make_http_service_metadata(http_service);
        let file_text = "Функция НеверныйОбработчик(Запрос, ВторойПараметр)\n\tВозврат Неопределено;\nКонецФункции";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Задайте всего один параметр"));
    }

    #[test]
    fn test_valid_handler() {
        let method = bsl_metadata::HTTPServiceMethodBuilder::new()
            .name("GET")
            .http_method("GET")
            .handler("ВерныйОбработчик")
            .build();

        let template = bsl_metadata::HTTPServiceURLTemplateBuilder::new()
            .name("URLTemplate1")
            .template("/test")
            .add_method(method)
            .build();

        let http_service = bsl_metadata::HTTPServiceBuilder::new()
            .name("HTTPСервис1")
            .root_url("/api")
            .add_url_template(template)
            .build();

        let metadata = make_http_service_metadata(http_service);
        let file_text = "Функция ВерныйОбработчик(Запрос)\n\tВозврат Неопределено;\nКонецФункции";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_not_http_service_module() {
        let metadata = ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            http_service: None,
            form: None,
        };

        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_disabled_diagnostic() {
        let method = bsl_metadata::HTTPServiceMethodBuilder::new()
            .name("GET")
            .http_method("GET")
            .handler("")
            .build();

        let template = bsl_metadata::HTTPServiceURLTemplateBuilder::new()
            .name("URLTemplate1")
            .template("/test")
            .add_method(method)
            .build();

        let http_service = bsl_metadata::HTTPServiceBuilder::new()
            .name("HTTPСервис1")
            .root_url("/api")
            .add_url_template(template)
            .build();

        let metadata = make_http_service_metadata(http_service);

        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::WrongHttpServiceHandler);

        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics = crate::test_utils::check_metadata_diagnostic_with_config(
            metadata,
            file_text,
            config,
            from_metadata,
        );

        assert!(diagnostics.is_empty());
    }
}
