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
use std::collections::HashMap;
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

#[derive(Debug, Clone)]
struct LineInfo {
    max_char_position: usize,
    has_code: bool,
}

impl LineInfo {
    fn new() -> Self {
        Self { max_char_position: 0, has_code: false }
    }
}

/// Calculate the line number and character end position for a token.
/// Returns (line_number, char_position_at_end).
/// Line numbers are 0-based, character positions count characters not bytes.
fn calculate_token_end_position(file_text: &str, token_range: TextRange) -> (u32, usize) {
    let start_byte: usize = token_range.start().into();
    let end_byte: usize = token_range.end().into();

    let mut line = 0u32;
    let mut col_start_byte = 0usize;
    let mut byte_pos = 0usize;
    let mut token_line = 0u32;
    let mut token_start_col = 0usize;

    for ch in file_text.chars() {
        if byte_pos == start_byte {
            token_line = line;
            token_start_col = file_text[col_start_byte..start_byte].chars().count();
            break;
        }

        if ch == '\n' {
            line += 1;
            col_start_byte = byte_pos + 1;
        }

        byte_pos += ch.len_utf8();
    }

    let token_text = &file_text[start_byte..end_byte];
    let token_char_len = token_text.chars().count();
    let char_end_pos = token_start_col + token_char_len;

    (token_line, char_end_pos)
}

/// Process code tokens and update line map.
fn process_code_tokens(root: &SyntaxNode, file_text: &str, line_map: &mut HashMap<u32, LineInfo>) {
    let mut prev_token_kind: Option<SyntaxKind> = None;

    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            let kind = token.kind();

            if kind == SyntaxKind::WHITESPACE || kind == SyntaxKind::NEWLINE {
                continue;
            }

            if kind == SyntaxKind::COMMENT {
                continue;
            }

            if matches!(kind, SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL) {
                prev_token_kind = Some(kind);
                continue;
            }

            if kind == SyntaxKind::SEMICOLON {
                if let Some(prev) = prev_token_kind {
                    if matches!(prev, SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL) {
                        prev_token_kind = Some(kind);
                        continue;
                    }
                }
            }

            let range = token.text_range();
            let (line, char_pos_end) = calculate_token_end_position(file_text, range);

            let line_info = line_map.entry(line).or_insert_with(LineInfo::new);
            line_info.max_char_position = line_info.max_char_position.max(char_pos_end);
            line_info.has_code = true;

            prev_token_kind = Some(kind);
        }
    }
}

/// Find method description comment ranges.
/// Returns a vector of TextRanges covering method description comments.
/// Only includes comments that are immediately before a method (no blank lines).
fn find_method_description_ranges(root: &SyntaxNode) -> Vec<TextRange> {
    use syntax::ast::{FunctionDef, ProcedureDef};

    let mut all_method_desc_ranges = Vec::new();

    let mut all_comments: Vec<TextRange> = Vec::new();
    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            if token.kind() == SyntaxKind::COMMENT {
                all_comments.push(token.text_range());
            }
        }
    }

    for node in root.descendants() {
        let method_start = ProcedureDef::cast(node.clone())
            .map(|proc| proc.syntax().text_range().start())
            .or_else(|| {
                FunctionDef::cast(node.clone()).map(|func| func.syntax().text_range().start())
            });

        if let Some(method_start_pos) = method_start {
            let mut method_desc_comments = Vec::new();

            for &comment_range in &all_comments {
                if comment_range.end() <= method_start_pos {
                    method_desc_comments.push(comment_range);
                }
            }

            method_desc_comments.sort_by_key(|r| r.start());

            // Java uses MethodSymbol.getDescription() to properly parse method descriptions.
            // We use a simpler heuristic: take contiguous comment block before method.
            if !method_desc_comments.is_empty() {
                let mut desc_block = vec![*method_desc_comments.last().unwrap()];

                for i in (0..method_desc_comments.len() - 1).rev() {
                    let curr = method_desc_comments[i];
                    let next = method_desc_comments[i + 1];

                    let gap = usize::from(next.start()) - usize::from(curr.end());
                    if gap < 100 {
                        desc_block.push(curr);
                    } else {
                        break;
                    }
                }

                if let (Some(&first), Some(&last)) = (desc_block.last(), desc_block.first()) {
                    let desc_range = TextRange::new(first.start(), last.end());
                    all_method_desc_ranges.push(desc_range);
                }
            }
        }
    }

    all_method_desc_ranges
}

/// Process comment tokens and update line map.
fn process_comments(
    root: &SyntaxNode,
    file_text: &str,
    line_map: &mut HashMap<u32, LineInfo>,
    method_desc_ranges: &[TextRange],
    config: &Config,
) {
    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            if token.kind() != SyntaxKind::COMMENT {
                continue;
            }

            let token_range = token.text_range();

            if !config.check_method_description {
                let is_method_desc = method_desc_ranges.iter().any(|desc_range| {
                    token_range.start() >= desc_range.start()
                        && token_range.end() <= desc_range.end()
                });
                if is_method_desc {
                    continue;
                }
            }

            if config.exclude_trailing_comments {
                let (line, _) = calculate_token_end_position(file_text, token_range);
                if let Some(line_info) = line_map.get(&line) {
                    if line_info.has_code {
                        continue;
                    }
                }
            }

            let (line, char_pos_end) = calculate_token_end_position(file_text, token_range);

            let line_info = line_map.entry(line).or_insert_with(LineInfo::new);
            line_info.max_char_position = line_info.max_char_position.max(char_pos_end);
        }
    }
}

/// Convert line/column character positions to byte range.
fn line_char_range_to_byte_range(
    file_text: &str,
    line: u32,
    start_char: usize,
    end_char: usize,
) -> TextRange {
    let mut current_line = 0u32;
    let mut current_char = 0usize;
    let mut byte_pos = 0usize;
    let mut start_byte = None;
    let mut end_byte = None;

    for ch in file_text.chars() {
        if current_line == line {
            if current_char == start_char && start_byte.is_none() {
                start_byte = Some(byte_pos);
            }
            if current_char == end_char {
                end_byte = Some(byte_pos);
                break;
            }
        }

        if ch == '\n' {
            // If we're on the target line and reach newline, end is at newline
            if current_line == line && start_byte.is_some() && end_byte.is_none() {
                end_byte = Some(byte_pos);
                break;
            }
            current_line += 1;
            current_char = 0;
            byte_pos += 1;
        } else {
            current_char += 1;
            byte_pos += ch.len_utf8();
        }
    }

    let start = start_byte.unwrap_or(0) as u32;
    let end = end_byte.unwrap_or(byte_pos) as u32;

    TextRange::new(start.into(), end.into())
}

/// Generate diagnostics for lines exceeding max length.
fn generate_diagnostics(
    line_map: HashMap<u32, LineInfo>,
    max_line_length: usize,
    file_text: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (line_number, line_info) in line_map {
        if line_info.max_char_position > max_line_length {
            let range = line_char_range_to_byte_range(
                file_text,
                line_number,
                0,
                line_info.max_char_position,
            );

            diagnostics.push(Diagnostic {
                code: DiagnosticCode::LineLength,
                message: format!(
                    "Длина строки {} превышает максимальную {}",
                    line_info.max_char_position, max_line_length
                ),
                severity: Severity::Warning,
                range,
                tags: vec![],
                fixes: vec![],
            });
        }
    }

    diagnostics.sort_by_key(|d| d.range.start());

    diagnostics
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

    let mut line_map: HashMap<u32, LineInfo> = HashMap::new();
    process_code_tokens(&root, file_text.as_ref(), &mut line_map);

    let method_desc_ranges = if !config.check_method_description {
        find_method_description_ranges(&root)
    } else {
        Vec::new()
    };

    process_comments(&root, file_text.as_ref(), &mut line_map, &method_desc_ranges, &config);

    let diagnostics = generate_diagnostics(line_map, config.max_line_length, file_text.as_ref());

    tracing::debug!(count = diagnostics.len(), "LineLength diagnostics found");

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
