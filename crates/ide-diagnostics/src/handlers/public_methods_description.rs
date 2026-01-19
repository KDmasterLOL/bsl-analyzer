//! PublicMethodsDescription diagnostic.
//!
//! Checks that all export methods in API regions have descriptions (comments before method).
//!
//! ## Configuration
//!
//! - `checkAllRegion` (boolean, default: false)
//!   - `true`: Check all export methods regardless of region
//!   - `false`: Only check export methods in API regions (ПрограммныйИнтерфейс/Public)
//!
//! ## API Regions
//!
//! Unlike `is_api_region_name()` which includes Internal/СлужебныйПрограммныйИнтерфейс,
//! this diagnostic only checks:
//! - Russian: ПрограммныйИнтерфейс
//! - English: Public
//!
//! ## Examples
//!
//! ### Bad practice (export function without description in API region)
//! ```bsl
//! #Область ПрограммныйИнтерфейс
//!
//! Функция БезОписания() Экспорт
//!     Возврат Неопределено;
//! КонецФункции
//!
//! #КонецОбласти
//! ```
//!
//! ### Good practice
//! ```bsl
//! #Область ПрограммныйИнтерфейс
//!
//! // Возвращает значение по умолчанию.
//! //
//! // Возвращаемое значение:
//! //   Неопределено
//! //
//! Функция ЗначениеПоУмолчанию() Экспорт
//!     Возврат Неопределено;
//! КонецФункции
//!
//! #КонецОбласти
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir_def::item_tree::ModItem;
use hir_def::region_tree::RegionTree;
use hir_def::MethodId;
use ide_db::TextRange;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::PublicMethodsDescription) {
        return Vec::new();
    }

    let check_all_region = ctx
        .config
        .get_bool(DiagnosticCode::PublicMethodsDescription, "checkAllRegion")
        .unwrap_or(false);

    let region_tree = ctx.region_tree();
    let item_tree = ctx.item_tree();
    let module_data = ctx.module_data();

    let mut diagnostics = Vec::new();

    for method_id in module_data.procedures.iter().chain(module_data.functions.iter()) {
        if let Some(diag) =
            check_method(ctx, *method_id, &item_tree, &region_tree, check_all_region)
        {
            diagnostics.push(diag);
        }
    }

    diagnostics
}

fn check_method(
    ctx: &DiagnosticsContext,
    method_id: MethodId,
    item_tree: &hir_def::ItemTree,
    region_tree: &RegionTree,
    check_all_region: bool,
) -> Option<Diagnostic> {
    let (is_export, name_range, source_range) = item_tree
        .top_level_items()
        .get(method_id.local_id as usize)
        .and_then(|item| match item {
            ModItem::Function(func_idx) => {
                let func = item_tree.function(*func_idx);
                Some((func.is_export, func.name_range, func.source_range))
            }
            ModItem::Procedure(proc_idx) => {
                let proc = item_tree.procedure(*proc_idx);
                Some((proc.is_export, proc.name_range, proc.source_range))
            }
            _ => None,
        })?;

    if !is_export {
        return None;
    }

    let docs = ctx.method_docs(method_id);
    let has_description = docs.as_ref().is_some_and(|d| !d.raw.is_empty());

    if has_description {
        return None;
    }

    if check_all_region {
        return Some(create_diagnostic(name_range));
    }

    let region_idx = region_tree.region_containing(source_range)?;
    let root_idx = region_tree.root_ancestor(region_idx);
    let root_region = region_tree.region(root_idx);

    if is_public_api_region(root_region.name.as_str()) {
        return Some(create_diagnostic(name_range));
    }

    None
}

fn is_public_api_region(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "программныйинтерфейс" || lower == "public"
}

fn create_diagnostic(range: TextRange) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::PublicMethodsDescription,
        message: "Добавьте описание метода программного интерфейса".to_string(),
        severity: Severity::Information,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};

    #[test]
    fn test_default_mode() {
        let code = include_str!("../../test_data/PublicMethodsDescriptionDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 2, "Expected 2 diagnostics in default mode");

        assert_diagnostic_range(code, &diagnostics[0], 41, 8, 25);
        assert_diagnostic_range(code, &diagnostics[1], 55, 8, 25);
    }

    #[test]
    fn test_check_all_region() {
        let code = include_str!("../../test_data/PublicMethodsDescriptionDiagnostic.bsl");

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::PublicMethodsDescription,
            serde_json::json!({"checkAllRegion": true}),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert_eq!(diagnostics.len(), 3, "Expected 3 diagnostics with checkAllRegion=true");

        assert_diagnostic_range(code, &diagnostics[0], 41, 8, 25);
        assert_diagnostic_range(code, &diagnostics[1], 55, 8, 25);
        assert_diagnostic_range(code, &diagnostics[2], 103, 8, 25);
    }

    #[test]
    fn test_method_with_description() {
        let code = r#"
#Область ПрограммныйИнтерфейс

// Описание метода
Функция СОписанием() Экспорт
    Возврат Неопределено;
КонецФункции

#КонецОбласти
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Method with description should not trigger");
    }

    #[test]
    fn test_non_export_method() {
        let code = r#"
#Область ПрограммныйИнтерфейс

Функция НеЭкспортная()
    Возврат Неопределено;
КонецФункции

#КонецОбласти
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Non-export method should not trigger");
    }

    #[test]
    fn test_export_outside_api_region() {
        let code = r#"
#Область СлужебныеПроцедурыИФункции

Функция СлужебнаяЭкспортная() Экспорт
    Возврат Неопределено;
КонецФункции

#КонецОбласти
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Export method outside API region should not trigger in default mode"
        );
    }

    #[test]
    fn test_export_outside_api_region_with_check_all() {
        let code = r#"
#Область СлужебныеПроцедурыИФункции

Функция СлужебнаяЭкспортная() Экспорт
    Возврат Неопределено;
КонецФункции

#КонецОбласти
"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::PublicMethodsDescription,
            serde_json::json!({"checkAllRegion": true}),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "Export method outside API region should trigger with checkAllRegion=true"
        );
    }

    #[test]
    fn test_nested_region() {
        let code = r#"
#Область ПрограммныйИнтерфейс

#Область Вложенная

Функция БезОписания() Экспорт
    Возврат Неопределено;
КонецФункции

#КонецОбласти

#КонецОбласти
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "Export method in nested region inside API region should trigger"
        );
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
#Region Public

Function NoDescription() Export
    Return Undefined;
EndFunction

#EndRegion
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "English keywords should work");
    }

    #[test]
    fn test_internal_region_not_checked() {
        let code = r#"
#Region Internal

Function NoDescription() Export
    Return Undefined;
EndFunction

#EndRegion
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Internal region should not be checked (only Public/ПрограммныйИнтерфейс)"
        );
    }

    #[test]
    fn test_service_api_region_not_checked() {
        let code = r#"
#Область СлужебныйПрограммныйИнтерфейс

Функция БезОписания() Экспорт
    Возврат Неопределено;
КонецФункции

#КонецОбласти
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "СлужебныйПрограммныйИнтерфейс region should not be checked"
        );
    }
}
