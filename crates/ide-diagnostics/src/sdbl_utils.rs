//! Shared utilities for SDBL diagnostics.
//!
//! Contains common code for extracting SDBL queries from BSL string literals
//! and mapping diagnostic positions between SDBL and BSL coordinate systems.
//!
//! This module is shared by all SDBL diagnostics to avoid code duplication.

use syntax::{SyntaxKind, SyntaxNode, TextRange};

/// Maps SDBL positions back to BSL source positions.
///
/// Handles multiline strings with `|` prefixes. When SDBL is extracted from BSL strings,
/// the `|` prefixes and quotes are removed, so diagnostic positions in SDBL don't correspond
/// to the original BSL source positions. This mapper tracks the BSL literal position and
/// converts SDBL TextRange to BSL TextRange.
///
/// ## Algorithm
///
/// Based on reference implementation from bsl-language-server-rust:
/// - Line mapping: `bsl_line = bsl_literal_line + sdbl_line`
/// - Column mapping:
///   - First line: `bsl_col = bsl_literal_col + sdbl_col + 1` (+1 for opening quote)
///   - Multiline: `bsl_col = sdbl_col` (already aligned after `|` removal)
///
/// ## Performance
///
/// Uses `&str` instead of `String` to avoid copying the entire BSL source for each query.
/// This is critical for files with many SDBL queries (e.g., 100+ queries).
///
/// Caches BSL literal starting position to avoid recalculating for each diagnostic.
#[derive(Debug, Clone)]
pub struct SdblPositionMapper<'a> {
    /// Position of the string literal (LITERAL node) in BSL source
    /// Kept for debugging/inspection purposes
    #[allow(dead_code)]
    bsl_literal_range: TextRange,

    /// Original BSL file content (for line/column calculations)
    /// OPTIMIZATION: Reference instead of owned String to avoid massive allocations
    bsl_source: &'a str,

    /// Cached BSL literal starting position (line, column)
    /// OPTIMIZATION: Computed once, reused for all diagnostics in this query
    bsl_literal_line: u32,
    bsl_literal_col: u32,
}

impl<'a> SdblPositionMapper<'a> {
    /// Create a new position mapper from a LITERAL node.
    pub fn new(bsl_literal_node: &SyntaxNode, bsl_source: &'a str) -> Self {
        let bsl_literal_range = bsl_literal_node.text_range();
        let (bsl_literal_line, bsl_literal_col) =
            byte_offset_to_line_col(bsl_source, u32::from(bsl_literal_range.start()));

        Self { bsl_literal_range, bsl_source, bsl_literal_line, bsl_literal_col }
    }

    /// Create a new position mapper from a cached TextRange.
    ///
    /// This is used when working with cached SDBL queries where we already
    /// have the literal range but not the original SyntaxNode.
    ///
    /// OPTIMIZATION: Uses `&str` reference instead of copying the entire source.
    /// OPTIMIZATION: Caches literal position to avoid recalculating for each diagnostic.
    pub fn new_from_range(bsl_literal_range: TextRange, bsl_source: &'a str) -> Self {
        let (bsl_literal_line, bsl_literal_col) =
            byte_offset_to_line_col(bsl_source, u32::from(bsl_literal_range.start()));

        Self { bsl_literal_range, bsl_source, bsl_literal_line, bsl_literal_col }
    }

    /// Map SDBL TextRange to BSL TextRange.
    ///
    /// Takes a range within the extracted SDBL text and returns the corresponding
    /// range in the original BSL source file.
    pub fn map_range(&self, sdbl_range: TextRange, sdbl_text: &str) -> TextRange {
        // 1. Convert SDBL byte offsets to line:column
        let (sdbl_start_line, sdbl_start_col) =
            byte_offset_to_line_col(sdbl_text, u32::from(sdbl_range.start()));
        let (sdbl_end_line, sdbl_end_col) =
            byte_offset_to_line_col(sdbl_text, u32::from(sdbl_range.end()));

        // 2. Use cached BSL literal starting position (computed in constructor)
        let bsl_literal_line = self.bsl_literal_line;
        let bsl_literal_col = self.bsl_literal_col;

        // 3. Map SDBL → BSL accounting for removed | prefix
        let bsl_start_line = bsl_literal_line + sdbl_start_line;
        let bsl_start_col = if sdbl_start_line == 0 {
            // First line of SDBL (same line as opening quote in BSL)
            bsl_literal_col + sdbl_start_col + 1 // +1 for opening quote
        } else {
            // Multiline: find where | is in BSL line
            let bsl_line_text = self.bsl_source.lines().nth(bsl_start_line as usize).unwrap_or("");
            if let Some(pipe_pos) = bsl_line_text.find('|') {
                // Count whitespace after | that was kept in SDBL
                let after_pipe = &bsl_line_text[pipe_pos + 1..];
                let whitespace_count =
                    after_pipe.chars().take_while(|c| c.is_whitespace() && *c != '\n').count();
                let content_start_col = (pipe_pos as u32) + 1 + (whitespace_count as u32);
                // content_start_col points to first non-whitespace in BSL
                // sdbl_start_col includes leading whitespace, so we need to subtract it
                content_start_col + sdbl_start_col - (whitespace_count as u32)
            } else {
                sdbl_start_col // Fallback if no | found
            }
        };

        // Same mapping for end position
        let bsl_end_line = bsl_literal_line + sdbl_end_line;
        let bsl_end_col = if sdbl_end_line == 0 {
            bsl_literal_col + sdbl_end_col + 1
        } else {
            let bsl_line_text = self.bsl_source.lines().nth(bsl_end_line as usize).unwrap_or("");
            if let Some(pipe_pos) = bsl_line_text.find('|') {
                let after_pipe = &bsl_line_text[pipe_pos + 1..];
                let whitespace_count =
                    after_pipe.chars().take_while(|c| c.is_whitespace() && *c != '\n').count();
                let content_start_col = (pipe_pos as u32) + 1 + (whitespace_count as u32);
                content_start_col + sdbl_end_col - (whitespace_count as u32)
            } else {
                sdbl_end_col
            }
        };

        // 4. Convert back to TextRange (byte offsets in BSL)
        let bsl_start_offset =
            line_col_to_byte_offset(self.bsl_source, bsl_start_line, bsl_start_col);
        let bsl_end_offset = line_col_to_byte_offset(self.bsl_source, bsl_end_line, bsl_end_col);

        TextRange::new(bsl_start_offset.into(), bsl_end_offset.into())
    }
}

/// Convert byte offset to (line, column) position - 0-indexed.
///
/// Iterates through the text counting newlines and character positions.
pub fn byte_offset_to_line_col(text: &str, offset: u32) -> (u32, u32) {
    let mut line = 0;
    let mut col = 0;

    for (idx, ch) in text.char_indices() {
        if idx as u32 >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    (line, col)
}

/// Convert (line, column) position to byte offset - 0-indexed.
///
/// Iterates through the text to find the byte offset at the given line and column.
pub fn line_col_to_byte_offset(text: &str, target_line: u32, target_col: u32) -> u32 {
    let mut line = 0;
    let mut col = 0;

    for (idx, ch) in text.char_indices() {
        if line == target_line && col == target_col {
            return idx as u32;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    // If we reach here, we're at EOF - return length
    text.len() as u32
}

/// Check if a LITERAL node is part of string concatenation.
///
/// Detects patterns like: `"text" + variable` or `"text" + "more text"`
/// These are skipped because extraction would be incomplete.
pub fn has_string_concatenation(node: &SyntaxNode) -> bool {
    // Check if there's a PLUS token after this literal
    if let Some(next) = node.next_sibling_or_token() {
        if let Some(token) = next.as_token() {
            if token.kind() == SyntaxKind::PLUS {
                return true;
            }
        }
    }

    // Check if there's a PLUS token before this literal
    if let Some(prev) = node.prev_sibling_or_token() {
        if let Some(token) = prev.as_token() {
            if token.kind() == SyntaxKind::PLUS {
                return true;
            }
        }
    }

    false
}

/// Extract string content from a LITERAL node containing STRING tokens.
///
/// Handles both simple strings and multiline strings:
/// - Simple: `"text"` → one STRING token
/// - Multiline: `"line1\n|line2"` → STRING_START + NEWLINE + STRING_PART + ... + STRING_TAIL
pub fn extract_string_content(node: &SyntaxNode) -> Option<String> {
    let mut result = String::new();
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
            // Remove outer quotes
            let inner = &text[1..text.len() - 1];
            // Unescape quotes (BSL uses "" for escaped ")
            result = inner.replace("\"\"", "\"");
        }
        SyntaxKind::STRING_START => {
            // Multiline string: "line1\n|line2\n|line3"
            // STRING_START contains: "line1
            let text = first_token.text();
            if text.is_empty() {
                return None;
            }
            // Remove opening quote
            result.push_str(&text[1..]);

            // Process remaining tokens
            for token in tokens {
                match token.kind() {
                    SyntaxKind::NEWLINE => {
                        result.push('\n');
                    }
                    SyntaxKind::STRING_PART => {
                        // STRING_PART contains: |line (with | prefix)
                        let text = token.text();
                        // Remove | prefix
                        if let Some(content) = text.strip_prefix('|') {
                            result.push_str(content);
                        }
                    }
                    SyntaxKind::STRING_TAIL => {
                        // STRING_TAIL contains: |line" (with | prefix and closing quote)
                        let text = token.text();
                        // Remove | prefix and closing quote
                        if let Some(content) = text.strip_prefix('|') {
                            if let Some(content) = content.strip_suffix('"') {
                                result.push_str(content);
                            }
                        }
                        break;
                    }
                    _ => {}
                }
            }

            // Unescape quotes
            result = result.replace("\"\"", "\"");
        }
        _ => return None,
    }

    Some(result)
}
