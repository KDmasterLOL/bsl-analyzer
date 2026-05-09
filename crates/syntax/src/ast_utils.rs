//! Utilities for working with AST nodes.
//!
//! This module provides helper functions for extracting information from syntax trees,
//! particularly for working with comments and method documentation.

use crate::{SyntaxKind, SyntaxNode, SyntaxToken};

/// Extract leading comments before a syntax node from raw source text.
///
/// This function extracts comments that appear immediately before the given node
/// by analyzing the source text. Comments are extracted line by line working backwards
/// from the node's start position.
///
/// # Examples
///
/// ```text
/// // This is comment 1
/// // This is comment 2
/// Функция Example()
/// ```
///
/// Returns: `["This is comment 1", "This is comment 2"]`
///
/// # Arguments
/// * `node` - The syntax node to extract comments for
/// * `source_text` - The complete source text of the file
///
/// # Returns
/// `Some(Vec<String>)` if there are comments before the node, `None` otherwise.
pub fn extract_leading_comments(node: &SyntaxNode, source_text: &str) -> Option<Vec<String>> {
    let node_start: usize = node.text_range().start().into();
    extract_leading_comments_at_offset(node_start, source_text)
}

/// Extract leading comments before a given offset in source text.
///
/// This is the optimized version that doesn't require AST node lookup.
/// Use this when you already have the offset (e.g., from ItemTree.source_range).
///
/// # Arguments
/// * `offset` - Byte offset in source text where the construct starts
/// * `source_text` - The complete source text of the file
///
/// # Returns
/// `Some(Vec<String>)` if there are comments before the offset, `None` otherwise.
pub fn extract_leading_comments_at_offset(offset: usize, source_text: &str) -> Option<Vec<String>> {
    if offset > source_text.len() {
        return None;
    }

    // Find the line where the node starts
    let text_before_node = &source_text[..offset];

    // Split into lines and work backwards
    let lines: Vec<&str> = text_before_node.lines().collect();

    let mut comments = Vec::new();

    // Work backwards from the last line before the node
    for line in lines.iter().rev() {
        let trimmed = line.trim();

        if trimmed.starts_with("//") {
            // Extract comment text (remove "//" prefix and trim)
            let comment_text = trimmed.trim_start_matches("//").trim();
            if !comment_text.is_empty() {
                comments.push(comment_text.to_string());
            }
        } else if trimmed.is_empty() {
            // Empty line - continue searching backwards
            continue;
        } else {
            // Non-comment, non-empty line - stop
            break;
        }
    }

    if comments.is_empty() {
        return None;
    }

    // Reverse to restore top-down order
    comments.reverse();
    Some(comments)
}

/// Check if a variable has a trailing comment on the same line.
///
/// For BSL variable declarations, a trailing comment is a valid description:
/// ```bsl
/// Перем Переменная; // это описание
/// ```
pub fn has_trailing_comment(node: &SyntaxNode, source_text: &str) -> bool {
    let node_range = node.text_range();
    let node_end: usize = node_range.end().into();

    if node_end >= source_text.len() {
        return false;
    }

    let text_after = &source_text[node_end..];
    let mut chars = text_after.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\n' | '\r' => return false,
            '/' => {
                if chars.peek() == Some(&'/') {
                    return true;
                }
                return false;
            }
            ' ' | '\t' => continue,
            _ => return false,
        }
    }
    false
}

/// Check if a variable has a leading description (comment directly above).
///
/// For variables, an empty line between the comment and declaration
/// means the comment is NOT a description.
///
/// # Arguments
/// * `var_keyword_offset` - Byte offset of the VAR/Перем keyword
/// * `source_text` - Complete source text
/// * `first_annotation_offset` - Optional offset of the first annotation (for annotated variables)
///
/// # Returns
/// `true` if there's a comment directly above (without empty lines)
pub fn has_variable_leading_description(
    var_keyword_offset: usize,
    source_text: &str,
    first_annotation_offset: Option<usize>,
) -> bool {
    let check_from = first_annotation_offset.unwrap_or(var_keyword_offset);

    if check_from == 0 || check_from > source_text.len() {
        return false;
    }

    let text_before = &source_text[..check_from];

    let lines: Vec<&str> = text_before.split('\n').collect();
    if lines.is_empty() {
        return false;
    }

    let start_idx = lines.len().saturating_sub(1);
    let first_line = lines[start_idx].trim();
    let check_start = if first_line.is_empty() || first_line.starts_with('&') {
        if start_idx == 0 {
            return false;
        }
        start_idx - 1
    } else {
        start_idx
    };

    for i in (0..=check_start).rev() {
        let line = lines[i].trim();

        if line.starts_with("//") {
            return true;
        }

        if line.is_empty() {
            return false;
        }

        if line.starts_with('&') {
            continue;
        }

        return false;
    }

    false
}

/// Check if a variable has any description (trailing or leading).
///
/// A variable has a description if:
/// 1. It has a trailing comment on the same line: `Перем X; // description`
/// 2. It has a leading comment directly above (no empty lines)
/// 3. For annotated variables, the comment can be:
///    - Above the first annotation
///    - Between annotations
///    - Below annotations (before VAR keyword)
///    - On the same line as VAR
pub fn has_variable_description(
    node: &SyntaxNode,
    var_keyword_offset: usize,
    source_text: &str,
    first_annotation_offset: Option<usize>,
) -> bool {
    if has_trailing_comment(node, source_text) {
        return true;
    }

    if first_annotation_offset.is_some()
        && has_annotation_comments(var_keyword_offset, source_text, first_annotation_offset)
    {
        return true;
    }

    has_variable_leading_description(var_keyword_offset, source_text, first_annotation_offset)
}

/// Gather every comment line attached to a module-level variable declaration.
///
/// Mirrors the three-region scan of [`has_variable_description`] but returns
/// the actual comment text instead of a binary verdict, so a structured doc
/// parser (`hir_def::docs::parse_variable_docs`) can interpret it. Stays
/// pure-text — callers walk the AST once to obtain the offsets.
///
/// # Regions, in source order
///
/// 1. **Leading**: contiguous `// …` block directly above the variable
///    declaration (or above the first annotation, when present). A blank
///    line breaks the connection. `&Annotation` lines are transparent —
///    a comment between or above annotations counts as leading.
/// 2. **Inter-annotation**: any `// …` line in the slice between the
///    first annotation and the var keyword. Captures comments between
///    `&Annotation`s and the `Перем`/`Var` keyword.
/// 3. **Trailing**: a `// …` comment on the same line as the closing `;`
///    (after the variable's full source range).
///
/// Each captured line is stripped of its `//` prefix and trimmed. Empty
/// `//` markers are filtered out — a whitespace-only comment looks like
/// "no docs" to the caller, which is the desired behaviour for the
/// `MissingVariablesDescription` audit gap.
///
/// # Arguments
///
/// * `file_text` — full source text.
/// * `var_keyword_offset` — byte offset of the `Перем`/`Var` keyword
///   token. Required even when annotations are present so the leading
///   scan and the inter-annotation scan use the precise boundary.
/// * `var_end_offset` — byte offset of the variable declaration's `;`
///   (one past the last character of the statement, i.e.
///   `Variable::source_range.end()`).
/// * `first_annotation_offset` — byte offset of the first `&Annotation`
///   if any, else `None`.
///
/// # Returns
///
/// `Some(Vec<String>)` of non-empty trimmed comment lines in source
/// order, or `None` when no description anywhere.
pub fn extract_variable_comments_at_offset(
    file_text: &str,
    var_keyword_offset: usize,
    var_end_offset: usize,
    first_annotation_offset: Option<usize>,
) -> Option<Vec<String>> {
    // Public-API contract: every offset must fall on a UTF-8 char boundary
    // (parser-derived `TextRange`s always do). A misuse from an arbitrary
    // `usize` arithmetic mistake would silently slice through Cyrillic
    // bytes and panic deep inside the helper; surface it at the boundary.
    debug_assert!(
        var_keyword_offset == 0 || file_text.is_char_boundary(var_keyword_offset),
        "var_keyword_offset {var_keyword_offset} not on a char boundary"
    );
    debug_assert!(
        var_end_offset == 0 || file_text.is_char_boundary(var_end_offset),
        "var_end_offset {var_end_offset} not on a char boundary"
    );
    debug_assert!(
        first_annotation_offset.is_none_or(|o| o == 0 || file_text.is_char_boundary(o)),
        "first_annotation_offset {first_annotation_offset:?} not on a char boundary"
    );

    let mut comments: Vec<String> = Vec::new();

    let leading_anchor = first_annotation_offset.unwrap_or(var_keyword_offset);
    if let Some(leading) = collect_variable_leading_comments(file_text, leading_anchor) {
        comments.extend(leading);
    }

    if let Some(first_ann) = first_annotation_offset {
        if first_ann < var_keyword_offset && var_keyword_offset <= file_text.len() {
            let block = &file_text[first_ann..var_keyword_offset];
            for line in block.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("//") {
                    let comment_text = rest.trim();
                    if !comment_text.is_empty() {
                        comments.push(comment_text.to_string());
                    }
                }
            }
        }
    }

    if let Some(trailing) = scan_variable_trailing_comment(file_text, var_end_offset) {
        comments.push(trailing);
    }

    if comments.is_empty() {
        None
    } else {
        Some(comments)
    }
}

fn collect_variable_leading_comments(file_text: &str, anchor: usize) -> Option<Vec<String>> {
    if anchor == 0 || anchor > file_text.len() {
        return None;
    }

    let text_before = &file_text[..anchor];
    let lines: Vec<&str> = text_before.split('\n').collect();
    if lines.is_empty() {
        return None;
    }

    let start_idx = lines.len().saturating_sub(1);
    let first_line = lines[start_idx].trim();
    let check_start = if first_line.is_empty() || first_line.starts_with('&') {
        if start_idx == 0 {
            return None;
        }
        start_idx - 1
    } else {
        start_idx
    };

    let mut comments: Vec<String> = Vec::new();
    for i in (0..=check_start).rev() {
        let line = lines[i].trim();

        if let Some(rest) = line.strip_prefix("//") {
            let comment_text = rest.trim();
            if !comment_text.is_empty() {
                comments.push(comment_text.to_string());
            }
            continue;
        }

        if line.is_empty() {
            break;
        }

        if line.starts_with('&') {
            continue;
        }

        break;
    }

    if comments.is_empty() {
        None
    } else {
        comments.reverse();
        Some(comments)
    }
}

fn scan_variable_trailing_comment(file_text: &str, var_end_offset: usize) -> Option<String> {
    if var_end_offset >= file_text.len() {
        return None;
    }
    let text_after = &file_text[var_end_offset..];
    for (i, ch) in text_after.char_indices() {
        match ch {
            '\n' | '\r' => return None,
            ' ' | '\t' => continue,
            '/' => {
                let after_first = &text_after[i + ch.len_utf8()..];
                if !after_first.starts_with('/') {
                    return None;
                }
                let after_slashes = &after_first['/'.len_utf8()..];
                let line = after_slashes.lines().next().unwrap_or("").trim();
                if line.is_empty() {
                    return None;
                }
                return Some(line.to_string());
            }
            _ => return None,
        }
    }
    None
}

fn has_annotation_comments(
    var_keyword_offset: usize,
    source_text: &str,
    first_annotation_offset: Option<usize>,
) -> bool {
    let first_ann = match first_annotation_offset {
        Some(off) => off,
        None => return false,
    };

    if first_ann >= var_keyword_offset {
        return false;
    }

    let annotation_block = &source_text[first_ann..var_keyword_offset];

    for line in annotation_block.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            return true;
        }
    }

    false
}

/// Return the name token sitting in the field-tail slot of a `FIELD_EXPR`.
///
/// In BSL the parser accepts every keyword after `.` as a legal field
/// name (`is_ident_or_keyword` in `crates/parser/src/grammar/expressions.rs`),
/// so the field-tail token can be `IDENT` or any `is_keyword()` token —
/// classic case is `Запрос.Выполнить()` where `Выполнить` is `KW_EXECUTE`.
///
/// The scan stays at *direct* `children_with_tokens()` and starts only
/// **after** the `DOT`, so it cannot pull a token out of the receiver
/// subtree even under parser error recovery.
///
/// Returns `None` when the input is not a `FIELD_EXPR`, has no `DOT`,
/// or has no name token after the dot.
pub fn field_tail_name_token(field_expr: &SyntaxNode) -> Option<SyntaxToken> {
    if field_expr.kind() != SyntaxKind::FIELD_EXPR {
        return None;
    }
    let mut saw_dot = false;
    field_expr.children_with_tokens().filter_map(|el| el.into_token()).find(|tok| {
        if !saw_dot {
            saw_dot = tok.kind() == SyntaxKind::DOT;
            return false;
        }
        tok.kind().is_name_token()
    })
}

/// Return the type-name token of a `NEW_EXPR` (`Новый Запрос` →
/// `Запрос`).
///
/// Same parser-permissive rule as `field_tail_name_token`: tokens after
/// `KW_NEW` may in principle be any `is_name_token()`, including a
/// keyword (the platform has no current keyword-typed constructors but
/// the parser is permissive, and we keep the rule consistent across
/// every name slot).
///
/// Walks direct children — never `descendants_with_tokens`, which would
/// pull tokens out of constructor-argument subtrees under recovery.
///
/// Returns `None` when the input is not a `NEW_EXPR` or has no name
/// token following `KW_NEW`.
pub fn new_expr_type_name_token(new_expr: &SyntaxNode) -> Option<SyntaxToken> {
    if new_expr.kind() != SyntaxKind::NEW_EXPR {
        return None;
    }
    let mut saw_new = false;
    new_expr.children_with_tokens().filter_map(|el| el.into_token()).find(|tok| {
        if !saw_new {
            saw_new = tok.kind() == SyntaxKind::KW_NEW;
            return false;
        }
        tok.kind().is_name_token()
    })
}

// Tests for extract_leading_comments are in ide-diagnostics
// to avoid circular dependency (syntax <- parser <- syntax). The
// `field_tail_name_token` / `new_expr_type_name_token` helpers also
// need parsed input, so their behavioural tests live in
// `crates/hir/tests/classify_token.rs` next to the classifier they
// power.

#[cfg(test)]
mod variable_comment_extractor_tests {
    use super::extract_variable_comments_at_offset;

    /// Helper: locate the byte offset where the marker substring starts.
    fn off(text: &str, marker: &str) -> usize {
        text.find(marker).unwrap_or_else(|| panic!("marker {marker:?} not found in {text:?}"))
    }

    #[test]
    fn no_comments_returns_none() {
        let text = "Перем X;";
        let var_kw = off(text, "Перем");
        let var_end = text.len();
        assert_eq!(extract_variable_comments_at_offset(text, var_kw, var_end, None), None);
    }

    #[test]
    fn leading_single_line() {
        let text = "// purpose\nПерем X;";
        let var_kw = off(text, "Перем");
        let var_end = text.len();
        let got = extract_variable_comments_at_offset(text, var_kw, var_end, None).unwrap();
        assert_eq!(got, vec!["purpose".to_string()]);
    }

    #[test]
    fn leading_multiline_block() {
        let text = "// first\n// second\nПерем X;";
        let var_kw = off(text, "Перем");
        let var_end = text.len();
        let got = extract_variable_comments_at_offset(text, var_kw, var_end, None).unwrap();
        assert_eq!(got, vec!["first".to_string(), "second".to_string()]);
    }

    #[test]
    fn blank_line_breaks_leading() {
        let text = "// far away\n\nПерем X;";
        let var_kw = off(text, "Перем");
        let var_end = text.len();
        assert_eq!(extract_variable_comments_at_offset(text, var_kw, var_end, None), None);
    }

    #[test]
    fn trailing_only() {
        let text = "Перем X; // trailing";
        let var_kw = off(text, "Перем");
        let var_end = off(text, ";") + 1;
        let got = extract_variable_comments_at_offset(text, var_kw, var_end, None).unwrap();
        assert_eq!(got, vec!["trailing".to_string()]);
    }

    #[test]
    fn empty_trailing_marker_filtered() {
        let text = "Перем X; //";
        let var_kw = off(text, "Перем");
        let var_end = off(text, ";") + 1;
        assert_eq!(extract_variable_comments_at_offset(text, var_kw, var_end, None), None);
    }

    #[test]
    fn empty_leading_marker_filtered() {
        // Closes the audit gap: an isolated whitespace-only `//` does not
        // count as a description, so the caller can detect emptiness.
        let text = "//\nПерем X;";
        let var_kw = off(text, "Перем");
        let var_end = text.len();
        assert_eq!(extract_variable_comments_at_offset(text, var_kw, var_end, None), None);
    }

    #[test]
    fn leading_then_trailing_combined() {
        let text = "// purpose\nПерем X; // remark";
        let var_kw = off(text, "Перем");
        let var_end = off(text, ";") + 1;
        let got = extract_variable_comments_at_offset(text, var_kw, var_end, None).unwrap();
        assert_eq!(got, vec!["purpose".to_string(), "remark".to_string()]);
    }

    #[test]
    fn inter_annotation_capture() {
        let text = "&Идентификатор\n// inter\n&Колонка\nПерем X;";
        let var_kw = off(text, "Перем");
        let first_ann = off(text, "&Идентификатор");
        let var_end = text.len();
        let got =
            extract_variable_comments_at_offset(text, var_kw, var_end, Some(first_ann)).unwrap();
        assert_eq!(got, vec!["inter".to_string()]);
    }

    #[test]
    fn leading_above_first_annotation() {
        let text = "// header\n&Идентификатор\nПерем X;";
        let var_kw = off(text, "Перем");
        let first_ann = off(text, "&Идентификатор");
        let var_end = text.len();
        let got =
            extract_variable_comments_at_offset(text, var_kw, var_end, Some(first_ann)).unwrap();
        assert_eq!(got, vec!["header".to_string()]);
    }

    #[test]
    fn trailing_with_annotations() {
        let text = "&Идентификатор\nПерем X; // tail";
        let var_kw = off(text, "Перем");
        let first_ann = off(text, "&Идентификатор");
        let var_end = off(text, ";") + 1;
        let got =
            extract_variable_comments_at_offset(text, var_kw, var_end, Some(first_ann)).unwrap();
        assert_eq!(got, vec!["tail".to_string()]);
    }

    #[test]
    fn leading_blank_above_annotation_breaks_connection() {
        let text = "// orphan\n\n&Идентификатор\nПерем X;";
        let var_kw = off(text, "Перем");
        let first_ann = off(text, "&Идентификатор");
        let var_end = text.len();
        // Blank line between header comment and the annotation block:
        // header is no longer "leading" by the contiguous-block rule.
        assert_eq!(
            extract_variable_comments_at_offset(text, var_kw, var_end, Some(first_ann)),
            None
        );
    }

    #[test]
    fn crlf_line_endings_are_handled() {
        // Source files in real-world BSL are commonly CRLF; the
        // extractor must not anchor to `\n`-only.
        let text = "// purpose\r\nПерем X;\r\n";
        let var_kw = off(text, "Перем");
        let var_end = off(text, ";") + 1;
        let got = extract_variable_comments_at_offset(text, var_kw, var_end, None).unwrap();
        assert_eq!(got, vec!["purpose".to_string()]);
    }

    #[test]
    fn cyrillic_variable_name_offsets() {
        // Multi-byte UTF-8 in the variable name and prose. All offsets
        // are derived via `find` (char-boundary by construction), so this
        // exercises the slicing paths against multi-byte content.
        let text = "// заголовок\nПерем СчётчикВызовов; // примечание";
        let var_kw = off(text, "Перем");
        let var_end = off(text, ";") + 1;
        let got = extract_variable_comments_at_offset(text, var_kw, var_end, None).unwrap();
        assert_eq!(got, vec!["заголовок".to_string(), "примечание".to_string()]);
    }

    #[test]
    fn all_three_regions_combined() {
        let text = "// header\n&Идентификатор\n// inter\n&Колонка\nПерем X; // tail";
        let var_kw = off(text, "Перем");
        let first_ann = off(text, "&Идентификатор");
        let var_end = off(text, ";") + 1;
        let got =
            extract_variable_comments_at_offset(text, var_kw, var_end, Some(first_ann)).unwrap();
        assert_eq!(got, vec!["header".to_string(), "inter".to_string(), "tail".to_string()]);
    }
}
