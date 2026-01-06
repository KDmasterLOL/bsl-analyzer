//! Diagnostic helpers for lowering.
//!
//! This module contains helper functions for various diagnostics collected during lowering:
//! - Async call detection (CodeAfterAsyncCall)
//! - Transaction checking (BeginTransactionBeforeTryCatch)
//! - Deprecated method detection
//! - Duplicated code block detection

use std::collections::HashSet;

use syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::body::BodyDiagnostic;

use super::LoweringCtx;

// =============================================================================
// Duplicated code block detection
// =============================================================================

/// Check for duplicated code blocks in if/elsif/else branches.
///
/// Compares all pairs of branches and emits diagnostics for identical blocks.
pub(crate) fn check_duplicated_code_blocks(ctx: &mut LoweringCtx, branch_nodes: &[SyntaxNode]) {
    if branch_nodes.len() < 2 {
        return;
    }

    // Track which blocks we've already reported as duplicates
    let mut reported: HashSet<usize> = HashSet::new();

    // Compare all pairs of code blocks
    for i in 0..branch_nodes.len() - 1 {
        if reported.contains(&i) {
            continue;
        }

        let current_block = &branch_nodes[i];

        // Find all identical blocks after current one
        let mut has_duplicate = false;
        for (j, other_block) in branch_nodes.iter().enumerate().skip(i + 1) {
            // Skip empty blocks (both must be non-empty for comparison)
            if is_empty_block(current_block) && is_empty_block(other_block) {
                continue;
            }

            // Compare blocks structurally
            if are_blocks_identical(current_block, other_block) {
                has_duplicate = true;
                reported.insert(j);
            }
        }

        if has_duplicate {
            // Report diagnostic on the first block with duplicates
            ctx.emit(BodyDiagnostic::IfElseDuplicatedCodeBlock {
                range: current_block.text_range(),
            });
        }
    }
}

/// Check if a code block is empty (no children or only whitespace).
fn is_empty_block(block: &SyntaxNode) -> bool {
    block.children().next().is_none()
}

/// Compare two code blocks for structural equality.
///
/// Uses normalized text comparison (case-insensitive, whitespace-normalized).
fn are_blocks_identical(block1: &SyntaxNode, block2: &SyntaxNode) -> bool {
    // Normalize and compare text content
    let text1 = normalize_code_block(block1);
    let text2 = normalize_code_block(block2);

    if text1 != text2 {
        return false;
    }

    // Additional structural check: same number of statements
    let stmt_count1 = count_statements(block1);
    let stmt_count2 = count_statements(block2);

    stmt_count1 == stmt_count2 && stmt_count1 > 0
}

/// Normalize code block for comparison.
///
/// Removes whitespace and converts to lowercase (bilingual support).
fn normalize_code_block(block: &SyntaxNode) -> String {
    block
        .text()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

/// Count the number of statement nodes in a code block.
fn count_statements(block: &SyntaxNode) -> usize {
    block
        .descendants()
        .filter(|node| {
            matches!(
                node.kind(),
                SyntaxKind::CALL_STMT
                    | SyntaxKind::ASSIGN_STMT
                    | SyntaxKind::RETURN_STMT
                    | SyntaxKind::IF_STMT
                    | SyntaxKind::WHILE_STMT
                    | SyntaxKind::FOR_STMT
                    | SyntaxKind::BREAK_STMT
                    | SyntaxKind::CONTINUE_STMT
                    | SyntaxKind::RAISE_STMT
                    | SyntaxKind::TRY_STMT
            )
        })
        .count()
}

// =============================================================================
// Deprecated method detection
// =============================================================================

/// Check if a method name is deprecated (8.3.10 or 8.3.17).
/// Returns true if the method is deprecated.
pub(crate) fn is_deprecated_method(name: &str) -> bool {
    let lower = name.to_lowercase();

    // Deprecated methods from 8.3.10 and 8.3.17
    matches!(
        lower.as_str(),
        // 8.3.10 - Client application methods
        "установитькраткийзаголовокприложения"
            | "получитькраткийзаголовокприложения"
            | "установитьзаголовокклиентскогоприложения"
            | "получитьзаголовокклиентскогоприложения"
            | "текущийвариантосновногошрифтаклиентскогоприложения"
            | "текущийвариантинтерфейсаклиентскогоприложения"
            | "setshortapplicationcaption"
            | "getshortapplicationcaption"
            | "setclientapplicationcaption"
            | "getclientapplicationcaption"
            | "clientapplicationbasefontcurrentvariant"
            | "clientapplicationinterfacecurrentvariant"
            // 8.3.17 - Error handling methods
            | "краткоепредставлениеошибки"
            | "подробноепредставлениеошибки"
            | "показатьинформациюобошибке"
            | "brieferrorrepresentation"
            | "detailederrorrepresentation"
            | "showerrorinformation"
            // Common
            | "получитьформу"
            | "getform"
    )
}

// =============================================================================
// Transaction checking
// =============================================================================

/// Check if a statement is a global BeginTransaction/НачатьТранзакцию call.
///
/// Returns true if the statement is a non-qualified call to BeginTransaction/НачатьТранзакцию.
/// Filters out:
/// - Non-CALL_STMT nodes
/// - Qualified calls like `Connector.BeginTransaction()`
pub(crate) fn is_global_begin_transaction_call(node: &SyntaxNode) -> bool {
    // Must be CALL_STMT
    if node.kind() != SyntaxKind::CALL_STMT {
        return false;
    }

    // Skip if contains FIELD_EXPR (qualified call like Object.Method())
    if node.descendants().any(|n| n.kind() == SyntaxKind::FIELD_EXPR) {
        return false;
    }

    // Get first identifier token (method name)
    let ident = node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT);

    let Some(ident) = ident else {
        return false;
    };

    let name = ident.text().to_lowercase();
    name == "начатьтранзакцию" || name == "begintransaction"
}

/// Check if a node is inside a Try-Catch block body.
///
/// Walks up the AST tree looking for TRY_STMT ancestors.
pub(crate) fn is_inside_try_body(node: &SyntaxNode) -> bool {
    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        if parent.kind() == SyntaxKind::TRY_STMT {
            return true;
        }
        current = parent;
    }
    false
}

// =============================================================================
// CodeAfterAsyncCall diagnostic support
// =============================================================================

/// List of asynchronous methods that trigger CodeAfterAsyncCall diagnostic.
///
/// Contains 50 methods (25 Russian + 25 English):
/// - Dialog methods: ShowQueryBox/ПоказатьВопрос, ShowValue/ПоказатьЗначение, etc.
/// - Input methods: ShowInputNumber/ПоказатьВводЧисла, etc.
/// - File operations: BeginPutFile/НачатьПомещениеФайла, etc.
/// - Extension operations: BeginInstallAddIn/НачатьУстановкуВнешнейКомпоненты, etc.
const ASYNC_METHODS: &[&str] = &[
    // Russian names (25)
    "показатьвопрос",
    "показатьзначение",
    "показатьпредупреждение",
    "показатьвводдаты",
    "показатьвводзначения",
    "показатьвводстроки",
    "показатьвводчисла",
    "начатьустановкувнешнейкомпоненты",
    "начатьустановкурасширенияработысфайлами",
    "начатьустановкурасширенияработыскриптографией",
    "начатьподключениерасширенияработыскриптографией",
    "начатьподключениерасширенияработысфайлами",
    "начатьпомещениефайла",
    "начатькопированиефайла",
    "начатьперемещениефайла",
    "начатьпоискфайлов",
    "начатьудалениефайлов",
    "начатьсозданиекаталога",
    "начатьполучениекаталогавременныхфайлов",
    "начатьполучениекаталогадокументов",
    "начатьполучениерабочегокаталогаданныхпользователя",
    "начатьполучениефайлов",
    "начатьпомещениефайлов",
    "начатьзапросразрешенияпользователя",
    "начатьзапускприложения",
    // English names (25)
    "showquerybox",
    "showvalue",
    "showmessagebox",
    "showinputdate",
    "showinputvalue",
    "showinputstring",
    "showinputnumber",
    "begininstalladdin",
    "begininstallfilesystemextension",
    "begininstallcryptoextension",
    "beginattachingcryptoextension",
    "beginattachingfilesystemextension",
    "beginputfile",
    "begincopyingfile",
    "beginmovingfile",
    "beginfindingfiles",
    "begindeletingfiles",
    "begincreatingdirectory",
    "begingettingtempfilesdir",
    "begingettingdocumentsdir",
    "begingettinguserdataworkdir",
    "begingettingfiles",
    "beginputtingfiles",
    "beginrequestinguserpermission",
    "beginrunningapplication",
];

/// Check if a method name is an asynchronous method (case-insensitive).
fn is_async_method(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    ASYNC_METHODS.contains(&name_lower.as_str())
}

/// Check for CodeAfterAsyncCall diagnostic in a method body.
///
/// Finds all global async method calls and checks if there's executable code after them.
pub(crate) fn check_code_after_async_call(ctx: &mut LoweringCtx, stmt_list: &SyntaxNode) {
    // Find all async method calls in the statement list
    for node in stmt_list.descendants() {
        if node.kind() != SyntaxKind::CALL_STMT {
            continue;
        }

        // Check if this is a global async call
        if !is_global_async_call(&node) {
            continue;
        }

        // Get method name for diagnostic message
        let Some(method_name) = get_call_method_name(&node) else {
            continue;
        };

        // Check if there's code after this async call
        if has_code_after_async(&node) {
            let extended_range = extend_range_with_semicolon(&node, node.text_range());
            ctx.emit(BodyDiagnostic::CodeAfterAsyncCall { method_name, range: extended_range });
        }
    }
}

/// Check if a CALL_STMT is a global call to an async method.
///
/// Returns false for:
/// - Non-CALL_STMT nodes
/// - Qualified calls (Object.Method())
/// - Non-async methods
fn is_global_async_call(node: &SyntaxNode) -> bool {
    if node.kind() != SyntaxKind::CALL_STMT {
        return false;
    }

    // Find ARG_LIST position to only check call structure, not arguments
    let arg_list_start = node
        .descendants()
        .find(|n| n.kind() == SyntaxKind::ARG_LIST)
        .map(|n| n.text_range().start());

    // Check for FIELD_EXPR only BEFORE ARG_LIST (in the call target, not in arguments)
    // Qualified calls like Object.Method() have FIELD_EXPR before the ARG_LIST
    for child in node.descendants() {
        if child.kind() == SyntaxKind::FIELD_EXPR {
            if let Some(al_start) = arg_list_start {
                if child.text_range().start() < al_start {
                    return false;
                }
            } else {
                return false;
            }
        }
    }

    // Get method name and check if it's async
    let Some(name) = get_call_method_name(node) else {
        return false;
    };

    is_async_method(&name)
}

/// Extract method name from a CALL_STMT node.
fn get_call_method_name(node: &SyntaxNode) -> Option<String> {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
}

/// Check if there's executable code after an async call statement.
///
/// Algorithm:
/// 1. Check immediate siblings in the same block
/// 2. If first sibling is Return → false (safe exit)
/// 3. If first sibling is Break → check parent blocks
/// 4. Skip code inside exception handlers
/// 5. If any executable statement found → true
/// 6. Recursively check parent blocks for code after control structures
fn has_code_after_async(stmt: &SyntaxNode) -> bool {
    let Some(parent) = stmt.parent() else {
        return false;
    };

    let mut first_stmt_is_return = false;
    let mut first_stmt_is_break = false;
    let mut has_any_stmts = false;
    let mut in_exception_handler = false;

    let mut sibling = stmt.next_sibling();
    while let Some(next) = sibling {
        // Track exception handler boundaries
        if is_except_keyword(&next) {
            in_exception_handler = true;
        }
        if is_end_try_keyword(&next) {
            in_exception_handler = false;
        }

        // Skip code inside exception handlers
        if in_exception_handler {
            sibling = next.next_sibling();
            continue;
        }

        // Check if this is an executable statement or return/break
        if is_executable_statement(&next) || is_return_or_break(&next) {
            if !has_any_stmts {
                if next.kind() == SyntaxKind::RETURN_STMT {
                    first_stmt_is_return = true;
                } else if next.kind() == SyntaxKind::BREAK_STMT {
                    first_stmt_is_break = true;
                }
            }
            has_any_stmts = true;
        }

        sibling = next.next_sibling();
    }

    // If first statement is Return, it's a safe exit
    if first_stmt_is_return {
        return false;
    }

    // If there are statements and first is NOT break, that's an error
    // If first is break, still need to check parent
    let immediate_error = !first_stmt_is_break && has_any_stmts;
    immediate_error || check_parent_block_for_async(&parent)
}

/// Recursively check parent blocks for code after control structures containing the async call.
fn check_parent_block_for_async(node: &SyntaxNode) -> bool {
    let mut current = node.clone();

    loop {
        match current.kind() {
            SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT => {
                let mut sibling = current.next_sibling();
                while let Some(next) = sibling {
                    if is_else_clause(&next) {
                        sibling = next.next_sibling();
                        continue;
                    }

                    if is_return_or_break(&next) {
                        return false;
                    }

                    if is_executable_statement(&next) {
                        return true;
                    }

                    sibling = next.next_sibling();
                }

                if let Some(parent) = current.parent() {
                    current = parent;
                } else {
                    return false;
                }
            }

            SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF => {
                return false;
            }

            _ => {
                if let Some(parent) = current.parent() {
                    current = parent;
                } else {
                    return false;
                }
            }
        }
    }
}

/// Check if a node is an executable statement.
fn is_executable_statement(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::ASSIGN_STMT
            | SyntaxKind::CALL_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::EXECUTE_STMT
            | SyntaxKind::RAISE_STMT
    )
}

/// Check if a node is a Return or Break statement.
fn is_return_or_break(node: &SyntaxNode) -> bool {
    matches!(node.kind(), SyntaxKind::RETURN_STMT | SyntaxKind::BREAK_STMT)
}

/// Check if a node contains the EXCEPT keyword (starts exception handler).
fn is_except_keyword(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| t.kind() == SyntaxKind::KW_EXCEPT)
}

/// Check if a node contains the END_TRY keyword (ends try-except block).
fn is_end_try_keyword(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| t.kind() == SyntaxKind::KW_END_TRY)
}

/// Check if a node is an Else or ElseIf clause.
fn is_else_clause(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| matches!(t.kind(), SyntaxKind::KW_ELSIF | SyntaxKind::KW_ELSE))
}

/// Extend a text range to include the following semicolon token if present.
///
/// Java BSLParser.StatementContext includes the SEMICOLON in the statement range.
/// Our CALL_STMT does not include SEMICOLON (it's a separate token).
/// To match Java ranges, we extend the range to include the semicolon.
pub(crate) fn extend_range_with_semicolon(
    node: &SyntaxNode,
    original_range: TextRange,
) -> TextRange {
    if let Some(NodeOrToken::Token(token)) = node.next_sibling_or_token() {
        if token.kind() == SyntaxKind::SEMICOLON {
            return original_range.cover(token.text_range());
        }
    }
    original_range
}
