use syntax::SyntaxNode;
use text_size::TextSize;

use crate::context_detector::is_sdbl_query;
use crate::literal::{extract_query_text, map_offset_to_query};
use crate::SdblQueryInfo;

/// Detect if a position is inside an SDBL query string.
///
/// This function checks if the given offset in the syntax tree falls within a string literal
/// that appears to contain an SDBL query (detected by presence of SDBL keywords).
///
/// # Arguments
///
/// * `root` - Root syntax node (typically from `parse.syntax_node()`)
/// * `offset` - Byte offset in the file
///
/// # Returns
///
/// `Some(SdblQueryInfo)` if position is inside an SDBL query, `None` otherwise.
///
/// # Example
///
/// ```ignore
/// use sdbl_hir::detect_sdbl_at_position;
/// use text_size::TextSize;
///
/// let parse = parser::parse("Запрос = \"ВЫБРАТЬ * ИЗ Справочник.Валюты\";");
/// let root = parse.syntax_node();
/// let offset = TextSize::from(15); // Inside the query string
///
/// if let Some(info) = detect_sdbl_at_position(&root, offset) {
///     println!("Query: {}", info.query_text);
///     println!("Offset in query: {}", info.offset_in_query);
/// }
/// ```
pub fn detect_sdbl_at_position(root: &SyntaxNode, offset: TextSize) -> Option<SdblQueryInfo> {
    use syntax::SyntaxKind;

    let _span = tracing::debug_span!("detect_sdbl_at_position", ?offset).entered();

    // DEBUG: Show text around cursor position in BSL file
    let bsl_text = root.text().to_string();
    let offset_usize: usize = offset.into();

    tracing::info!(
        offset = offset_usize,
        bsl_text_len = bsl_text.len(),
        is_char_boundary = bsl_text.is_char_boundary(offset_usize),
        "BSL file basic info"
    );

    if offset_usize <= bsl_text.len() {
        // Find char boundaries for context (UTF-8 safe)
        let context_start = (offset_usize.saturating_sub(50)..=offset_usize)
            .rev()
            .find(|&i| bsl_text.is_char_boundary(i))
            .unwrap_or(0);
        let context_end = (offset_usize..=(offset_usize + 50).min(bsl_text.len()))
            .find(|&i| bsl_text.is_char_boundary(i))
            .unwrap_or(bsl_text.len());

        let text_before = &bsl_text[context_start..offset_usize];
        let text_after = &bsl_text[offset_usize..context_end];
        tracing::info!(
            context_start = context_start,
            context_end = context_end,
            text_before_len = text_before.len(),
            text_after_len = text_after.len(),
            text_before = %text_before,
            text_after = %text_after,
            "BSL file context around cursor"
        );
    } else {
        tracing::warn!(
            offset = offset_usize,
            bsl_text_len = bsl_text.len(),
            "Offset is BEYOND file length!"
        );
    }

    // Find token at offset (prefer token to the left of cursor)
    let token = root.token_at_offset(offset).left_biased()?;

    let token_text = token.text();
    let offset_in_token = usize::from(offset - token.text_range().start());

    // Show token text with cursor position marked
    let token_before_cursor = if offset_in_token <= token_text.len() {
        &token_text[..offset_in_token]
    } else {
        token_text
    };
    let token_after_cursor =
        if offset_in_token < token_text.len() { &token_text[offset_in_token..] } else { "" };

    // DEBUG: Show full token text and bytes
    let token_bytes = token.text().as_bytes();

    tracing::info!(
        token_kind = ?token.kind(),
        token_range = ?token.text_range(),
        token_text_len = token.text().len(),
        offset_in_token = offset_in_token,
        token_text_full = %token.text(),
        token_bytes = ?token_bytes,
        token_before = %token_before_cursor,
        token_after = %token_after_cursor,
        "Found token at offset"
    );

    // Check if it's a string token (including multiline string parts)
    if !matches!(
        token.kind(),
        SyntaxKind::STRING
            | SyntaxKind::STRING_START
            | SyntaxKind::STRING_TAIL
            | SyntaxKind::STRING_PART
    ) {
        tracing::trace!("token is not a string: {:?}", token.kind());
        return None;
    }

    // Find parent LITERAL node (which contains the full multiline string)
    let literal_node = token.parent_ancestors().find(|node| node.kind() == SyntaxKind::LITERAL)?;

    // Get full text of literal (includes all STRING_START + STRING_PART + STRING_TAIL)
    let literal_text = literal_node.text().to_string();

    // Check if literal contains SDBL keywords
    if !is_sdbl_query(&literal_text) {
        tracing::trace!("literal does not contain SDBL keywords");
        return None;
    }

    // Calculate offset within the literal node
    // BUG FIX: offset - literal_start is CORRECT for position in original file!
    // The issue is that literal_text is constructed from literal_node.text() which
    // gives us the ORIGINAL file text (including gaps/newlines between tokens).
    //
    // So offset_in_literal should just be offset - literal_start.
    // The real bug is elsewhere - let's keep original calculation and trace it.

    let literal_start = literal_node.text_range().start();
    let offset_in_literal = offset - literal_start;

    // DEBUG: Show literal text around offset_in_literal
    let lit_offset_usize: usize = offset_in_literal.into();

    // Find nearest char boundary to offset (for safe slicing)
    let safe_offset = if literal_text.is_char_boundary(lit_offset_usize) {
        lit_offset_usize
    } else {
        // Walk backwards to find char boundary
        (0..lit_offset_usize).rev().find(|&i| literal_text.is_char_boundary(i)).unwrap_or(0)
    };

    // Find char boundaries for context window (exclude safe_offset itself)
    let lit_start = (safe_offset.saturating_sub(50)..safe_offset)
        .rev()
        .find(|&i| literal_text.is_char_boundary(i))
        .unwrap_or(0);
    let lit_end = ((safe_offset + 1)..=(safe_offset + 50).min(literal_text.len()))
        .find(|&i| literal_text.is_char_boundary(i))
        .unwrap_or(literal_text.len());

    let lit_before = &literal_text[lit_start..safe_offset];
    let lit_after = &literal_text[safe_offset..lit_end];

    // DEBUG: Show literal bytes around safe_offset (show raw bytes to see newlines)
    let debug_start = safe_offset.saturating_sub(10);
    let debug_end = (safe_offset + 10).min(literal_text.len());
    let literal_bytes = &literal_text.as_bytes()[debug_start..debug_end];

    tracing::info!(
        "detect_sdbl_at_position: offset={:?}, literal_start={:?}, offset_in_literal={:?}, literal_text_len={}, safe_offset={}, lit_start={}, lit_end={}, lit_before={:?}, lit_after={:?}",
        offset,
        literal_start,
        offset_in_literal,
        literal_text.len(),
        safe_offset,
        lit_start,
        lit_end,
        lit_before,
        lit_after
    );

    tracing::info!(
        "literal bytes [{}-{}] around offset {}: {:?}",
        debug_start,
        debug_end,
        safe_offset,
        literal_bytes
    );

    // Extract query text by removing quotes and | prefixes
    let query_text = extract_query_text(&literal_text);

    // Map offset from literal (with quotes/|) to query text (without quotes/|)
    let offset_in_query = map_offset_to_query(&literal_text, offset_in_literal);

    // DEBUG: Show query text around mapped offset
    let offset_q_usize: usize = offset_in_query.into();

    // Find char boundaries for safe slicing
    let q_start = (offset_q_usize.saturating_sub(30)..=offset_q_usize)
        .rev()
        .find(|&i| query_text.is_char_boundary(i))
        .unwrap_or(0);
    let q_end = (offset_q_usize..=(offset_q_usize + 30).min(query_text.len()))
        .find(|&i| query_text.is_char_boundary(i))
        .unwrap_or(query_text.len());

    let query_before =
        if offset_q_usize <= query_text.len() && query_text.is_char_boundary(offset_q_usize) {
            &query_text[q_start..offset_q_usize]
        } else {
            "<not char boundary>"
        };
    let query_after =
        if offset_q_usize < query_text.len() && query_text.is_char_boundary(offset_q_usize) {
            &query_text[offset_q_usize..q_end]
        } else {
            ""
        };

    tracing::info!(
        literal_len = literal_text.len(),
        query_len = query_text.len(),
        offset_in_query = offset_q_usize,
        query_before = %query_before,
        query_after = %query_after,
        "detected SDBL query at position"
    );

    Some(SdblQueryInfo {
        query_text,
        offset_in_query,
        bsl_literal_range: literal_node.text_range(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_bsl(code: &str) -> SyntaxNode {
        parser::parse(code).syntax_node()
    }

    #[test]
    fn test_detect_sdbl_inside_query() {
        let code = r#"Запрос = "ВЫБРАТЬ * ИЗ Справочник.Валюты";"#;
        let root = parse_bsl(code);

        // Position inside "ВЫБРАТЬ" word
        // "Запрос = " = 12 bytes (cyrillic) + 3 bytes (" = ") = 15 bytes
        // Opening quote = 1 byte, so first char inside string is at offset 16
        let offset = TextSize::from(18); // Inside the string, at 'Ы' in ВЫБРАТЬ
        let info = detect_sdbl_at_position(&root, offset);

        assert!(info.is_some());
        let info = info.unwrap();
        assert!(info.query_text.contains("ВЫБРАТЬ"));
        assert!(info.offset_in_query > TextSize::from(0));
    }

    #[test]
    fn test_detect_sdbl_english_query() {
        let code = r#"Query = "SELECT * FROM Catalog.Currencies";"#;
        let root = parse_bsl(code);

        // Position inside "SELECT" word
        let offset = TextSize::from(14);
        let info = detect_sdbl_at_position(&root, offset);

        assert!(info.is_some());
        let info = info.unwrap();
        assert!(info.query_text.contains("SELECT"));
    }

    #[test]
    fn test_detect_sdbl_not_in_string() {
        let code = r#"Переменная = 123;"#;
        let root = parse_bsl(code);

        // Position on number
        let offset = TextSize::from(14);
        let info = detect_sdbl_at_position(&root, offset);

        assert!(info.is_none());
    }

    #[test]
    fn test_detect_sdbl_non_query_string() {
        let code = r#"Сообщение = "Это обычная строка, не запрос";"#;
        let root = parse_bsl(code);

        // Position inside regular string
        let offset = TextSize::from(20);
        let info = detect_sdbl_at_position(&root, offset);

        assert!(info.is_none(), "Regular string should not be detected as SDBL query");
    }

    #[test]
    fn test_detect_sdbl_offset_calculation() {
        let code = r#"Запрос = "ВЫБРАТЬ * ИЗ Справочник.Валюты";"#;
        let root = parse_bsl(code);

        // Position calculation:
        // "Запрос = " = 12 bytes (cyrillic "Запрос") + 3 bytes (" = ") = 15 bytes
        // Opening quote `"` = 1 byte at offset 15
        // String content starts at offset 16
        // "ВЫБРАТЬ" first char 'В' is at offset 16
        let offset = TextSize::from(16); // At 'В' (first char in string content)

        let info = detect_sdbl_at_position(&root, offset);

        assert!(info.is_some());
        let info = info.unwrap();
        // Offset should be relative to start of query content (after opening quote)
        // offset=16, token starts at 15 (opening quote), so offset_in_token=1
        // After skipping opening quote (query_start_offset=1), offset_in_query = 1-1 = 0
        assert_eq!(info.offset_in_query, TextSize::from(0)); // At very start of query content
    }

    #[test]
    fn test_detect_sdbl_multiline_string() {
        let code = r#"
Запрос = "ВЫБРАТЬ
    *
ИЗ
    Справочник.Валюты";
"#;
        let root = parse_bsl(code);

        // Position on second line, inside "*"
        let offset = TextSize::from(25);
        let info = detect_sdbl_at_position(&root, offset);

        assert!(info.is_some(), "Should detect SDBL in multiline string");
    }

    #[test]
    fn test_detect_sdbl_incomplete_query() {
        let code = r#"Запрос = "ВЫБРАТЬ * ИЗ Справочник.";"#;
        let root = parse_bsl(code);

        // Position at end of incomplete query
        let offset = TextSize::from(40);
        let info = detect_sdbl_at_position(&root, offset);

        assert!(info.is_some(), "Should detect even incomplete SDBL queries");
    }
}
