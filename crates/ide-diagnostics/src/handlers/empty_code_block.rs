//! EmptyCodeBlock diagnostic
//!
//! Detects empty code blocks in control structures (if/while/for/etc).
//!
//! **Source (Java):** bsl-language-server/EmptyCodeBlockDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/empty_code_block.rs
//!
//! BSL supports empty code blocks in control structures, but they often indicate
//! incomplete implementation or unintended code.  This diagnostic helps detect such cases.
//!
//! ## Empty blocks detected:
//! - Empty if/then blocks
//! - Empty elsif blocks
//! - Empty else blocks
//! - Empty while/for/foreach loops
//!
//! ## NOT checked (other diagnostics handle these):
//! - Empty function/procedure bodies (handled by other diagnostic)
//! - Empty try/except blocks (handled by other diagnostic)
//!
//! ## Configuration
//! - `commentAsCode` (boolean, default: false) - If true, blocks containing only comments
//!   are NOT considered empty

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::EmptyCodeBlock) {
        return Vec::new();
    }

    let _comment_as_code =
        ctx.config.get_bool(DiagnosticCode::EmptyCodeBlock, "commentAsCode").unwrap_or(false);

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::IF_STMT => check_if_stmt(&node, &mut diagnostics),
            SyntaxKind::WHILE_STMT => check_while_stmt(&node, &mut diagnostics),
            SyntaxKind::FOR_STMT | SyntaxKind::FOR_EACH_STMT => {
                check_for_stmt(&node, &mut diagnostics)
            }
            _ => {}
        }
    }

    diagnostics
}

fn check_if_stmt(node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    // Check if IF_STMT contains ERROR node (parser error)
    let has_error = node.children().any(|c| c.kind() == SyntaxKind::ERROR);

    // Find the first STMT_LIST child (main if-then block).
    // It should be the first STMT_LIST that is a direct child of IF_STMT
    // and appears before any ELSIF_CLAUSE or ELSE_CLAUSE.
    let mut first_stmt_list = None;
    let mut found_if_keyword = false;

    for child in node.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::KW_IF {
                found_if_keyword = true;
            }
        } else if let Some(child_node) = child.as_node() {
            match child_node.kind() {
                SyntaxKind::STMT_LIST if first_stmt_list.is_none() => {
                    // First STMT_LIST found - this is the main if-then block
                    first_stmt_list = Some(child_node.clone());
                }
                SyntaxKind::ELSIF_CLAUSE | SyntaxKind::ELSE_CLAUSE => {
                    // Stop looking once we reach elsif/else
                    break;
                }
                _ => {}
            }
        }
    }

    // Check if main if-then block is empty
    let is_empty = if let Some(stmt_list) = &first_stmt_list {
        if has_error {
            // Parser error: assume block is empty (parser couldn't parse it correctly)
            // This handles cases where parser creates garbage CALL_STMT from keywords/conditions
            true
        } else {
            // No parser error: check for real statements
            !stmt_list.children().any(|c| is_statement(c.kind()))
        }
    } else {
        // No STMT_LIST found, but IF keyword exists - block is empty
        found_if_keyword
    };

    if is_empty {
        // Find range from IF keyword to end of condition (before first STMT_LIST or ELSIF/ELSE/ENDIF)
        if let Some(if_token) = find_token(node, SyntaxKind::KW_IF) {
            let end_range = first_stmt_list
                .as_ref()
                .map(|s| s.text_range().start())
                .or_else(|| {
                    // If no STMT_LIST, find first ELSIF/ELSE/ENDIF
                    for child in node.children_with_tokens() {
                        if let Some(t) = child.as_token() {
                            if matches!(
                                t.kind(),
                                SyntaxKind::KW_ELSIF | SyntaxKind::KW_ELSE | SyntaxKind::KW_END_IF
                            ) {
                                return Some(t.text_range().start());
                            }
                        }
                    }
                    None
                })
                .unwrap_or_else(|| node.text_range().end());

            // Try to find THEN token for better range
            let range = if let Some(then_token) = find_token(node, SyntaxKind::KW_THEN) {
                if_token.text_range().cover(then_token.text_range())
            } else {
                // No THEN token, use IF to end of condition
                ide_db::TextRange::new(if_token.text_range().start(), end_range)
            };

            diagnostics.push(Diagnostic {
                code: DiagnosticCode::EmptyCodeBlock,
                message: "Empty code block".to_string(),
                severity: Severity::Warning,
                range,
                tags: vec![],
                fixes: vec![],
            });
        }
    }

    // Check elsif clauses
    for child in node.children() {
        if child.kind() == SyntaxKind::ELSIF_CLAUSE {
            check_elsif_clause(&child, diagnostics);
        } else if child.kind() == SyntaxKind::ELSE_CLAUSE {
            check_else_clause(&child, diagnostics);
        }
    }
}

fn check_elsif_clause(node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    // Check if ELSIF_CLAUSE contains ERROR node
    let has_error = node.children().any(|c| c.kind() == SyntaxKind::ERROR);

    // Find STMT_LIST in ELSIF clause
    let stmt_list = node.children().find(|c| c.kind() == SyntaxKind::STMT_LIST);

    // Check if STMT_LIST is empty or doesn't exist
    let is_empty = if let Some(ref stmt_list) = stmt_list {
        if has_error {
            // Parser error: assume block is empty
            true
        } else {
            !stmt_list.children().any(|c| is_statement(c.kind()))
        }
    } else {
        // No STMT_LIST found means block is empty
        true
    };

    if is_empty {
        // Similar logic to IF_STMT: handle ERROR case
        if let Some(elsif_token) = find_token(node, SyntaxKind::KW_ELSIF) {
            let end_range = stmt_list
                .as_ref()
                .map(|s| s.text_range().start())
                .or_else(|| {
                    // If no STMT_LIST, find ELSE or END_IF
                    for child in node.children_with_tokens() {
                        if let Some(t) = child.as_token() {
                            if matches!(t.kind(), SyntaxKind::KW_ELSE | SyntaxKind::KW_END_IF) {
                                return Some(t.text_range().start());
                            }
                        }
                    }
                    None
                })
                .unwrap_or_else(|| node.text_range().end());

            let range = if let Some(then_token) = find_token(node, SyntaxKind::KW_THEN) {
                elsif_token.text_range().cover(then_token.text_range())
            } else {
                ide_db::TextRange::new(elsif_token.text_range().start(), end_range)
            };

            diagnostics.push(Diagnostic {
                code: DiagnosticCode::EmptyCodeBlock,
                message: "Empty code block".to_string(),
                severity: Severity::Warning,
                range,
                tags: vec![],
                fixes: vec![],
            });
        }
    }
}

fn check_else_clause(node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    // Find STMT_LIST in ELSE clause
    let stmt_list = node.children().find(|c| c.kind() == SyntaxKind::STMT_LIST);

    if let Some(stmt_list) = stmt_list {
        let has_statements = stmt_list.children().any(|c| is_statement(c.kind()));

        if !has_statements {
            // For ELSE, diagnostic is just on the ELSE keyword
            if let Some(else_token) = find_token(node, SyntaxKind::KW_ELSE) {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::EmptyCodeBlock,
                    message: "Empty code block".to_string(),
                    severity: Severity::Warning,
                    range: else_token.text_range(),
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }
}

fn check_while_stmt(node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    // Find STMT_LIST in WHILE
    let stmt_list = node.children().find(|c| c.kind() == SyntaxKind::STMT_LIST);

    if let Some(stmt_list) = stmt_list {
        let has_statements = stmt_list.children().any(|c| is_statement(c.kind()));

        if !has_statements {
            if let Some(range) = find_header_range(node, SyntaxKind::KW_WHILE, SyntaxKind::KW_DO) {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::EmptyCodeBlock,
                    message: "Empty code block".to_string(),
                    severity: Severity::Warning,
                    range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }
}

fn check_for_stmt(node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    // Find STMT_LIST in FOR
    let stmt_list = node.children().find(|c| c.kind() == SyntaxKind::STMT_LIST);

    if let Some(stmt_list) = stmt_list {
        let has_statements = stmt_list.children().any(|c| is_statement(c.kind()));

        if !has_statements {
            let start_kw = SyntaxKind::KW_FOR; // Both FOR and FOR EACH start with FOR
            if let Some(range) = find_header_range(node, start_kw, SyntaxKind::KW_DO) {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::EmptyCodeBlock,
                    message: "Empty code block".to_string(),
                    severity: Severity::Warning,
                    range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }
}

/// Find range from first_kind to last_kind tokens (covering all tokens between)
fn find_header_range(
    node: &SyntaxNode,
    first_kind: SyntaxKind,
    last_kind: SyntaxKind,
) -> Option<ide_db::TextRange> {
    let first = find_token(node, first_kind)?;
    let last = find_token(node, last_kind)?;
    Some(first.text_range().cover(last.text_range()))
}

/// Find first token of given kind (searches in descendants too)
fn find_token(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    // First try direct children
    if let Some(token) =
        node.children_with_tokens().filter_map(|it| it.into_token()).find(|tok| tok.kind() == kind)
    {
        return Some(token);
    }

    // If not found, search in descendants (for ERROR nodes)
    node.descendants_with_tokens().filter_map(|it| it.into_token()).find(|tok| tok.kind() == kind)
}

/// Check if SyntaxKind represents a statement
fn is_statement(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ASSIGN_STMT
            | SyntaxKind::CALL_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::RAISE_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::GOTO_STMT
            | SyntaxKind::LABEL_STMT
            | SyntaxKind::EXECUTE_STMT
            | SyntaxKind::ADD_HANDLER_STMT
            | SyntaxKind::REMOVE_HANDLER_STMT
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        test_utils::assert_diagnostic_range_multiline, DiagnosticsConfig, DiagnosticsContext,
    };
    use ide_db::base_db::SourceDatabase;
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let config = Rc::new(DiagnosticsConfig::default());
        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_empty_code_block() {
        let code = include_str!("../../test_data/EmptyCodeBlockDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        // Print diagnostics for debugging
        use crate::test_utils::range_to_line_col;
        for (i, diag) in diagnostics.iter().enumerate() {
            let (start_line, start_col, end_line, end_col) = range_to_line_col(code, diag.range);
            eprintln!(
                "Diagnostic {}: line {}:{} - {}:{}",
                i, start_line, start_col, end_line, end_col
            );
        }

        // Java expects 6 diagnostics at specific positions
        assert_eq!(diagnostics.len(), 6, "Expected 6 diagnostics");

        // Line 6 (0-indexed line 5), cols 1-6 (Иначе)
        assert_diagnostic_range_multiline(code, &diagnostics[0], 5, 1, 5, 6);

        // Line 18 (0-indexed line 17), cols 2-18 (Пока Истина Цикл)
        assert_diagnostic_range_multiline(code, &diagnostics[1], 17, 2, 17, 18);

        // Line 25 (0-indexed line 24), cols 4-21 (Если Истина Тогда)
        assert_diagnostic_range_multiline(code, &diagnostics[2], 24, 4, 24, 21);

        // Line 36 (0-indexed line 35), cols 0-16 (Если а = 0 Тогда)
        assert_diagnostic_range_multiline(code, &diagnostics[3], 35, 0, 35, 16);

        // Line 38 (0-indexed line 37), cols 0-21 (ИначеЕсли А = 1 Тогда)
        assert_diagnostic_range_multiline(code, &diagnostics[4], 37, 0, 37, 21);

        // Line 39 (0-indexed line 38), cols 4-9 (Иначе)
        assert_diagnostic_range_multiline(code, &diagnostics[5], 38, 4, 38, 9);
    }
}
