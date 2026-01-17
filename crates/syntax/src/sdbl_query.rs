//! SDBL query information extracted from BSL files.
//!
//! This module provides the SdblQueryInfo structure which caches parsed SDBL queries
//! along with their positions in BSL source files. Used by Salsa to avoid re-parsing
//! SDBL queries when running multiple SDBL diagnostics.

use crate::{Parse, SyntaxKind, SyntaxNode, TextRange};

/// Information about a single SDBL query found in BSL file.
///
/// This structure is cached by Salsa to avoid re-parsing SDBL queries
/// when running multiple SDBL diagnostics.
///
/// ## Usage
///
/// ```ignore
/// // In diagnostics:
/// let sdbl_queries = ctx.db.sdbl_queries(ctx.file_id);
/// for query_info in sdbl_queries.iter() {
///     if query_info.is_valid() {
///         // Use query_info.query_ast to analyze SDBL
///         // Use query_info.bsl_literal_range for position mapping
///     }
/// }
/// ```
///
/// ## Performance
///
/// - Eager parsing: SDBL AST is parsed during extraction (not lazy)
/// - Keyword filtering: Only strings containing SELECT/ВЫБРАТЬ are parsed
/// - Parse validation: Only successfully parsed queries are cached
/// - Cached by Salsa with LRU=256
///
/// ## Implementation Notes
///
/// This struct must be `Clone`, `PartialEq`, and `Eq` to work with Salsa.
/// It uses `Option<Parse<SyntaxNode>>` for the query AST because:
/// - Some strings might look like SDBL but fail to parse (rare)
/// - Parse errors are stored in the Parse structure itself
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblQueryInfo {
    /// Text range of the LITERAL node in BSL source
    pub bsl_literal_range: TextRange,

    /// Extracted SDBL query text (with | prefixes removed, "" unescaped to ")
    pub query_text: String,

    /// Parsed SDBL AST (None if parse failed)
    pub query_ast: Option<Parse<SyntaxNode>>,

    /// Quote escape corrections: (sdbl_byte_offset, chars_added_in_bsl)
    /// Tracks ""→" replacements for accurate position mapping.
    /// When mapping SDBL position X to BSL column: bsl_col = sdbl_col + sum(chars for positions < X)
    /// NOTE: This stores CHARACTER count, not byte count, for correct UTF-8 handling.
    pub quote_corrections: Vec<(usize, usize)>,
}

impl SdblQueryInfo {
    /// Create a new SDBL query info.
    ///
    /// ## Arguments
    ///
    /// - `bsl_literal_range`: Position of the LITERAL node in BSL source
    /// - `query_text`: Extracted SDBL query text (multiline | prefixes already removed, "" unescaped)
    /// - `query_ast`: Parsed SDBL AST (or None if parsing failed)
    /// - `quote_corrections`: Quote escape corrections for position mapping
    pub fn new(
        bsl_literal_range: TextRange,
        query_text: String,
        query_ast: Option<Parse<SyntaxNode>>,
        quote_corrections: Vec<(usize, usize)>,
    ) -> Self {
        Self { bsl_literal_range, query_text, query_ast, quote_corrections }
    }

    /// Check if SDBL parse was successful and has no errors.
    ///
    /// Returns `false` if:
    /// - Parse failed (query_ast is None)
    /// - Parse succeeded but has syntax errors
    ///
    /// Diagnostics should skip queries where `is_valid() == false`.
    pub fn is_valid(&self) -> bool {
        self.query_ast.as_ref().map(|p| !p.has_errors()).unwrap_or(false)
    }

    /// Get the SDBL root node if parse was successful.
    ///
    /// This is a convenience method for diagnostics that need to access the AST.
    pub fn syntax_node(&self) -> Option<SyntaxNode> {
        self.query_ast.as_ref().map(|p| p.syntax_node())
    }
}

/// Extract SDBL query text with quote escape tracking.
///
/// Returns (text, quote_corrections) where quote_corrections tracks `""` → `"` replacements.
/// Each correction is (sdbl_offset, bytes_added_in_bsl) - when SDBL has position X,
/// BSL position = X + sum(bytes for corrections before X).
///
/// This is the canonical function for SDBL extraction - single source of truth.
///
/// Handles both simple strings and multiline strings:
/// - Simple: `"text"` → one STRING token
/// - Multiline: `"line1\n|line2"` → STRING_START + NEWLINE + STRING_PART + ... + STRING_TAIL
pub fn extract_sdbl_with_corrections(node: &SyntaxNode) -> Option<(String, Vec<(usize, usize)>)> {
    let mut result = String::new();
    let mut corrections = Vec::new();
    let mut tokens = node.children_with_tokens().filter_map(|it| it.into_token());

    // Check first token to determine string type
    let first_token = tokens.next()?;

    match first_token.kind() {
        SyntaxKind::STRING => {
            // Simple string: "text"
            let text = first_token.text();
            if text.len() < 2 {
                return None;
            }
            // Remove outer quotes and track "" → " replacements
            let inner = &text[1..text.len() - 1];

            let mut pos = 0;
            let mut chars = inner.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '"' && chars.peek() == Some(&'"') {
                    // Found "" escape
                    chars.next(); // skip second "
                    result.push('"');
                    corrections.push((pos, 1)); // at SDBL pos, BSL has 1 extra byte
                    pos += 1;
                } else {
                    result.push(ch);
                    pos += ch.len_utf8();
                }
            }
        }
        SyntaxKind::STRING_START => {
            // Multiline string
            let text = first_token.text();
            if text.is_empty() {
                return None;
            }

            // Process first line (after opening quote)
            let first_line = &text[1..];
            let mut pos = 0;
            let mut chars = first_line.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '"' && chars.peek() == Some(&'"') {
                    chars.next();
                    result.push('"');
                    corrections.push((pos, 1));
                    pos += 1;
                } else {
                    result.push(ch);
                    pos += ch.len_utf8();
                }
            }

            // Process remaining tokens
            for token in tokens {
                match token.kind() {
                    SyntaxKind::NEWLINE => {
                        result.push('\n');
                        pos += 1;
                    }
                    SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL => {
                        let text = token.text();
                        // Remove | prefix (and closing " for TAIL)
                        let content = if let Some(c) = text.strip_prefix('|') {
                            if token.kind() == SyntaxKind::STRING_TAIL {
                                c.strip_suffix('"').unwrap_or(c)
                            } else {
                                c
                            }
                        } else {
                            text
                        };

                        let mut chars = content.chars().peekable();
                        while let Some(ch) = chars.next() {
                            if ch == '"' && chars.peek() == Some(&'"') {
                                chars.next();
                                result.push('"');
                                corrections.push((pos, 1));
                                pos += 1;
                            } else {
                                result.push(ch);
                                pos += ch.len_utf8();
                            }
                        }

                        if token.kind() == SyntaxKind::STRING_TAIL {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => return None,
    }

    Some((result, corrections))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sdbl_query_info_creation() {
        let range = TextRange::new(0u32.into(), 10u32.into());
        let query_text = "SELECT * FROM Table".to_string();

        let info = SdblQueryInfo::new(range, query_text.clone(), None, vec![]);

        assert_eq!(info.bsl_literal_range, range);
        assert_eq!(info.query_text, query_text);
        assert!(!info.is_valid()); // No AST
        assert!(info.syntax_node().is_none()); // No AST
    }

    #[test]
    fn test_is_valid_with_no_ast() {
        let info = SdblQueryInfo::new(
            TextRange::new(0u32.into(), 10u32.into()),
            "INVALID QUERY".to_string(),
            None,
            vec![],
        );
        assert!(!info.is_valid());
        assert!(info.syntax_node().is_none());
    }

    #[test]
    fn test_clone_and_equality() {
        let query = "SELECT Name AS N FROM Table";

        let info1 = SdblQueryInfo::new(
            TextRange::new(0u32.into(), 10u32.into()),
            query.to_string(),
            None,
            vec![],
        );

        let info2 = info1.clone();

        assert_eq!(info1, info2);
        assert_eq!(info1.bsl_literal_range, info2.bsl_literal_range);
        assert_eq!(info1.query_text, info2.query_text);
    }

    // Note: Tests with parsed AST are in base-db tests where parser is available
}
