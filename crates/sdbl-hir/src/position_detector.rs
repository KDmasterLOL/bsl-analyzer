use syntax::SyntaxNode;
use text_size::TextSize;

use crate::context_detector::is_sdbl_query;
use crate::literal::{extract_query_text, map_offset_to_query};
use crate::SdblQueryInfo;

/// The nearest char boundary at or before `offset`, capped at the text end.
///
/// A caller can hand over an offset that splits a character: a column the editor
/// measured in units other than bytes, or a position taken against another
/// revision of the text. Snapping once, here, is what keeps every slice below —
/// the token, the literal, the query — inside the same character.
fn snap_to_char_boundary(text: &str, offset: TextSize) -> TextSize {
    let mut offset = usize::from(offset).min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    TextSize::from(offset as u32)
}

pub fn detect_sdbl_at_position(root: &SyntaxNode, offset: TextSize) -> Option<SdblQueryInfo> {
    use syntax::SyntaxKind;

    let _span = tracing::debug_span!("detect_sdbl_at_position", ?offset).entered();

    let bsl_text = root.text().to_string();
    let offset = snap_to_char_boundary(&bsl_text, offset);

    let token = root.token_at_offset(offset).left_biased()?;

    tracing::trace!(
        token_kind = ?token.kind(),
        token_range = ?token.text_range(),
        "token under the cursor"
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

    let offset_in_literal = offset - literal_node.text_range().start();
    let query_text = extract_query_text(&literal_text);
    let offset_in_query = map_offset_to_query(&literal_text, offset_in_literal);

    tracing::debug!(
        literal_len = literal_text.len(),
        query_len = query_text.len(),
        offset_in_query = u32::from(offset_in_query),
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
    fn offset_inside_a_multibyte_char_is_snapped_not_fatal() {
        let code = r#"Запрос = "ВЫБРАТЬ * ИЗ Справочник.Валюты";"#;
        let root = parse_bsl(code);

        // Byte 1 is the tail of 'З' in `Запрос` — outside any literal.
        assert!(detect_sdbl_at_position(&root, TextSize::from(1)).is_none());

        // One byte into 'В' of ВЫБРАТЬ: the same character the cursor would
        // sit on, so the query is still detected at that character's start.
        let quote = code.find('"').unwrap() as u32;
        let info = detect_sdbl_at_position(&root, TextSize::from(quote + 2))
            .expect("the query literal is detected from inside its first character");

        assert!(info.query_text.contains("ВЫБРАТЬ"));
        assert_eq!(info.offset_in_query, TextSize::from(0));
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
