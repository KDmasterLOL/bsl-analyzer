//! SDBL HIR - semantic representation of SDBL queries.
//!
//! This crate provides High-level Intermediate Representation (HIR) for SDBL
//! (Structured Data Base Language) - the query language used in 1C:Enterprise.
//!
//! # Features
//!
//! - **Type inference**: Infer field types from 1C metadata
//! - **Name resolution**: Resolve tables to metadata objects, fields to definitions
//! - **Semantic diagnostics**: Collected during lowering (QueryToMissingMetadata, etc.)
//! - **LSP support**: Foundation for completion, hover, go-to-definition
//!
//! # Architecture
//!
//! ```text
//! SDBL AST (from parser)
//!     ↓
//! lower_sdbl_to_hir() with metadata context
//!     ↓
//! SdblHir (typed, resolved)
//!     ↓
//! Diagnostics + LSP features
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use sdbl_hir::{lower_sdbl_to_hir, SdblHir};
//!
//! let sdbl_hir = lower_sdbl_to_hir(&sdbl_ast, &metadata);
//!
//! // Check for semantic errors
//! for diag in &sdbl_hir.diagnostics {
//!     println!("Error: {:?}", diag);
//! }
//!
//! // Access typed fields
//! for field in &sdbl_hir.select.fields {
//!     println!("Field: {} (type: {:?})", field.alias_or_name(), field.ty);
//! }
//! ```

mod diagnostics;
mod hir;
mod lower;
mod scope;
mod source_map;
mod standard_fields;
mod types;

pub use diagnostics::SdblDiagnostic;
pub use hir::{
    ExprHir, FieldDef, FieldHir, GroupByHir, JoinHir, JoinType, OrderByHir, ResolvedTable, SdblHir,
    SelectHir, TableRef, UnionHir,
};
pub use lower::{lower_sdbl_to_hir, SdblLowerResult};
pub use scope::Scope;
pub use source_map::{SdblSourceMap, TokenCategory, TokenInfo};
pub use types::{MdoRef, SdblType};

use syntax::SyntaxNode;
use text_size::TextSize;

/// Information about SDBL query at a specific position.
///
/// Returned by `detect_sdbl_at_position()` when cursor is inside an SDBL query string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblQueryInfo {
    /// Full query text (including quotes if present)
    pub query_text: String,
    /// Offset within the query string (relative to query start, after opening quote)
    pub offset_in_query: TextSize,
}

/// SDBL completion context - describes what kind of completion is appropriate at cursor position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdblCompletionContext {
    /// Cursor is after FROM/ИЗ keyword - suggest MDO types (Справочник, Catalog, etc.)
    AfterFromKeyword,

    /// Cursor is inside MDO type reference - suggest MDO objects of that type
    /// Example: "Справочник.Вал$0" -> suggest all catalogs starting with "Вал"
    InsideMdoType {
        /// Metadata object type
        mdo_type: bsl_metadata::MdoType,
        /// Prefix already typed (for filtering)
        prefix: String,
    },

    /// Cursor is after MDO object name - suggest nested elements (tabular sections, virtual tables)
    /// Example: "Справочник.Номенклатура.$0" -> suggest tabular sections
    /// Example: "РегистрСведений.МойРегистр.$0" -> suggest virtual tables
    AfterMdoObject {
        /// Metadata object type
        mdo_type: bsl_metadata::MdoType,
        /// Object name (e.g., "Номенклатура")
        object_name: String,
        /// Prefix already typed (for filtering)
        prefix: String,
    },

    /// No specific completion context detected
    None,
}

/// Extract query text from BSL string literal.
///
/// Removes quotes and | prefixes from multiline strings:
/// ```text
/// Input:  "ВЫБРАТЬ\n|    Код\n|ИЗ Справочник"
/// Output: "ВЫБРАТЬ\n    Код\nИЗ Справочник"
/// ```
fn extract_query_text(literal_text: &str) -> String {
    let mut result = String::new();
    let mut first_line = true;

    for line in literal_text.lines() {
        if first_line {
            // First line: skip opening quote
            let line_text = line.trim_start_matches('"');
            result.push_str(line_text);
            first_line = false;
        } else {
            // Continuation lines: skip | prefix
            result.push('\n');
            let line_text = line.trim_start().trim_start_matches('|');
            result.push_str(line_text);
        }
    }

    // Remove closing quote if present
    result.trim_end_matches('"').to_string()
}

/// Map offset from literal text (with quotes/|) to query text offset (without quotes/|).
///
/// # Arguments
///
/// * `literal_text` - Full literal text including quotes and | prefixes
/// * `offset_in_literal` - Offset within the literal text
///
/// # Returns
///
/// Offset within the extracted query text (without quotes/|).
///
/// Note: The returned offset is guaranteed to be on a UTF-8 char boundary.
fn map_offset_to_query(literal_text: &str, offset_in_literal: TextSize) -> TextSize {
    let offset_usize: usize = offset_in_literal.into();

    // First extract the query text to validate char boundaries
    let query_text = extract_query_text(literal_text);

    tracing::info!(
        "map_offset_to_query: offset_in_literal={}, query_text_len={}",
        offset_usize,
        query_text.len()
    );

    let mut literal_pos = 0;
    let mut query_pos = 0;
    let mut first_line = true;
    let mut line_num = 0;

    for line in literal_text.lines() {
        let line_len = line.len();
        line_num += 1;

        tracing::info!(
            "  line {}: literal_pos={}, line_len={}, line_text={:?}",
            line_num,
            literal_pos,
            line_len,
            line
        );

        if literal_pos + line_len >= offset_usize {
            // Cursor is on this line
            let offset_in_line = offset_usize - literal_pos;

            if first_line {
                // First line: skip opening quote (1 char)
                let skip = if line.starts_with('"') { 1 } else { 0 };
                if offset_in_line > skip {
                    query_pos += offset_in_line - skip;
                }
                tracing::info!(
                    "  -> FOUND on first line: offset_in_line={}, skip={}, final query_pos={}",
                    offset_in_line,
                    skip,
                    query_pos
                );
            } else {
                // Continuation line: skip whitespace + | prefix + whitespace after |
                let trimmed = line.trim_start();
                let skip_whitespace_before = line.len() - trimmed.len();

                let (skip_pipe, after_pipe) = if let Some(stripped) = trimmed.strip_prefix('|') {
                    (1, stripped)
                } else {
                    (0, trimmed)
                };

                // Skip whitespace AFTER the pipe
                let content = after_pipe.trim_start();
                let skip_whitespace_after = after_pipe.len() - content.len();

                let skip_total = skip_whitespace_before + skip_pipe + skip_whitespace_after;

                // Add newline before this line's content
                query_pos += 1;

                if offset_in_line > skip_total {
                    query_pos += offset_in_line - skip_total;
                }
                tracing::info!(
                    "  -> FOUND on continuation line: offset_in_line={}, skip_ws_before={}, skip_pipe={}, skip_ws_after={}, skip_total={}, final query_pos={}",
                    offset_in_line,
                    skip_whitespace_before,
                    skip_pipe,
                    skip_whitespace_after,
                    skip_total,
                    query_pos
                );
            }

            // Ensure we're on a char boundary in the extracted query text
            let result = ensure_char_boundary(&query_text, query_pos);
            tracing::info!("  -> after ensure_char_boundary: {:?}", result);
            return result;
        }

        // Move to next line
        literal_pos += line_len + 1; // +1 for newline

        if first_line {
            let skip = if line.starts_with('"') { 1 } else { 0 };
            query_pos += line_len - skip;
            first_line = false;
        } else {
            query_pos += 1; // newline
            let trimmed = line.trim_start();
            let skip_whitespace_before = line.len() - trimmed.len();

            let (skip_pipe, after_pipe) = if let Some(stripped) = trimmed.strip_prefix('|') {
                (1, stripped)
            } else {
                (0, trimmed)
            };

            // Skip whitespace AFTER the pipe
            let content = after_pipe.trim_start();
            let skip_whitespace_after = after_pipe.len() - content.len();

            let skip_total = skip_whitespace_before + skip_pipe + skip_whitespace_after;
            query_pos += line_len - skip_total;
        }
    }

    // Ensure final position is on char boundary
    ensure_char_boundary(&query_text, query_pos)
}

/// Ensure offset is on a UTF-8 char boundary.
///
/// If offset is not on a char boundary, walks backwards to find the nearest one.
fn ensure_char_boundary(text: &str, offset: usize) -> TextSize {
    if offset <= text.len() && text.is_char_boundary(offset) {
        TextSize::from(offset as u32)
    } else {
        // Walk backwards to find char boundary
        let safe_offset =
            (0..=offset.min(text.len())).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
        TextSize::from(safe_offset as u32)
    }
}

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

    // Find token at offset (prefer token to the left of cursor)
    let token = root.token_at_offset(offset).left_biased()?;

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
    let literal_start = literal_node.text_range().start();
    let offset_in_literal = offset - literal_start;

    tracing::info!(
        "detect_sdbl_at_position: offset={:?}, literal_start={:?}, offset_in_literal={:?}, literal_text_len={}",
        offset,
        literal_start,
        offset_in_literal,
        literal_text.len()
    );

    // Extract query text by removing quotes and | prefixes
    let query_text = extract_query_text(&literal_text);

    // Map offset from literal (with quotes/|) to query text (without quotes/|)
    let offset_in_query = map_offset_to_query(&literal_text, offset_in_literal);

    tracing::debug!(
        literal_len = literal_text.len(),
        query_len = query_text.len(),
        offset_in_query = u32::from(offset_in_query),
        "detected SDBL query at position"
    );

    Some(SdblQueryInfo { query_text, offset_in_query })
}

/// Detect completion context within an SDBL query.
///
/// Analyzes the query text and cursor position to determine what kind of
/// completion suggestions are appropriate.
///
/// # Arguments
///
/// * `query_text` - Full SDBL query text
/// * `offset` - Cursor offset within the query (after opening quote)
///
/// # Returns
///
/// `SdblCompletionContext` describing the completion context.
///
/// # Example
///
/// ```ignore
/// use sdbl_hir::{detect_context, SdblCompletionContext};
/// use text_size::TextSize;
///
/// let query = "SELECT * FROM ";
/// let offset = TextSize::from(query.len() as u32);
/// let context = detect_context(query, offset);
///
/// assert!(matches!(context, SdblCompletionContext::AfterFromKeyword));
/// ```
pub fn detect_context(query_text: &str, offset: TextSize) -> SdblCompletionContext {
    let offset_usize: usize = offset.into();

    // Get text before cursor (ensure we're on a char boundary for UTF-8 safety)
    let text_before_cursor = if offset_usize <= query_text.len() {
        // Find the nearest char boundary at or before offset
        let safe_offset = if query_text.is_char_boundary(offset_usize) {
            offset_usize
        } else {
            // Walk backwards to find char boundary
            (0..offset_usize).rev().find(|&i| query_text.is_char_boundary(i)).unwrap_or(0)
        };
        &query_text[..safe_offset]
    } else {
        query_text
    };

    // Safely get last ~100 chars for logging (find char boundary)
    let log_start = text_before_cursor.len().saturating_sub(200); // ~100 chars in UTF-8
    let log_start = (log_start..=text_before_cursor.len())
        .find(|&i| text_before_cursor.is_char_boundary(i))
        .unwrap_or(0);
    let text_before_sample = &text_before_cursor[log_start..];

    tracing::info!(
        query_len = query_text.len(),
        offset = offset_usize,
        text_before_len = text_before_cursor.len(),
        text_before_sample = %text_before_sample,
        "detect_context"
    );

    // Check for "FROM " or "ИЗ " keyword immediately before cursor
    if is_after_from_keyword(text_before_cursor) {
        tracing::info!("detected AfterFromKeyword");
        return SdblCompletionContext::AfterFromKeyword;
    }

    // Check for dot-separated path (handles both 2-part and 3-part paths)
    if let Some(path_parts) = parse_dot_path(text_before_cursor) {
        match path_parts.len() {
            // "Справочник.Вал" -> InsideMdoType
            2 => {
                if let Ok(mdo_type) = path_parts[0].parse::<bsl_metadata::MdoType>() {
                    let prefix = path_parts[1].clone();
                    tracing::info!(?mdo_type, prefix = %prefix, "detected InsideMdoType (2-part path)");
                    return SdblCompletionContext::InsideMdoType { mdo_type, prefix };
                }
            }
            // "Справочник.Номенклатура.Шт" -> AfterMdoObject
            3 => {
                if let Ok(mdo_type) = path_parts[0].parse::<bsl_metadata::MdoType>() {
                    let object_name = path_parts[1].clone();
                    let prefix = path_parts[2].clone();
                    tracing::info!(
                        ?mdo_type,
                        object_name = %object_name,
                        prefix = %prefix,
                        "detected AfterMdoObject (3-part path)"
                    );
                    return SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix };
                }
            }
            _ => {
                tracing::debug!(parts_len = path_parts.len(), "unexpected path parts count");
            }
        }
    }

    tracing::info!("no context detected");
    SdblCompletionContext::None
}

/// Check if cursor is after FROM/ИЗ keyword.
///
/// Looks for pattern: "... FROM " or "... ИЗ " at the end of text.
fn is_after_from_keyword(text_before: &str) -> bool {
    let text_upper = text_before.to_uppercase();

    // Check if text ends with FROM or ИЗ followed by whitespace
    text_upper.trim_end().ends_with("FROM") || text_upper.trim_end().ends_with("ИЗ")
}

/// Parse dot-separated path from text before cursor.
///
/// Extracts the last whitespace-separated word and splits it by dots.
/// Returns the parts as a Vec of Strings.
///
/// # Examples
///
/// ```ignore
/// parse_dot_path("SELECT * FROM Справочник.Номенклатура.Шт")
/// // -> Some(vec!["Справочник", "Номенклатура", "Шт"])
///
/// parse_dot_path("SELECT * FROM Справочник.")
/// // -> Some(vec!["Справочник", ""])
///
/// parse_dot_path("SELECT * FROM NoDotsHere")
/// // -> None (no dots)
/// ```
fn parse_dot_path(text_before: &str) -> Option<Vec<String>> {
    // Get last whitespace-separated word
    let words: Vec<&str> = text_before.split_whitespace().collect();
    let last_word = words.last()?;

    // Check if it contains a dot (otherwise not a path)
    if !last_word.contains('.') {
        return None;
    }

    // Split by dots and collect into Vec<String>
    let parts: Vec<String> = last_word.split('.').map(|s| s.to_string()).collect();

    Some(parts)
}

/// Check if a string contains SDBL keywords.
///
/// This is a heuristic check - we look for common SDBL keywords like
/// ВЫБРАТЬ, SELECT, ИЗ, FROM, etc.
fn is_sdbl_query(text: &str) -> bool {
    let text_upper = text.to_uppercase();

    // Russian keywords
    text_upper.contains("ВЫБРАТЬ")
        || text_upper.contains("ВЫБОР")
        // English keywords
        || text_upper.contains("SELECT")
        // FROM clause (both languages)
        || text_upper.contains("ИЗ ")
        || text_upper.contains("FROM ")
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
    fn test_is_sdbl_query_russian() {
        assert!(is_sdbl_query("ВЫБРАТЬ * ИЗ Справочник.Валюты"));
        assert!(is_sdbl_query("выбрать * из справочник.валюты"));
        assert!(is_sdbl_query("\"ВЫБРАТЬ * ИЗ Справочник.Валюты\""));
    }

    #[test]
    fn test_is_sdbl_query_english() {
        assert!(is_sdbl_query("SELECT * FROM Catalog.Currencies"));
        assert!(is_sdbl_query("select * from catalog.currencies"));
        assert!(is_sdbl_query("\"SELECT * FROM Catalog.Currencies\""));
    }

    #[test]
    fn test_is_sdbl_query_negative() {
        assert!(!is_sdbl_query("Это обычная строка"));
        assert!(!is_sdbl_query("Regular string without keywords"));
        assert!(!is_sdbl_query("123"));
        assert!(!is_sdbl_query(""));
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

    // --- Context detection tests ---

    #[test]
    fn test_detect_context_after_from_russian() {
        let query = "ВЫБРАТЬ * ИЗ ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        assert_eq!(context, SdblCompletionContext::AfterFromKeyword);
    }

    #[test]
    fn test_detect_context_after_from_english() {
        let query = "SELECT * FROM ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        assert_eq!(context, SdblCompletionContext::AfterFromKeyword);
    }

    #[test]
    fn test_detect_context_inside_mdo_type_russian() {
        use bsl_metadata::MdoType;

        let query = "ВЫБРАТЬ * ИЗ Справочник.Вал";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::Catalog);
                assert_eq!(prefix, "Вал");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_inside_mdo_type_english() {
        use bsl_metadata::MdoType;

        let query = "SELECT * FROM Catalog.Curr";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::Catalog);
                assert_eq!(prefix, "Curr");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_inside_mdo_type_document() {
        use bsl_metadata::MdoType;

        let query = "ВЫБРАТЬ * ИЗ Документ.Заказ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::Document);
                assert_eq!(prefix, "Заказ");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_inside_mdo_type_empty_prefix() {
        use bsl_metadata::MdoType;

        let query = "SELECT * FROM Catalog.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::Catalog);
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_none_in_select() {
        let query = "ВЫБРАТЬ * ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        assert_eq!(context, SdblCompletionContext::None);
    }

    #[test]
    fn test_detect_context_none_inside_word() {
        let query = "ВЫБРАТЬ * ИЗ Спр";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        // "Спр" without dot is not a complete MDO type pattern
        assert_eq!(context, SdblCompletionContext::None);
    }

    #[test]
    fn test_detect_context_register() {
        use bsl_metadata::MdoType;

        let query = "SELECT * FROM InformationRegister.Settings";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::InformationRegister);
                assert_eq!(prefix, "Settings");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_completion_after_mdo_type_with_trailing_space() {
        use bsl_metadata::MdoType;

        // Test with trailing space after dot (common when typing)
        let query = "ВЫБРАТЬ * ИЗ РегистрСведений. ";
        let offset = TextSize::from((query.len() - 1) as u32); // Position after dot, before space
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::InformationRegister);
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    // --- AfterMdoObject context tests ---

    #[test]
    fn test_detect_context_after_catalog_object() {
        use bsl_metadata::MdoType;

        let query = "ВЫБРАТЬ * ИЗ Справочник.Номенклатура.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix } => {
                assert_eq!(mdo_type, MdoType::Catalog);
                assert_eq!(object_name, "Номенклатура");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_catalog_object_with_prefix() {
        use bsl_metadata::MdoType;

        let query = "ВЫБРАТЬ * ИЗ Справочник.Номенклатура.Шт";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix } => {
                assert_eq!(mdo_type, MdoType::Catalog);
                assert_eq!(object_name, "Номенклатура");
                assert_eq!(prefix, "Шт");
            }
            _ => panic!("Expected AfterMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_document_object() {
        use bsl_metadata::MdoType;

        let query = "SELECT * FROM Document.SalesOrder.T";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix } => {
                assert_eq!(mdo_type, MdoType::Document);
                assert_eq!(object_name, "SalesOrder");
                assert_eq!(prefix, "T");
            }
            _ => panic!("Expected AfterMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_information_register() {
        use bsl_metadata::MdoType;

        let query = "ВЫБРАТЬ * ИЗ РегистрСведений.МойРегистр.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix } => {
                assert_eq!(mdo_type, MdoType::InformationRegister);
                assert_eq!(object_name, "МойРегистр");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_accumulation_register() {
        use bsl_metadata::MdoType;

        let query = "SELECT * FROM AccumulationRegister.TaskCount.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix } => {
                assert_eq!(mdo_type, MdoType::AccumulationRegister);
                assert_eq!(object_name, "TaskCount");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_parse_dot_path_helper() {
        // 2-part path
        let parts = parse_dot_path("SELECT * FROM Справочник.Вал");
        assert_eq!(parts, Some(vec!["Справочник".to_string(), "Вал".to_string()]));

        // 3-part path
        let parts = parse_dot_path("SELECT * FROM Справочник.Номенклатура.Шт");
        assert_eq!(
            parts,
            Some(vec!["Справочник".to_string(), "Номенклатура".to_string(), "Шт".to_string()])
        );

        // Empty prefix after dot
        let parts = parse_dot_path("SELECT * FROM Catalog.");
        assert_eq!(parts, Some(vec!["Catalog".to_string(), "".to_string()]));

        // No dots - should return None
        let parts = parse_dot_path("SELECT * FROM NoDots");
        assert_eq!(parts, None);
    }
}
