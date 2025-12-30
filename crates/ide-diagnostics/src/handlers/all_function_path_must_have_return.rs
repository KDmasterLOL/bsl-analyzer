//! AllFunctionPathMustHaveReturn diagnostic.
//!
//! Checks that ALL execution paths in a function return a value using CFG analysis.
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
//! ## Implementation
//!
//! Ported from:
//! - AllFunctionPathMustHaveReturnDiagnostic.java (bsl-language-server)
//! - all_function_path_must_have_return.rs (bsl-language-server-rust)
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use cfg::{BasicBlockVertex, CfgBuilder, CfgEdgeType, CfgVertex, ControlFlowGraph, NodeIndex};
use syntax::{SyntaxKind, SyntaxNode};

/// Configuration for AllFunctionPathMustHaveReturn diagnostic
///
/// ## Parameters (from .bslls.json):
///
/// ```json
/// {
///   "diagnostics": {
///     "parameters": {
///       "AllFunctionPathMustHaveReturn": {
///         "loopsExecutedAtLeastOnce": true,
///         "ignoreMissingElseOnExit": false
///       }
///     }
///   }
/// }
/// ```
///
/// ### loopsExecutedAtLeastOnce (default: true)
/// Assume that loops (while/for/foreach) execute at least once.
/// - `true`: Loop bypass path (when loop doesn't execute) is acceptable without explicit return
/// - `false`: All paths including loop bypass must have explicit return
///
/// ### ignoreMissingElseOnExit (default: false)
/// Ignore missing else clause on conditional that leads to exit.
/// - `true`: If/ElsIf without Else on exit path is acceptable
/// - `false`: Report missing else clause as missing return path
#[derive(Debug, Clone)]
struct Config {
    /// Assume loops execute at least once (default: true)
    loops_executed_at_least_once: bool,
    /// Ignore missing else on exit (default: false)
    ignore_missing_else_on_exit: bool,
}

impl Config {
    /// Read configuration from DiagnosticsContext
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        Self {
            loops_executed_at_least_once: ctx
                .config
                .get_bool(DiagnosticCode::AllFunctionPathMustHaveReturn, "loopsExecutedAtLeastOnce")
                .unwrap_or(true),
            ignore_missing_else_on_exit: ctx
                .config
                .get_bool(DiagnosticCode::AllFunctionPathMustHaveReturn, "ignoreMissingElseOnExit")
                .unwrap_or(false),
        }
    }
}

/// Runs the AllFunctionPathMustHaveReturn diagnostic.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Check if diagnostic is disabled
    if ctx.config.is_disabled(DiagnosticCode::AllFunctionPathMustHaveReturn) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    // Read configuration from context
    let config = Config::from_context(ctx);

    let mut diagnostics = Vec::new();

    // Find all function definitions (not procedures)
    for node in root.descendants() {
        if node.kind() == SyntaxKind::FUNCTION_DEF {
            if let Some(diag) = check_function(&node, &config) {
                diagnostics.push(diag);
            }
        }
    }

    diagnostics
}

/// Check a single function for missing returns on all paths
fn check_function(func_node: &SyntaxNode, config: &Config) -> Option<Diagnostic> {
    // Check if function has at least one return statement
    // (to avoid duplicating with FunctionShouldHaveReturn diagnostic)
    if !has_return_statements(func_node) {
        return None;
    }

    // Find function body
    let body = func_node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST)?;

    // Build CFG for this function
    let mut builder = CfgBuilder::new();
    builder.produce_loop_iterations(config.loops_executed_at_least_once);
    let cfg = builder.build_graph(&body);

    // Check for missing returns
    if !has_missing_return(&cfg, config) {
        return None;
    }

    // Get function name for diagnostic range
    let name_range = func_node
        .children()
        .find(|n| n.kind() == SyntaxKind::IDENT)
        .map(|n| n.text_range())
        .unwrap_or_else(|| func_node.text_range());

    Some(Diagnostic {
        code: DiagnosticCode::AllFunctionPathMustHaveReturn,
        message: "Не все пути выполнения функции возвращают значение".to_string(),
        severity: Severity::Warning,
        range: name_range,
        tags: vec![],
        fixes: vec![],
    })
}

/// Check if function has at least one return statement
fn has_return_statements(func_node: &SyntaxNode) -> bool {
    func_node.descendants().any(|n| n.kind() == SyntaxKind::RETURN_STMT)
}

/// Check if CFG has paths with missing returns
fn has_missing_return(cfg: &ControlFlowGraph, config: &Config) -> bool {
    let exit_point = cfg.exit_point();

    // Check all incoming edges to exit point
    cfg.incoming_edges(exit_point).any(|(source_idx, edge_type)| {
        if let Some(vertex) = cfg.vertex(source_idx) {
            vertex_has_missing_return(source_idx, vertex, edge_type, cfg, config)
        } else {
            false
        }
    })
}

/// Check if a vertex represents a path with missing return
fn vertex_has_missing_return(
    vertex_idx: NodeIndex,
    vertex: &CfgVertex,
    edge_type: &CfgEdgeType,
    cfg: &ControlFlowGraph,
    config: &Config,
) -> bool {
    match vertex {
        CfgVertex::BasicBlock(block) => basic_block_missing_return(vertex_idx, block, cfg, config),
        CfgVertex::WhileLoop(_) | CfgVertex::ForLoop(_) | CfgVertex::ForEachLoop(_) => {
            loop_vertex_missing_return(edge_type, config)
        }
        CfgVertex::Conditional(_) => conditional_vertex_missing_return(config),
        _ => false,
    }
}

/// Check if a basic block is missing an explicit return
fn basic_block_missing_return(
    vertex_idx: NodeIndex,
    block: &BasicBlockVertex,
    cfg: &ControlFlowGraph,
    _config: &Config,
) -> bool {
    // Empty blocks can occur in:
    // - Missing else branches
    // - Exception handlers without returns
    // - Merge points where both branches have returns
    if block.is_empty() {
        // Check incoming edges to determine if this is truly a missing return
        let incoming_edges: Vec<_> = cfg.incoming_edges(vertex_idx).collect();

        // Check if this empty block comes from a conditional's false branch (missing else)
        let has_false_branch_from_conditional = incoming_edges.iter().any(|(source_idx, edge)| {
            matches!(edge, CfgEdgeType::FalseBranch)
                && matches!(cfg.vertex(*source_idx), Some(CfgVertex::Conditional(_)))
        });

        if has_false_branch_from_conditional {
            return true; // Missing else clause
        }

        // Otherwise, check if all incoming edges have explicit returns
        let all_incoming_have_returns = incoming_edges.iter().all(|(source_idx, _)| {
            if let Some(CfgVertex::BasicBlock(src_block)) = cfg.vertex(*source_idx) {
                // Check if last statement is return or raise
                src_block.last_statement().is_some_and(|stmt| {
                    matches!(stmt.kind(), SyntaxKind::RETURN_STMT | SyntaxKind::RAISE_STMT)
                })
            } else {
                false
            }
        });

        return !all_incoming_have_returns;
    }

    // Non-empty block: check if last statement is return or raise
    let has_explicit_return = block.last_statement().is_some_and(|stmt| {
        matches!(stmt.kind(), SyntaxKind::RETURN_STMT | SyntaxKind::RAISE_STMT)
    });

    !has_explicit_return
}

/// Check if loop vertex represents a missing return path
fn loop_vertex_missing_return(edge_type: &CfgEdgeType, config: &Config) -> bool {
    // If we assume loops execute at least once, then the false branch
    // (loop not executed) is acceptable without explicit return
    if config.loops_executed_at_least_once && *edge_type == CfgEdgeType::FalseBranch {
        return false;
    }

    // Otherwise, loop exit paths need explicit returns
    // (handled by basic block check)
    false
}

/// Check if conditional vertex represents a missing return path (missing else)
fn conditional_vertex_missing_return(config: &Config) -> bool {
    // If we ignore missing else on exit, don't report
    if config.ignore_missing_else_on_exit {
        return false;
    }

    // Missing else clause on path to exit
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use ide_db::RootDatabase;
    use std::sync::Arc;
    use vfs::Vfs;

    /// Helper to create a test database
    fn create_test_db() -> (Arc<dyn RootDatabase>, Vfs) {
        // TODO: Initialize real database when ready
        // For now, return mock to make tests compile
        todo!("Parser integration required")
    }

    /// Helper to run diagnostic on test code
    fn check_diagnostic(_code: &str, _config: DiagnosticsConfig) -> Vec<Diagnostic> {
        let (_db, _vfs) = create_test_db();
        // TODO: Parse code and run diagnostic
        // For now, return empty to make tests compile
        todo!("Parser integration required")
    }

    /// Helper to convert TextRange to (line, column) positions
    ///
    /// Used to verify diagnostics match Java test expectations.
    /// Line and column are 0-indexed.
    fn range_to_line_col(text: &str, range: syntax::TextRange) -> (u32, u32, u32, u32) {
        let start_offset = u32::from(range.start());
        let end_offset = u32::from(range.end());

        let mut line = 0;
        let mut col = 0;
        let mut start_line = 0;
        let mut start_col = 0;
        let mut end_line = 0;
        let mut end_col = 0;

        for (i, ch) in text.chars().enumerate() {
            let offset = i as u32;

            if offset == start_offset {
                start_line = line;
                start_col = col;
            }
            if offset == end_offset {
                end_line = line;
                end_col = col;
                break;
            }

            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }

        (start_line, start_col, end_line, end_col)
    }

    /// Helper to assert diagnostic range matches expected line:column
    #[allow(dead_code)]
    fn assert_diagnostic_range(
        text: &str,
        diagnostic: &Diagnostic,
        expected_line: u32,
        expected_start_col: u32,
        expected_end_col: u32,
    ) {
        let (start_line, start_col, _end_line, end_col) = range_to_line_col(text, diagnostic.range);

        assert_eq!(
            start_line, expected_line,
            "Diagnostic line mismatch: expected {}, got {}",
            expected_line, start_line
        );
        assert_eq!(
            start_col, expected_start_col,
            "Diagnostic start column mismatch: expected {}, got {}",
            expected_start_col, start_col
        );
        assert_eq!(
            end_col, expected_end_col,
            "Diagnostic end column mismatch: expected {}, got {}",
            expected_end_col, end_col
        );
    }

    /// Integration test matching Java test structure
    ///
    /// Based on AllFunctionPathMustHaveReturnDiagnosticTest.java
    /// Uses the same test file: AllFunctionPathMustHaveReturnDiagnostic.bsl
    #[test]
    #[ignore = "Requires BSL parser implementation"]
    fn test_all_function_path_must_have_return() {
        let code = include_str!("../../test_data/AllFunctionPathMustHaveReturnDiagnostic.bsl");

        // Test 1: Default behavior (loopsExecutedAtLeastOnce=true, ignoreMissingElseOnExit=false)
        // Expected: 2 diagnostics at lines 0 and 25
        {
            let config = DiagnosticsConfig::default();
            let diagnostics = check_diagnostic(code, config);

            assert_eq!(diagnostics.len(), 2, "Default config: expected 2 diagnostics");

            // Line 0, columns 8-27: function ОпределитьСтавкуНДС (missing else branch)
            assert_eq!(diagnostics[0].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
            assert_eq!(diagnostics[0].severity, Severity::Warning);
            assert_diagnostic_range(code, &diagnostics[0], 0, 8, 27);

            // Line 25, columns 8-19: function СуммаСкидки (missing return in elsif)
            assert_eq!(diagnostics[1].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
            assert_eq!(diagnostics[1].severity, Severity::Warning);
            assert_diagnostic_range(code, &diagnostics[1], 25, 8, 19);
        }

        // Test 2: loopsExecutedAtLeastOnce=false
        // Expected: 3 diagnostics at lines 0, 25, and 36
        {
            let mut config = DiagnosticsConfig::default();
            let mut params = serde_json::Map::new();
            params.insert("loopsExecutedAtLeastOnce".to_string(), serde_json::Value::Bool(false));
            config.parameters.insert(
                DiagnosticCode::AllFunctionPathMustHaveReturn,
                serde_json::Value::Object(params),
            );

            let diagnostics = check_diagnostic(code, config);

            assert_eq!(
                diagnostics.len(),
                3,
                "loopsExecutedAtLeastOnce=false: expected 3 diagnostics"
            );

            // Line 0, columns 8-27: ОпределитьСтавкуНДС
            assert_eq!(diagnostics[0].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
            assert_diagnostic_range(code, &diagnostics[0], 0, 8, 27);

            // Line 25, columns 8-19: СуммаСкидки
            assert_eq!(diagnostics[1].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
            assert_diagnostic_range(code, &diagnostics[1], 25, 8, 19);

            // Line 36, columns 8-23: ЦиклДляПроверки (loop may not execute)
            assert_eq!(diagnostics[2].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
            assert_diagnostic_range(code, &diagnostics[2], 36, 8, 23);
        }

        // Test 3: ignoreMissingElseOnExit=true
        // Expected: 1 diagnostic at line 25 only
        {
            let mut config = DiagnosticsConfig::default();
            let mut params = serde_json::Map::new();
            params.insert("ignoreMissingElseOnExit".to_string(), serde_json::Value::Bool(true));
            config.parameters.insert(
                DiagnosticCode::AllFunctionPathMustHaveReturn,
                serde_json::Value::Object(params),
            );

            let diagnostics = check_diagnostic(code, config);

            assert_eq!(diagnostics.len(), 1, "ignoreMissingElseOnExit=true: expected 1 diagnostic");

            // Line 25, columns 8-19: СуммаСкидки (only this one, missing else is ignored)
            assert_eq!(diagnostics[0].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
            assert_diagnostic_range(code, &diagnostics[0], 25, 8, 19);
        }

        // Test 4: Diagnostic can be disabled
        {
            let mut config = DiagnosticsConfig::default();
            config.disabled.push(DiagnosticCode::AllFunctionPathMustHaveReturn);

            let diagnostics = check_diagnostic(code, config);

            assert_eq!(diagnostics.len(), 0, "Disabled diagnostic should not run");
        }
    }

    /// Test empty if bodies (from Java test: testEmptyIfBodies)
    #[test]
    #[ignore = "Requires BSL parser implementation"]
    fn test_empty_if_bodies() {
        let code = r#"Функция Тест()
  Список = Новый СписокЗначений;
  #Если Сервер Или ТолстыйКлиентОбычноеПриложение Или ВнешнееСоединение Тогда
      Если Условие Тогда
      Иначе
      КонецЕсли;
  #КонецЕсли
  Возврат Список;
КонецФункции"#;
        let config = DiagnosticsConfig::default();

        let diagnostics = check_diagnostic(code, config);

        assert_eq!(diagnostics.len(), 0, "Empty if bodies should not trigger diagnostic");
    }

    /// Test exit by raise exception (from Java test: testExitByRaiseException)
    #[test]
    #[ignore = "Requires BSL parser implementation"]
    fn test_exit_by_raise_exception() {
        let code = r#"Функция Тест()
  #Если Не ВебКлиент Тогда
    Массив = Новый Массив;
    Если Условие Тогда
        Возврат Массив;
    КонецЕсли;
    Возврат ПустойМассив;
  #Иначе
    ВызватьИсключение "Упс";
  #КонецЕсли
КонецФункции"#;
        let config = DiagnosticsConfig::default();

        let diagnostics = check_diagnostic(code, config);

        assert_eq!(diagnostics.len(), 0, "Raise should count as exit");
    }
}
