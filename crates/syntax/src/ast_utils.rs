//! Utilities for working with AST nodes.
//!
//! This module provides helper functions for extracting information from syntax trees,
//! particularly for working with comments and method documentation.

use crate::SyntaxNode;

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

// Tests for extract_leading_comments are in ide-diagnostics
// to avoid circular dependency (syntax <- parser <- syntax)
