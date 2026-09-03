use parser_error::{ParseError, RecoveryKind};

use crate::{Parse, SyntaxKind, SyntaxNode, TextRange};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblQueryInfo {
    pub bsl_literal_range: TextRange,

    pub query_text: String,

    pub query_ast: Option<Parse<SyntaxNode>>,

    pub quote_corrections: Vec<(usize, usize)>,

    pub error_ranges_in_bsl: Vec<(TextRange, ParseError)>,
}

/// The parsed query of one string literal without the literal's position —
/// what a lowered body stores, so that the body stays equal when the literal
/// moves. [`SdblQueryInfo`] is the same payload placed in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblQuery {
    pub query_text: String,

    pub query_ast: Option<Parse<SyntaxNode>>,

    pub quote_corrections: Vec<(usize, usize)>,

    /// Parse errors in the coordinates of the literal text.
    pub error_ranges_in_bsl: Vec<(TextRange, ParseError)>,
}

impl SdblQuery {
    pub fn is_valid(&self) -> bool {
        self.query_ast.as_ref().map(|p| !p.has_errors()).unwrap_or(false)
    }

    pub fn syntax_node(&self) -> Option<SyntaxNode> {
        self.query_ast.as_ref().map(|p| p.syntax_node())
    }
}

impl SdblQueryInfo {
    pub fn from_query(bsl_literal_range: TextRange, query: &SdblQuery) -> Self {
        Self {
            bsl_literal_range,
            query_text: query.query_text.clone(),
            query_ast: query.query_ast.clone(),
            quote_corrections: query.quote_corrections.clone(),
            error_ranges_in_bsl: query.error_ranges_in_bsl.clone(),
        }
    }

    pub fn new(
        bsl_literal_range: TextRange,
        query_text: String,
        query_ast: Option<Parse<SyntaxNode>>,
        quote_corrections: Vec<(usize, usize)>,
        error_ranges_in_bsl: Vec<(TextRange, ParseError)>,
    ) -> Self {
        Self { bsl_literal_range, query_text, query_ast, quote_corrections, error_ranges_in_bsl }
    }

    pub fn is_valid(&self) -> bool {
        self.query_ast.as_ref().map(|p| !p.has_errors()).unwrap_or(false)
    }

    pub fn syntax_node(&self) -> Option<SyntaxNode> {
        self.query_ast.as_ref().map(|p| p.syntax_node())
    }
}

/// Every parse error of an SDBL query, in the coordinates of the query text itself.
///
/// Two sources, and the second is not derivable from the tree: the parser's own
/// [`ParseError`]s, plus a synthetic one for a reference path that ends on a dot. `ERROR`
/// nodes carry no `ParseError`, so a consumer that walks the tree instead of calling this
/// cannot render the same messages.
///
/// Callers embedding the query in a BSL literal map each range through
/// [`map_range_query_to_literal`]; a caller validating a bare query text uses them as they are.
pub fn collect_query_parse_errors(query_ast: &Parse<SyntaxNode>) -> Vec<(TextRange, ParseError)> {
    let mut errors: Vec<(TextRange, ParseError)> =
        query_ast.errors().iter().map(|err| (err.range(), err.structured().clone())).collect();

    let root = query_ast.syntax_node();
    for refs in root.descendants().filter(|node| node.kind() == SyntaxKind::SDBL_REFS_EXPR) {
        if let Some(dot_range) = trailing_dot_range(&refs) {
            errors.push((
                dot_range,
                ParseError::Custom {
                    message: "незавершённый путь в ссылке",
                    recovery: RecoveryKind::Custom,
                },
            ));
        }
    }

    errors
}

fn trailing_dot_range(refs: &SyntaxNode) -> Option<TextRange> {
    for child in refs.children_with_tokens().collect::<Vec<_>>().into_iter().rev() {
        let kind = child.kind();
        if kind.is_trivia() {
            continue;
        }

        return if kind == SyntaxKind::DOT {
            child.as_token().map(|token| token.text_range())
        } else {
            None
        };
    }

    None
}

pub fn extract_sdbl_with_corrections(node: &SyntaxNode) -> Option<(String, Vec<(usize, usize)>)> {
    let mut result = String::new();
    let mut corrections = Vec::new();
    let mut tokens = node.children_with_tokens().filter_map(|it| it.into_token());

    let first_token = tokens.next()?;

    match first_token.kind() {
        SyntaxKind::STRING => {
            let text = first_token.text();
            if text.len() < 2 {
                return None;
            }
            let inner = &text[1..text.len() - 1];

            let mut pos = 0;
            let mut chars = inner.chars().peekable();
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
        }
        SyntaxKind::STRING_START => {
            let text = first_token.text();
            if text.is_empty() {
                return None;
            }

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

            for token in tokens {
                match token.kind() {
                    SyntaxKind::NEWLINE => {
                        result.push('\n');
                        pos += 1;
                    }
                    SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL => {
                        let text = token.text();
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
    if content_start < first_line.end && bytes[content_start] == b'"' {
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
    let mut end =
        if query_start == query_end { start } else { projection.projected_end.unwrap_or(fallback) };

    if end < start {
        end = start;
    }
    debug_assert!(start <= end, "sdbl_query projection produced inverted range");

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
    debug_assert!(text.is_char_boundary(start), "start must be on a UTF-8 char boundary");
    debug_assert!(text.is_char_boundary(end), "end must be on a UTF-8 char boundary");
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
    debug_assert!(
        literal_text.is_char_boundary(start),
        "process_content start must be on a UTF-8 char boundary"
    );
    debug_assert!(
        literal_text.is_char_boundary(end),
        "process_content end must be on a UTF-8 char boundary"
    );
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

        let info = SdblQueryInfo::new(range, query_text.clone(), None, vec![], vec![]);

        assert_eq!(info.bsl_literal_range, range);
        assert_eq!(info.query_text, query_text);
        assert!(info.error_ranges_in_bsl.is_empty());
        assert!(!info.is_valid());
        assert!(info.syntax_node().is_none());
    }

    #[test]
    fn test_is_valid_with_no_ast() {
        let info = SdblQueryInfo::new(
            TextRange::new(0u32.into(), 10u32.into()),
            "INVALID QUERY".to_string(),
            None,
            vec![],
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
            vec![],
        );

        let info2 = info1.clone();

        assert_eq!(info1, info2);
        assert_eq!(info1.bsl_literal_range, info2.bsl_literal_range);
        assert_eq!(info1.query_text, info2.query_text);
    }

    #[test]
    fn test_error_ranges_in_bsl_empty_round_trip() {
        let info = SdblQueryInfo::new(
            TextRange::new(3u32.into(), 42u32.into()),
            "SELECT * FROM Table".to_string(),
            None,
            vec![],
            vec![],
        );

        assert!(info.error_ranges_in_bsl.is_empty());
    }

    #[test]
    fn test_error_ranges_in_bsl_preserves_structured_parse_error() {
        let range = TextRange::new(5u32.into(), 6u32.into());
        let error = ParseError::Custom {
            message: "незавершённый путь в ссылке",
            recovery: parser_error::RecoveryKind::Custom,
        };

        let info = SdblQueryInfo::new(
            TextRange::new(0u32.into(), 20u32.into()),
            "SELECT * FROM Table".to_string(),
            None,
            vec![],
            vec![(range, error.clone())],
        );

        assert_eq!(info.error_ranges_in_bsl, vec![(range, error)]);
        assert!(matches!(
            &info.error_ranges_in_bsl[0].1,
            ParseError::Custom {
                message: "незавершённый путь в ссылке",
                recovery: parser_error::RecoveryKind::Custom,
            }
        ));
    }
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

    #[test]
    fn out_of_bounds_query_end_does_not_invert_projection() {
        let literal = "\"X\n|Y\n|Z\"";
        let projected = map_range_query_to_literal(literal, range(3, 1000));
        assert!(
            projected.start() <= projected.end(),
            "projection produced inverted range: {:?}",
            projected,
        );
    }
}
