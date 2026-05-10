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
        check_diagnostics_snapshot_for, check_hir_diagnostic_with_config, format_diags,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NumberOfParams,
            expect![[r#"
            NumberOfParams @ 15:9..15:29
              message: Уменьшите количество параметров c 8 до допустимого 7
              severity: Information"#]],
        );
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
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::NumberOfParams).collect();
        expect![[r#"
            NumberOfParams @ 5:11..5:18
              message: Уменьшите количество параметров c 6 до допустимого 1
              severity: Information
            NumberOfParams @ 10:9..10:16
              message: Уменьшите количество параметров c 7 до допустимого 1
              severity: Information
            NumberOfParams @ 15:9..15:29
              message: Уменьшите количество параметров c 8 до допустимого 1
              severity: Information
            NumberOfParams @ 19:11..19:45
              message: Уменьшите количество параметров c 7 до допустимого 1
              severity: Information
            NumberOfParams @ 23:11..23:52
              message: Уменьшите количество параметров c 7 до допустимого 1
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_at_threshold() {
        let code = r#"Функция Тест(А, Б, В, Г, Д, Е, Ж) Возврат 0; КонецФункции"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::NumberOfParams, expect![[r#""#]]);
    }

    #[test]
    fn test_no_params() {
        let code = r#"Процедура Тест() КонецПроцедуры"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::NumberOfParams, expect![[r#""#]]);
    }

    #[test]
    fn test_multiple_excess_params() {
        // NOTE: Using 2-letter names because single "И" is lexed as KwAnd (logical AND),
        // not as Ident. This is expected BSL behavior - keywords can't be used as identifiers.
        let code = r#"Процедура Тест(Аа, Бб, Вв, Гг, Дд, Ее, Жж, Зз, Ии)
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NumberOfParams,
            expect![[r#"
            NumberOfParams @ 1:11..1:15
              message: Уменьшите количество параметров c 9 до допустимого 7
              severity: Information"#]],
        );
    }
}
