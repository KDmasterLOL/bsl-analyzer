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
    let node_range = node.text_range();
    let node_start: usize = node_range.start().into();

    // Find the line where the node starts
    let text_before_node = &source_text[..node_start];

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

// Tests for extract_leading_comments are in ide-diagnostics
// to avoid circular dependency (syntax <- parser <- syntax)
