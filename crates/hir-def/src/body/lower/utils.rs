use syntax::{SyntaxKind, SyntaxNode};

pub(crate) fn looks_like_sdbl(s: &str) -> bool {
    if s.len() < 6 {
        return false;
    }

    let first_word = first_significant_word(s);
    let upper_first = first_word.to_uppercase();

    match upper_first.as_str() {
        "DROP" | "УНИЧТОЖИТЬ" => true,
        "SELECT" | "ВЫБРАТЬ" => has_sdbl_keyword(s),
        _ => false,
    }
}

fn has_sdbl_keyword(s: &str) -> bool {
    let upper = s.to_uppercase();

    const KEYWORDS: &[&str] = &[
        "AS",
        "КАК",
        "FROM",
        "ИЗ",
        "WHERE",
        "ГДЕ",
        "JOIN",
        "СОЕДИНЕНИЕ",
        "UNION",
        "ОБЪЕДИНИТЬ",
        "GROUP",
        "СГРУППИРОВАТЬ",
        "ORDER",
        "УПОРЯДОЧИТЬ",
        "HAVING",
        "ИМЕЮЩИЕ",
        "INTO",
        "ПОМЕСТИТЬ",
    ];

    for kw in KEYWORDS {
        if let Some(pos) = upper.find(kw) {
            let before_ok = pos == 0 || is_word_boundary(upper.as_bytes()[pos - 1]);
            let after_pos = pos + kw.len();
            let after_ok =
                after_pos >= upper.len() || is_word_boundary(upper.as_bytes()[after_pos]);
            if before_ok && after_ok {
                return true;
            }
        }
    }
    false
}

fn is_word_boundary(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b',' | b';' | b'(' | b')' | b'.')
}

fn first_significant_word(s: &str) -> &str {
    let mut remaining = s;

    loop {
        remaining = remaining.trim_start();

        if remaining.is_empty() {
            return "";
        }

        if remaining.starts_with("//") {
            match remaining.find('\n') {
                Some(pos) => {
                    remaining = &remaining[pos + 1..];
                    continue;
                }
                None => return "",
            }
        }

        let word_end = remaining
            .find(|c: char| c.is_whitespace() || c == '(' || c == ',' || c == ';')
            .unwrap_or(remaining.len());

        return &remaining[..word_end];
    }
}

pub(crate) fn extract_string_content(node: &SyntaxNode) -> Option<String> {
    let mut result = String::new();
    let mut tokens = node.children_with_tokens().filter_map(|it| it.into_token());

    let first_token = tokens.next()?;

    match first_token.kind() {
        SyntaxKind::STRING => {
            let text = first_token.text();
            if text.len() < 2 {
                return None;
            }
            let inner = &text[1..text.len() - 1];
            result = inner.replace("\"\"", "\"");
        }
        SyntaxKind::STRING_START => {
            let text = first_token.text();
            if text.is_empty() {
                return None;
            }
            result.push_str(&text[1..]);

            for token in tokens {
                match token.kind() {
                    SyntaxKind::NEWLINE => {
                        result.push('\n');
                    }
                    SyntaxKind::STRING_PART => {
                        let text = token.text();
                        if let Some(content) = text.strip_prefix('|') {
                            result.push_str(content);
                        }
                    }
                    SyntaxKind::STRING_TAIL => {
                        let text = token.text();
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

            result = result.replace("\"\"", "\"");
        }
        _ => return None,
    }

    Some(result)
}
