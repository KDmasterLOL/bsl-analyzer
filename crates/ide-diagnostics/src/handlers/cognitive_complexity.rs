//! CognitiveComplexity diagnostic.
//!
//! Detects functions and procedures with high cognitive complexity.
//!
//! ## Why?
//! Cognitive complexity measures how difficult code is to understand for humans.
//! Unlike cyclomatic complexity, it penalizes nested structures more heavily,
//! better reflecting the actual mental effort required to comprehend code.
//!
//! High cognitive complexity makes code harder to:
//! - Understand and maintain
//! - Test thoroughly
//! - Debug when issues arise
//! - Modify safely without introducing bugs
//!
//! ## Algorithm
//! Based on SonarSource Cognitive Complexity specification v1.4:
//!
//! **Structural increment** (if, for, while, foreach, except, ternary):
//! - Add: 1 + current_nesting_level
//! - Then increase nesting for children
//!
//! **Hybrid increment** (elsif, else):
//! - Add: 1 (no nesting penalty on the keyword itself)
//! - But increase nesting for children
//!
//! **Fundamental increment** (goto, AND/OR operators):
//! - Add: 1 per construct (no nesting, no nesting increase)
//!
//! ## Bad practice
//! Deeply nested code with multiple decision points:
//! ```bsl
//! Функция ОбработатьДанные(Данные)
//!     Если ТипЗнч(Данные) = Тип("Массив") Тогда           // +1
//!         Для Каждого Элемент Из Данные Цикл             // +2 (1 + nesting)
//!             Если Элемент.Активен Тогда                 // +3 (1 + nesting)
//!                 Для Каждого Поле Из Элемент Цикл      // +4 (1 + nesting)
//!                     Если Поле.Значение <> 0 Тогда     // +5 (1 + nesting)
//!                         // Обработка
//!                     КонецЕсли;
//!                 КонецЦикла;
//!             КонецЕсли;
//!         КонецЦикла;
//!     КонецЕсли;
//! КонецФункции
//! // Total complexity: 15 (at threshold)
//! ```
//!
//! ## Good practice
//! Extract nested logic into separate functions with clear names:
//! ```bsl
//! Функция ОбработатьДанные(Данные)
//!     Если ТипЗнч(Данные) <> Тип("Массив") Тогда
//!         Возврат;
//!     КонецЕсли;
//!
//!     Для Каждого Элемент Из Данные Цикл
//!         ОбработатьЭлемент(Элемент);
//!     КонецЦикла;
//! КонецФункции
//!
//! Функция ОбработатьЭлемент(Элемент)
//!     Если НЕ Элемент.Активен Тогда
//!         Возврат;
//!     КонецЕсли;
//!
//!     Для Каждого Поле Из Элемент Цикл
//!         ОбработатьПоле(Поле);
//!     КонецЦикла;
//! КонецФункции
//! ```
//!
//! ## Configuration
//! - **complexityThreshold** (default: 15) - Maximum allowed cognitive complexity
//! - **Enabled by default:** Yes
//! - **Severity:** Warning
//! - **Tags:** BRAINOVERLOAD
//! - **Minutes to fix:** 15
//!
//! ## Implementation
//! Uses HIR-based complexity calculation for:
//! - Better performance (Salsa caching)
//! - Cleaner code (structured HIR vs raw AST)
//! - Reusability (same calculation for code lens)

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{MethodId, ModItem, ModuleId};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 1.0,
    lsp_severity_override: "",
};

/// Track 2 Phase B §6.4 — handler-side detection consuming the cached
/// [`hir::metrics::HirMethodMetrics::cognitive`] field via
/// `ctx.module_hir_metrics()`. Replaces the legacy `from_hir` adapter
/// (BodyDiagnostic-fed) and the per-method HIR walk in
/// `hir-def/cognitive_complexity.rs`.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CognitiveComplexity;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let threshold = ctx.config_int(code, "complexityThreshold", 15) as u32;
    let module_metrics = ctx.module_hir_metrics();
    if module_metrics.is_empty() {
        return Vec::new();
    }
    let module_bodies = ctx.module_bodies();
    let item_tree = ctx.item_tree();
    let module_id = ModuleId::new(ctx.file_id);

    // Sort by `local_id` for deterministic output ordering — matches
    // the §6.4 cohort follow-up applied to method_size etc.
    let mut local_ids: Vec<u32> = module_bodies.iter_bodies().map(|(id, _)| id).collect();
    local_ids.sort_unstable();

    let mut out = Vec::new();
    for local_id in local_ids {
        let Some(metrics) = module_metrics.get(local_id) else { continue };
        // Track 2 Phase B §6.5: SonarSource Cognitive Complexity v1.4
        // recursion penalty — `+1` when the method is self-recursive
        // or part of a recursion cycle (intra-module SCC). Sourced
        // from `EffectSummary::is_recursive` (§1.4) so the penalty
        // tracks the same call-graph fixed-point the security-state
        // handlers consume.
        let method_id = MethodId { module: module_id, local_id };
        let effect = ctx.method_effect_summary(method_id);
        let recursion_bonus = if effect.is_recursive { 1 } else { 0 };
        let total = metrics.cognitive + recursion_bonus;
        if total <= threshold {
            continue;
        }
        let Some(item) = item_tree.top_level_items().get(local_id as usize) else { continue };
        let (name, name_range, is_function) = match item {
            ModItem::Procedure(idx) => {
                let p = item_tree.procedure(*idx);
                (p.name.as_str().to_string(), p.name_range, false)
            }
            ModItem::Function(idx) => {
                let f = item_tree.function(*idx);
                (f.name.as_str().to_string(), f.name_range, true)
            }
            ModItem::Variable(_) => continue,
        };
        let method_type = if is_function { "Функция" } else { "Процедура" };
        out.push(Diagnostic {
            code,
            message: format!(
                "{} '{}' имеет когнитивную сложность {} (максимум: {}). \
                 Упростите логику или уменьшите вложенность",
                method_type, name, total, threshold
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
    use super::*;
    use crate::test_utils::{
        assert_diagnostic_range, check_hir_diagnostic, check_hir_diagnostic_with_config,
    };
    use crate::{DiagnosticsConfig, Severity};
    use hir::ModuleId;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::vfs::{FileSet, VfsPath};
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;
    #[test]
    fn test_simple_function() {
        let code = r#"Функция ПростаяФункция(Параметр)
    Возврат Параметр + 1;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CognitiveComplexity).collect();
        assert_eq!(diagnostics.len(), 0, "Simple function should have complexity 0");
    }

    #[test]
    fn test_nested_if_higher_complexity() {
        let code = r#"Функция ВложенныеУсловия(А, Б)
    Если А > 0 Тогда
        Если Б > 0 Тогда
            Возврат А + Б;
        КонецЕсли;
    КонецЕсли;
    Возврат 0;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CognitiveComplexity).collect();
        assert_eq!(diagnostics.len(), 0, "Complexity should be 1 + 2 = 3, below default threshold");
    }

    #[test]
    fn test_deeply_nested_complexity() {
        let code = r#"Функция ГлубокаяВложенность(П1, П2, П3)
    Если П1 > 0 Тогда
        Если П2 > 0 Тогда
            Для Каждого Э Из П3 Цикл
                Если Э > 5 Тогда
                    Возврат 1;
                КонецЕсли;
            КонецЦикла;
        КонецЕсли;
    КонецЕсли;
    Возврат 0;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CognitiveComplexity).collect();
        assert_eq!(
            diagnostics.len(),
            0,
            "Complexity should be 1 + 2 + 3 + 4 = 10, below default threshold of 15"
        );
    }

    #[test]
    fn test_elseif_no_extra_nesting() {
        let code = r#"Функция СМножественнымиУсловиями(Х)
    Если Х = 1 Тогда
        Возврат "один";
    ИначеЕсли Х = 2 Тогда
        Возврат "два";
    ИначеЕсли Х = 3 Тогда
        Возврат "три";
    Иначе
        Возврат "другое";
    КонецЕсли;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CognitiveComplexity).collect();
        assert_eq!(
            diagnostics.len(),
            0,
            "Complexity should be 4 (if + 3 elseif/else), below threshold"
        );
    }

    #[test]
    fn test_custom_threshold() {
        let code = r#"Функция Тест()
    Если А Тогда
        Если Б Тогда
            Возврат 1;
        КонецЕсли;
    КонецЕсли;
КонецФункции"#;

        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("complexityThreshold".to_string(), serde_json::Value::Number(2.into()));
        config
            .parameters
            .insert(DiagnosticCode::CognitiveComplexity, serde_json::Value::Object(params));

        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CognitiveComplexity).collect();
        assert_eq!(diagnostics.len(), 1, "Complexity is 3 (1 + 2), should exceed threshold of 2");
    }

    const COMPLEX_FUNCTION: &str = r#"Функция ОбработатьКоллекцию(Данные, Флаг)
    Итог = 0;

    Если Данные = Неопределено Тогда
        Возврат Итог;
    КонецЕсли;

    Для Каждого Элемент Из Данные Цикл
        Если Элемент.Актуален Тогда
            Если Флаг Тогда
                Для Каждого Строка Из Элемент.Строки Цикл
                    Если Строка.Сумма > 0 Тогда
                        Итог = Итог + Строка.Сумма;
                    ИначеЕсли Строка.Ошибка Тогда
                        Продолжить;
                    Иначе
                        Прервать;
                    КонецЕсли;
                КонецЦикла;
            ИначеЕсли Элемент.Важный Тогда
                Пока Элемент.ТребуетПроверки Цикл
                    Итог = Итог + 1;
                    Прервать;
                КонецЦикла;
            Иначе
                Итог = Итог + 2;
            КонецЕсли;
        КонецЕсли;
    КонецЦикла;

    Возврат Итог;
КонецФункции

Процедура БезСложности()
КонецПроцедуры
"#;

    #[test]
    fn test_comprehensive() {
        let code = COMPLEX_FUNCTION;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CognitiveComplexity).collect();

        // Expected 1 diagnostic for function ОбработатьКоллекцию
        assert_eq!(diagnostics.len(), 1, "Should find 1 diagnostic");

        // Expected diagnostic on function name
        assert_diagnostic_range(code, diagnostics[0], 0, 8, 27);

        // Verify diagnostic details
        assert_eq!(diagnostics[0].code, DiagnosticCode::CognitiveComplexity);
        assert_eq!(diagnostics[0].severity, Severity::Warning);

        // Verify the actual cognitive complexity value is mentioned in the message
        assert!(
            diagnostics[0].message.contains("25"),
            "Message should contain complexity value 25, got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("15"),
            "Message should contain threshold 15, got: {}",
            diagnostics[0].message
        );
    }

    /// Track 2 Phase B §6.5 — recursion penalty regression guard.
    /// SonarSource Cognitive Complexity v1.4 spec adds `+1` for any
    /// recursive call (self or mutual). The §6.4 visitor's HIR-walk
    /// can't see call edges, so the penalty is sourced from
    /// `EffectSummary::is_recursive` (§1.4) and applied in the
    /// handler. This test pins the +1 increment against a tight
    /// fixture: a self-recursive function with cognitive=2 (one
    /// `Если` increment) plus the recursion bonus = 3, fires when
    /// `complexityThreshold = 2`.
    #[test]
    fn test_recursion_penalty_self_call() {
        let code = r#"Функция Факториал(N)
    Если N <= 1 Тогда
        Возврат 1;
    КонецЕсли;
    Возврат N * Факториал(N - 1);
КонецФункции"#;

        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("complexityThreshold".to_string(), serde_json::Value::Number(1.into()));
        config
            .parameters
            .insert(DiagnosticCode::CognitiveComplexity, serde_json::Value::Object(params));

        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CognitiveComplexity).collect();
        assert_eq!(
            diagnostics.len(),
            1,
            "raw cognitive=1, recursion bonus=+1 → total 2; with threshold 1 should fire after \
             the §6.5 penalty"
        );
        assert!(
            diagnostics[0].message.contains("сложность 2"),
            "Message should contain total complexity 2 (1 + 1 recursion), got: {}",
            diagnostics[0].message
        );
    }

    /// Track 2 Phase B §6.4 — pin the cognitive value the §6.1 visitor
    /// produces against the same fixture the legacy
    /// `calculate_complexity` test asserted on. The migration path runs
    /// through `hir::metrics::compute_hir_metrics` instead of the
    /// retired wrapper; the expected number is unchanged.
    #[test]
    fn test_compute_hir_metrics_cognitive_value() {
        let code = COMPLEX_FUNCTION;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let module_id = ModuleId::new(file_id);
        let module_bodies = db.module_bodies(module_id);

        let body = module_bodies.body(0).expect("Should have first method body");
        let metrics = hir::metrics::compute_hir_metrics(body);

        assert_eq!(metrics.cognitive, 25, "ОбработатьКоллекцию should have cognitive 25");
    }
}
