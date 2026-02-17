use text_size::TextSize;

/// Count number of escaped quotes ("") in a string.
///
/// In BSL string literals, quotes are escaped by doubling: "" represents one "
/// This function counts how many such escaped quotes exist.
/// The count represents how many bytes will be removed during unescape.
pub(crate) fn count_escaped_quotes(text: &str) -> usize {
    let mut count = 0;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '"' {
            // Check if next char is also a quote (escaped quote in BSL)
            if chars.peek() == Some(&'"') {
                chars.next(); // Skip the second quote
                count += 1; // One byte will be removed during unescape
            }
        }
    }

    count
}

/// Unescape doubled quotes in a string.
///
/// In BSL string literals, quotes are escaped by doubling: "" -> "
/// This function converts escaped quotes back to single quotes.
pub(crate) fn unescape_bsl_quotes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '"' {
            // Check if next char is also a quote (escaped quote in BSL)
            if chars.peek() == Some(&'"') {
                chars.next(); // Skip the second quote
                result.push('"'); // Add single quote to result
            } else {
                // Single quote (shouldn't happen in middle of SDBL, but handle it)
                result.push('"');
            }
        } else {
            result.push(ch);
        }
    }

    result
}

pub(crate) fn extract_query_text(literal_text: &str) -> String {
    let mut result = String::new();
    let mut first_line = true;
    let mut line_num = 0;

    for line in literal_text.lines() {
        line_num += 1;
        if first_line {
            // First line: skip leading whitespace, then opening quote
            let trimmed = line.trim_start();
            let line_text = trimmed.trim_start_matches('"');
            tracing::info!(
                "extract_query_text line {}: input={:?} → after trim={:?} → output={:?}",
                line_num,
                line,
                trimmed,
                line_text
            );
            result.push_str(line_text);
            first_line = false;
        } else {
            let trimmed = line.trim_start();

            // Skip BSL comment lines (they are separate COMMENT tokens in BSL lexer)
            // This matches behavior of syntax::extract_sdbl_with_corrections()
            if trimmed.starts_with("//") {
                tracing::info!(
                    "extract_query_text line {}: input={:?} → BSL comment, SKIP",
                    line_num,
                    line
                );
                continue;
            }

            // Continuation lines: skip whitespace before | and the | itself,
            // but preserve whitespace AFTER | (it's part of SDBL formatting)
            result.push('\n');

            if let Some(line_text) = trimmed.strip_prefix('|') {
                // Line with pipe: skip whitespace before | and the | itself
                tracing::info!(
                    "extract_query_text line {}: input={:?} → skip ws + | → output={:?}",
                    line_num,
                    line,
                    line_text
                );
                result.push_str(line_text);
            } else {
                // Line without pipe and not a comment: preserve as-is
                // This shouldn't normally happen in valid BSL multiline strings
                tracing::warn!(
                    "extract_query_text line {}: input={:?} → no pipe and not comment, keep as-is",
                    line_num,
                    line
                );
                result.push_str(line);
            }
        }
    }

    // Remove closing quote if present
    let mut final_result = result.trim_end_matches('"').to_string();

    // Unescape doubled quotes: "" -> "
    // In BSL string literals, quotes are escaped by doubling them
    final_result = unescape_bsl_quotes(&final_result);

    tracing::info!("extract_query_text: final result length={}", final_result.len());
    final_result
}

/// Map offset from literal text (with quotes/|) to query text offset (without quotes/|).
///
/// # Arguments
///
/// * `literal_text` - Full literal text including quotes and | prefixes
/// * `offset_in_literal` - Offset within the literal text
///
/// # Returns
///
/// Offset within the extracted query text (without quotes/|).
///
/// Note: The returned offset is guaranteed to be on a UTF-8 char boundary.
pub(crate) fn map_offset_to_query(literal_text: &str, offset_in_literal: TextSize) -> TextSize {
    let offset_usize: usize = offset_in_literal.into();

    // First extract the query text to validate char boundaries
    let query_text = extract_query_text(literal_text);

    tracing::info!(
        "map_offset_to_query: offset_in_literal={}, query_text_len={}",
        offset_usize,
        query_text.len()
    );

    let mut literal_pos = 0;
    let mut query_pos = 0;
    let mut first_line = true;
    let mut line_num = 0;

    for line in literal_text.lines() {
        let line_len = line.len();
        line_num += 1;

        // Determine line ending size for this line
        // .lines() removes \n, \r\n, or \r, so we need to check what was actually there
        let line_end_in_literal = literal_pos + line_len;
        let newline_len = if line_end_in_literal < literal_text.len() {
            // Check if next bytes are \r\n (Windows), \n (Unix), or \r (old Mac)
            let remaining = &literal_text.as_bytes()[line_end_in_literal..];
            if remaining.starts_with(b"\r\n") {
                2
            } else if remaining.starts_with(b"\n") || remaining.starts_with(b"\r") {
                1
            } else {
                0 // Last line without newline
            }
        } else {
            0 // Last line
        };

        tracing::info!(
            "  line {}: literal_pos={}, line_len={}, newline_len={}, line_text={:?}",
            line_num,
            literal_pos,
            line_len,
            newline_len,
            line
        );

        // Check if this is a BSL comment line (should be skipped in query text)
        let trimmed = line.trim_start();
        let is_bsl_comment = !first_line && trimmed.starts_with("//");

        if literal_pos + line_len >= offset_usize {
            // Cursor is on this line
            let offset_in_line = offset_usize - literal_pos;

            if first_line {
                // First line: skip leading whitespace, then opening quote
                let skip_whitespace = line.len() - trimmed.len();
                let skip_quote = if trimmed.starts_with('"') { 1 } else { 0 };
                let skip_total = skip_whitespace + skip_quote;

                let mut pos_in_query = offset_in_line.saturating_sub(skip_total);

                // Count escaped quotes in this line segment
                if skip_total < line.len() && offset_in_line > skip_total {
                    let line_segment = &line[skip_total..offset_in_line.min(line.len())];
                    let escaped_quotes_count = count_escaped_quotes(line_segment);
                    pos_in_query = pos_in_query.saturating_sub(escaped_quotes_count);
                }

                query_pos += pos_in_query;
                tracing::info!(
                    "  -> FOUND on first line: offset_in_line={}, skip_ws={}, skip_quote={}, skip_total={}, final query_pos={}",
                    offset_in_line,
                    skip_whitespace,
                    skip_quote,
                    skip_total,
                    query_pos
                );
            } else if is_bsl_comment {
                // Cursor is on BSL comment line - these are skipped in extracted SDBL
                // Return position at end of previous query content
                tracing::info!(
                    "  -> FOUND on BSL comment line: returning query_pos={} (end of previous content)",
                    query_pos
                );
            } else {
                // Continuation line with pipe
                let skip_whitespace_before = line.len() - trimmed.len();

                let skip_total = if trimmed.starts_with('|') {
                    // Line with pipe: skip whitespace before | and the | itself
                    skip_whitespace_before + 1
                } else {
                    // Line without pipe and not a comment: shouldn't happen normally
                    0
                };

                // Add newline before this line's content
                query_pos += 1;

                let mut pos_in_query = offset_in_line.saturating_sub(skip_total);

                // Count escaped quotes in this line segment
                if skip_total < line.len() && offset_in_line > skip_total {
                    let line_segment = &line[skip_total..offset_in_line.min(line.len())];
                    let escaped_quotes_count = count_escaped_quotes(line_segment);
                    pos_in_query = pos_in_query.saturating_sub(escaped_quotes_count);
                }

                query_pos += pos_in_query;
                tracing::info!(
                    "  -> FOUND on continuation line: offset_in_line={}, skip_total={}, final query_pos={}",
                    offset_in_line,
                    skip_total,
                    query_pos
                );
            }

            // Ensure we're on a char boundary in the extracted query text
            let result = ensure_char_boundary(&query_text, query_pos);
            tracing::info!("  -> after ensure_char_boundary: {:?}", result);
            return result;
        }

        // Move to next line (including line ending bytes)
        literal_pos += line_len + newline_len;

        if first_line {
            // Skip leading whitespace, then opening quote
            let trimmed = line.trim_start();
            let skip_whitespace = line.len() - trimmed.len();
            let skip_quote = if trimmed.starts_with('"') { 1 } else { 0 };
            let skip_total = skip_whitespace + skip_quote;

            if skip_total < line.len() {
                let line_content = &line[skip_total..];
                let escaped_quotes_count = count_escaped_quotes(line_content);
                query_pos += line_content.len() - escaped_quotes_count;
            }
            first_line = false;
        } else if is_bsl_comment {
            // BSL comment line: skip entirely (don't add to query_pos)
            // These lines are not included in extracted SDBL
        } else {
            query_pos += 1; // newline in query text (always \n regardless of source)
            let skip_whitespace_before = line.len() - trimmed.len();

            let skip_total = if trimmed.starts_with('|') {
                // Line with pipe: skip whitespace before | and the | itself
                skip_whitespace_before + 1
            } else {
                // Line without pipe: shouldn't happen normally
                0
            };

            let line_content = &line[skip_total..];
            let escaped_quotes_count = count_escaped_quotes(line_content);
            query_pos += line_content.len() - escaped_quotes_count;
        }
    }

    // Ensure final position is on char boundary
    ensure_char_boundary(&query_text, query_pos)
}

/// Ensure offset is on a UTF-8 char boundary.
///
/// If offset is not on a char boundary, walks backwards to find the nearest one.
pub(crate) fn ensure_char_boundary(text: &str, offset: usize) -> TextSize {
    if offset <= text.len() && text.is_char_boundary(offset) {
        TextSize::from(offset as u32)
    } else {
        // Walk backwards to find char boundary
        let safe_offset =
            (0..=offset.min(text.len())).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
        TextSize::from(safe_offset as u32)
    }
}
