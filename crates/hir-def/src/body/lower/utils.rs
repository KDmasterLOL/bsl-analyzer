//! Utility functions for lowering.
//!
//! This module contains helper functions used across the lowering process,
//! such as string extraction and SDBL detection.

use syntax::{SyntaxKind, SyntaxNode};

/// Check if string looks like SDBL query.
pub(crate) fn looks_like_sdbl(s: &str) -> bool {
    if s.len() < 15 {
        return false;
    }
    let upper = s.to_uppercase();
    upper.contains("SELECT") || upper.contains("ВЫБРАТЬ")
}

/// Extract string content from LITERAL node.
///
/// Handles both simple strings ("text") and multiline strings with | prefixes.
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
