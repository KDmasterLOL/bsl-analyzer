//! MissingTempStorageDeletion diagnostic.
//!
//! Detects temporary storage data retrieved with GetFromTempStorage() that is not properly deleted.
//!
//! ## Why?
//!
//! Temporary storage data retrieved with `ПолучитьИзВременногоХранилища()` / `GetFromTempStorage()` must be
//! explicitly deleted after use with `УдалитьИзВременногоХранилища()` / `DeleteFromTempStorage()`.
//! Failure to delete temporary storage data can:
//! - Exhaust memory in temporary storage
//! - Cause performance degradation in 1C:Enterprise applications
//! - Leave sensitive data in memory longer than necessary
//!
//! ## Bad practice
//!
//! ```bsl
//! Процедура ОбработатьДанные(АдресТоваров)
//!     Товары = ПолучитьИзВременногоХранилища(АдресТоваров);
//!     // ... use data ...
//! КонецПроцедуры  // ❌ Temporary storage not cleaned up!
//! ```
//!
//! ## Good practice
//!
//! ```bsl
//! Процедура ОбработатьДанные(АдресТоваров)
//!     Товары = ПолучитьИзВременногоХранилища(АдресТоваров);
//!     Попытка
//!         // ... use data ...
//!     Исключение
//!         УдалитьИзВременногоХранилища(АдресТоваров);  // ✅ Clean up on error
//!         ВызватьИсключение;
//!     КонецПопытки;
//!     УдалитьИзВременногоХранилища(АдресТоваров);  // ✅ Clean up on success
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//!
//! This diagnostic has NO configuration parameters (unlike MissingTemporaryFileDeletion).
//! It can only be enabled/disabled:
//!
//! ```json
//! {
//!   "diagnostics": {
//!     "MissingTempStorageDeletion": true
//!   }
//! }
//! ```
//!
//! - **Enabled by default:** No (false)
//! - **Severity:** Critical
//! - **Tags:** STANDARD, PERFORMANCE, BADPRACTICE
//! - **Minutes to fix:** 3
//!
//! ## Implementation
//!
//! Ported from:
//! - MissingTempStorageDeletionDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//!
//! Key difference from MissingTemporaryFileDeletion:
//! - Uses STRUCTURAL AST EQUALITY for parameter comparison (not string matching)
//! - This allows matching `Результат.АдресРезультата` correctly

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use syntax::ast::{self, AstNode};
use syntax::{NodeOrToken, SyntaxKind, SyntaxNode, SyntaxToken};

/// Main entry point for MissingTempStorageDeletion diagnostic.
///
/// Detects temporary storage data retrieved with GetFromTempStorage() that is not deleted.
///
/// ## Algorithm
///
/// 1. Collect all tokens once (O(n) optimization)
/// 2. Find GetFromTempStorage calls using token pattern (IDENT + LPAREN without preceding DOT)
/// 3. For each call:
///    - Extract address parameter (full EXPR node for structural comparison)
///    - Find enclosing scope (method body or file-level)
///    - Search for DeleteFromTempStorage calls AFTER this call
///    - Check if any deletion uses the SAME address (structural equality)
///    - Create diagnostic if no matching deletion found
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("MissingTempStorageDeletion::check").entered();

    if ctx.config.is_disabled(DiagnosticCode::MissingTempStorageDeletion) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    // ✅ OPTIMIZATION: Collect tokens ONCE (O(n) instead of O(n²))
    let tokens: Vec<_> = root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    let mut diagnostics = Vec::new();

    // Find all global GetFromTempStorage calls using token pattern
    for (i, token) in tokens.iter().enumerate() {
        if token.kind() == SyntaxKind::IDENT {
            let next_is_lparen =
                tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

            if next_is_lparen {
                // Check if it's NOT a member access (Module.Method)
                let prev_is_dot = i
                    .checked_sub(1)
                    .and_then(|idx| tokens.get(idx))
                    .map(|t| t.kind() == SyntaxKind::DOT)
                    .unwrap_or(false);

                if !prev_is_dot && is_get_from_temp_storage(token.text()) {
                    tracing::trace!(
                        token_text = %token.text(),
                        range = ?token.text_range(),
                        "Found GetFromTempStorage call"
                    );

                    // Found GetFromTempStorage call
                    if let Some(diag) = check_temp_storage_usage(token) {
                        diagnostics.push(diag);
                    }
                }
            }
        }
    }

    tracing::debug!(count = diagnostics.len(), "MissingTempStorageDeletion diagnostics found");
    diagnostics
}

/// Check if token text is GetFromTempStorage (case-insensitive, bilingual)
fn is_get_from_temp_storage(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "получитьизвременногохранилища" || lower == "getfromtempstorage"
}

/// Check if token text is DeleteFromTempStorage (case-insensitive, bilingual)
fn is_delete_from_temp_storage(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "удалитьизвременногохранилища" || lower == "deletefromtempstorage"
}

/// Check temporary storage usage for a GetFromTempStorage call.
///
/// Returns diagnostic if no matching DeleteFromTempStorage is found after this call.
fn check_temp_storage_usage(call_token: &SyntaxToken) -> Option<Diagnostic> {
    // Extract address parameter (full EXPR node for structural comparison)
    let address_param = extract_address_parameter(call_token)?;

    tracing::trace!(
        address_param = %address_param,
        call_pos = ?call_token.text_range(),
        "Extracted address parameter"
    );

    // Find enclosing scope (method body or file-level)
    let parent = call_token.parent()?;
    let scope = find_enclosing_scope(parent)?;

    // Search for deletion calls after this point
    if has_deletion_call(&scope, call_token, &address_param) {
        tracing::trace!("Found matching deletion call");
        return None; // Data is deleted, no diagnostic
    }

    // No deletion found - create diagnostic
    tracing::trace!("No deletion found, creating diagnostic");

    // Range spans the full call expression
    let call_range = get_call_expression_range(call_token);

    Some(Diagnostic {
        code: DiagnosticCode::MissingTempStorageDeletion,
        message: "Нужно добавить удаление данных из временного хранилища после использования, вызвав \"УдалитьИзВременногоХранилища\"".to_string(),
        severity: Severity::Critical,
        range: call_range,
        tags: vec![],
        fixes: vec![],
    })
}

/// Extract address parameter from GetFromTempStorage call.
///
/// Returns the full EXPR node of the first argument (address parameter).
/// This is critical for structural comparison - we need the full AST subtree,
/// not just the text, to correctly match `Результат.АдресРезультата`.
fn extract_address_parameter(call_token: &SyntaxToken) -> Option<SyntaxNode> {
    // The token is wrapped in IDENT node, and ARG_LIST is a sibling
    // Structure: IDENT_node(IDENT_token) + ARG_LIST_node
    // We need to go: token -> IDENT_node -> find ARG_LIST sibling

    let ident_node = call_token.parent()?;

    // Find ARG_LIST among siblings
    for sibling in ident_node.siblings(syntax::Direction::Next) {
        if sibling.kind() == SyntaxKind::ARG_LIST {
            // Find first EXPR child (the address argument)
            return sibling.children().find(|child| child.kind() == SyntaxKind::EXPR);
        }
    }

    None
}

/// Find the enclosing scope (method body or file-level code).
///
/// Uses AST wrappers to find ProcedureDef or FunctionDef,
/// then extracts the statement list body.
/// If no method found, returns SOURCE_FILE for file-level code.
fn find_enclosing_scope(node: SyntaxNode) -> Option<SyntaxNode> {
    // Try method first
    for ancestor in node.ancestors() {
        if let Some(proc) = ast::ProcedureDef::cast(ancestor.clone()) {
            return proc.body().map(|b| b.syntax().clone());
        }

        if let Some(func) = ast::FunctionDef::cast(ancestor.clone()) {
            return func.body().map(|b| b.syntax().clone());
        }

        // File-level code
        if ancestor.kind() == SyntaxKind::SOURCE_FILE {
            return Some(ancestor);
        }
    }
    None
}

/// Check if there's a deletion call for the address after the GetFromTempStorage call.
///
/// Searches for DeleteFromTempStorage calls AFTER the get call (by byte offset)
/// that use the SAME address parameter (structural AST equality).
///
/// This is the CRITICAL difference from MissingTemporaryFileDeletion:
/// - Uses structural AST comparison, not string matching
/// - Allows matching `Результат.АдресРезультата` correctly
fn has_deletion_call(
    scope: &SyntaxNode,
    get_call_token: &SyntaxToken,
    address_param: &SyntaxNode,
) -> bool {
    let get_call_offset = get_call_token.text_range().end();

    // Collect all tokens once for efficiency
    let tokens: Vec<_> = scope.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    // Find all DeleteFromTempStorage calls AFTER the get call
    for (i, token) in tokens.iter().enumerate() {
        // Must come AFTER get call (by byte offset)
        if token.text_range().start() <= get_call_offset {
            continue;
        }

        if token.kind() == SyntaxKind::IDENT && is_delete_from_temp_storage(token.text()) {
            // Check if it's a call (next token is LPAREN)
            let next_is_lparen =
                tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

            // Check if it's NOT a member access (previous token is DOT)
            let prev_is_dot = i
                .checked_sub(1)
                .and_then(|idx| tokens.get(idx))
                .map(|t| t.kind() == SyntaxKind::DOT)
                .unwrap_or(false);

            if !prev_is_dot && next_is_lparen {
                // This is a global DeleteFromTempStorage call
                if let Some(delete_param) = extract_address_parameter(token) {
                    // CRITICAL: Structural AST equality check
                    if nodes_equal(address_param, &delete_param) {
                        tracing::trace!(
                            delete_pos = ?token.text_range(),
                            "Found matching DeleteFromTempStorage call"
                        );
                        return true; // Found match!
                    }
                }
            }
        }
    }

    false
}

/// Structural AST equality check for nodes.
///
/// This is the CRITICAL function that differentiates this diagnostic
/// from MissingTemporaryFileDeletion.
///
/// Performs recursive structural comparison of two AST subtrees:
/// - Node kinds must match
/// - All children must match (recursively)
/// - Token texts compared case-insensitively for IDENT
/// - Token texts compared exactly for STRING
///
/// This allows matching complex expressions like:
/// - `Результат.АдресРезультата` (member access)
/// - `ПолучитьАдрес()` (method call)
/// - Simple identifiers like `Адрес`
fn nodes_equal(left: &SyntaxNode, right: &SyntaxNode) -> bool {
    // Check node type
    if left.kind() != right.kind() {
        return false;
    }

    // Get all children (both nodes and tokens)
    let left_children: Vec<_> = left.children_with_tokens().collect();
    let right_children: Vec<_> = right.children_with_tokens().collect();

    if left_children.len() != right_children.len() {
        return false;
    }

    // Compare each child
    for (l, r) in left_children.iter().zip(right_children.iter()) {
        match (l, r) {
            (NodeOrToken::Token(lt), NodeOrToken::Token(rt)) => {
                if !tokens_equal(lt, rt) {
                    return false;
                }
            }
            (NodeOrToken::Node(ln), NodeOrToken::Node(rn)) => {
                if !nodes_equal(ln, rn) {
                    return false;
                }
            }
            _ => return false, // Token vs Node mismatch
        }
    }

    true
}

/// Token equality check.
///
/// - STRING tokens: exact match
/// - Other tokens (IDENT, keywords): case-insensitive match
fn tokens_equal(left: &SyntaxToken, right: &SyntaxToken) -> bool {
    if left.kind() != right.kind() {
        return false;
    }

    // STRING tokens: exact match
    if left.kind() == SyntaxKind::STRING {
        return left.text() == right.text();
    }

    // Identifiers/keywords: case-insensitive (handle both ASCII and Cyrillic)
    left.text().to_lowercase() == right.text().to_lowercase()
}

/// Get the range for the GetFromTempStorage call expression.
///
/// Spans from the method name to the closing paren.
fn get_call_expression_range(call_token: &SyntaxToken) -> TextRange {
    // Walk up to find the call expression (EXPR node containing the call with arguments)
    // Return the first EXPR node whose parent is NOT an EXPR
    let mut current = call_token.parent();

    while let Some(node) = current {
        if node.kind() == SyntaxKind::EXPR {
            // Check if parent is also EXPR
            if let Some(parent) = node.parent() {
                if parent.kind() != SyntaxKind::EXPR {
                    // Parent is not EXPR, so this is the outermost call expression
                    return node.text_range();
                }
            } else {
                // No parent, use this EXPR's range
                return node.text_range();
            }
        }
        current = node.parent();
    }

    // Fallback: just the token itself
    call_token.text_range()
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;

    #[test]
    fn test_missing_temp_storage_deletion() {
        let code = include_str!("../../test_data/MissingTempStorageDeletionDiagnostic.bsl");
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Expect 4 diagnostics
        assert_eq!(diagnostics.len(), 4, "Expected 4 diagnostics");

        // 0-indexed lines and columns (character positions)
        assert_diagnostic_range(code, &diagnostics[0], 3, 24, 77); // Line 4
        assert_diagnostic_range(code, &diagnostics[1], 13, 24, 77); // Line 14
        assert_diagnostic_range(code, &diagnostics[2], 21, 24, 77); // Line 22
        assert_diagnostic_range(code, &diagnostics[3], 33, 24, 77); // Line 34
    }

    #[test]
    fn test_structural_equality() {
        // Test that member access parameters work correctly
        let code = r#"
Процедура Тест()
    Настройки = ПолучитьИзВременногоХранилища(Результат.АдресРезультата);
    УдалитьИзВременногоХранилища(Результат.АдресРезультата);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Member access should match structurally");
    }

    #[test]
    fn test_different_parameters() {
        let code = r#"
Процедура Тест()
    Данные = ПолучитьИзВременногоХранилища(АдресТоваров);
    УдалитьИзВременногоХранилища(ДругойАдрес);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Different parameters should trigger error");
    }

    #[test]
    fn test_bilingual() {
        // Test both Russian and English
        let code = r#"
Procedure Test()
    Data = GetFromTempStorage(Address);
    DeleteFromTempStorage(Address);
EndProcedure
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "English keywords should work");
    }

    #[test]
    fn test_simple_valid_case() {
        let code = r#"
Процедура Тест()
    Адрес = "";
    Данные = ПолучитьИзВременногоХранилища(Адрес);
    ОбработатьДанные(Данные);
    УдалитьИзВременногоХранилища(Адрес);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not report when data is deleted");
    }

    #[test]
    fn test_simple_invalid_case() {
        let code = r#"
Процедура Тест()
    Адрес = "";
    Данные = ПолучитьИзВременногоХранилища(Адрес);
    ОбработатьДанные(Данные);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should report when data is not deleted");
    }

    #[test]
    fn test_delete_before_get() {
        // Delete before get should trigger error (wrong order)
        let code = r#"
Процедура Тест()
    Адрес = "";
    УдалитьИзВременногоХранилища(Адрес);
    Данные = ПолучитьИзВременногоХранилища(Адрес);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Delete before get should trigger error");
    }

    #[test]
    fn test_case_insensitive() {
        // Test case-insensitive matching
        let code = r#"
Процедура Тест()
    Адрес = "";
    Данные = ПОЛУЧИТЬИЗВРЕМЕННОГОХРАНИЛИЩА(адрес);
    ОбработатьДанные(Данные);
    удалитьизвременногохранилища(АДРЕС);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Should handle case-insensitive method names and parameters"
        );
    }
}
