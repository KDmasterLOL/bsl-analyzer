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
    SdblPackage, SdblQuery, SelectHir, TableRef, UnionHir,
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

    /// General SDBL keywords - suggest SQL keywords (SELECT, WHERE, JOIN, etc.)
    /// Example: "ВЫБРАТЬ * $0" -> suggest FROM, WHERE, etc.
    SdblKeywords {
        /// Prefix already typed (for filtering)
        prefix: String,
    },

    /// Cursor is after table alias - suggest fields from that table
    /// Example: "ВЫБРАТЬ Т.$0" -> suggest fields from table with alias Т
    /// Example: "ВЫБРАТЬ Т.Код$0" -> suggest fields starting with "Код"
    AfterTableAlias {
        /// Table alias (e.g., "Т", "Т1", "Т2")
        alias: String,
        /// Prefix already typed (for filtering)
        prefix: String,
    },

    /// Cursor is after AS/КАК keyword - suggest alias name
    /// Example: "ВЫБРАТЬ Т.Код КАК $0" -> suggest "Код" (field name)
    /// Example: "ИЗ Справочник.Номенклатура КАК $0" -> suggest "Номенклатура" (table name)
    AfterAsKeyword {
        /// Context where AS appears (SELECT field vs FROM/JOIN table)
        context: AsContext,
        /// Suggested alias name (extracted from preceding expression)
        suggestion: Option<String>,
    },

    /// Cursor is at position where JOIN type keyword should appear
    /// Example: "ИЗ Справочник.Валюты КАК Т Л$0" -> suggest "ЛЕВОЕ СОЕДИНЕНИЕ", "LEFT JOIN"
    JoinTypeKeyword {
        /// Prefix already typed (for filtering)
        prefix: String,
    },

    /// Cursor is after ON/ПО keyword - suggest table aliases
    /// Example: "ЛЕВОЕ СОЕДИНЕНИЕ ... ПО $0" -> suggest available aliases (Т, Т1, Т2)
    AfterOnKeyword {
        /// Prefix already typed (for filtering)
        prefix: String,
    },

    /// No specific completion context detected
    None,
}

/// Context for AS/КАК keyword - determines what kind of alias to suggest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsContext {
    /// AS in SELECT clause - suggest field name from expression
    /// Example: "ВЫБРАТЬ Т.Код КАК |" → suggest "Код"
    InSelectField,

    /// AS in FROM clause - suggest table name from MDO reference
    /// Example: "ИЗ Справочник.Номенклатура КАК |" → suggest "Номенклатура"
    InFromClause,

    /// AS in JOIN clause - suggest table name from MDO reference
    /// Example: "ЛЕВОЕ СОЕДИНЕНИЕ Документ.Продажа КАК |" → suggest "Продажа"
    InJoinClause,
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

    // DEBUG: Show text around cursor position in BSL file
    let bsl_text = root.text().to_string();
    let offset_usize: usize = offset.into();

    tracing::info!(
        offset = offset_usize,
        bsl_text_len = bsl_text.len(),
        is_char_boundary = bsl_text.is_char_boundary(offset_usize),
        "BSL file basic info"
    );

    if offset_usize <= bsl_text.len() {
        // Find char boundaries for context (UTF-8 safe)
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

    // Find token at offset (prefer token to the left of cursor)
    let token = root.token_at_offset(offset).left_biased()?;

    let token_text = token.text();
    let offset_in_token = usize::from(offset - token.text_range().start());

    // Show token text with cursor position marked
    let token_before_cursor = if offset_in_token <= token_text.len() {
        &token_text[..offset_in_token]
    } else {
        token_text
    };
    let token_after_cursor =
        if offset_in_token < token_text.len() { &token_text[offset_in_token..] } else { "" };

    tracing::info!(
        token_kind = ?token.kind(),
        token_range = ?token.text_range(),
        token_text_len = token.text().len(),
        offset_in_token = offset_in_token,
        token_before = %token_before_cursor,
        token_after = %token_after_cursor,
        "Found token at offset"
    );

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

    // DEBUG: Show literal text around offset_in_literal
    let lit_offset_usize: usize = offset_in_literal.into();

    // Find char boundaries
    let lit_start = (lit_offset_usize.saturating_sub(50)..=lit_offset_usize)
        .rev()
        .find(|&i| literal_text.is_char_boundary(i))
        .unwrap_or(0);
    let lit_end = (lit_offset_usize..=(lit_offset_usize + 50).min(literal_text.len()))
        .find(|&i| literal_text.is_char_boundary(i))
        .unwrap_or(literal_text.len());

    let lit_before = &literal_text[lit_start..lit_offset_usize];
    let lit_after = &literal_text[lit_offset_usize..lit_end];

    tracing::info!(
        "detect_sdbl_at_position: offset={:?}, literal_start={:?}, offset_in_literal={:?}, literal_text_len={}, lit_before={:?}, lit_after={:?}",
        offset,
        literal_start,
        offset_in_literal,
        literal_text.len(),
        lit_before,
        lit_after
    );

    // Extract query text by removing quotes and | prefixes
    let query_text = extract_query_text(&literal_text);

    // Map offset from literal (with quotes/|) to query text (without quotes/|)
    let offset_in_query = map_offset_to_query(&literal_text, offset_in_literal);

    // DEBUG: Show query text around mapped offset
    let offset_q_usize: usize = offset_in_query.into();
    let q_start = offset_q_usize.saturating_sub(30);
    let q_end = (offset_q_usize + 30).min(query_text.len());
    let query_before = if query_text.is_char_boundary(offset_q_usize) {
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
/// Check if cursor is after AS/КАК keyword and determine context.
///
/// Returns AsContext if the last word before cursor is AS/КАК, along with suggested alias.
///
/// # Algorithm
///
/// 1. Check if last word is AS/КАК
/// 2. Look backwards for context keywords:
///    - ВЫБРАТЬ/SELECT → InSelectField (suggest field name from expression)
///    - ИЗ/FROM → InFromClause (suggest table name)
///    - СОЕДИНЕНИЕ/JOIN → InJoinClause (suggest table name)
///
/// # Examples
///
/// ```ignore
/// is_after_as_keyword("ВЫБРАТЬ Т.Код КАК")
/// // -> Some((InSelectField, Some("Код")))
///
/// is_after_as_keyword("ИЗ Справочник.Номенклатура КАК")
/// // -> Some((InFromClause, Some("Номенклатура")))
/// ```
fn is_after_as_keyword(text_before: &str) -> Option<(AsContext, Option<String>)> {
    let words: Vec<&str> = text_before.split_whitespace().collect();

    if words.len() < 2 {
        return None;
    }

    // Check if last word is AS/КАК
    let last_word_upper = words.last()?.to_uppercase();
    if last_word_upper != "КАК" && last_word_upper != "AS" {
        return None;
    }

    // Look backwards to determine context and extract suggestion
    let text_upper = text_before.to_uppercase();

    // Find the most recent context keyword (working backwards)
    let context = if text_upper.contains("ВЫБРАТЬ") || text_upper.contains("SELECT") {
        // Check if we're in FROM/JOIN or still in SELECT
        let last_from = text_upper.rfind("ИЗ").or(text_upper.rfind("FROM"));
        let last_join = text_upper.rfind("СОЕДИНЕНИЕ").or(text_upper.rfind("JOIN"));
        let last_select = text_upper.rfind("ВЫБРАТЬ").or(text_upper.rfind("SELECT"));

        // If FROM/JOIN appears after SELECT, we're in table context
        let in_table_context = last_from
            .or(last_join)
            .map(|pos| last_select.map(|sel_pos| pos > sel_pos).unwrap_or(true))
            .unwrap_or(false);

        if in_table_context {
            if last_join.is_some() && last_join > last_from {
                AsContext::InJoinClause
            } else {
                AsContext::InFromClause
            }
        } else {
            AsContext::InSelectField
        }
    } else if text_upper.contains("ИЗ") || text_upper.contains("FROM") {
        // In FROM clause (no SELECT found)
        AsContext::InFromClause
    } else {
        // Default to FROM clause
        AsContext::InFromClause
    };

    // Extract suggestion based on context
    let suggestion = match context {
        AsContext::InSelectField => extract_field_name_before_as(text_before),
        AsContext::InFromClause | AsContext::InJoinClause => {
            extract_table_name_before_as(text_before)
        }
    };

    Some((context, suggestion))
}

/// Extract field name from expression before AS keyword.
///
/// Parses expressions like "Т.Код КАК" → "Код" or "Т.ВидНоменклатуры.Наименование КАК" → "Наименование"
fn extract_field_name_before_as(text_before: &str) -> Option<String> {
    let words: Vec<&str> = text_before.split_whitespace().collect();

    if words.len() < 2 {
        return None;
    }

    // Get word before AS/КАК
    let expression = words[words.len() - 2];

    // If it contains dots, take the last part
    if expression.contains('.') {
        let parts: Vec<&str> = expression.split('.').collect();
        return Some(parts.last()?.to_string());
    }

    // Simple identifier
    Some(expression.to_string())
}

/// Extract table name from MDO reference before AS keyword.
///
/// Parses patterns like:
/// - "Справочник.Номенклатура КАК" → "Номенклатура"
/// - "Документ.Продажа.Товары КАК" → "Товары" (tabular section)
fn extract_table_name_before_as(text_before: &str) -> Option<String> {
    let words: Vec<&str> = text_before.split_whitespace().collect();

    if words.len() < 2 {
        return None;
    }

    // Get word before AS/КАК
    let reference = words[words.len() - 2];

    // Parse dot-separated path
    if reference.contains('.') {
        let parts: Vec<&str> = reference.split('.').collect();

        // For "Справочник.Номенклатура", return "Номенклатура"
        // For "Документ.Продажа.Товары", return "Товары"
        return Some(parts.last()?.to_string());
    }

    None
}

/// Check if cursor is after ON/ПО keyword.
///
/// Returns true if:
/// - Text ends with "ПО" or "ON" (e.g., "...JOIN Table AS T ON")
/// - Last word before cursor came after "ПО" or "ON" (e.g., "...ON Т")
fn is_after_on_keyword(text_before: &str) -> bool {
    let words: Vec<&str> = text_before.split_whitespace().collect();

    if words.is_empty() {
        return false;
    }

    // Check if last word is ON/ПО
    let last_word = words.last().unwrap().to_uppercase();
    if last_word == "ПО" || last_word == "ON" {
        return true;
    }

    // Check if second-to-last word is ON/ПО (for "...ON prefix" case)
    if words.len() >= 2 {
        let prev_word = words[words.len() - 2].to_uppercase();
        if prev_word == "ПО" || prev_word == "ON" {
            return true;
        }
    }

    false
}

/// Check if cursor is at a position where JOIN type keyword should appear.
///
/// Detects patterns where user is starting to type a JOIN keyword after a table alias.
/// Examples:
/// - "ИЗ Справочник.Валюты КАК Т Л" (starting "ЛЕВОЕ")
/// - "... КАК Т1 ВН" (starting "ВНУТРЕННЕЕ")
///
/// # Algorithm
///
/// 1. Check if there's a КАК/AS keyword in the text (indicates table alias exists)
/// 2. Last word should NOT be a complete SQL keyword (FROM, WHERE, etc.)
/// 3. Last word should be a short partial word (1-3 chars starting with uppercase)
fn is_join_type_context(text_before: &str) -> Option<String> {
    let text_upper = text_before.to_uppercase();

    // Must have КАК/AS in the text (indicates table alias context)
    if !text_upper.contains("КАК") && !text_upper.contains("AS") {
        return None;
    }

    // Already in a JOIN clause - don't suggest again
    if text_upper.ends_with("СОЕДИНЕНИЕ") || text_upper.ends_with("JOIN") {
        return None;
    }

    // Don't suggest JOIN keywords after ON/ПО (that's for ON clause, not JOIN)
    let text_trimmed = text_upper.trim_end();
    if text_trimmed.ends_with("ПО") || text_trimmed.ends_with("ON") {
        return None;
    }

    // Get last word
    let words: Vec<&str> = text_before.split_whitespace().collect();
    let last_word = words.last()?;

    // If word contains a dot, it's likely an alias field pattern (e.g., "Т."), not a JOIN keyword
    if last_word.contains('.') {
        return None;
    }

    // Check if it's a partial word that could be a JOIN keyword
    // Russian: ЛЕВОЕ (Л), ПРАВОЕ (ПР), ВНУТРЕННЕЕ (ВН), ПОЛНОЕ (ПОЛ)
    // English: LEFT (L), RIGHT (R), INNER (I), FULL (F)
    if !last_word.is_empty() && last_word.len() <= 4 {
        let first_char = last_word.chars().next()?;
        if first_char.is_uppercase() {
            // Check it's not a complete keyword we already handle
            let word_upper = last_word.to_uppercase();
            if !matches!(
                word_upper.as_str(),
                "ИЗ" | "FROM"
                    | "ГДЕ"
                    | "WHERE"
                    | "И"
                    | "AND"
                    | "ИЛИ"
                    | "OR"
                    | "КАК"
                    | "AS"
                    | "ПО"
                    | "ON"
            ) {
                return Some(last_word.to_string());
            }
        }
    }

    None
}

/// Check if a string is an MDO type keyword.
///
/// Returns true for known metadata object type names in both Russian and English.
fn is_mdo_type(s: &str) -> bool {
    let s_upper = s.to_uppercase();
    matches!(
        s_upper.as_str(),
        // Catalogs
        "СПРАВОЧНИК" | "CATALOG" |
        // Documents
        "ДОКУМЕНТ" | "DOCUMENT" |
        // Registers
        "РЕГИСТРСВЕДЕНИЙ" | "INFORMATIONREGISTER" |
        "РЕГИСТРНАКОПЛЕНИЯ" | "ACCUMULATIONREGISTER" |
        "РЕГИСТРБУХГАЛТЕРИИ" | "ACCOUNTINGREGISTER" |
        "РЕГИСТРРАСЧЕТА" | "CALCULATIONREGISTER" |
        // Charts
        "ПЛАНВИДОВХАРАКТЕРИСТИК" | "CHARTOFCHARACTERISTICTYPES" |
        "ПЛАНСЧЕТОВ" | "CHARTOFACCOUNTS" |
        "ПЛАНВИДОВРАСЧЕТА" | "CHARTOFCALCULATIONTYPES" |
        // Business processes
        "БИЗНЕСПРОЦЕСС" | "BUSINESSPROCESS" |
        "ЗАДАЧА" | "TASK" |
        // Other
        "ПЕРЕЧИСЛЕНИЕ" | "ENUM" |
        "ОБРАБОТКА" | "DATAPROCESSOR" |
        "ОТЧЕТ" | "REPORT" |
        "КОНСТАНТА" | "CONSTANT" |
        "ПОСЛЕДОВАТЕЛЬНОСТЬ" | "SEQUENCE" |
        "КРИТЕРИЙОТБОРА" | "FILTERCRITERION" |
        "ПЛАНОБМЕНА" | "EXCHANGEPLAN" |
        "ВНЕШНИЙИСТОЧНИКДАННЫХ" | "EXTERNALDATASOURCE"
    )
}

/// Parse table alias and field name from "alias.field" pattern.
///
/// Distinguishes between table aliases (e.g., "Т", "Т1", "Очередь") and MDO types (e.g., "Справочник").
///
/// # Heuristics
///
/// A word before dot is considered an alias if:
/// - Length is 1-20 characters (reasonable for typical aliases)
/// - Starts with uppercase letter (Т, Т1, Очередь, Items)
/// - NOT an MDO type keyword (Справочник, Документ, etc.)
///
/// # Examples
///
/// ```ignore
/// parse_table_alias_field("ВЫБРАТЬ Т.Код") -> Some(("Т", "Код"))
/// parse_table_alias_field("ВЫБРАТЬ Очередь.") -> Some(("Очередь", ""))
/// parse_table_alias_field("ВЫБРАТЬ Справочник.Валюты") -> None (MDO type, not alias)
/// ```
fn parse_table_alias_field(text_before: &str) -> Option<(String, String)> {
    // Walk backwards from cursor to find "Alias.Field" pattern
    // This correctly handles cases like "Исполнение.," where comma follows the dot

    let chars: Vec<char> = text_before.chars().collect();
    let mut pos = chars.len();

    // Skip trailing punctuation and whitespace (e.g., skip ',' in "Исполнение.,")
    while pos > 0
        && !chars[pos - 1].is_alphanumeric()
        && chars[pos - 1] != '_'
        && chars[pos - 1] != '.'
    {
        pos -= 1;
    }

    // Collect field prefix (identifier chars before cursor)
    let field_start = pos;
    while pos > 0 && (chars[pos - 1].is_alphanumeric() || chars[pos - 1] == '_') {
        pos -= 1;
    }
    let field_prefix: String = chars[pos..field_start].iter().collect();

    // Expect a dot
    if pos == 0 || chars[pos - 1] != '.' {
        return None;
    }
    pos -= 1; // skip dot

    // Collect alias (identifier before dot)
    let alias_end = pos;
    while pos > 0 && (chars[pos - 1].is_alphanumeric() || chars[pos - 1] == '_') {
        pos -= 1;
    }
    let potential_alias: String = chars[pos..alias_end].iter().collect();

    // Check if what's before alias is whitespace or start of text
    // (to ensure we got the complete alias, not middle of a word or MDO path)
    // Reject if:
    // - Previous char is alphanumeric (middle of word)
    // - Previous char is '.' (MDO path like "Справочник.Номенклатура.")
    if pos > 0 && (chars[pos - 1].is_alphanumeric() || chars[pos - 1] == '.') {
        return None;
    }

    // Check heuristics for table alias:
    // 1. Not empty
    // 2. Starts with uppercase (Т, Т1, Т2, Очередь, Items, ДескрипторыДоступаРегистров, etc.)
    // 3. NOT an MDO type keyword (Справочник, Документ, etc.)
    if !potential_alias.is_empty()
        && potential_alias.chars().next()?.is_uppercase()
        && !is_mdo_type(&potential_alias)
    {
        return Some((potential_alias, field_prefix));
    }

    None
}

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

    // Check for table alias field pattern (e.g., "Т.Код" or "Т1.")
    // IMPORTANT: Check this EARLY to avoid misinterpreting as other contexts
    // For example, "ПО Т." should be AfterTableAlias, not AfterOnKeyword
    if let Some((alias, prefix)) = parse_table_alias_field(text_before_cursor) {
        tracing::info!(
            alias = %alias,
            prefix = %prefix,
            "detected AfterTableAlias (alias.field pattern)"
        );
        return SdblCompletionContext::AfterTableAlias { alias, prefix };
    }

    // Check for ON/ПО keyword (suggest table aliases)
    if is_after_on_keyword(text_before_cursor) {
        // Extract word AFTER the ON/ПО keyword (not the keyword itself)
        let words: Vec<&str> = text_before_cursor.split_whitespace().collect();
        let last_word = words.last().unwrap_or(&"");

        // If last word is ON/ПО, prefix is empty
        // Otherwise, prefix is the word after ON/ПО
        let prefix = if last_word.to_uppercase() == "ПО" || last_word.to_uppercase() == "ON" {
            String::new()
        } else {
            last_word.to_string()
        };

        tracing::info!(prefix = %prefix, "detected AfterOnKeyword");
        return SdblCompletionContext::AfterOnKeyword { prefix };
    }

    // Check for AS/КАК keyword (suggest alias name)
    if let Some((context, suggestion)) = is_after_as_keyword(text_before_cursor) {
        tracing::info!(
            ?context,
            suggestion = ?suggestion,
            "detected AfterAsKeyword"
        );
        return SdblCompletionContext::AfterAsKeyword { context, suggestion };
    }

    // Check for JOIN type keyword context (suggest JOIN keywords)
    if let Some(prefix) = is_join_type_context(text_before_cursor) {
        tracing::info!(prefix = %prefix, "detected JoinTypeKeyword context");
        return SdblCompletionContext::JoinTypeKeyword { prefix };
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

    // Default: suggest SDBL keywords
    // Extract last word as prefix for filtering
    let words: Vec<&str> = text_before_cursor.split_whitespace().collect();
    let prefix = words.last().map(|s| s.to_string()).unwrap_or_default();

    tracing::info!(prefix = %prefix, "detected SdblKeywords context");
    SdblCompletionContext::SdblKeywords { prefix }
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
    fn test_detect_context_keywords_in_select() {
        let query = "ВЫБРАТЬ * ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        // Now returns SdblKeywords instead of None
        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "*");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_keywords_inside_word() {
        let query = "ВЫБРАТЬ * ИЗ Спр";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        // "Спр" without dot - suggest keywords
        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "Спр");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
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

    // --- SdblKeywords context tests ---

    #[test]
    fn test_detect_context_sdbl_keywords_simple() {
        let query = "ВЫБРАТЬ * ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "*");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_sdbl_keywords_partial_word() {
        let query = "ГДЕ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "ГДЕ");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_sdbl_keywords_empty() {
        let query = "";
        let offset = TextSize::from(0);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_sdbl_keywords_after_space() {
        let query = "ВЫБРАТЬ * ИЗ Справочник.Валюты ГД";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "ГД");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    // ========== AfterTableAlias tests ==========

    #[test]
    fn test_detect_context_after_table_alias_no_prefix() {
        let query = "ВЫБРАТЬ Т.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_table_alias_with_prefix() {
        let query = "ВЫБРАТЬ Т.Код";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т");
                assert_eq!(prefix, "Код");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_table_alias_multichar() {
        let query = "ВЫБРАТЬ Т1.Наименование";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т1");
                assert_eq!(prefix, "Наименование");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_not_alias_справочник() {
        // "Справочник." should be recognized as InsideMdoType, NOT AfterTableAlias
        let query = "ВЫБРАТЬ * ИЗ Справочник.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Catalog);
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_not_alias_document() {
        // "Документ.Продажа" should be recognized as InsideMdoType, NOT AfterTableAlias
        let query = "ВЫБРАТЬ * ИЗ Документ.Продажа";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Document);
                assert_eq!(prefix, "Продажа");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_alias_in_where_clause() {
        let query = "ВЫБРАТЬ * ИЗ Справочник.Валюты КАК Т ГДЕ Т.Код";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т");
                assert_eq!(prefix, "Код");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_alias_in_join() {
        let query = "ВЫБРАТЬ Т2.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т2");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_long_alias() {
        // Test for longer alias names (e.g., "Очередь", "Items")
        let query = "ВЫБРАТЬ Очередь.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Очередь");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterTableAlias for long alias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_long_alias_with_prefix() {
        let query = "ВЫБРАТЬ Очередь.Поп";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Очередь");
                assert_eq!(prefix, "Поп");
            }
            _ => panic!("Expected AfterTableAlias for long alias with prefix, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_very_long_alias() {
        // Test for very long alias names (no arbitrary length limit)
        // Real-world example: ДескрипторыДоступаРегистров (27 chars)
        let query = "ВЫБРАТЬ ДескрипторыДоступаРегистров.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "ДескрипторыДоступаРегистров");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterTableAlias for very long alias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_extremely_long_alias() {
        // Test for extremely long alias names - no arbitrary limit
        let query =
            "ВЫБРАТЬ ОченьДлинноеНазваниеТаблицыПростоПотомуЧтоМогуНазыватьКакУгодно.Ссылка";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(
                    alias,
                    "ОченьДлинноеНазваниеТаблицыПростоПотомуЧтоМогуНазыватьКакУгодно"
                );
                assert_eq!(prefix, "Ссылка");
            }
            _ => panic!("Expected AfterTableAlias for extremely long alias, got {:?}", context),
        }
    }

    // ========== AfterAsKeyword tests ==========

    #[test]
    fn test_detect_context_after_as_in_select() {
        let query = "ВЫБРАТЬ Т.Код КАК";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InSelectField);
                assert_eq!(suggestion, Some("Код".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_as_in_select_chain() {
        let query = "ВЫБРАТЬ Т.ВидНоменклатуры.Наименование КАК";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InSelectField);
                assert_eq!(suggestion, Some("Наименование".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_as_in_from() {
        let query = "ИЗ Справочник.Номенклатура КАК";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InFromClause);
                assert_eq!(suggestion, Some("Номенклатура".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_as_in_join() {
        let query = "ВЫБРАТЬ * ИЗ Справочник.Валюты ЛЕВОЕ СОЕДИНЕНИЕ Документ.Продажа КАК";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InJoinClause);
                assert_eq!(suggestion, Some("Продажа".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_as_tabular_section() {
        let query = "ИЗ Документ.ЗаказПокупателя.Товары КАК";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InFromClause);
                assert_eq!(suggestion, Some("Товары".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_as_english() {
        let query = "SELECT T.Code AS";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InSelectField);
                assert_eq!(suggestion, Some("Code".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_as_complex_query() {
        // Test that context detection works correctly in complex queries
        let query = "ВЫБРАТЬ Т1.Код ИЗ Справочник.Валюты КАК Т1 ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Номенклатура КАК";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InJoinClause);
                assert_eq!(suggestion, Some("Номенклатура".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    // ========== JoinTypeKeyword tests ==========

    #[test]
    fn test_detect_context_join_type_russian_л() {
        let query = "ИЗ Справочник.Валюты КАК Т Л";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::JoinTypeKeyword { prefix } => {
                assert_eq!(prefix, "Л");
            }
            _ => panic!("Expected JoinTypeKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_join_type_russian_вн() {
        let query = "ИЗ Справочник.Валюты КАК Т ВН";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::JoinTypeKeyword { prefix } => {
                assert_eq!(prefix, "ВН");
            }
            _ => panic!("Expected JoinTypeKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_join_type_english_l() {
        let query = "FROM Catalog.Currencies AS T L";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::JoinTypeKeyword { prefix } => {
                assert_eq!(prefix, "L");
            }
            _ => panic!("Expected JoinTypeKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_not_join_type_without_alias() {
        // Should not detect JOIN context if no КАК/AS present
        let query = "ИЗ Справочник.Валюты Л";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        // Should be keywords, not JoinTypeKeyword
        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "Л");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    // ========== AfterOnKeyword tests ==========

    #[test]
    fn test_detect_context_after_on_russian() {
        let query = "ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Номенклатура КАК Т2 ПО";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterOnKeyword { prefix } => {
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterOnKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_on_with_prefix() {
        let query = "ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Номенклатура КАК Т2 ПО Т";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterOnKeyword { prefix } => {
                assert_eq!(prefix, "Т");
            }
            _ => panic!("Expected AfterOnKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_on_english() {
        let query = "LEFT JOIN Catalog.Items AS T2 ON";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterOnKeyword { prefix } => {
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterOnKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_on_then_alias_field() {
        // After typing "ПО Т.", should detect AfterTableAlias (not AfterOnKeyword)
        let query = "ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Номенклатура КАК Т2 ПО Т.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }
}
