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
//! - **Severity:** Warning (CRITICAL in Java for compatibility)
//! - **Tags:** BRAINOVERLOAD
//! - **Minutes to fix:** 15
//!
//! ## Implementation
//! Uses HIR-based complexity calculation for:
//! - Better performance (Salsa caching)
//! - Cleaner code (structured HIR vs raw AST)
//! - Reusability (same calculation for code lens)

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::hir_def::{self, item_tree::ModItem, ModuleId};

#[derive(Debug, Clone)]
struct Config {
    complexity_threshold: u32,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let complexity_threshold = ctx
            .config
            .get_int(DiagnosticCode::CognitiveComplexity, "complexityThreshold")
            .unwrap_or(15) as u32;

        Self { complexity_threshold }
    }
}

/// Main entry point for CognitiveComplexity diagnostic.
///
/// Detects functions and procedures with cognitive complexity exceeding the threshold.
/// Default threshold is 15 (configurable via complexityThreshold parameter).
///
/// Uses HIR-based complexity calculation for better performance and reusability.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CognitiveComplexity) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let module_id = ModuleId::new(ctx.file_id);

    // Get ItemTree for method metadata (names, ranges)
    let item_tree = ctx.db.item_tree(ctx.file_id);

    // Get ModuleBodies for HIR-based complexity calculation
    let module_bodies = ctx.db.module_bodies(module_id);

    let mut diagnostics = Vec::new();

    // Iterate over all methods in the module
    for (idx, item) in item_tree.top_level_items().iter().enumerate() {
        let local_id = idx as u32;

        match item {
            ModItem::Procedure(proc_idx) => {
                let proc = item_tree.procedure(*proc_idx);

                // Get HIR body and calculate complexity
                if let Some(body) = module_bodies.body(local_id) {
                    let complexity = hir_def::cognitive_complexity::calculate_complexity(body);

                    if complexity > config.complexity_threshold {
                        diagnostics.push(Diagnostic {
                            code: DiagnosticCode::CognitiveComplexity,
                            message: format!(
                                "Процедура '{}' имеет когнитивную сложность {} (максимум: {}). \
                                 Упростите логику или уменьшите вложенность",
                                proc.name, complexity, config.complexity_threshold
                            ),
                            severity: Severity::Warning,
                            range: proc.name_range,
                            tags: vec![],
                            fixes: vec![],
                        });
                    }
                }
            }
            ModItem::Function(func_idx) => {
                let func = item_tree.function(*func_idx);

                // Get HIR body and calculate complexity
                if let Some(body) = module_bodies.body(local_id) {
                    let complexity = hir_def::cognitive_complexity::calculate_complexity(body);

                    if complexity > config.complexity_threshold {
                        diagnostics.push(Diagnostic {
                            code: DiagnosticCode::CognitiveComplexity,
                            message: format!(
                                "Функция '{}' имеет когнитивную сложность {} (максимум: {}). \
                                 Упростите логику или уменьшите вложенность",
                                func.name, complexity, config.complexity_threshold
                            ),
                            severity: Severity::Warning,
                            range: func.name_range,
                            tags: vec![],
                            fixes: vec![],
                        });
                    }
                }
            }
            ModItem::Variable(_) => {
                // Variables don't have cognitive complexity
            }
        }
    }

    diagnostics
}

/// Calculate cognitive complexity for a method body (HIR-based).
///
/// This is a PUBLIC function that can be reused for:
/// - Code lenses (showing complexity in editor)
/// - Metrics collection
/// - Other diagnostics
///
/// Uses the HIR-based implementation from `hir_def::cognitive_complexity`.
pub fn calculate_complexity(body: &hir_def::Body) -> u32 {
    hir_def::cognitive_complexity::calculate_complexity(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    use crate::DiagnosticsConfig;
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

        let diagnostics = check_ast_diagnostic(code, check);
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

        let diagnostics = check_ast_diagnostic(code, check);
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

        let diagnostics = check_ast_diagnostic(code, check);
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

        let diagnostics = check_ast_diagnostic(code, check);
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

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();

        // Set up source root for module_bodies to work
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
        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("complexityThreshold".to_string(), serde_json::Value::Number(2.into()));
        config
            .parameters
            .insert(DiagnosticCode::CognitiveComplexity, serde_json::Value::Object(params));

        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 1, "Complexity is 3 (1 + 2), should exceed threshold of 2");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CognitiveComplexityDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // Java expects 1 diagnostic for function СерверныйМодульМенеджера
        assert_eq!(diagnostics.len(), 1, "Should match Java implementation (1 diagnostic)");

        // Java expects diagnostic at line 0, columns 8-32 (function name)
        assert_diagnostic_range(code, &diagnostics[0], 0, 8, 32);

        // Verify diagnostic details
        assert_eq!(diagnostics[0].code, DiagnosticCode::CognitiveComplexity);
        assert_eq!(diagnostics[0].severity, Severity::Warning);

        // Verify the actual cognitive complexity value is mentioned in the message
        // The function СерверныйМодульМенеджера has cognitive complexity of 82
        assert!(
            diagnostics[0].message.contains("82"),
            "Message should contain complexity value 82, got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("15"),
            "Message should contain threshold 15, got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn test_calculate_complexity_directly() {
        // Test direct complexity calculation using HIR
        let code = include_str!("../../test_data/CognitiveComplexityDiagnostic.bsl");
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();

        // Set up source root for module_bodies to work
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

        // Get the first method body (СерверныйМодульМенеджера)
        let body = module_bodies.body(0).expect("Should have first method body");
        let complexity = calculate_complexity(body);

        // The function СерверныйМодульМенеджера has 82 cognitive complexity
        // This matches the Java implementation and Rust reference
        assert_eq!(complexity, 82, "СерверныйМодульМенеджера should have complexity 82");
    }
}
