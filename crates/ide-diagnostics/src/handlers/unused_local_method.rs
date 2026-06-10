use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::AnnotationKind;
use ide_db::TextRange;
use rustc_hash::FxHashSet;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[bsl_metadata::ModuleType::CommonModule, bsl_metadata::ModuleType::ObjectModule, bsl_metadata::ModuleType::HTTPServiceModule, bsl_metadata::ModuleType::WebServiceModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Suspicious, MetadataTag::Unused],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_ATTACHABLE_PREFIXES: &str = "подключаемый_,attachable_";

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UnusedLocalMethod;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let attachable_prefixes_str = ctx
        .config
        .get_string(code, "attachableMethodPrefixes")
        .unwrap_or(DEFAULT_ATTACHABLE_PREFIXES);

    let attachable_prefixes: Vec<String> = attachable_prefixes_str
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let check_object_module = ctx.config.get_bool(code, "checkObjectModule").unwrap_or(false);

    let metadata = ctx.module_metadata();

    if !check_object_module && metadata.module_type == bsl_metadata::ModuleType::ObjectModule {
        return Vec::new();
    }

    let item_tree = ctx.item_tree();

    let module_id = hir::ModuleId::new(ctx.file_id);
    let summary = ctx.call_summary(module_id);

    let mut called_methods: FxHashSet<String> = FxHashSet::default();

    for edge in &summary.call_edges {
        if let hir::call_graph::CallTarget::Local { callee_local_id } = &edge.target {
            if let Some(method) = summary.methods.iter().find(|m| m.local_id == *callee_local_id) {
                called_methods.insert(method.name.as_str().to_lowercase());
            }
        }
    }

    let module_bodies = ctx.module_bodies();
    for (_, body) in module_bodies.iter_bodies() {
        collect_method_call_names(body, &mut called_methods);
    }
    if let Some(module_code) = module_bodies.module_code_result() {
        collect_method_call_names(&module_code.body, &mut called_methods);
    }

    if let Some(ref form) = metadata.form {
        for handler in form.event_handlers() {
            called_methods.insert(handler.handler_name.to_lowercase());
        }
        for handler in form.command_handlers() {
            called_methods.insert(handler.to_lowercase());
        }
    }

    if let Some(ref http_service) = metadata.http_service {
        for (_template, method) in http_service.all_methods() {
            if !method.is_handler_empty() {
                called_methods.insert(method.handler().to_lowercase());
            }
        }
    }

    if let Some(ref web_service) = metadata.web_service {
        for operation in web_service.operations() {
            if !operation.is_handler_empty() {
                called_methods.insert(operation.procedure_name().to_lowercase());
            }
        }
    }

    let mut diagnostics = Vec::new();

    for (_, proc) in item_tree.procedures() {
        if let Some(diag) = check_method_unused(
            &proc.name,
            proc.name_range,
            proc.is_export,
            &proc.annotations,
            &attachable_prefixes,
            &called_methods,
            code,
            ctx,
        ) {
            diagnostics.push(diag);
        }
    }

    for (_, func) in item_tree.functions() {
        if let Some(diag) = check_method_unused(
            &func.name,
            func.name_range,
            func.is_export,
            &func.annotations,
            &attachable_prefixes,
            &called_methods,
            code,
            ctx,
        ) {
            diagnostics.push(diag);
        }
    }

    diagnostics
}

fn collect_method_call_names(body: &hir::Body, called_methods: &mut FxHashSet<String>) {
    for (_, expr) in body.exprs_iter() {
        if let hir::Expr::MethodCall { method, .. } = expr {
            called_methods.insert(method.as_str().to_lowercase());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_method_unused(
    name: &hir::Name,
    name_range: TextRange,
    is_export: bool,
    annotations: &[hir::Annotation],
    attachable_prefixes: &[String],
    called_methods: &FxHashSet<String>,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    if is_export {
        return None;
    }

    if has_extension_annotation(annotations) {
        return None;
    }

    let name_lower = name.as_str().to_lowercase();

    if is_attachable_method(&name_lower, attachable_prefixes) {
        return None;
    }

    if is_handler_method(&name_lower) {
        return None;
    }

    if called_methods.contains(&name_lower) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!("Неиспользуемый локальный метод \"{}\"", name.as_str()),
        severity: ctx.severity(code),
        range: name_range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

fn has_extension_annotation(annotations: &[hir::Annotation]) -> bool {
    annotations.iter().any(|ann| {
        matches!(
            ann.kind,
            AnnotationKind::Before
                | AnnotationKind::After
                | AnnotationKind::Instead
                | AnnotationKind::ChangeAndValidate
        )
    })
}

fn is_attachable_method(name_lower: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|prefix| name_lower.starts_with(prefix))
}

fn is_handler_method(name_lower: &str) -> bool {
    crate::utils::platform_event_handlers::is_platform_event_handler(name_lower)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        check_diagnostics_snapshot_for, check_hir_diagnostic, check_hir_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;

    #[test]
    fn test_call_inside_dot_function_condition_is_counted() {
        let code = r#"
Процедура ВывестиКолонкуФункция()
КонецПроцедуры

Процедура Главная(ПараметрыПечати)
    Если ПараметрыПечати.Функция Тогда
        ВывестиКолонкуФункция();
    КонецЕсли;
КонецПроцедуры

Главная(Неопределено);
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UnusedLocalMethod, expect![[r#""#]]);
    }

    #[test]
    fn test_detects_unused_local_methods() {
        let code = r#"
Процедура НеИспользуется() // Тут
КонецПроцедуры



&Вместо("ИспользуетсяВРасширенииВместо")
Функция Расш_ИспользуетсяВРасширенииВместо()
КонецФункции

&Перед("ИспользуетсяВРасширенииПеред")
Функция Расш_ИспользуетсяВРасширенииПеред()
КонецФункции

&После("ИспользуетсяВРасширенииПосле")
Функция Расш_ИспользуетсяВРасширенииПосле()
КонецФункции

&ИзменениеИКонтроль("ИспользуетсяВРасширенииИзменениеИКонтроль")
Функция Расш_ИспользуетсяВРасширенииИзменениеИКонтроль()
КонецФункции

Процедура НеИспользуетсяЭкспорт() Экспорт
КонецПроцедуры

Процедура ИспользуетсяВОсновномТеле()
КонецПроцедуры

Процедура ИспользуетсяВМетоде()
КонецПроцедуры

Процедура ИспользуетсяВУсловии()
КонецПроцедуры

Функция ИспользуетсяВПрисвоении()
КонецФункции

Функция ИспользуетсяВПарметре()
КонецФункции

Функция ИспользуетсяВПарметреПриПрисвоении()
КонецФункции

Функция СВызовами(Параметры)

	ИспользуетсяВМетоде();
	B = ИспользуетсяВПрисвоении();

КонецФункции

Функция СВызовами2()

	Если ИспользуетсяВУсловии() Тогда
	КонецЕсли;

	ГлобальныйМетод(ИспользуетсяВПарметре());
	А = СВызовами(ИспользуетсяВПарметреПриПрисвоении());

КонецФункции

Процедура Подключаемый_КакойтоОбработчик()
КонецПроцедуры

Процедура Attachable__КакойтоОбработчик()
КонецПроцедуры

Процедура ПриСозданииОбъекта(Параметр1)

КонецПроцедуры

Процедура ПодключаемаяМоя_НужнаяПроцедура()
КонецПроцедуры

ИспользуетсяВОсновномТеле();
СВызовами();
СВызовами2();
"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnusedLocalMethod,
            expect![[r#"
            UnusedLocalMethod @ 2:11..2:25
              message: Неиспользуемый локальный метод "НеИспользуется"
              severity: Warning
            UnusedLocalMethod @ 71:11..71:42
              message: Неиспользуемый локальный метод "ПодключаемаяМоя_НужнаяПроцедура"
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_configure_prefixes() {
        let code = r#"
Процедура НеИспользуется() // Тут
КонецПроцедуры



&Вместо("ИспользуетсяВРасширенииВместо")
Функция Расш_ИспользуетсяВРасширенииВместо()
КонецФункции

&Перед("ИспользуетсяВРасширенииПеред")
Функция Расш_ИспользуетсяВРасширенииПеред()
КонецФункции

&После("ИспользуетсяВРасширенииПосле")
Функция Расш_ИспользуетсяВРасширенииПосле()
КонецФункции

&ИзменениеИКонтроль("ИспользуетсяВРасширенииИзменениеИКонтроль")
Функция Расш_ИспользуетсяВРасширенииИзменениеИКонтроль()
КонецФункции

Процедура НеИспользуетсяЭкспорт() Экспорт
КонецПроцедуры

Процедура ИспользуетсяВОсновномТеле()
КонецПроцедуры

Процедура ИспользуетсяВМетоде()
КонецПроцедуры

Процедура ИспользуетсяВУсловии()
КонецПроцедуры

Функция ИспользуетсяВПрисвоении()
КонецФункции

Функция ИспользуетсяВПарметре()
КонецФункции

Функция ИспользуетсяВПарметреПриПрисвоении()
КонецФункции

Функция СВызовами(Параметры)

	ИспользуетсяВМетоде();
	B = ИспользуетсяВПрисвоении();

КонецФункции

Функция СВызовами2()

	Если ИспользуетсяВУсловии() Тогда
	КонецЕсли;

	ГлобальныйМетод(ИспользуетсяВПарметре());
	А = СВызовами(ИспользуетсяВПарметреПриПрисвоении());

КонецФункции

Процедура Подключаемый_КакойтоОбработчик()
КонецПроцедуры

Процедура Attachable__КакойтоОбработчик()
КонецПроцедуры

Процедура ПриСозданииОбъекта(Параметр1)

КонецПроцедуры

Процедура ПодключаемаяМоя_НужнаяПроцедура()
КонецПроцедуры

ИспользуетсяВОсновномТеле();
СВызовами();
СВызовами2();
"#;

        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert(
            "attachableMethodPrefixes".to_string(),
            serde_json::Value::String("ПодключаемаяМоя_".to_string()),
        );
        config
            .parameters
            .insert(DiagnosticCode::UnusedLocalMethod, serde_json::Value::Object(params));

        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        assert_eq!(
            unused_diags.len(),
            3,
            "Expected 3 diagnostics with custom prefixes, got {}",
            unused_diags.len()
        );

        crate::test_utils::assert_diagnostic_range(code, unused_diags[0], 1, 10, 24);
        crate::test_utils::assert_diagnostic_range(code, unused_diags[1], 60, 10, 40);
        crate::test_utils::assert_diagnostic_range(code, unused_diags[2], 63, 10, 39);
    }

    #[test]
    fn test_exported_method_not_flagged() {
        let code = r#"
Процедура ПубличнаяПроцедура() Экспорт
КонецПроцедуры

Процедура ЛокальнаяПроцедура()
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnusedLocalMethod,
            expect![[r#"
            UnusedLocalMethod @ 5:11..5:29
              message: Неиспользуемый локальный метод "ЛокальнаяПроцедура"
              severity: Warning"#]],
        );
        assert!(unused_diags[0].message.contains("ЛокальнаяПроцедура"));
    }

    #[test]
    fn test_used_method_not_flagged() {
        let code = r#"
Процедура ИспользуемаяПроцедура()
КонецПроцедуры

Процедура Главная()
    ИспользуемаяПроцедура();
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnusedLocalMethod,
            expect![[r#"
            UnusedLocalMethod @ 5:11..5:18
              message: Неиспользуемый локальный метод "Главная"
              severity: Warning"#]],
        );
        assert!(unused_diags[0].message.contains("Главная"));
    }

    #[test]
    fn test_extension_annotations_not_flagged() {
        let code = r#"
&Вместо("ОригинальныйМетод")
Процедура Расш_ОригинальныйМетод()
КонецПроцедуры

&Перед("ДругойМетод")
Процедура Расш_ДругойМетод()
КонецПроцедуры

&После("ТретийМетод")
Процедура Расш_ТретийМетод()
КонецПроцедуры

&ИзменениеИКонтроль("ЧетвертыйМетод")
Процедура Расш_ЧетвертыйМетод()
КонецПроцедуры
"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::UnusedLocalMethod, expect![[r#""#]]);
    }

    #[test]
    fn test_attachable_methods_not_flagged() {
        let code = r#"
Процедура Подключаемый_ОбработчикСобытия()
КонецПроцедуры

Процедура Attachable_EventHandler()
КонецПроцедуры
"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::UnusedLocalMethod, expect![[r#""#]]);
    }

    #[test]
    fn test_handler_methods_not_flagged() {
        let code = r#"
Процедура ПриСозданииОбъекта(Параметр)
КонецПроцедуры

Процедура OnObjectCreate(Parameter)
КонецПроцедуры
"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::UnusedLocalMethod, expect![[r#""#]]);
    }

    #[test]
    fn test_platform_event_handlers_not_flagged() {
        let code = r#"
Процедура ПередЗаписью(Отказ)
КонецПроцедуры

Процедура ПриЗаписи(Отказ)
КонецПроцедуры

Процедура ПередУдалением(Отказ)
КонецПроцедуры

Процедура ОбработкаЗаполнения(ДанныеЗаполнения)
КонецПроцедуры
"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::UnusedLocalMethod, expect![[r#""#]]);
    }

    #[test]
    fn test_method_called_in_module_code() {
        let code = r#"
Процедура ВызываемаяПроцедура()
КонецПроцедуры

ВызываемаяПроцедура();
"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::UnusedLocalMethod, expect![[r#""#]]);
    }

    #[test]
    fn test_http_service_handler_not_flagged() {
        use std::sync::Arc;

        let code = r#"
Функция createPOST(Запрос)
    Возврат Запрос;
КонецФункции

Функция НеИспользуемая()
    Возврат 1;
КонецФункции
"#;

        let method = bsl_metadata::HTTPServiceMethodBuilder::new()
            .name("POST")
            .http_method("POST")
            .handler("createPOST")
            .build();
        let template = bsl_metadata::HTTPServiceURLTemplateBuilder::new()
            .name("create")
            .template("/client/create")
            .add_method(method)
            .build();
        let http_service =
            bsl_metadata::HTTPServiceBuilder::new().name("lk").add_url_template(template).build();

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
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        assert_eq!(unused_diags.len(), 1);
        assert!(unused_diags[0].message.contains("НеИспользуемая"));
    }

    #[test]
    fn test_web_service_operation_handler_not_flagged() {
        use std::sync::Arc;

        let code = r#"
Функция GetJobsCount(СтрокаПараметров)
    Возврат 0;
КонецФункции

Функция НеИспользуемая()
    Возврат 1;
КонецФункции
"#;

        let operation = bsl_metadata::WebServiceOperationBuilder::new()
            .name("GetJobsCount")
            .procedure_name("GetJobsCount")
            .build();
        let web_service = bsl_metadata::WebServiceBuilder::new()
            .name("WMSMobileClientExchange")
            .add_operation(operation)
            .build();

        let metadata = hir::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::WebServiceModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            form: None,
            http_service: None,
            web_service: Some(Arc::new(web_service)),
        };

        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, code, |_metadata, ctx| {
                super::check(ctx)
            });
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        assert_eq!(
            unused_diags.len(),
            1,
            "operation-bound method must be exempt, unbound one must stay flagged: {unused_diags:?}"
        );
        assert!(unused_diags[0].message.contains("НеИспользуемая"));
    }

    #[test]
    fn test_form_element_event_handler_not_flagged() {
        use std::sync::Arc;

        let code = r#"
&НаКлиенте
Процедура СписокПриАктивизацииСтроки(Элемент)
КонецПроцедуры

&НаСервере
Процедура ПриСозданииНаСервере(Отказ, СтандартнаяОбработка)
КонецПроцедуры

Процедура НеИспользуемая()
КонецПроцедуры
"#;

        let form_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" version="2.20">
    <Events>
        <Event name="OnCreateAtServer">ПриСозданииНаСервере</Event>
    </Events>
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
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        assert_eq!(
            unused_diags.len(),
            1,
            "Expected 1 diagnostic, got {}. Diagnostics: {:?}",
            unused_diags.len(),
            unused_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        assert!(unused_diags[0].message.contains("НеИспользуемая"));
    }

    #[test]
    fn test_case_insensitive_call() {
        let code = r#"
Процедура МояПроцедура()
КонецПроцедуры

Процедура Главная()
    МОЯПРОЦЕДУРА();
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnusedLocalMethod,
            expect![[r#"
            UnusedLocalMethod @ 5:11..5:18
              message: Неиспользуемый локальный метод "Главная"
              severity: Warning"#]],
        );
        assert!(unused_diags[0].message.contains("Главная"));
    }
}
