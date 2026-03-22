//! Diagnostic: UnusedParameters
//!
//! Detects parameters that are declared but never used in the method body.
//!
//! Uses HIR traversal to find any mention of parameter name in the method body.
//!
//! ## Exclusions
//!
//! - Empty methods (no code in body)
//! - Platform event handlers (fixed signature defined by 1C platform)
//! - Form element event/command handlers
//! - HTTP service handlers

use crate::define_metadata;
use crate::metadata::*;
use crate::utils::platform_event_handlers::is_platform_event_handler;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Expr, ModItem};
use ide_db::TextRange;
use rustc_hash::FxHashSet;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Os,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design, MetadataTag::Unused],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UnusedParameters;

    if ctx.is_disabled_with_metadata(code) {
        return vec![];
    }

    let mut diagnostics = Vec::new();

    let module_bodies = ctx.module_bodies();
    let item_tree = ctx.item_tree();

    // Collect handler names with fixed signatures defined by the platform
    let metadata = ctx.module_metadata();
    let mut fixed_signature_handlers: FxHashSet<String> = FxHashSet::default();
    if let Some(ref form) = metadata.form {
        for handler in form.event_handlers() {
            fixed_signature_handlers.insert(handler.to_lowercase());
        }
        for handler in form.command_handlers() {
            fixed_signature_handlers.insert(handler.to_lowercase());
        }
    }
    if let Some(ref http_service) = metadata.http_service {
        for (_, method) in http_service.all_methods() {
            if !method.is_handler_empty() {
                fixed_signature_handlers.insert(method.handler().to_lowercase());
            }
        }
    }

    for (local_id, body) in module_bodies.iter_bodies() {
        let method_name = get_method_name(&item_tree, local_id);

        diagnostics.extend(check_method(
            local_id,
            body,
            method_name.as_deref(),
            &module_bodies,
            &fixed_signature_handlers,
            code,
            ctx,
        ));
    }

    diagnostics
}

fn get_method_name(item_tree: &hir::ItemTree, local_id: u32) -> Option<String> {
    let items = item_tree.top_level_items();
    let item = items.get(local_id as usize)?;
    match item {
        ModItem::Procedure(idx) => Some(item_tree.procedure(*idx).name.as_str().to_string()),
        ModItem::Function(idx) => Some(item_tree.function(*idx).name.as_str().to_string()),
        ModItem::Variable(_) => None,
    }
}

fn check_method(
    local_id: u32,
    body: &hir::Body,
    method_name: Option<&str>,
    module_bodies: &hir::ModuleBodies,
    fixed_signature_handlers: &FxHashSet<String>,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if body.params().next().is_none() {
        return diagnostics;
    }

    if is_empty_body(body) {
        return diagnostics;
    }

    if method_name.is_some_and(is_platform_event_handler) {
        return diagnostics;
    }

    // Form/HTTP service handlers have fixed signatures defined by the platform
    if method_name.is_some_and(|name| fixed_signature_handlers.contains(&name.to_lowercase())) {
        return diagnostics;
    }

    let source_map = match module_bodies.source_map(local_id) {
        Some(sm) => sm,
        None => return diagnostics,
    };

    let used_names = collect_used_identifiers(body);

    for param_id in body.params() {
        let binding = body.binding(param_id);
        let param_name = binding.name.as_str();
        let param_name_lower = param_name.to_lowercase();

        if !used_names.contains(&param_name_lower) {
            if let Some(range) = source_map.binding_range(param_id) {
                diagnostics.push(create_diagnostic(param_name, range, code, ctx));
            }
        }
    }

    diagnostics
}

fn collect_used_identifiers(body: &hir::Body) -> FxHashSet<String> {
    let mut used = FxHashSet::default();

    for (_, expr) in body.exprs_iter() {
        if let Expr::Path(name) = expr {
            used.insert(name.as_str().to_lowercase());
        }
    }

    used
}

fn is_empty_body(body: &hir::Body) -> bool {
    body.body_stmts().next().is_none()
}

fn create_diagnostic(
    name: &str,
    range: TextRange,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: format!("Уберите неиспользуемый параметр \"{}\"", name),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;
    #[test]
    fn test_unused_parameter() {
        let code = r#"Процедура ВсеПлохо(А1, Знач Б1 = Ложь)
    ВызовМетода(А1);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedParameters).collect();

        assert_eq!(unused.len(), 1);
        assert!(unused[0].message.contains("Б1"));
        assert_diagnostic_range(code, unused[0], 0, 28, 30);
    }

    #[test]
    fn test_unused_parameter_export() {
        let code = r#"Процедура ВсеПлохоИЭкспорт(А2, Знач Б2 = Ложь) Экспорт
    Вызов(А2);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedParameters).collect();

        assert_eq!(unused.len(), 1);
        assert!(unused[0].message.contains("Б2"));
        assert_diagnostic_range(code, unused[0], 0, 36, 38);
    }

    #[test]
    fn test_all_parameters_used() {
        let code = r#"Процедура ВсеХорошо(А3, Б3)
    Б3 = А3 + 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedParameters).collect();

        assert_eq!(unused.len(), 0);
    }

    #[test]
    fn test_empty_body_no_diagnostic() {
        let code = r#"Процедура Просто(А) Экспорт
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedParameters).collect();

        assert_eq!(unused.len(), 0);
    }

    #[test]
    fn test_oncreate_handler_no_diagnostic() {
        let code = r#"Процедура ПриСозданииОбъекта(Отказ)
    Если ЧтоТо Тогда
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedParameters).collect();

        assert_eq!(unused.len(), 0);
    }

    #[test]
    fn test_platform_event_handler_no_diagnostic() {
        let code = r#"Процедура ПриЗаписи(Отказ)
    Если ЧтоТо Тогда
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedParameters).collect();

        assert_eq!(unused.len(), 0);
    }

    #[test]
    fn test_parameter_used_in_field_access() {
        let code = r#"Процедура ВсеХорошо(Объект, Объект2, Объект3)
    Объект.Поле = 1;
    Объект2.Поле.Метод(2);
    Чтото[Объект3];
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedParameters).collect();

        assert_eq!(unused.len(), 0);
    }

    #[test]
    fn test_java_fixture() {
        let code = r#"Процедура ВсеПлохо(А1, Знач Б1 = Ложь) // Параметр Б
    ВызовМетода(А1);
КонецПроцедуры

Процедура ВсеПлохоИЭкспорт(А2, Знач Б2 = Ложь) Экспорт
    Вызов(А2);
КонецПроцедуры

Процедура ВсеХорошо(А3, Б3)
    //Если А3 Тогда
    Б3 = А3 + 1;
    //КонецЕсли;
КонецПроцедуры

Процедура Просто(А) Экспорт
КонецПроцедуры

Процедура ПриСозданииОбъекта(Отказ)
    Если ЧтоТо Тогда
    КонецЕсли;
КонецПроцедуры

Процедура ВсеХорошо(Объект, Объект2, Объект3)
    Объект.Поле = 1;
    Объект2.Поле.Метод(2);
    Чтото[Объект3];
КонецПроцедуры

Процедура нпе( , знач "")
    Объект.Поле = 1;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedParameters).collect();

        assert_eq!(
            unused.len(),
            2,
            "Expected 2 unused parameters, got: {:?}",
            unused.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        assert_diagnostic_range(code, unused[0], 0, 28, 30);
        assert_diagnostic_range(code, unused[1], 4, 36, 38);
    }

    #[test]
    fn test_form_element_event_handler_no_diagnostic() {
        use std::sync::Arc;

        let code = r#"&НаКлиенте
Процедура СписокПриАктивизацииСтроки(Элемент)
    Если ЧтоТо Тогда
    КонецЕсли;
КонецПроцедуры
"#;

        let form_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" version="2.20">
    <ChildItems>
        <Table name="Список" id="1">
            <Events>
                <Event name="OnActivateRow">СписокПриАктивизацииСтроки</Event>
            </Events>
        </Table>
    </ChildItems>
</Form>"#;

        let form = bsl_metadata::xml_parser::parse_form_xml(form_xml).unwrap();

        let metadata = hir::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::FormModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            form: Some(Arc::new(form)),
            http_service: None,
            web_service: None,
        };

        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, code, |_metadata, ctx| {
                super::check(ctx)
            });
        let unused: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedParameters).collect();

        // СписокПриАктивизацииСтроки is a form element event handler — its parameter
        // signature is fixed by the platform, so unused params should not be flagged
        assert_eq!(unused.len(), 0);
    }

    #[test]
    fn test_http_service_handler_no_diagnostic() {
        use std::sync::Arc;

        let code = r#"Функция ЗапросPOST(Запрос)
    Возврат Новый HTTPСервисОтвет(200);
КонецФункции
"#;

        let http_service = bsl_metadata::HTTPServiceBuilder::new()
            .name("TestService")
            .root_url("test")
            .add_url_template(
                bsl_metadata::HTTPServiceURLTemplateBuilder::new()
                    .name("Запрос")
                    .template("/query")
                    .add_method(
                        bsl_metadata::HTTPServiceMethodBuilder::new()
                            .name("POST")
                            .http_method("POST")
                            .handler("ЗапросPOST")
                            .build(),
                    )
                    .build(),
            )
            .build();

        let metadata = hir::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::HTTPServiceModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            form: None,
            http_service: Some(Arc::new(http_service)),
            web_service: None,
        };

        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, code, |_metadata, ctx| {
                super::check(ctx)
            });
        let unused: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedParameters).collect();

        // ЗапросPOST is an HTTP service handler — its parameter
        // signature is fixed by the platform, so unused params should not be flagged
        assert_eq!(unused.len(), 0);
    }
}
