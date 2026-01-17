//! Shared utilities for SDBL diagnostics.
//!
//! Contains common code for extracting SDBL queries from BSL string literals
//! and mapping diagnostic positions between SDBL and BSL coordinate systems.
//!
//! This module is shared by all SDBL diagnostics to avoid code duplication.

use syntax::{SyntaxKind, SyntaxNode, TextRange};

/// Maps SDBL positions back to BSL source positions.
///
/// Handles multiline strings with `|` prefixes and escaped quotes `""` → `"`.
/// When SDBL is extracted from BSL strings:
/// - `|` prefixes are removed
/// - `""` is unescaped to `"` (SDBL canonical form)
///
/// This mapper tracks replacements and converts SDBL TextRange to BSL TextRange.
///
/// ## Algorithm
///
/// Based on reference implementation from bsl-language-server-rust:
/// - Line mapping: `bsl_line = bsl_literal_line + sdbl_line`
/// - Column mapping:
///   - First line: `bsl_col = bsl_literal_col + sdbl_col + 1` (+1 for opening quote)
///   - Multiline: `bsl_col = pipe_pos + 1 + sdbl_col`
///   - Quote escaping: `bsl_col += count_escaped_quotes_before(sdbl_col)`
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

    /// Line start positions (byte offsets) for O(1) line lookup
    /// OPTIMIZATION: Build once, use for all map_range() calls
    /// line_starts[i] = byte offset where line i starts
    line_starts: Vec<usize>,

    /// Quote escape corrections: (sdbl_offset, correction_bytes)
    /// Each entry represents a `""` → `"` replacement in SDBL extraction.
    /// When mapping SDBL position X to BSL, add sum of corrections for positions < X.
    quote_corrections: Vec<(usize, usize)>,
}

impl<'a> SdblPositionMapper<'a> {
    /// Create a new position mapper from a LITERAL node.
    pub fn new(
        bsl_literal_node: &SyntaxNode,
        bsl_source: &'a str,
        quote_corrections: Vec<(usize, usize)>,
    ) -> Self {
        let bsl_literal_range = bsl_literal_node.text_range();
        let (bsl_literal_line, bsl_literal_col) =
            byte_offset_to_line_col(bsl_source, u32::from(bsl_literal_range.start()));

        // Build line index for O(1) line lookups
        let line_starts = build_line_index(bsl_source);

        Self {
            bsl_literal_range,
            bsl_source,
            bsl_literal_line,
            bsl_literal_col,
            line_starts,
            quote_corrections,
        }
    }

    /// Create a new position mapper from a cached TextRange.
    ///
    /// This is used when working with cached SDBL queries where we already
    /// have the literal range but not the original SyntaxNode.
    ///
    /// OPTIMIZATION: Uses `&str` reference instead of copying the entire source.
    /// OPTIMIZATION: Caches literal position to avoid recalculating for each diagnostic.
    /// OPTIMIZATION: Builds line index once for O(1) line lookup.
    pub fn new_from_range(
        bsl_literal_range: TextRange,
        bsl_source: &'a str,
        quote_corrections: Vec<(usize, usize)>,
    ) -> Self {
        let (bsl_literal_line, bsl_literal_col) =
            byte_offset_to_line_col(bsl_source, u32::from(bsl_literal_range.start()));

        // Build line index for O(1) line lookups
        let line_starts = build_line_index(bsl_source);

        Self {
            bsl_literal_range,
            bsl_source,
            bsl_literal_line,
            bsl_literal_col,
            line_starts,
            quote_corrections,
        }
    }

    /// Create a new position mapper with pre-built line index.
    ///
    /// OPTIMIZATION: Reuses shared line index built once for the entire file.
    /// This eliminates 102× redundant line index builds for files with many SDBL queries.
    pub fn new_from_range_with_line_index(
        bsl_literal_range: TextRange,
        bsl_source: &'a str,
        line_starts: &'a [usize],
        quote_corrections: Vec<(usize, usize)>,
    ) -> Self {
        let (bsl_literal_line, bsl_literal_col) =
            byte_offset_to_line_col(bsl_source, u32::from(bsl_literal_range.start()));

        // Clone the line index (cheap - just Vec of usize)
        let line_starts = line_starts.to_vec();

        Self {
            bsl_literal_range,
            bsl_source,
            bsl_literal_line,
            bsl_literal_col,
            line_starts,
            quote_corrections,
        }
    }

    /// Create mapper from SdblQueryInfo (PREFERRED API).
    ///
    /// This is the canonical way to create a mapper - quote_corrections
    /// are taken from the query info (single source of truth).
    pub fn from_query_info(
        query_info: &syntax::SdblQueryInfo,
        bsl_source: &'a str,
        line_starts: &'a [usize],
    ) -> Self {
        Self::new_from_range_with_line_index(
            query_info.bsl_literal_range,
            bsl_source,
            line_starts,
            query_info.quote_corrections.clone(),
        )
    }

    /// Map SDBL TextRange to BSL TextRange.
    ///
    /// Takes a range within the extracted SDBL text and returns the corresponding
    /// range in the original BSL source file. Accounts for `""` → `"` quote escaping.
    pub fn map_range(&self, sdbl_range: TextRange, sdbl_text: &str) -> TextRange {
        // 1. Convert SDBL byte offsets to line:column (in SDBL coordinates WITHOUT corrections)
        let (sdbl_start_line, sdbl_start_col) =
            byte_offset_to_line_col(sdbl_text, u32::from(sdbl_range.start()));
        let (sdbl_end_line, sdbl_end_col) =
            byte_offset_to_line_col(sdbl_text, u32::from(sdbl_range.end()));

        // 2. Calculate quote escape corrections (PER-LINE!)
        // Corrections only apply to characters on the SAME line in SDBL
        let sdbl_start = u32::from(sdbl_range.start()) as usize;
        let sdbl_end = u32::from(sdbl_range.end()) as usize;

        // For start position: sum corrections on the same SDBL line, before start column
        let start_correction: usize = self
            .quote_corrections
            .iter()
            .filter(|(pos, _)| {
                let (line, _col) = byte_offset_to_line_col(sdbl_text, *pos as u32);
                line == sdbl_start_line && *pos < sdbl_start
            })
            .map(|(_, chars)| chars)
            .sum();

        // For end position: sum corrections on the same SDBL line, before end column
        let end_correction: usize = self
            .quote_corrections
            .iter()
            .filter(|(pos, _)| {
                let (line, _col) = byte_offset_to_line_col(sdbl_text, *pos as u32);
                line == sdbl_end_line && *pos < sdbl_end
            })
            .map(|(_, chars)| chars)
            .sum();

        // 3. Use cached BSL literal starting position (computed in constructor)
        let bsl_literal_line = self.bsl_literal_line;
        let bsl_literal_col = self.bsl_literal_col;

        // 4. Map SDBL → BSL accounting for removed | prefix AND quote corrections
        let bsl_start_line = bsl_literal_line + sdbl_start_line;
        let bsl_start_col = if sdbl_start_line == 0 {
            // First line of SDBL (same line as opening quote in BSL)
            bsl_literal_col + sdbl_start_col + 1 + (start_correction as u32) // +1 for opening quote + corrections
        } else {
            // Multiline: find where | is in BSL line
            // OPTIMIZATION: Use line index for O(1) line lookup instead of lines().nth() O(n)
            let bsl_line_text =
                get_line_text(self.bsl_source, &self.line_starts, bsl_start_line as usize);
            if let Some(pipe_pos) = bsl_line_text.find('|') {
                // SDBL column is relative to content after |
                // BSL column is: position of | + 1 (skip |) + SDBL column offset + quote corrections
                (pipe_pos as u32) + 1 + sdbl_start_col + (start_correction as u32)
            } else {
                sdbl_start_col + (start_correction as u32) // Fallback if no | found
            }
        };

        // Same mapping for end position
        let bsl_end_line = bsl_literal_line + sdbl_end_line;
        let bsl_end_col = if sdbl_end_line == 0 {
            bsl_literal_col + sdbl_end_col + 1 + (end_correction as u32)
        } else {
            // OPTIMIZATION: Use line index for O(1) line lookup instead of lines().nth() O(n)
            let bsl_line_text =
                get_line_text(self.bsl_source, &self.line_starts, bsl_end_line as usize);
            if let Some(pipe_pos) = bsl_line_text.find('|') {
                // SDBL column is relative to content after |
                // BSL column is: position of | + 1 (skip |) + SDBL column offset + quote corrections
                (pipe_pos as u32) + 1 + sdbl_end_col + (end_correction as u32)
            } else {
                sdbl_end_col + (end_correction as u32)
            }
        };

        // 5. Convert back to TextRange (byte offsets in BSL)
        // OPTIMIZATION: Use line index for O(col) conversion instead of O(total_text)
        let bsl_start_offset = line_col_to_byte_offset_fast(
            self.bsl_source,
            &self.line_starts,
            bsl_start_line,
            bsl_start_col,
        );
        let bsl_end_offset = line_col_to_byte_offset_fast(
            self.bsl_source,
            &self.line_starts,
            bsl_end_line,
            bsl_end_col,
        );

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

/// Build line index for O(1) line lookup (public API).
///
/// Returns a Vec where line_starts[i] = byte offset where line i starts.
/// Line 0 starts at offset 0, line 1 starts after the first \n, etc.
///
/// This should be called ONCE per file and reused for all mappers in that file.
pub fn build_line_index_shared(text: &str) -> Vec<usize> {
    build_line_index(text)
}

/// Build line index for O(1) line lookup (internal).
///
/// Returns a Vec where line_starts[i] = byte offset where line i starts.
/// Line 0 starts at offset 0, line 1 starts after the first \n, etc.
fn build_line_index(text: &str) -> Vec<usize> {
    let mut line_starts = vec![0]; // Line 0 starts at 0

    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            // Next line starts after this \n
            line_starts.push(idx + 1);
        }
    }

    line_starts
}

/// Get text of a specific line using line index (O(1) instead of O(n)).
///
/// Returns the text of the line without the trailing newline.
fn get_line_text<'a>(text: &'a str, line_starts: &[usize], line: usize) -> &'a str {
    if line >= line_starts.len() {
        return "";
    }

    let start = line_starts[line];
    let end = line_starts.get(line + 1).copied().unwrap_or(text.len());

    // Remove trailing \n if present
    let line_text = &text[start..end];
    line_text.strip_suffix('\n').unwrap_or(line_text)
}

/// Convert (line, column) to byte offset using line index.
///
/// This is faster than line_col_to_byte_offset() because it uses
/// the pre-built line index to skip directly to the target line (O(1)),
/// then only iterates through characters on that line (O(col)).
///
/// Overall: O(col) instead of O(total_text)
fn line_col_to_byte_offset_fast(
    text: &str,
    line_starts: &[usize],
    target_line: u32,
    target_col: u32,
) -> u32 {
    let line = target_line as usize;
    if line >= line_starts.len() {
        return line_starts.last().copied().unwrap_or(0) as u32;
    }

    let line_start = line_starts[line];

    // Handle column 0
    if target_col == 0 {
        return line_start as u32;
    }

    // Find byte offset by iterating through characters on this line only
    // (not through the entire file!)
    let next_line_start = line_starts.get(line + 1).copied().unwrap_or(text.len());
    let line_text = &text[line_start..next_line_start];

    for (char_count, (byte_idx, _ch)) in line_text.char_indices().enumerate() {
        if char_count as u32 == target_col {
            return (line_start + byte_idx) as u32;
        }
    }

    // If we reach here, column is past end of line
    next_line_start as u32
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

/// Re-export from syntax crate for backward compatibility.
pub use syntax::extract_sdbl_with_corrections;

/// Extract SDBL query text from a LITERAL node without unescaping quotes.
///
/// This is specifically for SDBL to preserve exact positions for semantic highlighting.
/// Unlike `extract_string_content()`, this does NOT unescape `""` → `"`.
///
/// Handles both simple strings and multiline strings:
/// - Simple: `"text"` → one STRING token
/// - Multiline: `"line1\n|line2"` → STRING_START + NEWLINE + STRING_PART + ... + STRING_TAIL
pub fn extract_sdbl_query_text(node: &SyntaxNode) -> Option<String> {
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
            // Remove outer quotes BUT keep doubled quotes as-is
            let inner = &text[1..text.len() - 1];
            result = inner.to_string();
        }
        SyntaxKind::STRING_START => {
            // Multiline string: "line1\n|line2\n|line3"
            // STRING_START contains: "line1
            let text = first_token.text();
            if text.is_empty() {
                return None;
            }
            // Remove opening quote BUT keep doubled quotes as-is
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

            // DON'T unescape quotes - keep "" as-is for position mapping
        }
        _ => return None,
    }

    Some(result)
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
