//! SDBL query information extracted from BSL files.
//!
//! This module provides the SdblQueryInfo structure which caches parsed SDBL queries
//! along with their positions in BSL source files. Used by Salsa to avoid re-parsing
//! SDBL queries when running multiple SDBL diagnostics.

use crate::{Parse, SyntaxKind, SyntaxNode, TextRange};

/// Information about a single SDBL query found in BSL file.
///
/// This structure is cached by Salsa to avoid re-parsing SDBL queries
/// when running multiple SDBL diagnostics.
///
/// ## Usage
///
/// ```ignore
/// // In diagnostics:
/// let sdbl_queries = ctx.db.sdbl_queries(ctx.file_id);
/// for query_info in sdbl_queries.iter() {
///     if query_info.is_valid() {
///         // Use query_info.query_ast to analyze SDBL
///         // Use query_info.bsl_literal_range for position mapping
///     }
/// }
/// ```
///
/// ## Performance
///
/// - Eager parsing: SDBL AST is parsed during extraction (not lazy)
/// - Keyword filtering: Only strings containing SELECT/ВЫБРАТЬ are parsed
/// - Parse validation: Only successfully parsed queries are cached
/// - Cached by Salsa with LRU=256
///
/// ## Implementation Notes
///
/// This struct must be `Clone`, `PartialEq`, and `Eq` to work with Salsa.
/// It uses `Option<Parse<SyntaxNode>>` for the query AST because:
/// - Some strings might look like SDBL but fail to parse (rare)
/// - Parse errors are stored in the Parse structure itself
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblQueryInfo {
    /// Text range of the LITERAL node in BSL source
    pub bsl_literal_range: TextRange,

    /// Extracted SDBL query text (with | prefixes removed, "" unescaped to ")
    pub query_text: String,

    /// Parsed SDBL AST (None if parse failed)
    pub query_ast: Option<Parse<SyntaxNode>>,

    /// Quote escape corrections: (sdbl_byte_offset, chars_added_in_bsl)
    /// Tracks ""→" replacements for accurate position mapping.
    /// When mapping SDBL position X to BSL column: bsl_col = sdbl_col + sum(chars for positions < X)
    /// NOTE: This stores CHARACTER count, not byte count, for correct UTF-8 handling.
    pub quote_corrections: Vec<(usize, usize)>,
}

impl SdblQueryInfo {
    /// Create a new SDBL query info.
    ///
    /// ## Arguments
    ///
    /// - `bsl_literal_range`: Position of the LITERAL node in BSL source
    /// - `query_text`: Extracted SDBL query text (multiline | prefixes already removed, "" unescaped)
    /// - `query_ast`: Parsed SDBL AST (or None if parsing failed)
    /// - `quote_corrections`: Quote escape corrections for position mapping
    pub fn new(
        bsl_literal_range: TextRange,
        query_text: String,
        query_ast: Option<Parse<SyntaxNode>>,
        quote_corrections: Vec<(usize, usize)>,
    ) -> Self {
        Self { bsl_literal_range, query_text, query_ast, quote_corrections }
    }

    /// Check if SDBL parse was successful and has no errors.
    ///
    /// Returns `false` if:
    /// - Parse failed (query_ast is None)
    /// - Parse succeeded but has syntax errors
    ///
    /// Diagnostics should skip queries where `is_valid() == false`.
    pub fn is_valid(&self) -> bool {
        self.query_ast.as_ref().map(|p| !p.has_errors()).unwrap_or(false)
    }

    /// Get the SDBL root node if parse was successful.
    ///
    /// This is a convenience method for diagnostics that need to access the AST.
    pub fn syntax_node(&self) -> Option<SyntaxNode> {
        self.query_ast.as_ref().map(|p| p.syntax_node())
    }
}

/// Extract SDBL query text with quote escape tracking.
///
/// Returns (text, quote_corrections) where quote_corrections tracks `""` → `"` replacements.
/// Each correction is (sdbl_offset, bytes_added_in_bsl) - when SDBL has position X,
/// BSL position = X + sum(bytes for corrections before X).
///
/// This is the canonical function for SDBL extraction - single source of truth.
///
/// Handles both simple strings and multiline strings:
/// - Simple: `"text"` → one STRING token
/// - Multiline: `"line1\n|line2"` → STRING_START + NEWLINE + STRING_PART + ... + STRING_TAIL
pub fn extract_sdbl_with_corrections(node: &SyntaxNode) -> Option<(String, Vec<(usize, usize)>)> {
    let mut result = String::new();
    let mut corrections = Vec::new();
    let mut tokens = node.children_with_tokens().filter_map(|it| it.into_token());

    // Check first token to determine string type
    let first_token = tokens.next()?;

    match first_token.kind() {
        SyntaxKind::STRING => {
            // Simple string: "text"
            let text = first_token.text();
            if text.len() < 2 {
                return None;
            }
            // Remove outer quotes and track "" → " replacements
            let inner = &text[1..text.len() - 1];

            let mut pos = 0;
            let mut chars = inner.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '"' && chars.peek() == Some(&'"') {
                    // Found "" escape
                    chars.next(); // skip second "
                    result.push('"');
                    corrections.push((pos, 1)); // at SDBL pos, BSL has 1 extra byte
                    pos += 1;
                } else {
                    result.push(ch);
                    pos += ch.len_utf8();
                }
            }
        }
        SyntaxKind::STRING_START => {
            // Multiline string
            let text = first_token.text();
            if text.is_empty() {
                return None;
            }

            // Process first line (after opening quote)
            let first_line = &text[1..];
            let mut pos = 0;
            let mut chars = first_line.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '"' && chars.peek() == Some(&'"') {
                    chars.next();
                    result.push('"');
                    corrections.push((pos, 1));
                    pos += 1;
                } else {
                    result.push(ch);
                    pos += ch.len_utf8();
                }
            }

            // Process remaining tokens
            for token in tokens {
                match token.kind() {
                    SyntaxKind::NEWLINE => {
                        result.push('\n');
                        pos += 1;
                    }
                    SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL => {
                        let text = token.text();
                        // Remove | prefix (and closing " for TAIL)
                        let content = if let Some(c) = text.strip_prefix('|') {
                            if token.kind() == SyntaxKind::STRING_TAIL {
                                c.strip_suffix('"').unwrap_or(c)
                            } else {
                                c
                            }
                        } else {
                            text
                        };

                        let mut chars = content.chars().peekable();
                        while let Some(ch) = chars.next() {
                            if ch == '"' && chars.peek() == Some(&'"') {
                                chars.next();
                                result.push('"');
                                corrections.push((pos, 1));
                                pos += 1;
                            } else {
                                result.push(ch);
                                pos += ch.len_utf8();
                            }
                        }

                        if token.kind() == SyntaxKind::STRING_TAIL {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => return None,
    }

    Some((result, corrections))
}

/// Project an SDBL-text range back into BSL-literal coordinates. All offsets are byte offsets.
/// literal_text is the raw BSL literal (including outer quote delimiters, pipe continuation
/// prefixes, escaped double-quotes). query_range is a TextRange within the extracted SDBL query
/// text (the string extract_query_text returns). Returns the corresponding TextRange relative to
/// literal_text start. Caller adds bsl_literal_range.start() to lift into file coordinates.
pub fn map_range_query_to_literal(literal_text: &str, query_range: TextRange) -> TextRange {
    let query_start: u32 = query_range.start().into();
    let query_end: u32 = query_range.end().into();
    let bytes = literal_text.as_bytes();

    let mut sdbl_byte = 0u32;
    let mut projection = RangeProjectionState::new(query_start, query_end);

    let Some(first_line) = next_line(bytes, 0) else {
        projection.record_boundary(0, 0, 0);
        let start = projection.projected_start.unwrap_or(0);
        let end = if query_start == query_end {
            start
        } else {
            projection.projected_end.unwrap_or(start)
        };
        return TextRange::new(start.into(), end.into());
    };

    let mut content_start = skip_leading_whitespace(literal_text, first_line.start, first_line.end);
    while content_start < first_line.end && bytes[content_start] == b'"' {
        content_start += 1;
    }
    projection.record_boundary(0, content_start as u32, content_start as u32);

    let mut content_end = line_content_end(bytes, first_line);
    process_content(literal_text, content_start, content_end, &mut sdbl_byte, &mut projection);

    let mut pending_linebreak_start =
        (first_line.next_start > first_line.end).then_some(first_line.end);
    let mut line_start = first_line.next_start;

    while line_start < bytes.len() {
        let Some(line) = next_line(bytes, line_start) else {
            break;
        };

        let trimmed_start = skip_leading_whitespace(literal_text, line.start, line.end);

        if bytes[trimmed_start..line.end].starts_with(b"//") {
            line_start = line.next_start;
            continue;
        }

        if let Some(linebreak_start) = pending_linebreak_start.take() {
            let (line_content_start, linebreak_end) =
                if trimmed_start < line.end && bytes[trimmed_start] == b'|' {
                    (trimmed_start + 1, trimmed_start + 1)
                } else {
                    (line.start, line.start)
                };

            projection.emit_segment(
                &mut sdbl_byte,
                1,
                linebreak_start,
                linebreak_end,
                linebreak_start,
            );

            content_end = line_content_end(bytes, line);
            process_content(
                literal_text,
                line_content_start,
                content_end,
                &mut sdbl_byte,
                &mut projection,
            );
        }

        pending_linebreak_start = (line.next_start > line.end).then_some(line.end);
        line_start = line.next_start;
    }

    let fallback = content_end as u32;
    projection.record_boundary(sdbl_byte, fallback, fallback);

    let start = projection.projected_start.unwrap_or(fallback);
    let end =
        if query_start == query_end { start } else { projection.projected_end.unwrap_or(fallback) };

    TextRange::new(start.into(), end.into())
}

struct RangeProjectionState {
    query_start: u32,
    query_end: u32,
    projected_start: Option<u32>,
    projected_end: Option<u32>,
}

impl RangeProjectionState {
    fn new(query_start: u32, query_end: u32) -> Self {
        Self { query_start, query_end, projected_start: None, projected_end: None }
    }

    fn record_boundary(&mut self, sdbl_byte: u32, bsl_start_bias: u32, bsl_end_bias: u32) {
        if self.projected_start.is_none() && sdbl_byte == self.query_start {
            self.projected_start = Some(bsl_start_bias);
        }
        if self.projected_end.is_none() && sdbl_byte == self.query_end {
            self.projected_end = Some(bsl_end_bias);
        }
    }

    fn emit_segment(
        &mut self,
        sdbl_byte: &mut u32,
        sdbl_len: u32,
        bsl_start: usize,
        bsl_end: usize,
        next_start_bias: usize,
    ) {
        let bsl_start = bsl_start as u32;
        let bsl_end = bsl_end as u32;
        let next_start_bias = next_start_bias as u32;

        self.record_boundary(*sdbl_byte, bsl_start, bsl_start);
        *sdbl_byte += sdbl_len;
        self.record_boundary(*sdbl_byte, next_start_bias, bsl_end);
    }
}

#[derive(Clone, Copy)]
struct LiteralLine {
    start: usize,
    end: usize,
    next_start: usize,
}

fn next_line(bytes: &[u8], start: usize) -> Option<LiteralLine> {
    if start > bytes.len() {
        return None;
    }

    let mut end = start;
    while end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }

    if end < bytes.len() {
        let line_end = if end > start && bytes[end - 1] == b'\r' { end - 1 } else { end };
        Some(LiteralLine { start, end: line_end, next_start: end + 1 })
    } else if start < bytes.len() {
        Some(LiteralLine { start, end, next_start: end })
    } else {
        None
    }
}

fn line_content_end(bytes: &[u8], line: LiteralLine) -> usize {
    if line.next_start == line.end && line.end > line.start && bytes[line.end - 1] == b'"' {
        line.end - 1
    } else {
        line.end
    }
}

fn skip_leading_whitespace(text: &str, start: usize, end: usize) -> usize {
    let mut pos = start;
    while pos < end {
        let ch = text[pos..end].chars().next().expect("valid UTF-8 string slice");
        if !ch.is_whitespace() {
            break;
        }
        pos += ch.len_utf8();
    }
    pos
}

fn process_content(
    literal_text: &str,
    start: usize,
    end: usize,
    sdbl_byte: &mut u32,
    projection: &mut RangeProjectionState,
) {
    let bytes = literal_text.as_bytes();
    let mut pos = start;

    while pos < end {
        if pos + 1 < end && bytes[pos] == b'"' && bytes[pos + 1] == b'"' {
            projection.emit_segment(sdbl_byte, 1, pos, pos + 2, pos + 2);
            pos += 2;
        } else {
            let ch = literal_text[pos..end].chars().next().expect("valid UTF-8 string slice");
            let len = ch.len_utf8();
            projection.emit_segment(sdbl_byte, len as u32, pos, pos + len, pos + len);
            pos += len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sdbl_query_info_creation() {
        let range = TextRange::new(0u32.into(), 10u32.into());
        let query_text = "SELECT * FROM Table".to_string();

        let info = SdblQueryInfo::new(range, query_text.clone(), None, vec![]);

        assert_eq!(info.bsl_literal_range, range);
        assert_eq!(info.query_text, query_text);
        assert!(!info.is_valid()); // No AST
        assert!(info.syntax_node().is_none()); // No AST
    }

    #[test]
    fn test_is_valid_with_no_ast() {
        let info = SdblQueryInfo::new(
            TextRange::new(0u32.into(), 10u32.into()),
            "INVALID QUERY".to_string(),
            None,
            vec![],
        );
        assert!(!info.is_valid());
        assert!(info.syntax_node().is_none());
    }

    #[test]
    fn test_clone_and_equality() {
        let query = "SELECT Name AS N FROM Table";

        let info1 = SdblQueryInfo::new(
            TextRange::new(0u32.into(), 10u32.into()),
            query.to_string(),
            None,
            vec![],
        );

        let info2 = info1.clone();

        assert_eq!(info1, info2);
        assert_eq!(info1.bsl_literal_range, info2.bsl_literal_range);
        assert_eq!(info1.query_text, info2.query_text);
    }

    // Note: Tests with parsed AST are in base-db tests where parser is available
}

#[cfg(test)]
mod tests_range_projection {
    use super::*;

    fn range(start: u32, end: u32) -> TextRange {
        TextRange::new(start.into(), end.into())
    }

    #[test]
    fn simple_literal_projects_after_opening_quote() {
        let literal = "\"X = 1\"";

        assert_eq!(map_range_query_to_literal(literal, range(0, 1)), range(1, 2));
    }

    #[test]
    fn empty_query_range_projects_to_zero_width_literal_range() {
        let literal = "\"X = 1\"";

        assert_eq!(map_range_query_to_literal(literal, range(2, 2)), range(3, 3));
    }

    #[test]
    fn multiline_pipe_projection_includes_newline_and_pipe_prefix() {
        let literal = "\"VYIBRAT X\n|IZ T\"";

        assert_eq!(map_range_query_to_literal(literal, range(10, 12)), range(10, 14));
    }

    #[test]
    fn escaped_quotes_projection_spans_doubled_quote_bytes() {
        let literal = "\"X = \"\"Y\"\"\"";

        assert_eq!(map_range_query_to_literal(literal, range(4, 7)), range(5, 10));
    }

    #[test]
    fn cyrillic_and_escaped_quotes_project_on_utf8_byte_boundaries() {
        let literal = "\"Поле = \"\"Я\"\"\"";

        assert_eq!(map_range_query_to_literal(literal, range(11, 15)), range(12, 18));
    }

    #[test]
    fn crlf_pipe_projection_includes_crlf_and_pipe_prefix() {
        let literal = "\"X\r\n|Y\"";

        assert_eq!(map_range_query_to_literal(literal, range(2, 3)), range(2, 6));
    }

    #[test]
    fn bsl_comment_continuation_projection_includes_comment_bytes() {
        let literal = "\"X\n// comment\n|Y\"";

        assert_eq!(
            map_range_query_to_literal(literal, range(2, 3)),
            range(2, (literal.len() - 1) as u32),
        );
    }
}
