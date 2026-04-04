//! ServerCallsInFormEvents diagnostic.
//!
//! Detects server method calls (`&НаСервере`, `&НаСервереБезКонтекста`) inside
//! form event handlers `ПриАктивизацииСтроки` and `НачалоВыбора`.
//!
//! ## Why?
//!
//! These events fire frequently during UI interaction (e.g., when user navigates table rows
//! or opens dropdown). Calling server methods in these events causes excessive network traffic
//! and degrades performance.
//!
//! ## Bad practice
//!
//! ```bsl
//! &НаСервере
//! Процедура СерверныйМетод()
//!     // ...
//! КонецПроцедуры
//!
//! &НаКлиенте
//! Процедура ТаблицаФормыПриАктивизацииСтроки(Элемент)
//!     СерверныйМетод();  // ERROR: server call in form event
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//!
//! ```bsl
//! &НаКлиенте
//! Процедура ТаблицаФормыПриАктивизацииСтроки(Элемент)
//!     // Use client-side code only
//!     Элементы.ОтображениеДанных.Видимость = Ложь;
//! КонецПроцедуры
//! ```
//!
//! ## Scope
//!
//! - Only triggers in FormModule (form modules)
//! - Checks local call chains via BFS over ModuleCallSummary
//! - Qualified common module calls are checked for immediate server-only dispatch
//!
//! ## Configuration
//!
//! - **Enabled by default:** Yes
//! - **Severity:** CRITICAL (ERROR)
//! - **Tags:** ERROR, PERFORMANCE

use std::collections::VecDeque;

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::call_graph::{CallTarget, CallerId, EdgeKind};
use ide_db::TextRange;
use rustc_hash::FxHashSet;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::FormModule],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const FORBIDDEN_EVENT_TYPES: &[&str] = &["OnActivateRow", "OnStartChoice"];
const MAX_DEPTH: usize = 64;
const MAX_VISITED: usize = 10_000;

/// Result of BFS: call site range, method name, whether path goes through idle handler.
struct ServerCallFinding {
    range: TextRange,
    method_name: String,
    through_idle: bool,
}

fn bfs_find_server_calls(
    summary: &hir::ModuleCallSummary,
    start_local_id: u32,
    ctx: &DiagnosticsContext,
) -> Vec<ServerCallFinding> {
    let mut results = Vec::new();
    let mut visited: FxHashSet<(vfs::FileId, u32)> = FxHashSet::default();
    // Queue: (file_id, local_id, through_idle)
    let mut queue: VecDeque<(vfs::FileId, u32, bool)> = VecDeque::new();

    let file_id = ctx.file_id;
    queue.push_back((file_id, start_local_id, false));
    visited.insert((file_id, start_local_id));

    let mut depth = 0;

    while !queue.is_empty() && depth < MAX_DEPTH {
        depth += 1;
        let level_size = queue.len();

        for _ in 0..level_size {
            let Some((current_file, current_id, through_idle)) = queue.pop_front() else {
                continue;
            };

            if visited.len() > MAX_VISITED {
                tracing::warn!(file_id = ?current_file, "BFS cap reached: max_visited");
                return results;
            }

            if current_file != file_id {
                continue;
            }

            let caller = CallerId::Method(current_id);

            // Follow synchronous call edges
            for edge in &summary.call_edges {
                if edge.caller != caller {
                    continue;
                }

                match (edge.kind, &edge.target) {
                    (EdgeKind::DirectLocal, CallTarget::Local { callee_local_id }) => {
                        let Some(method) = summary
                            .methods
                            .iter()
                            .find(|method| method.local_id == *callee_local_id)
                        else {
                            continue;
                        };

                        if method.dispatch.is_server_only() && !method.dispatch.no_context {
                            results.push(ServerCallFinding {
                                range: edge.range,
                                method_name: method.name.to_string(),
                                through_idle,
                            });
                            continue;
                        }

                        if method.dispatch.can_run_on_client {
                            let key = (current_file, *callee_local_id);
                            if visited.insert(key) {
                                queue.push_back((key.0, key.1, through_idle));
                            }
                        }
                    }
                    (
                        EdgeKind::DirectQualifiedModule,
                        CallTarget::QualifiedModule { module_name, method_name },
                    ) => {
                        let Some(target_file_id) =
                            ctx.module_index().resolve_common_module(module_name)
                        else {
                            continue;
                        };

                        let target_module_id = hir::ModuleId::new(target_file_id);
                        let target_tree = ctx.symbol_tree_for(target_module_id);
                        let Some(method_symbol) = target_tree.find_method(method_name) else {
                            continue;
                        };

                        if !method_symbol.is_export {
                            continue;
                        }

                        let dispatch = hir::call_graph::MethodDispatch::from_annotation(
                            method_symbol.annotations.first().map(|annotation| &annotation.kind),
                        );

                        if dispatch.is_server_only() && !dispatch.no_context {
                            results.push(ServerCallFinding {
                                range: edge.range,
                                method_name: method_name.to_string(),
                                through_idle,
                            });
                        }
                    }
                    _ => {}
                }
            }

            // Follow idle handler registrations (findings get lowered severity)
            for idle_reg in &summary.idle_handler_regs {
                if idle_reg.caller != caller {
                    continue;
                }

                let Some(handler_method) =
                    summary.methods.iter().find(|m| m.name.eq_ignore_case(&idle_reg.handler_name))
                else {
                    continue;
                };

                let key = (file_id, handler_method.local_id);
                if visited.insert(key) {
                    queue.push_back((key.0, key.1, true));
                }
            }
        }
    }

    if !queue.is_empty() {
        tracing::warn!(file_id = ?file_id, "BFS cap reached: max_depth");
    }

    results
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ServerCallsInFormEvents;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let metadata = ctx.module_metadata();
    if metadata.module_type != bsl_metadata::ModuleType::FormModule {
        return Vec::new();
    }

    let module_id = hir::ModuleId::new(ctx.file_id);
    let summary = ctx.call_summary(module_id);

    let mut diagnostics = Vec::new();

    for entry in &summary.form_entries {
        if !FORBIDDEN_EVENT_TYPES
            .iter()
            .any(|event_type| event_type.eq_ignore_ascii_case(&entry.event_type))
        {
            continue;
        }

        let handler_local_id = summary
            .methods
            .iter()
            .find(|method| method.name.eq_ignore_case(&entry.handler_name))
            .map(|method| method.local_id);

        let Some(handler_local_id) = handler_local_id else {
            continue;
        };

        for finding in bfs_find_server_calls(&summary, handler_local_id, ctx) {
            let (severity, message) = if finding.through_idle {
                (
                    crate::Severity::Information,
                    format!(
                        "В обработчике ожидания, подключённом из события формы, рекомендуется использовать &НаСервереБезКонтекста. Процедура \"{}\" выполняется на сервере с контекстом",
                        finding.method_name
                    ),
                )
            } else {
                (
                    ctx.severity(code),
                    format!(
                        "В событиях ПриАктивизацииСтроки и НачалоВыбора не должно быть вызовов серверных процедур. Процедура \"{}\" выполняется на сервере",
                        finding.method_name
                    ),
                )
            };

            diagnostics.push(Diagnostic {
                code,
                message,
                severity,
                range: finding.range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::test_utils::{
        check_hir_diagnostic, check_metadata_diagnostic, check_metadata_diagnostic_with_fixtures,
    };

    fn form_module_metadata(form_xml: &str) -> hir::ModuleMetadata {
        let form = bsl_metadata::xml_parser::parse_form_xml(form_xml).unwrap();

        hir::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::FormModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            form: Some(Arc::new(form)),
            http_service: None,
            web_service: None,
        }
    }

    #[test]
    fn test_no_diagnostic_without_form_module() {
        let code = r#"
&НаСервере
Процедура СерверныйМетод()
КонецПроцедуры

&НаКлиенте
Процедура ПриАктивизацииСтроки(Элемент)
    СерверныйМетод();
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let server_calls_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ServerCallsInFormEvents)
            .collect();

        assert_eq!(server_calls_diags.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_for_normal_procedure() {
        let code = r#"
&НаСервере
Процедура СерверныйМетод()
КонецПроцедуры

&НаКлиенте
Процедура ОбычнаяПроцедура()
    СерверныйМетод();
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let server_calls_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ServerCallsInFormEvents)
            .collect();

        assert_eq!(server_calls_diags.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_for_client_method_call() {
        let code = r#"
&НаКлиенте
Процедура КлиентскийМетод()
КонецПроцедуры

&НаКлиенте
Процедура ПриАктивизацииСтроки(Элемент)
    КлиентскийМетод();
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let server_calls_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ServerCallsInFormEvents)
            .collect();

        assert_eq!(server_calls_diags.len(), 0);
    }

    #[test]
    fn test_diagnostic_for_indirect_server_call_from_forbidden_form_event() {
        let code = r#"
&НаСервере
Процедура СерверныйМетод()
КонецПроцедуры

&НаКлиенте
Процедура ПромежуточныйКлиентскийМетод()
    СерверныйМетод();
КонецПроцедуры

&НаКлиенте
Процедура СписокПриАктивизацииСтроки(Элемент)
    ПромежуточныйКлиентскийМетод();
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

        let metadata = form_module_metadata(form_xml);
        let diagnostics = check_metadata_diagnostic(metadata, code, |_metadata, ctx| check(ctx));
        let server_calls_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ServerCallsInFormEvents)
            .collect();

        assert_eq!(server_calls_diags.len(), 1);
        assert_eq!(
            server_calls_diags[0].message,
            "В событиях ПриАктивизацииСтроки и НачалоВыбора не должно быть вызовов серверных процедур. Процедура \"СерверныйМетод\" выполняется на сервере"
        );
    }

    #[test]
    fn test_diagnostic_for_immediate_qualified_server_call() {
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Module.bsl
&НаСервере
Процедура СерверныйМетод() Экспорт
КонецПроцедуры

//- /test.bsl
&НаКлиенте
Процедура СписокНачалоВыбора(Элемент, ДанныеВыбора, СтандартнаяОбработка)
    ОбщийМодуль.СерверныйМетод();
КонецПроцедуры
"#;

        let form_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" version="2.20">
    <ChildItems>
        <InputField name="Поле" id="1">
            <Events>
                <Event name="OnStartChoice">СписокНачалоВыбора</Event>
            </Events>
        </InputField>
    </ChildItems>
</Form>"#;

        let metadata = form_module_metadata(form_xml);
        let diagnostics =
            check_metadata_diagnostic_with_fixtures(metadata, fixture, |_metadata, ctx| check(ctx));
        let server_calls_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ServerCallsInFormEvents)
            .collect();

        assert_eq!(server_calls_diags.len(), 1);
        assert!(
            server_calls_diags[0].message.contains("СерверныйМетод"),
            "unexpected message: {}",
            server_calls_diags[0].message
        );
    }

    #[test]
    fn test_no_diagnostic_for_server_no_context_call() {
        let code = r#"
&НаСервереБезКонтекста
Процедура СерверныйМетодБезКонтекста()
КонецПроцедуры

&НаКлиенте
Процедура СписокПриАктивизацииСтроки(Элемент)
    СерверныйМетодБезКонтекста();
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

        let metadata = form_module_metadata(form_xml);
        let diagnostics = check_metadata_diagnostic(metadata, code, |_metadata, ctx| check(ctx));
        let server_calls_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ServerCallsInFormEvents)
            .collect();

        assert_eq!(
            server_calls_diags.len(),
            0,
            "НаСервереБезКонтекста не должен вызывать диагностику"
        );
    }

    #[test]
    fn test_idle_handler_with_server_call_produces_info_diagnostic() {
        let code = r#"
&НаСервере
Процедура СерверныйМетод()
КонецПроцедуры

&НаКлиенте
Процедура ОтложенноеОбновление()
    СерверныйМетод();
КонецПроцедуры

&НаКлиенте
Процедура СписокПриАктивизацииСтроки(Элемент)
    ПодключитьОбработчикОжидания("ОтложенноеОбновление", 0.5, Истина);
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

        let metadata = form_module_metadata(form_xml);
        let diagnostics = check_metadata_diagnostic(metadata, code, |_metadata, ctx| check(ctx));
        let server_calls_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ServerCallsInFormEvents)
            .collect();

        assert_eq!(server_calls_diags.len(), 1, "Should detect server call through idle handler");
        assert_eq!(
            server_calls_diags[0].severity,
            crate::Severity::Information,
            "Severity should be Information for idle handler path"
        );
        assert!(
            server_calls_diags[0].message.contains("обработчике ожидания"),
            "Message should mention idle handler: {}",
            server_calls_diags[0].message
        );
    }

    #[test]
    fn test_handler_lookup_is_case_insensitive() {
        let code = r#"
&НаСервере
Процедура СерверныйМетод()
КонецПроцедуры

&НаКлиенте
Процедура СписокПриАктивизацииСтроки(Элемент)
    СерверныйМетод();
КонецПроцедуры
"#;

        let form_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" version="2.20">
    <ChildItems>
        <Table name="Список" id="1">
            <Events>
                <Event name="OnActivateRow">списокприактивизациистроки</Event>
            </Events>
        </Table>
    </ChildItems>
</Form>"#;

        let metadata = form_module_metadata(form_xml);
        let diagnostics = check_metadata_diagnostic(metadata, code, |_metadata, ctx| check(ctx));

        assert!(
            diagnostics.iter().any(|d| d.code == DiagnosticCode::ServerCallsInFormEvents),
            "expected case-insensitive handler lookup to find the method"
        );
    }
}
