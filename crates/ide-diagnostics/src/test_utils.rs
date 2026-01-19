//! Common test utilities for diagnostic tests.
//!
//! This module provides helper functions for testing diagnostics,
//! including position calculation and assertion helpers.

use crate::Diagnostic;
use ide_db::TextRange;

/// Convert TextRange (byte offsets) to (line, column) positions.
///
/// Handles UTF-8 properly by converting byte positions to character positions.
/// Lines and columns are 0-indexed.
///
/// # Arguments
/// * `text` - The source text
/// * `range` - The byte range to convert
///
/// # Returns
/// Tuple of (start_line, start_col, end_line, end_col)
pub fn range_to_line_col(text: &str, range: TextRange) -> (u32, u32, u32, u32) {
    let start_offset: usize = range.start().into();
    let end_offset: usize = range.end().into();

    let mut line = 0u32;
    let mut col = 0u32; // Character position in current line
    let mut byte_pos = 0usize; // Byte position in text
    let mut start_line = 0u32;
    let mut start_col = 0u32;
    let mut end_line = 0u32;
    let mut end_col = 0u32;

    for ch in text.chars() {
        // Check for start position BEFORE processing character
        if byte_pos == start_offset {
            start_line = line;
            start_col = col;
        }

        // Process character (update col and byte_pos)
        if ch == '\n' {
            line += 1;
            col = 0;
            byte_pos += 1; // newline is 1 byte
        } else {
            col += 1; // Increment character position
            byte_pos += ch.len_utf8(); // Increment byte position by character's UTF-8 length
        }

        // Check for end position AFTER processing character
        // LSP uses half-open ranges [start, end), so end points to position AFTER last character
        if byte_pos == end_offset {
            end_line = line;
            end_col = col;
            break;
        }
    }

    // Handle case where end_offset is at the very end
    if byte_pos == end_offset || end_offset >= text.len() {
        end_line = line;
        end_col = col;
    }

    (start_line, start_col, end_line, end_col)
}

/// Assert that a diagnostic has the expected line and column range.
///
/// # Arguments
/// * `text` - The source text
/// * `diagnostic` - The diagnostic to check
/// * `expected_line` - Expected line number (0-indexed)
/// * `expected_start_col` - Expected start column (0-indexed, character position)
/// * `expected_end_col` - Expected end column (0-indexed, character position)
///
/// # Panics
/// Panics if the diagnostic range doesn't match expectations.
pub fn assert_diagnostic_range(
    text: &str,
    diagnostic: &Diagnostic,
    expected_line: u32,
    expected_start_col: u32,
    expected_end_col: u32,
) {
    let (start_line, start_col, end_line, end_col) = range_to_line_col(text, diagnostic.range);
    assert_eq!(
        start_line, expected_line,
        "Diagnostic start line mismatch: expected {}, got {}",
        expected_line, start_line
    );
    assert_eq!(
        end_line, expected_line,
        "Diagnostic end line mismatch: expected {}, got {}",
        expected_line, end_line
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

/// Assert that a diagnostic has the expected multi-line range.
///
/// Use this for diagnostics that span multiple lines.
/// For single-line diagnostics, use `assert_diagnostic_range` instead.
///
/// # Arguments
/// * `text` - The source text
/// * `diagnostic` - The diagnostic to check
/// * `expected_start_line` - Expected start line number (0-indexed)
/// * `expected_start_col` - Expected start column (0-indexed, character position)
/// * `expected_end_line` - Expected end line number (0-indexed)
/// * `expected_end_col` - Expected end column (0-indexed, character position)
///
/// # Panics
/// Panics if the diagnostic range doesn't match expectations.
///
/// # Example
/// ```
/// // Diagnostic spans from line 3, col 0 to line 5, col 13
/// assert_diagnostic_range_multiline(&file_content, &diagnostic, 3, 0, 5, 13);
/// ```
pub fn assert_diagnostic_range_multiline(
    text: &str,
    diagnostic: &Diagnostic,
    expected_start_line: u32,
    expected_start_col: u32,
    expected_end_line: u32,
    expected_end_col: u32,
) {
    let (start_line, start_col, end_line, end_col) = range_to_line_col(text, diagnostic.range);
    assert_eq!(
        start_line, expected_start_line,
        "Diagnostic start line mismatch: expected {}, got {}",
        expected_start_line, start_line
    );
    assert_eq!(
        start_col, expected_start_col,
        "Diagnostic start column mismatch: expected {}, got {}",
        expected_start_col, start_col
    );
    assert_eq!(
        end_line, expected_end_line,
        "Diagnostic end line mismatch: expected {}, got {}",
        expected_end_line, end_line
    );
    assert_eq!(
        end_col, expected_end_col,
        "Diagnostic end column mismatch: expected {}, got {}",
        expected_end_col, end_col
    );
}

/// Run HIR-based diagnostics on test code.
///
/// Creates a database with the given code and runs all HIR diagnostics.
/// Used for testing individual HIR diagnostic handlers.
///
/// # Arguments
/// * `code` - The BSL source code to analyze
///
/// # Returns
/// Vector of diagnostics found in the code
///
/// # Example
/// ```ignore
/// let diagnostics = check_hir_diagnostic(r#"Функция Тест()
///     Перем Х;
/// КонецФункции"#);
/// assert!(diagnostics.iter().any(|d| d.code == DiagnosticCode::FunctionShouldHaveReturn));
/// ```
pub fn check_hir_diagnostic(code: &str) -> Vec<Diagnostic> {
    use crate::DiagnosticsConfig;
    use hir::ModuleId;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::sync::Arc;
    use test_fixture::Fixture;
    use vfs::VfsPath;

    let fixture_text = format!("//- /test.bsl\n{}", code);
    let fixture = Fixture::parse(&fixture_text);
    let file_id = fixture.first_file().expect("fixture should have at least one file");

    let mut db = RootDatabaseImpl::new();

    // Set up source root for module_bodies to work
    let mut file_set = vfs::FileSet::default();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    for (fid, file) in &fixture.files {
        db.set_file_text(*fid, &file.content);
    }

    #[allow(clippy::arc_with_non_send_sync)]
    let db = Arc::new(db) as Arc<dyn RootDatabase>;
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

    // Run HIR diagnostics via module_bodies
    // Uses hir_dispatch::dispatch_hir_diagnostic - single source of truth
    let module_id = ModuleId::new(file_id);
    let module_bodies = ctx.db.module_bodies(module_id);

    let mut diagnostics = Vec::new();
    for (method_id, body_diag) in module_bodies.all_diagnostics() {
        if let Some(diag) = crate::hir_dispatch::dispatch_hir_diagnostic(body_diag, method_id, &ctx)
        {
            diagnostics.push(diag);
        }
    }

    // Collect module-level diagnostics (e.g., ExportVariables from module_vars)
    for var in module_bodies.module_vars() {
        if var.is_export {
            if let Some(diag) =
                crate::handlers::export_variables::from_hir(&var.name, var.range, &ctx)
            {
                diagnostics.push(diag);
            }
        }
    }

    // Run dataflow-based diagnostics (UnusedLocalVariable with liveness analysis)
    diagnostics.extend(crate::handlers::unused_local_variable::check(&ctx));

    diagnostics
}

/// Alias for `check_ast_diagnostic` - runs a handler's check() function.
///
/// NOTE: Despite the name, this works for ANY handler with check() function,
/// not just SDBL diagnostics. Use `check_ast_diagnostic` for new code.
///
/// Kept for backward compatibility with existing tests.
#[inline]
pub fn check_sdbl_diagnostic<F>(code: &str, check_fn: F) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    check_ast_diagnostic(code, check_fn)
}

/// Alias for `check_ast_diagnostic_with_config`.
///
/// Kept for backward compatibility with existing tests.
#[inline]
pub fn check_sdbl_diagnostic_with_config<F>(
    code: &str,
    config: crate::DiagnosticsConfig,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    check_ast_diagnostic_with_config(code, config, check_fn)
}

/// Run a handler's check() function on test code with default configuration.
///
/// Creates a database with the given code and runs the provided check function.
/// Used for testing AST diagnostic handlers that don't use HIR.
///
/// # Arguments
/// * `code` - The BSL source code to analyze
/// * `check_fn` - Closure that runs diagnostics (typically `|ctx| handlers::some_handler::check(ctx)`)
///
/// # Returns
/// Vector of diagnostics found in the code
///
/// # Example
/// ```ignore
/// let diagnostics = check_ast_diagnostic(r#"
/// Процедура Тест()
///     // some code
/// КонецПроцедуры
/// "#, super::check);
/// ```
pub fn check_ast_diagnostic<F>(code: &str, check_fn: F) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    let config = crate::DiagnosticsConfig::default();
    check_ast_diagnostic_with_config(code, config, check_fn)
}

/// Run AST-based diagnostics on test code with custom configuration.
///
/// # Arguments
/// * `code` - The BSL source code to analyze
/// * `config` - The diagnostics configuration to use
/// * `check_fn` - Closure that runs diagnostics
///
/// # Returns
/// Vector of diagnostics found in the code
pub fn check_ast_diagnostic_with_config<F>(
    code: &str,
    config: crate::DiagnosticsConfig,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;
    use vfs::VfsPath;

    let fixture_text = format!("//- /test.bsl\n{}", code);
    let fixture = Fixture::parse(&fixture_text);
    let file_id = fixture.first_file().expect("fixture should have at least one file");

    let mut db = RootDatabaseImpl::new();

    let mut file_set = vfs::FileSet::default();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    for (fid, file) in &fixture.files {
        db.set_file_text(*fid, &file.content);
    }

    let config = Rc::new(config);
    let ctx = crate::DiagnosticsContext {
        db: &db,
        config: &config,
        file_id,
        provider: None,
        workspace_root: None,
        configuration_path: None,
        configuration_path_input: None,
        file_set: None,
    };

    check_fn(&ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_to_line_col_ascii() {
        let text = "Hello\nWorld\n";
        // "World" is on line 1, columns 0-5
        let range = TextRange::new(6.into(), 11.into());
        let (start_line, start_col, end_line, end_col) = range_to_line_col(text, range);
        assert_eq!(start_line, 1);
        assert_eq!(start_col, 0);
        assert_eq!(end_line, 1);
        assert_eq!(end_col, 5);
    }

    #[test]
    fn test_range_to_line_col_utf8() {
        // Russian text: "Привет" = 12 bytes, 6 characters
        let text = "// Привет\n";
        // "Привет" starts at byte 3, character 3
        let range = TextRange::new(3.into(), 15.into());
        let (start_line, start_col, end_line, end_col) = range_to_line_col(text, range);
        assert_eq!(start_line, 0);
        assert_eq!(start_col, 3); // Character position
        assert_eq!(end_line, 0);
        assert_eq!(end_col, 9); // Character position (3 + 6)
    }
}
