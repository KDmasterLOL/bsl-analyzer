//! MissingTemporaryFileDeletion diagnostic.
//!
//! Detects temporary files created with GetTempFileName() that are not properly deleted.
//!
//! ## Why?
//!
//! Temporary files created with `ПолучитьИмяВременногоФайла()` / `GetTempFileName()` must be
//! explicitly deleted after use. Failure to delete temporary files can:
//! - Exhaust disk space in temp directory
//! - Leave sensitive data on disk
//! - Cause issues in long-running server processes
//!
//! ## Bad practice
//!
//! ```bsl
//! Процедура ОбработатьДанные()
//!     ИмяФайла = ПолучитьИмяВременногоФайла("xml");
//!     // ... use file ...
//! КонецПроцедуры  // ❌ Temporary file not deleted!
//! ```
//!
//! ## Good practice
//!
//! ```bsl
//! Процедура ОбработатьДанные()
//!     ИмяФайла = ПолучитьИмяВременногоФайла("xml");
//!     Попытка
//!         // ... use file ...
//!     Исключение
//!         УдалитьФайлы(ИмяФайла);  // ✅ Clean up on error
//!         ВызватьИсключение;
//!     КонецПопытки;
//!     УдалитьФайлы(ИмяФайла);  // ✅ Clean up on success
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//!
//! The diagnostic supports one configuration parameter:
//!
//! - **searchDeleteFileMethod** (string, regex pattern):
//!   Pipe-separated list of method names that delete/move files.
//!   Default: `"УдалитьФайлы|DeleteFiles|НачатьУдалениеФайлов|BeginDeletingFiles|ПереместитьФайл|MoveFile"`
//!
//! Example configuration:
//!
//! ```json
//! {
//!   "diagnostics": {
//!     "MissingTemporaryFileDeletion": {
//!       "searchDeleteFileMethod": "УдалитьФайлы|DeleteFiles|РаботаСФайламиКлиент.УдалитьФайл"
//!     }
//!   }
//! }
//! ```
//!
//! ## Configuration
//!
//! - **Enabled by default:** Yes
//! - **Severity:** Major (ERROR)
//! - **Tags:** BADPRACTICE, STANDARD
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//!
//! Ported from:
//! - MissingTemporaryFileDeletionDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use regex::Regex;
use syntax::ast::{self, AstNode};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// Default deletion methods pattern
const DEFAULT_SEARCH_DELETE_FILE_METHOD: &str =
    "УдалитьФайлы|DeleteFiles|НачатьУдалениеФайлов|BeginDeletingFiles|ПереместитьФайл|MoveFile";

/// Configuration for MissingTemporaryFileDeletion diagnostic
#[derive(Debug, Clone)]
struct Config {
    /// Regex pattern for deletion/move methods (case-insensitive)
    deletion_methods: Regex,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let pattern = ctx
            .config
            .get_string(DiagnosticCode::MissingTemporaryFileDeletion, "searchDeleteFileMethod")
            .unwrap_or(DEFAULT_SEARCH_DELETE_FILE_METHOD);

        // Create case-insensitive regex with error handling
        // Anchor with ^ and $ to match full method names only (not substrings)
        let regex_pattern = format!("(?i)^({})$", pattern);
        let deletion_methods = Regex::new(&regex_pattern).unwrap_or_else(|e| {
            tracing::warn!(
                error = %e,
                pattern = %pattern,
                "Invalid searchDeleteFileMethod regex, using default"
            );
            Regex::new(&format!("(?i)^({})$", DEFAULT_SEARCH_DELETE_FILE_METHOD))
                .expect("Default regex must be valid")
        });

        tracing::debug!(pattern = %pattern, "MissingTemporaryFileDeletion config loaded");
        Self { deletion_methods }
    }
}

/// Main entry point for MissingTemporaryFileDeletion diagnostic.
///
/// Detects temporary files created with GetTempFileName() that are not deleted.
///
/// ## Algorithm
///
/// 1. Collect all tokens once (O(n) optimization)
/// 2. Find GetTempFileName calls using token pattern (IDENT + LPAREN without preceding DOT)
/// 3. For each call:
///    - Extract variable name from assignment
///    - Find enclosing method body
///    - Search for deletion calls after GetTempFileName
///    - Create diagnostic if no deletion found
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("MissingTemporaryFileDeletion::check").entered();
    let code = DiagnosticCode::MissingTemporaryFileDeletion;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let parse = ctx.parse();
    let root = parse.syntax_node();

    // ✅ OPTIMIZATION: Collect tokens ONCE (O(n) instead of O(n²))
    let tokens: Vec<_> = root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    let mut diagnostics = Vec::new();

    // Find all global GetTempFileName calls using token pattern
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

                tracing::trace!(
                    i = i,
                    token_text = %token.text(),
                    next_is_lparen = next_is_lparen,
                    prev_is_dot = prev_is_dot,
                    is_get_temp_filename = is_get_temp_filename(token.text()),
                    "Checking IDENT + LPAREN pattern"
                );

                if !prev_is_dot && is_get_temp_filename(token.text()) {
                    tracing::trace!(token_text = %token.text(), "Found GetTempFileName call");
                    // Found GetTempFileName call
                    if let Some(diag) = check_temp_file_usage(token, &config, code, ctx) {
                        diagnostics.push(diag);
                    }
                }
            }
        }
    }

    tracing::debug!(count = diagnostics.len(), "MissingTemporaryFileDeletion diagnostics found");
    diagnostics
}

/// Check if token text is GetTempFileName (case-insensitive)
fn is_get_temp_filename(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "получитьимявременногофайла" || lower == "gettempfilename"
}

/// Check temporary file usage for a GetTempFileName call.
///
/// Returns diagnostic if:
/// - GetTempFileName called without assignment (inline usage) - ALWAYS error
/// - Variable is assigned but not deleted
/// - Returns None only if deletion is found
fn check_temp_file_usage(
    call_token: &SyntaxToken,
    config: &Config,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    // Extract variable name from assignment
    let var_name = match extract_variable_from_assignment(call_token) {
        Some(v) => v,
        None => {
            // No assignment found - inline usage like: Func(GetTempFileName())
            // This is ALWAYS an error (matches Java behavior)
            tracing::trace!(
                call_pos = ?call_token.text_range(),
                "GetTempFileName called without assignment (inline usage)"
            );

            let call_range = get_call_expression_range(call_token);

            return Some(Diagnostic {
                code,
                message: "Нужно добавить удаление временного файла после использования".to_string(),
                severity: ctx.severity(code),
                range: call_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    };

    tracing::trace!(
        var_name = %var_name,
        call_pos = ?call_token.text_range(),
        "Found GetTempFileName assignment"
    );

    // Find enclosing method/procedure body
    let parent = match call_token.parent() {
        Some(p) => p,
        None => {
            return None;
        }
    };

    let method_body = match find_enclosing_method_body(parent) {
        Some(mb) => mb,
        None => {
            return None;
        }
    };

    // Search for deletion calls after this point
    if has_deletion_call(&method_body, call_token, &var_name, config) {
        tracing::trace!(var_name = %var_name, "Found deletion call");
        return None; // File is deleted, no diagnostic
    }

    // No deletion found - create diagnostic
    tracing::trace!(var_name = %var_name, "No deletion found, creating diagnostic");

    // Range spans from method name to closing paren
    let call_range = get_call_expression_range(call_token);

    Some(Diagnostic {
        code,
        message: format!(
            "Нужно добавить удаление временного файла '{}' после использования",
            var_name
        ),
        severity: ctx.severity(code),
        range: call_range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

/// Extract variable name from assignment statement.
///
/// Pattern: `Variable = GetTempFileName(...)`
///
/// Returns the left-hand side identifier ONLY if GetTempFileName is the
/// direct right-hand side of the assignment.
///
/// Returns None for inline usage:
/// - `Func(GetTempFileName())` - no assignment
/// - `Var = Func(GetTempFileName())` - GetTempFileName inside RHS expression
fn extract_variable_from_assignment(call_token: &SyntaxToken) -> Option<String> {
    // Walk up to find ASSIGN_STMT
    let mut current = call_token.parent();
    while let Some(node) = current {
        if node.kind() == SyntaxKind::ASSIGN_STMT {
            // ASSIGN_STMT structure: LHS EQ RHS
            // Find EQ token position
            let eq_pos = node.children_with_tokens().position(|el| {
                el.as_token().map(|t| t.kind() == SyntaxKind::EQ).unwrap_or(false)
            })?;

            // Check if GetTempFileName is the DIRECT RHS (not nested inside)
            // Strategy: GetTempFileName should be in the first EXPR after EQ
            let rhs_expr = node
                .children_with_tokens()
                .skip(eq_pos + 1) // Skip to after EQ
                .find_map(|el| el.as_node().filter(|n| n.kind() == SyntaxKind::EXPR).cloned())?;

            // Check if call_token is a direct descendant of the first RHS EXPR
            // If it's nested deeper (e.g., inside Новый Файл(...)), it's inline usage
            let mut depth = 0;
            let mut check_node = call_token.parent();
            while let Some(n) = check_node {
                if n == rhs_expr {
                    // Found the RHS EXPR
                    // If depth > a few levels, it's nested (inline usage)
                    // Direct assignment: Var = GetTempFileName() has depth ~4-5
                    // Nested: Var = Func(GetTempFileName()) has depth > 5
                    if depth > 5 {
                        return None; // Inline usage (too deep)
                    }
                    break;
                }
                depth += 1;
                check_node = n.parent();
            }

            // Get first IDENT before EQ by searching recursively in child nodes
            // The variable might be wrapped in EXPR nodes, so we need to search descendants
            let result = node.children_with_tokens().take(eq_pos).find_map(|el| {
                // For direct tokens, check if it's IDENT
                if let Some(token) = el.as_token() {
                    if token.kind() == SyntaxKind::IDENT {
                        return Some(token.text().to_string());
                    }
                }
                // For nodes (like EXPR), search recursively for IDENT
                if let Some(child_node) = el.as_node() {
                    return child_node
                        .descendants_with_tokens()
                        .filter_map(|desc| desc.into_token())
                        .find(|t| t.kind() == SyntaxKind::IDENT)
                        .map(|t| t.text().to_string());
                }
                None
            });

            return result;
        }
        current = node.parent();
    }

    // If no assignment found, return None
    // This handles cases like: `Func(GetTempFileName())` - inline usage
    None
}

/// Find the enclosing method/procedure body.
///
/// Uses AST wrappers to find ProcedureDef or FunctionDef,
/// then extracts the statement list body.
fn find_enclosing_method_body(node: SyntaxNode) -> Option<SyntaxNode> {
    // Use .ancestors() to walk up the tree
    for ancestor in node.ancestors() {
        // Try casting to ProcedureDef
        if let Some(proc) = ast::ProcedureDef::cast(ancestor.clone()) {
            return proc.body().map(|b| b.syntax().clone());
        }

        // Try casting to FunctionDef
        if let Some(func) = ast::FunctionDef::cast(ancestor.clone()) {
            return func.body().map(|b| b.syntax().clone());
        }
    }
    None
}

/// Check if there's a deletion call for the variable after the GetTempFileName call.
///
/// Searches for:
/// 1. Global deletion methods: `DeleteFiles(var)`
/// 2. Module-qualified methods: `Module.DeleteFile(var)`
///
/// Only checks calls AFTER the GetTempFileName call (same scope).
fn has_deletion_call(
    method_body: &SyntaxNode,
    get_temp_call_token: &SyntaxToken,
    var_name: &str,
    config: &Config,
) -> bool {
    let temp_call_offset = get_temp_call_token.text_range().start();

    // Find all ARG_LIST nodes (indicates method calls)
    for node in method_body.descendants() {
        // Only check nodes AFTER the GetTempFileName call
        if node.text_range().start() <= temp_call_offset {
            continue;
        }

        if node.kind() == SyntaxKind::ARG_LIST {
            if let Some(parent) = node.parent() {
                // Extract method name (handles both global and qualified calls)
                let method_path = extract_method_path(&parent);

                tracing::trace!(
                    method_path = %method_path,
                    var_name = %var_name,
                    range = ?parent.text_range(),
                    "Checking deletion call candidate"
                );

                // Check if method matches deletion pattern
                if config.deletion_methods.is_match(&method_path) {
                    // Check if variable is used in this call
                    if call_uses_variable(&parent, var_name) {
                        tracing::trace!(
                            method_path = %method_path,
                            var_name = %var_name,
                            "Found matching deletion call"
                        );
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Extract method path from a call node.
///
/// Examples:
/// - `УдалитьФайлы(...)` → "УдалитьФайлы"
/// - `РаботаСФайламиКлиент.УдалитьФайл(...)` → "РаботаСФайламиКлиент.УдалитьФайл"
/// - `Справочники.Модуль.Удалить(...)` → "Справочники.Модуль.Удалить"
///
/// Pattern from MissingCommonModuleMethod: collect IDENT tokens before ARG_LIST.
fn extract_method_path(call_node: &SyntaxNode) -> String {
    let mut idents: Vec<String> = Vec::new();

    for child in call_node.children_with_tokens() {
        // Stop at ARG_LIST
        if child.kind() == SyntaxKind::ARG_LIST {
            break;
        }

        // Collect IDENT tokens
        if let Some(element) = child.as_node() {
            for token in element.descendants_with_tokens() {
                if let Some(t) = token.as_token() {
                    if t.kind() == SyntaxKind::IDENT {
                        idents.push(t.text().to_string());
                    }
                }
            }
        } else if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                idents.push(token.text().to_string());
            }
        }
    }

    // Join with dots: ["Module", "Method"] → "Module.Method"
    idents.join(".")
}

/// Check if a call uses the specified variable in its arguments.
///
/// Performs case-insensitive comparison (BSL standard).
fn call_uses_variable(call_node: &SyntaxNode, var_name: &str) -> bool {
    // Check all IDENT tokens in the call's arguments
    for token in call_node.descendants_with_tokens() {
        if let Some(t) = token.as_token() {
            if t.kind() == SyntaxKind::IDENT && t.text().eq_ignore_ascii_case(var_name) {
                return true;
            }
        }
    }
    false
}

/// Get the range for the GetTempFileName call expression.
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
    use crate::{DiagnosticCode, DiagnosticsConfig};

    #[test]
    fn test_default_config() {
        let code = include_str!("../../test_data/MissingTemporaryFileDeletionDiagnostic.bsl");
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Expect 7 diagnostics with default configuration
        assert_eq!(diagnostics.len(), 7, "Expected 7 diagnostics with default config");

        // Verify exact positions match Java implementation
        // Line 6: ИмяПромежуточногоФайла = ПолучитьИмяВременногоФайла("xml")
        assert_diagnostic_range(code, &diagnostics[0], 6, 29, 62);

        // Line 19: ИмяПромежуточногоФайла4 = ПолучитьИмяВременногоФайла("xml")
        assert_diagnostic_range(code, &diagnostics[1], 19, 30, 63);

        // Line 25: ИмяПромежуточногоФайла5 = ПолучитьИмяВременногоФайла("xml")
        assert_diagnostic_range(code, &diagnostics[2], 25, 30, 63);

        // Line 45: ИмяПромежуточногоФайла = ПолучитьИмяВременногоФайла("xml")
        assert_diagnostic_range(code, &diagnostics[3], 45, 29, 62);

        // Line 49: ИмяПромежуточногоФайла2 = ПолучитьИмяВременногоФайла("txt")
        assert_diagnostic_range(code, &diagnostics[4], 49, 30, 63);

        // Line 64: ИмяПромежуточногоФайла3 = ПолучитьИмяВременногоФайла()
        assert_diagnostic_range(code, &diagnostics[5], 64, 30, 58);

        // Line 71: ИмяФайлаНаДиске = ПолучитьИмяВременногоФайла()
        assert_diagnostic_range(code, &diagnostics[6], 71, 26, 54);
    }

    #[test]
    fn test_extended_config() {
        let code = include_str!("../../test_data/MissingTemporaryFileDeletionDiagnostic.bsl");

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingTemporaryFileDeletion,
            serde_json::json!({
                "searchDeleteFileMethod": "УдалитьФайлы|DeleteFiles|НачатьУдалениеФайлов|BeginDeletingFiles|ПереместитьФайл|MoveFile|РаботаСФайламиСлужебныйКлиент.УдалитьФайл|Справочники.ОбщийМодуль.УдалитьВсеФайлы"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Expect 5 diagnostics with extended configuration
        assert_eq!(diagnostics.len(), 5, "Expected 5 diagnostics with extended config");

        // Lines 19 and 45 should no longer trigger (custom methods recognized)
        assert_diagnostic_range(code, &diagnostics[0], 6, 29, 62);
        assert_diagnostic_range(code, &diagnostics[1], 25, 30, 63);
        assert_diagnostic_range(code, &diagnostics[2], 49, 30, 63);
        assert_diagnostic_range(code, &diagnostics[3], 64, 30, 58);
        assert_diagnostic_range(code, &diagnostics[4], 71, 26, 54);
    }

    #[test]
    fn test_restrictive_config() {
        let code = include_str!("../../test_data/MissingTemporaryFileDeletionDiagnostic.bsl");

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingTemporaryFileDeletion,
            serde_json::json!({
                "searchDeleteFileMethod": "УдалитьФайл|DeleteFile|НачатьУдалениеФайловВсех|ОбщийМодуль.УдалитьВсеФайлы"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Debug: print all diagnostics
        println!("Got {} diagnostics:", diagnostics.len());
        for (i, d) in diagnostics.iter().enumerate() {
            let (line, start_col, _end_line, end_col) = range_to_line_col(code, d.range);
            println!("  {}: line {} col {}..{}", i + 1, line + 1, start_col, end_col);
        }

        // Expect 12 diagnostics with restrictive configuration
        assert_eq!(diagnostics.len(), 12, "Expected 12 diagnostics with restrictive config");
    }

    #[test]
    fn test_range_debug() {
        let code = r#"
Процедура Тест()
    ИмяПромежуточногоФайла = ПолучитьИмяВременногоФайла("xml"); // ошибка
КонецПроцедуры
        "#;
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert_eq!(diagnostics.len(), 1);
        let d = &diagnostics[0];
        println!("Range: {:?}", d.range);
        println!("Range start: {:?}, end: {:?}", d.range.start(), d.range.end());

        // Find the line
        let lines: Vec<&str> = code.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.contains("ПолучитьИмяВременногоФайла") {
                println!("Line {}: {}", i + 1, line);
                println!("Line length: {}", line.len());
            }
        }
    }

    #[test]
    fn test_debug() {
        use ide_db::base_db::{RootQueryDb, SourceDatabase};
        use ide_db::{RootDatabase, RootDatabaseImpl};
        use std::rc::Rc;
        use test_fixture::Fixture;

        // Debug test to see what tokens we're getting
        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
            КонецПроцедуры
        "#;
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let parse = db.parse(file_id);
        let root = parse.syntax_node();

        // Print all tokens to see what we have
        let tokens: Vec<_> =
            root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

        println!("\n=== TOKENS ===");
        for (i, token) in tokens.iter().enumerate() {
            println!("{}: {:?} = '{}'", i, token.kind(), token.text());
        }

        // Now run the actual diagnostic
        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = crate::DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = super::check(&ctx);
        println!("\n=== DIAGNOSTICS ===");
        println!("Found {} diagnostics", diagnostics.len());
        for diag in &diagnostics {
            println!("{:?}", diag);
        }
    }

    #[test]
    fn test_inline_usage() {
        // Java version creates diagnostic for inline GetTempFileName usage (without assignment)
        // Example: Func(GetTempFileName("xml"))
        // This is ALWAYS an error because deletion cannot be tracked

        // Test 1: Pure inline usage without any assignment
        let code = r#"
            Процедура Тест()
                Записать(GetTempFileName("txt"));
                ПолучитьИмяВременногоФайла("xml");  // standalone call
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            2,
            "Should create diagnostic for inline usage (matches Java behavior)"
        );

        // Both should have generic message (no variable name)
        for d in &diagnostics {
            assert_eq!(d.message, "Нужно добавить удаление временного файла после использования");
        }

        // Test 2: Inline usage inside expression
        let code2 = r#"
            Процедура Тест()
                Файл = Новый Файл(ПолучитьИмяВременногоФайла("xml"));
            КонецПроцедуры
        "#;
        let diagnostics2 = check_ast_diagnostic(code2, check);

        // This creates ONE diagnostic for the GetTempFileName call (inline usage)
        // Note: "Файл" is assigned but GetTempFileName itself has no assignment
        assert_eq!(diagnostics2.len(), 1, "Should create diagnostic for inline GetTempFileName");
    }

    #[test]
    fn test_comprehensive_java_compatibility() {
        // Comprehensive test to ensure 100% compatibility with Java implementation
        let code = r#"
            Процедура ТестВсехКейсов()
                // Case 1: Normal assignment with deletion - OK
                Файл1 = ПолучитьИмяВременногоФайла("xml");
                УдалитьФайлы(Файл1);

                // Case 2: Normal assignment without deletion - ERROR
                Файл2 = ПолучитьИмяВременногоФайла("xml");

                // Case 3: Inline usage in function call - ERROR
                Записать(GetTempFileName("txt"));

                // Case 4: Inline usage in expression - ERROR
                Файл3 = Новый Файл(ПолучитьИмяВременногоФайла("doc"));

                // Case 5: Standalone call - ERROR
                ПолучитьИмяВременногоФайла("tmp");

                // Case 6: Assignment with move (not deletion) - OK with default config
                Файл4 = ПолучитьИмяВременногоФайла("xml");
                ПереместитьФайл(Файл4, "новое_имя.xml");
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);

        // Expected errors:
        // - Файл2 (no deletion)
        // - GetTempFileName in Записать() (inline)
        // - ПолучитьИмяВременногоФайла in Новый Файл() (inline)
        // - Standalone ПолучитьИмяВременногоФайла (inline)
        // Total: 4 diagnostics

        println!("Found {} diagnostics:", diagnostics.len());
        for (i, d) in diagnostics.iter().enumerate() {
            println!("  {}: {}", i + 1, d.message);
        }

        assert_eq!(diagnostics.len(), 4, "Should find exactly 4 errors (3 inline + 1 no deletion)");

        // Verify messages
        let inline_count = diagnostics
            .iter()
            .filter(|d| d.message == "Нужно добавить удаление временного файла после использования")
            .count();

        let var_count = diagnostics.iter().filter(|d| d.message.contains("Файл")).count();

        assert_eq!(inline_count, 3, "Should have 3 inline usage errors");
        assert_eq!(var_count, 1, "Should have 1 variable without deletion error");
    }

    #[test]
    fn test_simple_cases() {
        // Valid: file is deleted
        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
                УдалитьФайлы(ИмяФайла);
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not report when file is deleted");

        // Invalid: file not deleted
        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should report when file is not deleted");

        // Valid: file moved
        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
                ПереместитьФайл(ИмяФайла, "новое_имя.xml");
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not report when file is moved");
    }

    #[test]
    fn test_case_insensitive() {
        // Test case-insensitive matching for GetTempFileName
        let code = r#"
            Процедура Тест()
                Файл1 = ПОЛУЧИТЬИМЯВРЕМЕННОГОФАЙЛА("xml");
                Файл2 = получитьимявременногофайла("xml");
                Файл3 = ПолучитьИмяВременногоФайла("xml");
                УдалитьФайлы(Файл3);
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        // Only Файл1 and Файл2 should trigger (Файл3 is deleted)
        assert_eq!(diagnostics.len(), 2, "Should handle case-insensitive GetTempFileName");
    }

    #[test]
    fn test_english_keywords() {
        // Test English keywords
        let code = r#"
            Procedure Test()
                TempFile = GetTempFileName("xml");
            EndProcedure
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect English GetTempFileName");

        // Test English deletion method
        let code = r#"
            Procedure Test()
                TempFile = GetTempFileName("xml");
                DeleteFiles(TempFile);
            EndProcedure
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should recognize English DeleteFiles");
    }

    #[test]
    fn test_module_qualified_calls() {
        // Test module-qualified deletion methods
        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
                РаботаСФайламиКлиент.УдалитьФайл(Неопределено, ИмяФайла);
            КонецПроцедуры
        "#;
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        // Should report error - РаботаСФайламиКлиент.УдалитьФайл not in default config
        assert_eq!(diagnostics.len(), 1, "Custom method not in default config");

        // Now add it to config
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingTemporaryFileDeletion,
            serde_json::json!({
                "searchDeleteFileMethod": "УдалитьФайлы|DeleteFiles|РаботаСФайламиКлиент.УдалитьФайл"
            }),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 0, "Custom method recognized in config");
    }
}
