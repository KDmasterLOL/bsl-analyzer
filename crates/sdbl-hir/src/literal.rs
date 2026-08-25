use text_size::TextSize;

pub(crate) fn count_escaped_quotes(text: &str) -> usize {
    let mut count = 0;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '"' && chars.peek() == Some(&'"') {
            chars.next();
            count += 1;
        }
    }

    count
}

pub(crate) fn unescape_bsl_quotes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '"' {
            if chars.peek() == Some(&'"') {
                chars.next();
                result.push('"');
            } else {
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
            let trimmed = line.trim_start();
            let line_text = trimmed.trim_start_matches('"');
            tracing::trace!(
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

            if trimmed.starts_with("//") {
                tracing::trace!(
                    "extract_query_text line {}: input={:?} → BSL comment, SKIP",
                    line_num,
                    line
                );
                continue;
            }

            result.push('\n');

            if let Some(line_text) = trimmed.strip_prefix('|') {
                tracing::trace!(
                    "extract_query_text line {}: input={:?} → skip ws + | → output={:?}",
                    line_num,
                    line,
                    line_text
                );
                result.push_str(line_text);
            } else {
                tracing::warn!(
                    "extract_query_text line {}: input={:?} → no pipe and not comment, keep as-is",
                    line_num,
                    line
                );
                result.push_str(line);
            }
        }
    }

    let mut final_result = result.trim_end_matches('"').to_string();

    final_result = unescape_bsl_quotes(&final_result);

    tracing::trace!("extract_query_text: final result length={}", final_result.len());
    final_result
}

pub(crate) fn map_offset_to_query(literal_text: &str, offset_in_literal: TextSize) -> TextSize {
    let offset_usize: usize = offset_in_literal.into();

    let query_text = extract_query_text(literal_text);

    tracing::trace!(
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

        let line_end_in_literal = literal_pos + line_len;
        let newline_len = if line_end_in_literal < literal_text.len() {
            let remaining = &literal_text.as_bytes()[line_end_in_literal..];
            if remaining.starts_with(b"\r\n") {
                2
            } else if remaining.starts_with(b"\n") || remaining.starts_with(b"\r") {
                1
            } else {
                0
            }
        } else {
            0
        };

        tracing::trace!(
            "  line {}: literal_pos={}, line_len={}, newline_len={}, line_text={:?}",
            line_num,
            literal_pos,
            line_len,
            newline_len,
            line
        );

        let trimmed = line.trim_start();
        let is_bsl_comment = !first_line && trimmed.starts_with("//");

        if literal_pos + line_len >= offset_usize {
            let offset_in_line = offset_usize - literal_pos;

            if first_line {
                let skip_whitespace = line.len() - trimmed.len();
                let skip_quote = if trimmed.starts_with('"') { 1 } else { 0 };
                let skip_total = skip_whitespace + skip_quote;

                let mut pos_in_query = offset_in_line.saturating_sub(skip_total);

                if skip_total < line.len() && offset_in_line > skip_total {
                    let line_segment = &line[skip_total..offset_in_line.min(line.len())];
                    let escaped_quotes_count = count_escaped_quotes(line_segment);
                    pos_in_query = pos_in_query.saturating_sub(escaped_quotes_count);
                }

                query_pos += pos_in_query;
                tracing::trace!(
                    "  -> FOUND on first line: offset_in_line={}, skip_ws={}, skip_quote={}, skip_total={}, final query_pos={}",
                    offset_in_line,
                    skip_whitespace,
                    skip_quote,
                    skip_total,
                    query_pos
                );
            } else if is_bsl_comment {
                tracing::trace!(
                    "  -> FOUND on BSL comment line: returning query_pos={} (end of previous content)",
                    query_pos
                );
            } else {
                let skip_whitespace_before = line.len() - trimmed.len();

                let skip_total =
                    if trimmed.starts_with('|') { skip_whitespace_before + 1 } else { 0 };

                query_pos += 1;

                let mut pos_in_query = offset_in_line.saturating_sub(skip_total);

                if skip_total < line.len() && offset_in_line > skip_total {
                    let line_segment = &line[skip_total..offset_in_line.min(line.len())];
                    let escaped_quotes_count = count_escaped_quotes(line_segment);
                    pos_in_query = pos_in_query.saturating_sub(escaped_quotes_count);
                }

                query_pos += pos_in_query;
                tracing::trace!(
                    "  -> FOUND on continuation line: offset_in_line={}, skip_total={}, final query_pos={}",
                    offset_in_line,
                    skip_total,
                    query_pos
                );
            }

            let result = ensure_char_boundary(&query_text, query_pos);
            tracing::trace!("  -> after ensure_char_boundary: {:?}", result);
            return result;
        }

        literal_pos += line_len + newline_len;

        if first_line {
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
        } else {
            query_pos += 1;
            let skip_whitespace_before = line.len() - trimmed.len();

            let skip_total = if trimmed.starts_with('|') { skip_whitespace_before + 1 } else { 0 };

            let line_content = &line[skip_total..];
            let escaped_quotes_count = count_escaped_quotes(line_content);
            query_pos += line_content.len() - escaped_quotes_count;
        }
    }

    ensure_char_boundary(&query_text, query_pos)
}

pub(crate) fn ensure_char_boundary(text: &str, offset: usize) -> TextSize {
    if offset <= text.len() && text.is_char_boundary(offset) {
        TextSize::from(offset as u32)
    } else {
        let safe_offset =
            (0..=offset.min(text.len())).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
        TextSize::from(safe_offset as u32)
    }
}
