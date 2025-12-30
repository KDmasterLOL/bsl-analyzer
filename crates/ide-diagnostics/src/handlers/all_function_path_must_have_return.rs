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
    // Get function name for debugging (unused but kept for future debugging)
    let _func_name = func_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)
        .map(|tok| tok.text().to_string())
        .unwrap_or_else(|| "<unnamed>".to_string());

    // Check if function has at least one return statement
    // (to avoid duplicating with FunctionShouldHaveReturn diagnostic)
    let has_returns = has_return_statements(func_node);
    if !has_returns {
        return None;
    }

    // Find function body
    let body = func_node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);
    let body = body?;

    // Build CFG for this function
    let mut builder = CfgBuilder::new();
    builder.produce_loop_iterations(config.loops_executed_at_least_once);
    let cfg = builder.build_graph(&body);

    // Check for missing returns
    let has_missing = has_missing_return(&cfg, config);
    if !has_missing {
        return None;
    }

    // Get function name for diagnostic range
    // The function name is the first IDENT token that appears before PARAM_LIST
    let name_token = func_node
        .children_with_tokens()
        .take_while(|el| !matches!(el.kind(), SyntaxKind::PARAM_LIST))
        .filter_map(|el| el.into_token())
        .filter(|tok| !tok.kind().is_trivia()) // Skip trivia tokens
        .find(|tok| tok.kind() == SyntaxKind::IDENT);

    let name_range =
        name_token.map(|tok| tok.text_range()).unwrap_or_else(|| func_node.text_range());

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
    let incoming: Vec<_> = cfg.incoming_edges(exit_point).collect();

    for (source_idx, edge_type) in incoming.iter() {
        if let Some(v) = cfg.vertex(*source_idx) {
            if vertex_has_missing_return(*source_idx, v, edge_type, cfg, config) {
                return true;
            }
        }
    }

    false
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
        CfgVertex::BasicBlock(block) => {
            // Check if this block is a bypass of an endless loop
            // or missing else path that should be ignored
            let incoming_edges: Vec<_> = cfg.incoming_edges(vertex_idx).collect();

            // Check if this is the bypass path (FalseBranch) of an endless loop
            let from_endless_loop_false_branch = incoming_edges.iter().any(|(source_idx, edge)| {
                matches!(edge, CfgEdgeType::FalseBranch)
                    && matches!(
                        cfg.vertex(*source_idx),
                        Some(CfgVertex::WhileLoop(loop_v)) if loop_v.is_endless()
                    )
            });

            if from_endless_loop_false_branch {
                return false; // Endless loop bypass is unreachable
            }

            // Check if this is a missing else path that should be ignored
            let from_conditional_false_branch = incoming_edges.iter().any(|(source_idx, edge)| {
                matches!(edge, CfgEdgeType::FalseBranch)
                    && matches!(cfg.vertex(*source_idx), Some(CfgVertex::Conditional(_)))
            });

            if from_conditional_false_branch && config.ignore_missing_else_on_exit {
                return false; // Missing else is ignored by config
            }

            basic_block_missing_return(vertex_idx, block, cfg, config)
        }
        CfgVertex::WhileLoop(loop_vertex) => {
            // Check if this is an endless loop (While Истина)
            if loop_vertex.is_endless() {
                return false; // Endless loops are assumed to always return inside
            }
            loop_vertex_missing_return(edge_type, config)
        }
        CfgVertex::ForLoop(_) | CfgVertex::ForEachLoop(_) => {
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
    config: &Config,
) -> bool {
    // Empty blocks can occur in:
    // - Missing else branches
    // - Exception handlers without returns
    // - Merge points where both branches have returns
    // - Loop exit points
    if block.is_empty() {
        // Check incoming edges to determine if this is truly a missing return
        let incoming_edges: Vec<_> = cfg.incoming_edges(vertex_idx).collect();

        // Check if this empty block comes from a loop's false branch
        // (loop didn't execute or completed without return)
        let has_false_branch_from_loop = incoming_edges.iter().any(|(source_idx, edge)| {
            matches!(edge, CfgEdgeType::FalseBranch)
                && matches!(
                    cfg.vertex(*source_idx),
                    Some(
                        CfgVertex::WhileLoop(_) | CfgVertex::ForLoop(_) | CfgVertex::ForEachLoop(_)
                    )
                )
        });

        if has_false_branch_from_loop && config.loops_executed_at_least_once {
            return false; // Loop assumed to execute at least once, false branch is OK
        }

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
    use crate::{test_utils::assert_diagnostic_range, DiagnosticsConfig};
    use ide_db::RootDatabase;
    use std::sync::Arc;

    /// Helper to run diagnostic on test code
    /// Returns (diagnostics, file_content_from_db) - file content is needed for range conversion
    fn check_diagnostic(code: &str, config: DiagnosticsConfig) -> (Vec<Diagnostic>, String) {
        use ide_db::base_db::SourceDatabase;
        use ide_db::RootDatabaseImpl;
        use test_fixture::Fixture;

        // Create fixture with test file
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        // Create database
        let mut db = RootDatabaseImpl::new();

        // Set file content in database from fixture
        // Also save the file content for range conversion
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        // Create diagnostics context
        // RootDatabase trait object is not Send/Sync (Salsa is single-threaded).
        // Arc is used for trait object lifetime management in tests, not thread-safety.
        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
        };

        // Run diagnostic
        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    /// Integration test matching Java test structure
    ///
    /// Based on AllFunctionPathMustHaveReturnDiagnosticTest.java
    /// Uses the same test file: AllFunctionPathMustHaveReturnDiagnostic.bsl
    #[test]
    fn test_all_function_path_must_have_return() {
        let code = include_str!("../../test_data/AllFunctionPathMustHaveReturnDiagnostic.bsl");

        // Test 1: Default behavior (loopsExecutedAtLeastOnce=true, ignoreMissingElseOnExit=false)
        // Expected: 2 diagnostics at lines 0 and 25
        {
            let config = DiagnosticsConfig::default();
            let (diagnostics, _file_content) = check_diagnostic(code, config);

            assert_eq!(diagnostics.len(), 2, "Default config: expected 2 diagnostics");

            // Line 0, columns 8-27: function ОпределитьСтавкуНДС (missing else branch)
            assert_eq!(diagnostics[0].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
            assert_eq!(diagnostics[0].severity, Severity::Warning);
            assert_diagnostic_range(&_file_content, &diagnostics[0], 0, 8, 27);

            // Line 25, columns 8-19: function СуммаСкидки (missing return in elsif)
            assert_eq!(diagnostics[1].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
            assert_eq!(diagnostics[1].severity, Severity::Warning);
            assert_diagnostic_range(&_file_content, &diagnostics[1], 25, 8, 19);
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

            let (diagnostics, _file_content) = check_diagnostic(code, config);

            assert_eq!(
                diagnostics.len(),
                3,
                "loopsExecutedAtLeastOnce=false: expected 3 diagnostics"
            );

            // Line 0, columns 8-27: ОпределитьСтавкуНДС
            assert_eq!(diagnostics[0].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
            assert_diagnostic_range(&_file_content, &diagnostics[0], 0, 8, 27);

            // Line 25, columns 8-19: СуммаСкидки
            assert_eq!(diagnostics[1].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
            assert_diagnostic_range(&_file_content, &diagnostics[1], 25, 8, 19);

            // Line 36, columns 8-23: ЦиклДляПроверки (loop may not execute)
            assert_eq!(diagnostics[2].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
            assert_diagnostic_range(&_file_content, &diagnostics[2], 36, 8, 23);
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

            let (diagnostics, _file_content) = check_diagnostic(code, config);

            assert_eq!(diagnostics.len(), 1, "ignoreMissingElseOnExit=true: expected 1 diagnostic");

            // Line 25, columns 8-19: СуммаСкидки (only this one, missing else is ignored)
            assert_eq!(diagnostics[0].code, DiagnosticCode::AllFunctionPathMustHaveReturn);
            assert_diagnostic_range(&_file_content, &diagnostics[0], 25, 8, 19);
        }

        // Test 4: Diagnostic can be disabled
        {
            let mut config = DiagnosticsConfig::default();
            config.disabled.push(DiagnosticCode::AllFunctionPathMustHaveReturn);

            let (diagnostics, _file_content) = check_diagnostic(code, config);

            assert_eq!(diagnostics.len(), 0, "Disabled diagnostic should not run");
        }
    }

    /// Test empty if bodies (from Java test: testEmptyIfBodies)
    #[test]
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

        let (diagnostics, _file_content) = check_diagnostic(code, config);

        assert_eq!(diagnostics.len(), 0, "Empty if bodies should not trigger diagnostic");
    }

    /// Test exit by raise exception (from Java test: testExitByRaiseException)
    #[test]
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

        let (diagnostics, _file_content) = check_diagnostic(code, config);

        assert_eq!(diagnostics.len(), 0, "Raise should count as exit");
    }
}
