//! LineLength diagnostic
//!
//! Checks that BSL code lines do not exceed maximum length.
//!
//! ## Why?
//! Long lines reduce code readability and make it harder to review changes.
//! Industry standard is 120 characters per line.
//!
//! ## Bad practice
//! ```bsl
//! А = "very long string literal that exceeds 120 characters and makes the code hard to read especially when reviewing in version control systems";
//! ```
//!
//! ## Good practice
//! ```bsl
//! А = "shorter string literal"
//!     + " that is split across"
//!     + " multiple lines";
//! ```
//!
//! ## Configuration
//! - `maxLineLength` (Integer, default: 120) - Maximum line length in characters
//! - `checkMethodDescription` (Boolean, default: true) - Include method description comments in check
//! - `excludeTrailingComments` (Boolean, default: false) - Exclude comments on same line as code

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use line_index::{LineIndex, TextSize};
use std::collections::HashSet;
use syntax::{ast::AstNode, SyntaxKind, SyntaxNode};

const DEFAULT_MAX_LINE_LENGTH: usize = 120;
const DEFAULT_CHECK_METHOD_DESCRIPTION: bool = true;
const DEFAULT_EXCLUDE_TRAILING_COMMENTS: bool = false;

#[derive(Debug, Clone)]
struct Config {
    max_line_length: usize,
    check_method_description: bool,
    exclude_trailing_comments: bool,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let code = DiagnosticCode::LineLength;

        let max_line_length = ctx
            .config
            .get_int(code, "maxLineLength")
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(DEFAULT_MAX_LINE_LENGTH);

        let check_method_description = ctx
            .config
            .get_bool(code, "checkMethodDescription")
            .unwrap_or(DEFAULT_CHECK_METHOD_DESCRIPTION);

        let exclude_trailing_comments = ctx
            .config
            .get_bool(code, "excludeTrailingComments")
            .unwrap_or(DEFAULT_EXCLUDE_TRAILING_COMMENTS);

        tracing::debug!(
            max_line_length = max_line_length,
            check_method_description = check_method_description,
            exclude_trailing_comments = exclude_trailing_comments,
            "LineLength config loaded"
        );

        Self { max_line_length, check_method_description, exclude_trailing_comments }
    }
}

/// Information about a line for length checking.
#[derive(Debug, Clone, Default)]
struct LineInfo {
    /// Maximum character position (end column) of code on this line.
    max_code_char_pos: usize,
    /// Maximum character position including comments.
    max_char_pos: usize,
    /// Whether this line has any code (non-comment, non-whitespace).
    has_code: bool,
    /// Whether this line is part of a multiline string.
    is_multiline_string: bool,
}

/// Main entry point for LineLength diagnostic.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("LineLength::check").entered();

    if ctx.config.is_disabled(DiagnosticCode::LineLength) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let file_text_input = ctx.db.file_text_input(ctx.file_id);
    let file_text = file_text_input.text(ctx.db);
    let file_text = file_text.as_ref();

    // Build line index once - O(n)
    let line_index = LineIndex::new(file_text);
    let num_lines = line_index.len_lines();

    // Pre-allocate line info for all lines
    let mut line_infos: Vec<LineInfo> = vec![LineInfo::default(); num_lines as usize];

    // Find lines that are part of multiline strings (to exclude from checking)
    mark_multiline_string_lines(&root, &line_index, &mut line_infos);

    // Process code tokens to find max code positions per line
    process_code_tokens(&root, file_text, &line_index, &mut line_infos);

    // Find method description comment lines (if needed)
    let method_desc_lines = if !config.check_method_description {
        find_method_description_lines(&root, &line_index)
    } else {
        HashSet::new()
    };

    // Process comments
    process_comments(&root, file_text, &line_index, &mut line_infos, &method_desc_lines, &config);

    // Generate diagnostics
    let diagnostics =
        generate_diagnostics(&line_infos, &line_index, file_text, config.max_line_length);

    tracing::debug!(count = diagnostics.len(), "LineLength diagnostics found");

    diagnostics
}

/// Mark lines that are part of multiline strings.
fn mark_multiline_string_lines(
    root: &SyntaxNode,
    line_index: &LineIndex,
    line_infos: &mut [LineInfo],
) {
    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            if matches!(token.kind(), SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL) {
                let range = token.text_range();
                let start_line = line_index.line_col(range.start()).line;
                let end_line = line_index.line_col(range.end()).line;

                for line in start_line..=end_line {
                    if let Some(info) = line_infos.get_mut(line as usize) {
                        info.is_multiline_string = true;
                    }
                }
            }
        }
    }
}

/// Process code tokens and update line info.
fn process_code_tokens(
    root: &SyntaxNode,
    file_text: &str,
    line_index: &LineIndex,
    line_infos: &mut [LineInfo],
) {
    let mut prev_token_kind: Option<SyntaxKind> = None;

    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            let kind = token.kind();

            // Skip whitespace and newlines
            if kind == SyntaxKind::WHITESPACE || kind == SyntaxKind::NEWLINE {
                continue;
            }

            // Skip comments (handled separately)
            if kind == SyntaxKind::COMMENT {
                continue;
            }

            // Skip multiline string parts
            if matches!(kind, SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL) {
                prev_token_kind = Some(kind);
                continue;
            }

            // Skip semicolon after multiline string
            if kind == SyntaxKind::SEMICOLON {
                if let Some(prev) = prev_token_kind {
                    if matches!(prev, SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL) {
                        prev_token_kind = Some(kind);
                        continue;
                    }
                }
            }

            let range = token.text_range();
            let end_pos = line_index.line_col(range.end());
            let line = end_pos.line as usize;

            if let Some(info) = line_infos.get_mut(line) {
                // Calculate character position (not byte position)
                let line_start = line_index.line_start(end_pos.line);
                let byte_col = u32::from(range.end()) - u32::from(line_start);
                let line_text_start: usize = line_start.into();
                let line_text_end = (line_text_start + byte_col as usize).min(file_text.len());
                let char_col = file_text[line_text_start..line_text_end].chars().count();

                info.max_code_char_pos = info.max_code_char_pos.max(char_col);
                info.max_char_pos = info.max_char_pos.max(char_col);
                info.has_code = true;
            }

            prev_token_kind = Some(kind);
        }
    }
}

/// Find lines that contain method description comments.
fn find_method_description_lines(root: &SyntaxNode, line_index: &LineIndex) -> HashSet<u32> {
    use syntax::ast::{FunctionDef, ProcedureDef};

    let mut method_desc_lines = HashSet::new();

    // Collect all comments with their line numbers
    let mut comments: Vec<(u32, TextRange)> = Vec::new();
    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            if token.kind() == SyntaxKind::COMMENT {
                let range = token.text_range();
                let line = line_index.line_col(range.start()).line;
                comments.push((line, range));
            }
        }
    }

    comments.sort_by_key(|(line, _)| *line);

    // Find method start lines
    for node in root.descendants() {
        let method_start = ProcedureDef::cast(node.clone())
            .map(|proc| proc.syntax().text_range().start())
            .or_else(|| {
                FunctionDef::cast(node.clone()).map(|func| func.syntax().text_range().start())
            });

        if let Some(method_start_pos) = method_start {
            let method_line = line_index.line_col(method_start_pos).line;

            // Find contiguous comment block immediately before method
            let mut desc_lines = Vec::new();
            for &(comment_line, _) in comments.iter().rev() {
                if comment_line >= method_line {
                    continue;
                }
                if desc_lines.is_empty() || desc_lines.last() == Some(&(comment_line + 1)) {
                    desc_lines.push(comment_line);
                } else {
                    break;
                }
            }

            for line in desc_lines {
                method_desc_lines.insert(line);
            }
        }
    }

    method_desc_lines
}

/// Process comment tokens and update line info.
fn process_comments(
    root: &SyntaxNode,
    file_text: &str,
    line_index: &LineIndex,
    line_infos: &mut [LineInfo],
    method_desc_lines: &HashSet<u32>,
    config: &Config,
) {
    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            if token.kind() != SyntaxKind::COMMENT {
                continue;
            }

            let range = token.text_range();
            let start_pos = line_index.line_col(range.start());
            let line = start_pos.line as usize;

            // Skip method description comments if configured
            if !config.check_method_description && method_desc_lines.contains(&(line as u32)) {
                continue;
            }

            // Skip trailing comments if configured
            if config.exclude_trailing_comments {
                if let Some(info) = line_infos.get(line) {
                    if info.has_code {
                        continue;
                    }
                }
            }

            // Calculate character position at end of comment
            let end_pos = line_index.line_col(range.end());
            let end_line = end_pos.line as usize;

            if let Some(info) = line_infos.get_mut(end_line) {
                let line_start = line_index.line_start(end_pos.line);
                let byte_col = u32::from(range.end()) - u32::from(line_start);
                let line_text_start: usize = line_start.into();
                let line_text_end = (line_text_start + byte_col as usize).min(file_text.len());
                let char_col = file_text[line_text_start..line_text_end].chars().count();

                info.max_char_pos = info.max_char_pos.max(char_col);
            }
        }
    }
}

/// Generate diagnostics for lines exceeding max length.
fn generate_diagnostics(
    line_infos: &[LineInfo],
    line_index: &LineIndex,
    file_text: &str,
    max_line_length: usize,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (line_num, info) in line_infos.iter().enumerate() {
        // Skip multiline string lines
        if info.is_multiline_string {
            continue;
        }

        if info.max_char_pos > max_line_length {
            let line = line_num as u32;
            let line_start = line_index.line_start(line);

            // Calculate byte position for max_char_pos
            let line_text_start: usize = line_start.into();
            let line_range = line_index.line_range(line);
            let line_text_end: usize =
                line_range.map(|r| r.end().into()).unwrap_or(file_text.len());
            let line_text = &file_text[line_text_start..line_text_end.min(file_text.len())];

            // Find byte offset for max_char_pos characters
            let mut byte_offset = 0usize;
            for (i, ch) in line_text.chars().enumerate() {
                if i >= info.max_char_pos {
                    break;
                }
                byte_offset += ch.len_utf8();
            }

            let range = TextRange::new(line_start, line_start + TextSize::from(byte_offset as u32));

            diagnostics.push(Diagnostic {
                code: DiagnosticCode::LineLength,
                message: format!(
                    "Длина строки {} превышает максимальную {}",
                    info.max_char_pos, max_line_length
                ),
                severity: Severity::Warning,
                range,
                tags: vec![],
                fixes: vec![],
            });
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::assert_diagnostic_range, DiagnosticsConfig};
    use ide_db::{base_db::SourceDatabase, RootDatabase, RootDatabaseImpl};
    use std::sync::Arc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str, config: DiagnosticsConfig) -> (Vec<Diagnostic>, String) {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_simple_long_line() {
        let code = r#"А = "фффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффф";"#;
        let config = DiagnosticsConfig::default();
        let (diagnostics, file_content) = check_diagnostic(code, config);

        // Line 0: 121 characters (exceeds 120), diagnostic ends at column 121
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(&file_content, &diagnostics[0], 0, 0, 121);
    }

    #[test]
    fn test_utf8_characters() {
        // Each Cyrillic character is 2 bytes but counts as 1 character
        let code = "А = \"фф\";  // Short line";
        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

        // Should not trigger (under 120 chars)
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/LineLengthDiagnostic.bsl");
        let config = DiagnosticsConfig::default();
        let (diagnostics, file_content) = check_diagnostic(code, config);

        // CRITICAL: Must match Java implementation (13 diagnostics)
        assert_eq!(
            diagnostics.len(),
            13,
            "Must match Java implementation exactly (13 diagnostics)"
        );

        // Verify exact positions (Java uses 0-based line numbers in hasRange)
        // Java lines: 4, 5, 8, 11, 12, 36, 40, 44, 47, 49, 52, 56, 60
        assert_diagnostic_range(&file_content, &diagnostics[0], 4, 0, 121);
        assert_diagnostic_range(&file_content, &diagnostics[1], 5, 0, 122);
        assert_diagnostic_range(&file_content, &diagnostics[2], 8, 0, 127);
        assert_diagnostic_range(&file_content, &diagnostics[3], 11, 0, 136);
        assert_diagnostic_range(&file_content, &diagnostics[4], 12, 0, 135);
        assert_diagnostic_range(&file_content, &diagnostics[5], 36, 0, 127);
        assert_diagnostic_range(&file_content, &diagnostics[6], 40, 0, 140);
        assert_diagnostic_range(&file_content, &diagnostics[7], 44, 0, 143);
        assert_diagnostic_range(&file_content, &diagnostics[8], 47, 0, 139);
        assert_diagnostic_range(&file_content, &diagnostics[9], 49, 0, 138);
        assert_diagnostic_range(&file_content, &diagnostics[10], 52, 0, 177);
        assert_diagnostic_range(&file_content, &diagnostics[11], 56, 0, 162);
        assert_diagnostic_range(&file_content, &diagnostics[12], 60, 0, 145);
    }

    #[test]
    fn test_configured_max_length() {
        let code = include_str!("../../test_data/LineLengthDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::LineLength, serde_json::json!({"maxLineLength": 119}));

        let (diagnostics, file_content) = check_diagnostic(code, config);

        // With maxLineLength=119, should have 14 diagnostics (adds line 3)
        assert_eq!(diagnostics.len(), 14);
        // First diagnostic should be on line 3 (0-based) with 120 characters
        assert_diagnostic_range(&file_content, &diagnostics[0], 3, 0, 120);
    }

    #[test]
    fn test_exclude_method_description() {
        let code = include_str!("../../test_data/LineLengthDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::LineLength,
            serde_json::json!({"checkMethodDescription": false}),
        );

        let (diagnostics, _) = check_diagnostic(code, config);

        // With checkMethodDescription=false, should have 11 diagnostics
        // (excludes method description comments on lines 55 and 59)
        assert_eq!(diagnostics.len(), 11);
    }

    #[test]
    fn test_exclude_trailing_comments() {
        let code = include_str!("../../test_data/LineLengthDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::LineLength,
            serde_json::json!({"excludeTrailingComments": true}),
        );

        let (diagnostics, _) = check_diagnostic(code, config);

        // With excludeTrailingComments=true, should have 12 diagnostics
        assert_eq!(diagnostics.len(), 12);
    }
}
