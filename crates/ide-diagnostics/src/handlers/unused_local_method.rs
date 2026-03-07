//! UnusedLocalMethod diagnostic.
//!
//! Detects local methods (procedures/functions) that are declared but never called.
//!
//! ## Why?
//! Unused methods:
//! - Increase code complexity and maintenance burden
//! - May indicate incomplete refactoring
//! - Clutter the codebase
//!
//! ## Excluded from check
//! - Exported methods (Экспорт)
//! - Extension methods (@Перед/@До, @После, @Вместо, @ИзменениеИКонтроль)
//! - Attachable methods (configurable prefix, default: подключаемый_, attachable_)
//! - Platform event handlers (fixed signature defined by 1C platform)
//!
//! ## Configuration
//! - **attachableMethodPrefixes** (string, default: "подключаемый_,attachable_") - comma-separated prefixes
//! - **checkObjectModule** (boolean, default: false) - check ObjectModule type
//!
//! ## Implementation

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
    modules: &[bsl_metadata::ModuleType::CommonModule, bsl_metadata::ModuleType::ObjectModule],
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
    let module_bodies = ctx.module_bodies();

    let mut called_methods: FxHashSet<String> = FxHashSet::default();

    for (_, body) in module_bodies.iter_bodies() {
        collect_method_calls(body, &mut called_methods);
    }

    if let Some(module_code) = module_bodies.module_code_result() {
        collect_method_calls(&module_code.body, &mut called_methods);
    }

    // Add form event and command handlers as "called" methods
    // These are called by the platform, not by code
    if let Some(ref form) = metadata.form {
        for handler in form.event_handlers() {
            called_methods.insert(handler.to_lowercase());
        }
        for handler in form.command_handlers() {
            called_methods.insert(handler.to_lowercase());
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

fn collect_method_calls(body: &hir::Body, called_methods: &mut FxHashSet<String>) {
    use cfg_types::IdConversion;

    for (_expr_id, expr) in body.exprs_iter() {
        match expr {
            hir::Expr::Call { callee, .. } => {
                let callee_opaque = cfg_types::ExprId::from_idx(*callee);
                if let hir::Expr::Path(name) = body.expr(callee_opaque) {
                    called_methods.insert(name.as_str().to_lowercase());
                }
            }
            hir::Expr::MethodCall { method, .. } => {
                called_methods.insert(method.as_str().to_lowercase());
            }
            _ => {}
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
        assert_diagnostic_range, check_hir_diagnostic, check_hir_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_java_fixture() {
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

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        assert_eq!(
            unused_diags.len(),
            2,
            "Expected 2 diagnostics, got {}. Diagnostics: {:?}",
            unused_diags.len(),
            unused_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        assert_diagnostic_range(code, unused_diags[0], 1, 10, 24);
        assert_diagnostic_range(code, unused_diags[1], 70, 10, 41);
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

        assert_diagnostic_range(code, unused_diags[0], 1, 10, 24);
        assert_diagnostic_range(code, unused_diags[1], 60, 10, 40);
        assert_diagnostic_range(code, unused_diags[2], 63, 10, 39);
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

        assert_eq!(unused_diags.len(), 1);
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

        assert_eq!(unused_diags.len(), 1);
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

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        assert_eq!(unused_diags.len(), 0);
    }

    #[test]
    fn test_attachable_methods_not_flagged() {
        let code = r#"
Процедура Подключаемый_ОбработчикСобытия()
КонецПроцедуры

Процедура Attachable_EventHandler()
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        assert_eq!(unused_diags.len(), 0);
    }

    #[test]
    fn test_handler_methods_not_flagged() {
        let code = r#"
Процедура ПриСозданииОбъекта(Параметр)
КонецПроцедуры

Процедура OnObjectCreate(Parameter)
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        assert_eq!(unused_diags.len(), 0);
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

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        assert_eq!(unused_diags.len(), 0);
    }

    #[test]
    fn test_method_called_in_module_code() {
        let code = r#"
Процедура ВызываемаяПроцедура()
КонецПроцедуры

ВызываемаяПроцедура();
"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalMethod).collect();

        assert_eq!(unused_diags.len(), 0);
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

        assert_eq!(unused_diags.len(), 1);
        assert!(unused_diags[0].message.contains("Главная"));
    }
}
