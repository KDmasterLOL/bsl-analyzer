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
//! Based on McCabe's Cyclomatic Complexity:
//!
//! **Base complexity:** 1 per method
//!
//! **Decision points** (+1 each, no nesting penalty):
//! - if, elsif, else
//! - for, while, foreach
//! - ternary operator (?)
//! - except clause (try-except)
//! - goto
//! - AND/OR operators in expressions
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
//! - **checkModuleBody** (default: true) - Check module-level code complexity
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

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::hir_def::{self, item_tree::ModItem};

#[derive(Debug, Clone)]
struct Config {
    complexity_threshold: u32,
    check_module_body: bool,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let complexity_threshold = ctx
            .config
            .get_int(DiagnosticCode::CyclomaticComplexity, "complexityThreshold")
            .unwrap_or(20) as u32;

        let check_module_body = ctx
            .config
            .get_bool(DiagnosticCode::CyclomaticComplexity, "checkModuleBody")
            .unwrap_or(true);

        Self { complexity_threshold, check_module_body }
    }
}

/// Main entry point for CyclomaticComplexity diagnostic.
///
/// Detects functions and procedures with cyclomatic complexity exceeding the threshold.
/// Default threshold is 20 (configurable via complexityThreshold parameter).
///
/// Uses HIR-based complexity calculation for better performance and reusability.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CyclomaticComplexity;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);

    // Get ItemTree for method metadata (names, ranges)
    let item_tree = ctx.item_tree();

    // Get ModuleBodies for HIR-based complexity calculation
    let module_bodies = ctx.module_bodies();

    let mut diagnostics = Vec::new();

    // Iterate over all methods in the module
    for (idx, item) in item_tree.top_level_items().iter().enumerate() {
        let local_id = idx as u32;

        match item {
            ModItem::Procedure(proc_idx) => {
                let proc = item_tree.procedure(*proc_idx);

                // Get HIR body and calculate complexity
                if let Some(body) = module_bodies.body(local_id) {
                    let complexity = hir_def::cyclomatic_complexity::calculate_complexity(body);

                    if complexity > config.complexity_threshold {
                        diagnostics.push(Diagnostic {
                            code,
                            message: format!(
                                "Процедура '{}' имеет цикломатическую сложность {} (максимум: {}). \
                                 Рассмотрите возможность упрощения или разбиения на более мелкие функции",
                                proc.name, complexity, config.complexity_threshold
                            ),
                            severity: ctx.severity(code),
                            range: proc.name_range,
                            tags: ctx.tags(code),
                            fixes: vec![],
                        });
                    }
                }
            }
            ModItem::Function(func_idx) => {
                let func = item_tree.function(*func_idx);

                // Get HIR body and calculate complexity
                if let Some(body) = module_bodies.body(local_id) {
                    let complexity = hir_def::cyclomatic_complexity::calculate_complexity(body);

                    if complexity > config.complexity_threshold {
                        diagnostics.push(Diagnostic {
                            code,
                            message: format!(
                                "Функция '{}' имеет цикломатическую сложность {} (максимум: {}). \
                                 Рассмотрите возможность упрощения или разбиения на более мелкие функции",
                                func.name, complexity, config.complexity_threshold
                            ),
                            severity: ctx.severity(code),
                            range: func.name_range,
                            tags: ctx.tags(code),
                            fixes: vec![],
                        });
                    }
                }
            }
            ModItem::Variable(_) => {}
        }
    }

    // Check module body complexity (if enabled)
    if config.check_module_body {
        if let Some(module_code) = module_bodies.module_code_result() {
            let complexity =
                hir_def::cyclomatic_complexity::calculate_complexity(&module_code.body);

            if complexity > config.complexity_threshold {
                // Get range of first statement
                if let Some(first_stmt_id) = module_code.body.body_stmts().next() {
                    if let Some(range) = module_code.source_map.stmt_range(first_stmt_id) {
                        diagnostics.push(Diagnostic {
                            code,
                            message: format!(
                                "Тело модуля имеет цикломатическую сложность {} (максимум: {}). \
                                 Рассмотрите возможность упрощения или переноса логики в функции",
                                complexity, config.complexity_threshold
                            ),
                            severity: ctx.severity(code),
                            range,
                            tags: ctx.tags(code),
                            fixes: vec![],
                        });
                    }
                }
            }
        }
    }

    diagnostics
}

/// Calculate cyclomatic complexity for a method body (HIR-based).
///
/// This is a PUBLIC function that can be reused for:
/// - Code lenses (showing complexity in editor)
/// - Metrics collection
/// - Other diagnostics
///
/// Uses the HIR-based implementation from `hir_def::cyclomatic_complexity`.
pub fn calculate_complexity(body: &hir_def::Body) -> u32 {
    hir_def::cyclomatic_complexity::calculate_complexity(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    use crate::Severity;
    use hir_def::ModuleId;
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

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Complexity 3 should not trigger (threshold 20)");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CyclomaticComplexityDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // Java expects 1 diagnostic for function СерверныйМодульМенеджера
        assert_eq!(diagnostics.len(), 1, "Should match Java (1 diagnostic)");

        // Java expects diagnostic at line 0, columns 8-32 (function name)
        assert_diagnostic_range(code, &diagnostics[0], 0, 8, 32);

        // Verify diagnostic details
        assert_eq!(diagnostics[0].code, DiagnosticCode::CyclomaticComplexity);
        assert_eq!(diagnostics[0].severity, Severity::Critical);

        // Verify the actual complexity value is mentioned in the message
        assert!(
            diagnostics[0].message.contains("21"),
            "Message should contain complexity 21, got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("20"),
            "Message should contain threshold 20, got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn test_calculate_complexity_directly() {
        // Test direct complexity calculation using HIR
        let code = include_str!("../../test_data/CyclomaticComplexityDiagnostic.bsl");
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

        // The function СерверныйМодульМенеджера has cyclomatic complexity 21
        // This matches the Java implementation
        assert_eq!(complexity, 21, "СерверныйМодульМенеджера should have complexity 21");
    }
}
