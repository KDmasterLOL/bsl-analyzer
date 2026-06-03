use syntax::SyntaxNode;
use text_size::TextSize;

use crate::context_detector::is_sdbl_query;
use crate::literal::{extract_query_text, map_offset_to_query};
use crate::SdblQueryInfo;

pub fn detect_sdbl_at_position(root: &SyntaxNode, offset: TextSize) -> Option<SdblQueryInfo> {
    use syntax::SyntaxKind;

    let _span = tracing::debug_span!("detect_sdbl_at_position", ?offset).entered();

    let bsl_text = root.text().to_string();
    let offset_usize: usize = offset.into();

    tracing::info!(
        offset = offset_usize,
        bsl_text_len = bsl_text.len(),
        is_char_boundary = bsl_text.is_char_boundary(offset_usize),
        "BSL file basic info"
    );

    if offset_usize <= bsl_text.len() {
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

    let token = root.token_at_offset(offset).left_biased()?;

    let token_text = token.text();
    let offset_in_token = usize::from(offset - token.text_range().start());

    let token_before_cursor = if offset_in_token <= token_text.len() {
        &token_text[..offset_in_token]
    } else {
        token_text
    };
    let token_after_cursor =
        if offset_in_token < token_text.len() { &token_text[offset_in_token..] } else { "" };

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

    let literal_node = token.parent_ancestors().find(|node| node.kind() == SyntaxKind::LITERAL)?;

    let literal_text = literal_node.text().to_string();

    if !is_sdbl_query(&literal_text) {
        tracing::trace!("literal does not contain SDBL keywords");
        return None;
    }

    let literal_start = literal_node.text_range().start();
    let offset_in_literal = offset - literal_start;

    let lit_offset_usize: usize = offset_in_literal.into();

    let safe_offset = if literal_text.is_char_boundary(lit_offset_usize) {
        lit_offset_usize
    } else {
        (0..lit_offset_usize).rev().find(|&i| literal_text.is_char_boundary(i)).unwrap_or(0)
    };

    let lit_start = (safe_offset.saturating_sub(50)..safe_offset)
        .rev()
        .find(|&i| literal_text.is_char_boundary(i))
        .unwrap_or(0);
    let lit_end = ((safe_offset + 1)..=(safe_offset + 50).min(literal_text.len()))
        .find(|&i| literal_text.is_char_boundary(i))
        .unwrap_or(literal_text.len());

    let lit_before = &literal_text[lit_start..safe_offset];
    let lit_after = &literal_text[safe_offset..lit_end];

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

    let query_text = extract_query_text(&literal_text);

    let offset_in_query = map_offset_to_query(&literal_text, offset_in_literal);

    let offset_q_usize: usize = offset_in_query.into();

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

        let offset = TextSize::from(18);
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

        let offset = TextSize::from(14);
        let info = detect_sdbl_at_position(&root, offset);

        assert!(info.is_none());
    }

    #[test]
    fn test_detect_sdbl_non_query_string() {
        let code = r#"Сообщение = "Это обычная строка, не запрос";"#;
        let root = parse_bsl(code);

        let offset = TextSize::from(20);
        let info = detect_sdbl_at_position(&root, offset);

        assert!(info.is_none(), "Regular string should not be detected as SDBL query");
    }

    #[test]
    fn test_detect_sdbl_offset_calculation() {
        let code = r#"Запрос = "ВЫБРАТЬ * ИЗ Справочник.Валюты";"#;
        let root = parse_bsl(code);

        let offset = TextSize::from(16);

        let info = detect_sdbl_at_position(&root, offset);

        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.offset_in_query, TextSize::from(0));
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

        let offset = TextSize::from(25);
        let info = detect_sdbl_at_position(&root, offset);

        assert!(info.is_some(), "Should detect SDBL in multiline string");
    }

    #[test]
    fn test_detect_sdbl_incomplete_query() {
        let code = r#"Запрос = "ВЫБРАТЬ * ИЗ Справочник.";"#;
        let root = parse_bsl(code);

        let offset = TextSize::from(40);
        let info = detect_sdbl_at_position(&root, offset);

        assert!(info.is_some(), "Should detect even incomplete SDBL queries");
    }
}
