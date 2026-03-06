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

    for (local_id, body) in module_bodies.iter_bodies() {
        let method_name = get_method_name(&item_tree, local_id);

        diagnostics.extend(check_method(
            local_id,
            body,
            method_name.as_deref(),
            &module_bodies,
            code,
            ctx,
        ));
    }

    diagnostics
}

fn get_method_name(item_tree: &hir_def::ItemTree, local_id: u32) -> Option<String> {
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
    body: &hir_def::Body,
    method_name: Option<&str>,
    module_bodies: &hir_def::ModuleBodies,
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

fn collect_used_identifiers(body: &hir_def::Body) -> FxHashSet<String> {
    let mut used = FxHashSet::default();

    for (_, expr) in body.exprs_iter() {
        if let Expr::Path(name) = expr {
            used.insert(name.as_str().to_lowercase());
        }
    }

    used
}

fn is_empty_body(body: &hir_def::Body) -> bool {
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
}
