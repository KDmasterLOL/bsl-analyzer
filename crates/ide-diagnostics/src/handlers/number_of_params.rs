//! NumberOfParams diagnostic.
//!
//! Reports methods with too many parameters.
//!
//! ## Track 2 Phase B §6.4 migration
//! Pre-migration this consumed `BodyDiagnostic::NumberOfParams` from
//! `lower::mod::emit_method_scoped_diagnostics`; the migrated handler
//! reads `HirMethodMetrics::params_count` (set by the visitor from
//! `body.params.len()`).

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::ModItem;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_MAX_PARAMS: i64 = 7;

/// Track 2 Phase B §6.4 — handler-side detection consuming the cached
/// `HirMethodMetrics::params_count` via `ctx.module_hir_metrics()`.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::NumberOfParams;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let max_params = ctx.config_int(code, "maxParamsCount", DEFAULT_MAX_PARAMS) as u32;

    let module_metrics = ctx.module_hir_metrics();
    if module_metrics.is_empty() {
        return Vec::new();
    }
    let module_bodies = ctx.module_bodies();
    let item_tree = ctx.item_tree();

    // Sort by `local_id` for deterministic output ordering — see the
    // matching note in `method_size::check`.
    let mut local_ids: Vec<u32> = module_bodies.iter_bodies().map(|(id, _)| id).collect();
    local_ids.sort_unstable();

    let mut out = Vec::new();
    for local_id in local_ids {
        let Some(metrics) = module_metrics.get(local_id) else { continue };
        if metrics.params_count <= max_params {
            continue;
        }
        let Some(item) = item_tree.top_level_items().get(local_id as usize) else { continue };
        let name_range = match item {
            ModItem::Procedure(idx) => item_tree.procedure(*idx).name_range,
            ModItem::Function(idx) => item_tree.function(*idx).name_range,
            ModItem::Variable(_) => continue,
        };
        out.push(Diagnostic {
            code,
            message: format!(
                "Уменьшите количество параметров c {} до допустимого {}",
                metrics.params_count, max_params
            ),
            severity: ctx.severity(code),
            range: name_range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        assert_diagnostic_range, check_hir_diagnostic, check_hir_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig, Severity};
    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура МимоРаз()

КонецПроцедуры

Процедура МимоДва(Раз, Два, Три, Четыре, Пять, Шесть)

КонецПроцедуры


Функция МимоТри(Раз, Два, Три, Четыре, Пять = 5, Шесть = 6, Семь = 7)
    Возврат;
КонецФункции


Функция СработкаПоКоличеству(Раз, Два, Три, Четыре, Пять, Шесть, Семь, Восемь)
    Возврат;
КонецФункции

Процедура СработкаПоКоличествуНеобязательных(Раз, Два, Три, Четыре = 4, Пять = 5, Шесть = 6, Семь = 7)

КонецПроцедуры

Процедура СработкаПоНеобязательныйПередОбязательным(Раз, Два = 2, Три, Четыре, Пять, Шесть, Семь)

КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NumberOfParams).collect();

        // Only "СработкаПоКоличеству" has 8 params (exceeds 7)
        // HIR produces 1 diagnostic per method at method name range
        assert_eq!(diagnostics.len(), 1);
        // "Функция " = 8 chars → name at col 8, "СработкаПоКоличеству" = 20 chars → end 28
        assert_diagnostic_range(code, diagnostics[0], 14, 8, 28);
        assert_eq!(diagnostics[0].code, DiagnosticCode::NumberOfParams);
        // CodeSmell + Minor → Information (per metadata mapping)
        assert_eq!(diagnostics[0].severity, Severity::Information);
    }

    #[test]
    fn test_custom_threshold() {
        let code = r#"Процедура МимоРаз()

КонецПроцедуры

Процедура МимоДва(Раз, Два, Три, Четыре, Пять, Шесть)

КонецПроцедуры


Функция МимоТри(Раз, Два, Три, Четыре, Пять = 5, Шесть = 6, Семь = 7)
    Возврат;
КонецФункции


Функция СработкаПоКоличеству(Раз, Два, Три, Четыре, Пять, Шесть, Семь, Восемь)
    Возврат;
КонецФункции

Процедура СработкаПоКоличествуНеобязательных(Раз, Два, Три, Четыре = 4, Пять = 5, Шесть = 6, Семь = 7)

КонецПроцедуры

Процедура СработкаПоНеобязательныйПередОбязательным(Раз, Два = 2, Три, Четыре, Пять, Шесть, Семь)

КонецПроцедуры"#;
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::NumberOfParams, serde_json::json!({ "maxParamsCount": 1 }));
        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NumberOfParams).collect();
        // HIR produces 1 diagnostic per method:
        // МимоДва, МимоТри, СработкаПоКоличеству,
        // СработкаПоКоличествуНеобязательных, СработкаПоНеобязательныйПередОбязательным
        assert_eq!(diagnostics.len(), 5);
    }

    #[test]
    fn test_at_threshold() {
        let code = r#"Функция Тест(А, Б, В, Г, Д, Е, Ж) Возврат 0; КонецФункции"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NumberOfParams).collect();
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_params() {
        let code = r#"Процедура Тест() КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NumberOfParams).collect();
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_multiple_excess_params() {
        // NOTE: Using 2-letter names because single "И" is lexed as KwAnd (logical AND),
        // not as Ident. This is expected BSL behavior - keywords can't be used as identifiers.
        let code = r#"Процедура Тест(Аа, Бб, Вв, Гг, Дд, Ее, Жж, Зз, Ии)
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NumberOfParams).collect();
        // HIR produces 1 diagnostic per method at method name range
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, diagnostics[0], 0, 10, 14); // Тест
    }
}
