//! CyclomaticComplexity diagnostic.
//!
//! Detects functions and procedures with high cyclomatic complexity.
//!
//! ## Why?
//! Cyclomatic complexity (McCabe) measures code complexity by counting decision points.
//! Unlike cognitive complexity, it treats all decision points equally without nesting penalties.
//!
//! High cyclomatic complexity indicates code that is:
//! - Difficult to test (many execution paths)
//! - Prone to bugs (complex logic)
//! - Hard to understand and maintain
//!
//! ## Algorithm
//! Track 2 Phase B §6.5 — SonarQube-style cyclomatic complexity:
//! `V(G) + boolean_ops + ternary` where `V(G) = E - N + 2*P` is the
//! textbook McCabe count from the cached CFG, and the boolean / ternary
//! extras come from the §6.1 HIR visitor (BSL `И` / `ИЛИ` and
//! `?(...)` evaluate inside basic blocks and don't add CFG edges, so
//! the textbook formula misses them).
//!
//! **Decision points** (+1 each, no nesting penalty):
//! - if, elsif (NOT else — SonarQube parity)
//! - for, while, foreach
//! - ternary operator `?(...)`
//! - except clause (try-except)
//! - AND / OR (`И`/`ИЛИ`) operators in expressions
//!
//! ## Bad practice
//! Many decision points regardless of nesting:
//! ```bsl
//! Функция СложнаяФункция(Данные)
//!     Если Условие1 Тогда        // +1
//!         Возврат 1;
//!     ИначеЕсли Условие2 Тогда   // +1
//!         Возврат 2;
//!     Иначе                       // +1
//!         Возврат 3;
//!     КонецЕсли;
//!     // Many more decision points...
//! КонецФункции
//! ```
//!
//! ## Good practice
//! Simplify logic or split into smaller functions:
//! ```bsl
//! Функция ОбработатьДанные(Данные)
//!     Если НЕ ПроверитьДанные(Данные) Тогда
//!         Возврат;
//!     КонецЕсли;
//!     ВыполнитьОбработку(Данные);
//! КонецФункции
//! ```
//!
//! ## Configuration
//! - **complexityThreshold** (default: 20) - Maximum allowed cyclomatic complexity
//! - **Enabled by default:** Yes
//! - **Severity:** CRITICAL
//! - **Tags:** BRAINOVERLOAD
//! - **Minutes to fix:** 25
//!
//! ## Implementation
//! Uses HIR-based complexity calculation for:
//! - Better performance (Salsa caching)
//! - Cleaner code (structured HIR vs raw AST)
//! - Reusability (same calculation for code lens)

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 25,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 1.0,
    lsp_severity_override: "",
};

/// Track 2 Phase B §6.4 — handler-side detection consuming the cached
/// CFG-based [`hir::cfg::cyclomatic_complexity`] via
/// `ctx.module_cyclomatic()`. Replaces the legacy `from_hir` adapter
/// (BodyDiagnostic-fed) and the per-method HIR-walk approximation
/// in `hir-def/cyclomatic_complexity.rs`.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use hir::ModItem;

    let code = DiagnosticCode::CyclomaticComplexity;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let threshold = ctx.config_int(code, "complexityThreshold", 20) as u32;
    let module_cyclomatic = ctx.module_cyclomatic();
    if module_cyclomatic.is_empty() {
        return Vec::new();
    }
    let module_bodies = ctx.module_bodies();
    let item_tree = ctx.item_tree();

    let mut out = Vec::new();
    for (local_id, _body) in module_bodies.iter_bodies() {
        let complexity = module_cyclomatic.get(local_id);
        if complexity <= threshold {
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
                "{} '{}' имеет цикломатическую сложность {} (максимум: {}). \
                 Рассмотрите возможность упрощения или разбиения на более мелкие функции",
                method_type, name, complexity, threshold
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
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::Severity;
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
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CyclomaticComplexity).collect();
        assert_eq!(diagnostics.len(), 0, "Complexity 1 should not trigger (threshold 20)");
    }

    #[test]
    fn test_else_counts() {
        let code = r#"Функция Тест()
    Если А Тогда
        Возврат 1;
    Иначе
        Возврат 2;
    КонецЕсли;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CyclomaticComplexity).collect();
        assert_eq!(diagnostics.len(), 0, "Complexity 3 should not trigger (threshold 20)");
    }

    #[test]
    fn test_high_complexity_triggers_diagnostic() {
        // Track 2 Phase B §6.5: SonarQube-style formula gives
        // `cyclomatic = 23` for this extended fixture (CFG-based 14 +
        // 7 boolean-op + 2 ternary). The fixture was enlarged here to
        // cross the default threshold (20) naturally, removing the
        // §6.4 "lower threshold to 10" compromise. The textbook
        // McCabe value the §6.4 commit pinned was 13 against the
        // smaller fixture; the SonarQube extras add `И`/`ИЛИ` and
        // ternary as decision points the CFG can't see (they evaluate
        // inside basic blocks). The legacy HIR-walk approximation
        // additionally counted each `Else` clause as a separate
        // decision; SonarQube does not, so the alignment intentionally
        // drops that contribution.
        let code = r#"Функция РассчитатьМаршрут(Сумма, ТипКлиента, Режим, ЕстьСкидка)
    Результат = 0;
    Если Сумма > 100 Тогда
        Результат = 1;
    ИначеЕсли Сумма > 50 Тогда
        Результат = 2;
    Иначе
        Результат = 3;
    КонецЕсли;
    Если ТипКлиента = "VIP" Тогда
        Результат = Результат + 10;
    ИначеЕсли ТипКлиента = "Retail" Тогда
        Результат = Результат + 5;
    Иначе
        Результат = Результат + 1;
    КонецЕсли;
    Для Индекс = 0 По 3 Цикл
        Если Индекс % 2 = 0 Тогда
            Результат = Результат + Индекс;
        Иначе
            Результат = Результат - Индекс;
        КонецЕсли;
    КонецЦикла;
    Пока Результат < 20 Цикл
        Если Режим = "A" Тогда
            Результат = Результат + 2;
        ИначеЕсли Режим = "B" Тогда
            Результат = Результат + 3;
        Иначе
            Результат = Результат + 4;
        КонецЕсли;
        Прервать;
    КонецЦикла;
    Попытка
        Значение = Результат;
    Исключение
        Значение = 0;
    КонецПопытки;
    Если ЕстьСкидка Тогда
        Результат = ?(Режим = "A", 10, ?(Режим = "B", 20, 30));
    КонецЕсли;
    Условие = Сумма > 0 И ТипКлиента <> "";
    Условие2 = Режим = "A" ИЛИ Режим = "B";
    Условие3 = ЕстьСкидка И Сумма > 50 ИЛИ Режим = "C" И ТипКлиента <> "";
    Если Условие И Условие2 Тогда
        Результат = Результат + 1;
    КонецЕсли;
    Если Условие3 ИЛИ ЕстьСкидка Тогда
        Результат = Результат + 2;
    КонецЕсли;
    Возврат Результат;
КонецФункции"#;

        // Default threshold = 20 (no per-test compromise needed after
        // §6.5 alignment).
        let diagnostics =
            crate::test_utils::check_hir_diagnostic(code).into_iter().collect::<Vec<_>>();
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CyclomaticComplexity).collect();

        assert_eq!(diagnostics.len(), 1, "Should find 1 diagnostic for high-complexity function");

        // Diagnostic points at function name "РассчитатьМаршрут" (col 8-25 on line 0)
        assert_diagnostic_range(code, diagnostics[0], 0, 8, 25);

        assert_eq!(diagnostics[0].code, DiagnosticCode::CyclomaticComplexity);
        // CodeSmell + Critical → Warning (per metadata mapping)
        assert_eq!(diagnostics[0].severity, Severity::Warning);

        assert!(
            diagnostics[0].message.contains("23"),
            "Message should contain SonarQube-style cyclomatic 23 \
             (CFG-based 14 + 7 boolean-op + 2 ternary), got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("20"),
            "Message should contain default threshold 20, got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn test_calculate_complexity_directly() {
        let code = r#"Функция РассчитатьМаршрут(Сумма, ТипКлиента, Режим, ЕстьСкидка)
    Результат = 0;
    Если Сумма > 100 Тогда
        Результат = 1;
    ИначеЕсли Сумма > 50 Тогда
        Результат = 2;
    Иначе
        Результат = 3;
    КонецЕсли;
    Если ТипКлиента = "VIP" Тогда
        Результат = Результат + 10;
    ИначеЕсли ТипКлиента = "Retail" Тогда
        Результат = Результат + 5;
    Иначе
        Результат = Результат + 1;
    КонецЕсли;
    Для Индекс = 0 По 3 Цикл
        Если Индекс % 2 = 0 Тогда
            Результат = Результат + Индекс;
        Иначе
            Результат = Результат - Индекс;
        КонецЕсли;
    КонецЦикла;
    Пока Результат < 20 Цикл
        Если Режим = "A" Тогда
            Результат = Результат + 2;
        ИначеЕсли Режим = "B" Тогда
            Результат = Результат + 3;
        Иначе
            Результат = Результат + 4;
        КонецЕсли;
        Прервать;
    КонецЦикла;
    Попытка
        Значение = Результат;
    Исключение
        Значение = 0;
    КонецПопытки;
    Если ЕстьСкидка Тогда
        Результат = ?(Режим = "A", 10, ?(Режим = "B", 20, 30));
    КонецЕсли;
    Условие = Сумма > 0 И ТипКлиента <> "";
    Условие2 = Режим = "A" ИЛИ Режим = "B";
    Условие3 = ЕстьСкидка И Сумма > 50 ИЛИ Режим = "C" И ТипКлиента <> "";
    Если Условие И Условие2 Тогда
        Результат = Результат + 1;
    КонецЕсли;
    Если Условие3 ИЛИ ЕстьСкидка Тогда
        Результат = Результат + 2;
    КонецЕсли;
    Возврат Результат;
КонецФункции"#;

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

        // Track 2 Phase B §6.5 — pin both the textbook CFG value and
        // the SonarQube extension separately. `V(G) = E - N + 2*P`
        // counts only structural decisions that produce CFG edges
        // (If/Elsif, While, For, ForEach, Try/Except). BSL `И` / `ИЛИ`
        // and ternary `?(...)` evaluate inside basic blocks and don't
        // add edges, so the SonarQube definition adds them back as
        // per-occurrence increments. The §6.4 commit asserted the
        // textbook value (13 for the smaller fixture); after §6.5 the
        // diagnostic reports `cfg + boolean_ops + ternary`, so this
        // test now pins all three components against the extended
        // fixture.
        let cfg =
            hir::cfg::CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), body, None);
        let complexity = hir::cfg::cyclomatic_complexity(&cfg);
        assert_eq!(complexity, 14, "РассчитатьМаршрут CFG-based cyclomatic should be 14");

        let metrics = hir::metrics::compute_hir_metrics(body);
        assert_eq!(
            metrics.boolean_ops_count, 7,
            "boolean ops contribute +7 to SonarQube cyclomatic"
        );
        assert_eq!(metrics.ternary_count, 2, "ternary expressions contribute +2");
        // The §6.5 SonarQube-aligned cyclomatic the diagnostic reports
        // is the sum: 14 (CFG) + 7 (boolean) + 2 (ternary) = 23.
        assert_eq!(complexity + metrics.boolean_ops_count + metrics.ternary_count, 23);
    }
}
