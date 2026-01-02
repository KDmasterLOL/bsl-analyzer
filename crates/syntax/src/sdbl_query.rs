//! SDBL query information extracted from BSL files.
//!
//! This module provides the SdblQueryInfo structure which caches parsed SDBL queries
//! along with their positions in BSL source files. Used by Salsa to avoid re-parsing
//! SDBL queries when running multiple SDBL diagnostics.

use crate::{Parse, SyntaxNode, TextRange};

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

    /// Extracted SDBL query text (with | prefixes removed)
    pub query_text: String,

    /// Parsed SDBL AST (None if parse failed)
    pub query_ast: Option<Parse<SyntaxNode>>,
}

impl SdblQueryInfo {
    /// Create a new SDBL query info.
    ///
    /// ## Arguments
    ///
    /// - `bsl_literal_range`: Position of the LITERAL node in BSL source
    /// - `query_text`: Extracted SDBL query text (multiline | prefixes already removed)
    /// - `query_ast`: Parsed SDBL AST (or None if parsing failed)
    pub fn new(
        bsl_literal_range: TextRange,
        query_text: String,
        query_ast: Option<Parse<SyntaxNode>>,
    ) -> Self {
        Self { bsl_literal_range, query_text, query_ast }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sdbl_query_info_creation() {
        let range = TextRange::new(0u32.into(), 10u32.into());
        let query_text = "SELECT * FROM Table".to_string();

        let info = SdblQueryInfo::new(range, query_text.clone(), None);

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
        );
        assert!(!info.is_valid());
        assert!(info.syntax_node().is_none());
    }

    #[test]
    fn test_clone_and_equality() {
        let query = "SELECT Name AS N FROM Table";

        let info1 =
            SdblQueryInfo::new(TextRange::new(0u32.into(), 10u32.into()), query.to_string(), None);

        let info2 = info1.clone();

        assert_eq!(info1, info2);
        assert_eq!(info1.bsl_literal_range, info2.bsl_literal_range);
        assert_eq!(info1.query_text, info2.query_text);
    }

    // Note: Tests with parsed AST are in base-db tests where parser is available
}
