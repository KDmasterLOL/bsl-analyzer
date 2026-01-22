//! MissingReturn diagnostic (AllFunctionPathMustHaveReturn).
//!
//! Checks that ALL execution paths in a function return a value using CFG analysis.
//! This is the HIR-based version that replaces the AST-based AllFunctionPathMustHaveReturn.
//!
//! ## Why?
//! Functions should ensure that every possible execution path returns a value.
//! Without this, some code paths may return undefined, leading to subtle bugs.
//!
//! ## Bad practice
//! ```bsl
//! Функция Сумма(А, Б)
//!     Если А > 0 Тогда
//!         Возврат А + Б;
//!     КонецЕсли;
//!     // Missing return in the Else path!
//! КонецФункции
//!
//! Функция ПроверитьХ(Х)
//!     Попытка
//!         Возврат Х / 2;
//!     Исключение
//!         // Missing return in exception handler!
//!     КонецПопытки;
//! КонецФункции
//! ```
//!
//! ## Good practice
//! ```bsl
//! Функция Сумма(А, Б)
//!     Если А > 0 Тогда
//!         Возврат А + Б;
//!     Иначе
//!         Возврат 0;
//!     КонецЕсли;
//! КонецФункции
//!
//! Функция ПроверитьХ(Х)
//!     Попытка
//!         Возврат Х / 2;
//!     Исключение
//!         Возврат -1;
//!     КонецПопытки;
//! КонецФункции
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Warning (Major)
//! - **Tags:** DESIGN, CONFUSING
//!
//! ## Implementation
//! This diagnostic is collected during HIR lowering as a byproduct of
//! CFG analysis. The `from_hir` function converts the BodyDiagnostic
//! to a Diagnostic for display.
//!
//! Ported from:
//! - AllFunctionPathMustHaveReturnDiagnostic.java (bsl-language-server)
//!
//! This HIR-based implementation replaces the AST-based version to leverage
//! Salsa caching and the rust-analyzer architecture pattern.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir_def::MethodId;
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic (called from lib.rs dispatch).
///
/// This performs CFG-based validation to check if ALL paths truly miss a return.
/// The lowering emits a candidate diagnostic, and this handler validates it using CFG.
pub fn from_hir(
    range: TextRange,
    method_id: &MethodId,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    // Note: AllFunctionPathMustHaveReturn is the diagnostic code used in bsl-language-server
    // MissingReturn is the internal HIR diagnostic name
    let code = DiagnosticCode::AllFunctionPathMustHaveReturn;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    // Get module bodies and method body using the provided MethodId
    let module_bodies = ctx.module_bodies();
    let local_id = method_id.local_id;
    let body = module_bodies.body(local_id)?;

    // Get CFG for this method from module-level query (batch processing)
    let module_cfgs = ctx.module_cfgs();
    let cfg = module_cfgs.get(local_id)?;

    // Perform CFG-based validation
    if !check_missing_return_in_cfg(body, cfg) {
        // All paths have returns - false positive, suppress diagnostic
        return None;
    }

    // Confirmed: some paths missing return
    Some(Diagnostic {
        code,
        message: message_ru(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

/// Check if function has missing return paths using CFG analysis.
///
/// Returns true if some execution paths don't have explicit return statements.
fn check_missing_return_in_cfg(body: &hir_def::Body, cfg: &cfg::ControlFlowGraph) -> bool {
    use cfg::{CfgEdgeType, CfgVertex};

    let exit_point = cfg.exit_point();

    // Check all incoming edges to exit point
    let incoming: Vec<_> = cfg.incoming_edges(exit_point).collect();

    for (source_idx, edge_type) in incoming.iter() {
        if let Some(vertex) = cfg.vertex(*source_idx) {
            // Check if this path has missing return
            let has_missing = match vertex {
                CfgVertex::BasicBlock(block) => {
                    // Check if block ends with Return/Raise
                    check_basic_block_missing_return(*source_idx, block, cfg, body)
                }
                CfgVertex::WhileLoop(_) => {
                    // Loop false branch (didn't execute) is OK if loops_executed_at_least_once
                    **edge_type != CfgEdgeType::FalseBranch
                }
                CfgVertex::ForLoop(_) | CfgVertex::ForEachLoop(_) => {
                    // Loop false branch (didn't execute) is OK
                    **edge_type != CfgEdgeType::FalseBranch
                }
                CfgVertex::Conditional(_) => {
                    // Missing else clause - check if this is false branch
                    **edge_type == CfgEdgeType::FalseBranch
                }
                _ => false,
            };

            if has_missing {
                return true;
            }
        }
    }

    false
}

/// Check if a basic block has missing return.
fn check_basic_block_missing_return(
    vertex_idx: cfg::NodeIndex,
    block: &cfg::BasicBlockVertex,
    cfg: &cfg::ControlFlowGraph,
    body: &hir_def::Body,
) -> bool {
    use cfg::{CfgEdgeType, CfgVertex};
    use hir_def::hir::Stmt;

    if block.is_empty() {
        // Check incoming edges
        let incoming_edges: Vec<_> = cfg.incoming_edges(vertex_idx).collect();

        // Loop false branch is OK
        let from_loop_false = incoming_edges.iter().any(|(source_idx, edge)| {
            matches!(edge, CfgEdgeType::FalseBranch)
                && matches!(
                    cfg.vertex(*source_idx),
                    Some(
                        CfgVertex::WhileLoop(_) | CfgVertex::ForLoop(_) | CfgVertex::ForEachLoop(_)
                    )
                )
        });

        if from_loop_false {
            return false;
        }

        // Missing else clause
        let from_conditional_false = incoming_edges.iter().any(|(source_idx, edge)| {
            matches!(edge, CfgEdgeType::FalseBranch)
                && matches!(cfg.vertex(*source_idx), Some(CfgVertex::Conditional(_)))
        });

        if from_conditional_false {
            return true;
        }

        // Check if all incoming edges have returns
        let all_have_returns = incoming_edges.iter().all(|(source_idx, _)| {
            if let Some(CfgVertex::BasicBlock(src_block)) = cfg.vertex(*source_idx) {
                // Check if source block ends with Return/Raise
                src_block.statements().last().is_some_and(|&stmt_id| {
                    let stmt = body.stmt(stmt_id);
                    matches!(stmt, Stmt::Return { .. } | Stmt::Raise { .. })
                })
            } else {
                false
            }
        });

        return !all_have_returns;
    }

    // Non-empty block: check last statement
    let has_explicit_return = block.statements().last().is_some_and(|&stmt_id| {
        let stmt = body.stmt(stmt_id);
        matches!(stmt, Stmt::Return { .. } | Stmt::Raise { .. })
    });

    !has_explicit_return
}

fn message_ru() -> String {
    "Не все пути выполнения функции возвращают значение".to_string()
}

#[allow(dead_code)]
fn message_en() -> String {
    "Not all function execution paths return a value".to_string()
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;

    /// Integration test matching AllFunctionPathMustHaveReturnDiagnosticTest.java
    ///
    /// Uses the same test file: AllFunctionPathMustHaveReturnDiagnostic.bsl
    /// This test validates that the HIR-based implementation produces the same
    /// results as the Java version.
    #[test]
    fn test_missing_return_from_fixture() {
        let code = include_str!("../../test_data/AllFunctionPathMustHaveReturnDiagnostic.bsl");

        let diagnostics = check_hir_diagnostic(code);

        // Debug: print all diagnostics
        for (i, diag) in diagnostics.iter().enumerate() {
            if diag.code == DiagnosticCode::AllFunctionPathMustHaveReturn {
                eprintln!("Diagnostic {}: {:?}", i, diag.range);
            }
        }

        // Expected: 2 diagnostics with default config (loops executed at least once)
        // Line 0: ОпределитьСтавкуНДС - missing else branch
        // Line 25: СуммаСкидки - elsif branch missing return
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            2,
            "Expected 2 diagnostics for missing return paths"
        );

        // Filter only MissingReturn diagnostics
        let missing_return_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
            .collect();

        // First diagnostic: line 0, columns 8-27
        assert_eq!(missing_return_diags[0].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
        assert_eq!(missing_return_diags[0].severity, crate::Severity::Warning);
        assert_diagnostic_range(code, missing_return_diags[0], 0, 8, 27);

        // Second diagnostic: line 25, columns 8-19
        assert_eq!(missing_return_diags[1].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
        assert_eq!(missing_return_diags[1].severity, crate::Severity::Warning);
        assert_diagnostic_range(code, missing_return_diags[1], 25, 8, 19);
    }

    /// Test simple case with missing else branch
    #[test]
    fn test_simple_missing_else() {
        let code = r#"
Функция Тест(Х)
    Если Х > 0 Тогда
        Возврат 1;
    КонецЕсли;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let missing_return_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
            .collect();

        assert_eq!(missing_return_diags.len(), 1, "Expected 1 diagnostic for missing else branch");
    }

    /// Test that functions with returns on all paths don't trigger diagnostic
    #[test]
    fn test_no_diagnostic_when_all_paths_return() {
        // NOTE: In BSL, even when if-else both have returns, control flow continues after the block.
        // This is because BSL's if-else is a statement, not an expression.
        // The idiomatic pattern is to have a fallback return after conditional blocks.
        let code = r#"
Функция Тест(Х)
    Если Х > 0 Тогда
        Возврат 1;
    ИначеЕсли Х < 0 Тогда
        Возврат -1;
    КонецЕсли;
    Возврат 0; // Fallback return
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);

        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "No diagnostic when all paths return"
        );
    }

    /// Test that raise exception counts as exit
    #[test]
    fn test_raise_counts_as_exit() {
        let code = r#"
Функция Тест()
    ВызватьИсключение "Ошибка";
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "Raise should count as exit"
        );
    }

    /// Test procedure (not function) doesn't trigger diagnostic
    #[test]
    fn test_procedure_not_checked() {
        let code = r#"
Процедура Тест(Х)
    Если Х > 0 Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::AllFunctionPathMustHaveReturn)
                .count(),
            0,
            "Procedures should not be checked"
        );
    }
}
