//! ConsecutiveEmptyLines diagnostic
//!
//! Checks that BSL code does not contain too many consecutive empty lines.
//!
//! ## Why?
//! Too many empty lines reduce code readability and waste vertical space.
//!
//! ## Configuration
//! - `allowedEmptyLinesCount` (Integer, default: 1) - Maximum allowed consecutive empty lines

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use line_index::LineIndex;

const DEFAULT_ALLOWED_EMPTY_LINES: usize = 1;

/// Main entry point for ConsecutiveEmptyLines diagnostic.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("ConsecutiveEmptyLines::check").entered();

    let code = DiagnosticCode::ConsecutiveEmptyLines;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let allowed_empty_lines: usize = ctx
        .config
        .get_int(DiagnosticCode::ConsecutiveEmptyLines, "allowedEmptyLinesCount")
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(DEFAULT_ALLOWED_EMPTY_LINES);

    let file_text = ctx.file_text();

    if file_text.is_empty() {
        return Vec::new();
    }

    // Get line index (cached, using helper method for streaming mode compatibility)
    let line_index = ctx.line_index();

    scan_consecutive_empty_lines(&file_text, &line_index, allowed_empty_lines, code, ctx)
}

/// Scan for consecutive empty lines using LineIndex.
fn scan_consecutive_empty_lines(
    text: &str,
    line_index: &LineIndex,
    allowed: usize,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let num_lines = line_index.len_lines();

    let mut consecutive_empty = 0usize;
    let mut empty_start_line: Option<u32> = None;

    for line in 0..num_lines {
        let line_start = line_index.line_start(line);
        let line_range = line_index.line_range(line);

        let is_empty = if let Some(range) = line_range {
            let line_text = &text[usize::from(range.start())..usize::from(range.end())];
            line_text.trim().is_empty()
        } else {
            // Last line without range - check remaining text
            let start: usize = line_start.into();
            if start < text.len() {
                text[start..].trim().is_empty()
            } else {
                true
            }
        };

        if is_empty {
            if consecutive_empty == 0 {
                empty_start_line = Some(line);
            }
            consecutive_empty += 1;
        } else {
            if consecutive_empty > allowed {
                if let Some(start_line) = empty_start_line {
                    diagnostics.push(create_diagnostic(
                        line_index,
                        start_line,
                        line - 1,
                        consecutive_empty,
                        allowed,
                        code,
                        ctx,
                    ));
                }
            }
            consecutive_empty = 0;
            empty_start_line = None;
        }
    }

    // Handle trailing empty lines
    if consecutive_empty > allowed {
        if let Some(start_line) = empty_start_line {
            diagnostics.push(create_diagnostic(
                line_index,
                start_line,
                num_lines - 1,
                consecutive_empty,
                allowed,
                code,
                ctx,
            ));
        }
    }

    tracing::debug!(count = diagnostics.len(), "ConsecutiveEmptyLines diagnostics found");

    diagnostics
}

/// Create a diagnostic for consecutive empty lines.
fn create_diagnostic(
    line_index: &LineIndex,
    start_line: u32,
    end_line: u32,
    count: usize,
    allowed: usize,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    let start_byte = line_index.line_start(start_line);
    let end_byte = line_index.line_start(end_line);

    let message = format!("Найдено {} подряд идущих пустых строк (максимум: {})", count, allowed);

    Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range: TextRange::new(start_byte, end_byte),
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range_multiline, check_ast_diagnostic};

    #[test]
    fn test_empty_file() {
        let code = "";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_single_empty_line() {
        let code = "Процедура А()\n\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_two_empty_lines() {
        let code = "Процедура А()\n\n\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range_multiline(code, &diagnostics[0], 1, 0, 2, 0);
    }

    #[test]
    fn test_spaces_only_line() {
        let code = "Процедура А()\n  \n  \nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_single_newline_file() {
        let text = "\n";
        let diagnostics = check_ast_diagnostic(text, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for single newline");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/ConsecutiveEmptyLinesDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 9, "Expected 9 diagnostics, got {}", diagnostics.len());

        assert_diagnostic_range_multiline(code, &diagnostics[0], 0, 0, 1, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[1], 5, 0, 6, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[2], 10, 0, 11, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[3], 14, 0, 15, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[4], 17, 0, 18, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[5], 22, 0, 23, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[6], 26, 0, 27, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[7], 29, 0, 31, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[8], 33, 0, 34, 0);
    }
}
