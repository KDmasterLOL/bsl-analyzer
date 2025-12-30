//! AssignAliasFieldsInQuery diagnostic.
//!
//! Checks that all fields in SDBL subqueries have explicit aliases with AS/КАК keyword.
//!
//! ## Why?
//! Subqueries are often used in FROM clauses. Explicit aliases make queries more readable
//! and maintainable. Without AS keyword, it's unclear whether the identifier is an alias
//! or part of the field expression.
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT * FROM (SELECT Ref FROM Catalog.Products) AS Sub";
//!         // Error: 'Ref' should be 'Ref AS Ref' (missing AS keyword)
//!
//! Query = "SELECT * FROM (SELECT Name, Code FROM Catalog.Products) AS Sub";
//!         // Error: both 'Name' and 'Code' need explicit AS keyword
//! ```
//!
//! ## Good practice
//! ```bsl
//! Query = "SELECT * FROM (SELECT Ref AS Ref FROM Catalog.Products) AS Sub";
//!
//! Query = "SELECT * FROM (SELECT * FROM Table) AS Sub"; // OK: asterisk doesn't need alias
//!
//! Query = "SELECT Name FROM Catalog.Products"; // OK: main query not checked
//! ```
//!
//! ## Rules
//! - Only subqueries are checked (not main queries)
//! - Asterisk fields (`*`, `Table.*`) don't require aliases
//! - AS/КАК keyword must be explicit (implicit aliases are forbidden)
//! - UNION: only first query in UNION is checked
//!
//! ## Implementation
//!
//! Ported from:
//! - AssignAliasFieldsInQueryDiagnostic.java (bsl-language-server)
//! - assign_alias_fields_in_query.rs (bsl-language-server-rust)
//!
//! Adapted to use full SDBL parser with AST instead of token-based approach.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use parser::parse_sdbl;
use syntax::{
    ast::{AstNode, SdblAlias, SdblQuery, SdblQueryPackage, SdblSelectQuery, SdblSelectedField},
    SyntaxKind, SyntaxNode, TextRange,
};

/// Maps SDBL positions back to BSL source positions.
///
/// Handles multiline strings with `|` prefixes. When SDBL is extracted from BSL strings,
/// the `|` prefixes and quotes are removed, so diagnostic positions in SDBL don't correspond
/// to the original BSL source positions. This mapper tracks the BSL literal position and
/// converts SDBL TextRange to BSL TextRange.
///
/// ## Algorithm
///
/// Based on reference implementation from bsl-language-server-rust:
/// - Line mapping: `bsl_line = bsl_literal_line + sdbl_line`
/// - Column mapping:
///   - First line: `bsl_col = bsl_literal_col + sdbl_col + 1` (+1 for opening quote)
///   - Multiline: `bsl_col = sdbl_col` (already aligned after `|` removal)
#[derive(Debug, Clone)]
struct SdblPositionMapper {
    /// Position of the string literal (LITERAL node) in BSL source
    bsl_literal_range: TextRange,

    /// Original BSL file content (for line/column calculations)
    bsl_source: String,
}

impl SdblPositionMapper {
    fn new(bsl_literal_node: &SyntaxNode, bsl_source: &str) -> Self {
        Self {
            bsl_literal_range: bsl_literal_node.text_range(),
            bsl_source: bsl_source.to_string(),
        }
    }

    /// Map SDBL TextRange to BSL TextRange.
    ///
    /// Takes a range within the extracted SDBL text and returns the corresponding
    /// range in the original BSL source file.
    fn map_range(&self, sdbl_range: TextRange, sdbl_text: &str) -> TextRange {
        // 1. Convert SDBL byte offsets to line:column
        let (sdbl_start_line, sdbl_start_col) =
            byte_offset_to_line_col(sdbl_text, u32::from(sdbl_range.start()));
        let (sdbl_end_line, sdbl_end_col) =
            byte_offset_to_line_col(sdbl_text, u32::from(sdbl_range.end()));

        // 2. Get BSL literal starting position
        let (bsl_literal_line, bsl_literal_col) =
            byte_offset_to_line_col(&self.bsl_source, u32::from(self.bsl_literal_range.start()));

        // 3. Map SDBL → BSL accounting for removed | prefix
        let bsl_start_line = bsl_literal_line + sdbl_start_line;
        let bsl_start_col = if sdbl_start_line == 0 {
            // First line of SDBL (same line as opening quote in BSL)
            bsl_literal_col + sdbl_start_col + 1 // +1 for opening quote
        } else {
            // Multiline: find where | is in BSL line
            let bsl_line_text = self.bsl_source.lines().nth(bsl_start_line as usize).unwrap_or("");
            if let Some(pipe_pos) = bsl_line_text.find('|') {
                // Count whitespace after | that was kept in SDBL
                let after_pipe = &bsl_line_text[pipe_pos + 1..];
                let whitespace_count =
                    after_pipe.chars().take_while(|c| c.is_whitespace() && *c != '\n').count();
                let content_start_col = (pipe_pos as u32) + 1 + (whitespace_count as u32);
                // content_start_col points to first non-whitespace in BSL
                // sdbl_start_col includes leading whitespace, so we need to subtract it
                content_start_col + sdbl_start_col - (whitespace_count as u32)
            } else {
                sdbl_start_col // Fallback if no | found
            }
        };

        // Same mapping for end position
        let bsl_end_line = bsl_literal_line + sdbl_end_line;
        let bsl_end_col = if sdbl_end_line == 0 {
            bsl_literal_col + sdbl_end_col + 1
        } else {
            let bsl_line_text = self.bsl_source.lines().nth(bsl_end_line as usize).unwrap_or("");
            if let Some(pipe_pos) = bsl_line_text.find('|') {
                let after_pipe = &bsl_line_text[pipe_pos + 1..];
                let whitespace_count =
                    after_pipe.chars().take_while(|c| c.is_whitespace() && *c != '\n').count();
                let content_start_col = (pipe_pos as u32) + 1 + (whitespace_count as u32);
                content_start_col + sdbl_end_col - (whitespace_count as u32)
            } else {
                sdbl_end_col
            }
        };

        // 4. Convert back to TextRange (byte offsets in BSL)
        let bsl_start_offset =
            line_col_to_byte_offset(&self.bsl_source, bsl_start_line, bsl_start_col);
        let bsl_end_offset = line_col_to_byte_offset(&self.bsl_source, bsl_end_line, bsl_end_col);

        TextRange::new(bsl_start_offset.into(), bsl_end_offset.into())
    }
}

/// Convert byte offset to (line, column) position - 0-indexed.
///
/// Iterates through the text counting newlines and character positions.
fn byte_offset_to_line_col(text: &str, offset: u32) -> (u32, u32) {
    let mut line = 0;
    let mut col = 0;

    for (idx, ch) in text.char_indices() {
        if idx as u32 >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    (line, col)
}

/// Convert (line, column) position to byte offset - 0-indexed.
///
/// Iterates through the text to find the byte offset at the given line and column.
fn line_col_to_byte_offset(text: &str, target_line: u32, target_col: u32) -> u32 {
    let mut line = 0;
    let mut col = 0;

    for (idx, ch) in text.char_indices() {
        if line == target_line && col == target_col {
            return idx as u32;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    // If we reach here, we're at EOF - return length
    text.len() as u32
}

/// Runs the AssignAliasFieldsInQuery diagnostic.
///
/// Walks the BSL AST to find string literals containing SDBL queries,
/// extracts the SDBL, parses it, and checks for fields without AS keyword.
/// Uses position mapping to report diagnostics at correct BSL source positions.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Check if diagnostic is disabled
    if ctx.config.is_disabled(DiagnosticCode::AssignAliasFieldsInQuery) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    // Get BSL source text for position mapping
    let input = ctx.db.file_text_input(ctx.file_id);
    let bsl_source = input.text(ctx.db);

    let mut diagnostics = Vec::new();

    // Walk the tree looking for string literals that might contain SDBL
    for node in root.descendants() {
        if node.kind() == SyntaxKind::LITERAL {
            // Skip if this string literal is part of string concatenation
            // (indicated by "+" or PLUS token after the string)
            if has_string_concatenation(&node) {
                continue;
            }

            // Try to extract and parse as SDBL
            if let Some(query_text) = extract_string_content(&node) {
                // Only check if it looks like SDBL (contains SELECT/ВЫБРАТЬ keyword)
                let uppercase = query_text.to_uppercase();
                if uppercase.contains("SELECT") || uppercase.contains("ВЫБРАТЬ") {
                    tracing::debug!(
                        "Found SDBL query string: {} chars, starts with: {:?}",
                        query_text.len(),
                        &query_text.chars().take(50).collect::<String>()
                    );

                    // Create position mapper for this string literal
                    let mapper = SdblPositionMapper::new(&node, &bsl_source);

                    // Check SDBL query with position mapping
                    check_sdbl_query_with_mapper(&query_text, &mapper, &mut diagnostics);
                }
            }
        }
    }

    diagnostics
}

/// Check if a LITERAL node is part of string concatenation.
///
/// Detects patterns like: "text" + variable or "text" + "more text"
/// These are skipped because extraction would be incomplete.
fn has_string_concatenation(node: &SyntaxNode) -> bool {
    // Check if there's a PLUS token after this literal
    if let Some(next) = node.next_sibling_or_token() {
        if let Some(token) = next.as_token() {
            if token.kind() == SyntaxKind::PLUS {
                return true;
            }
        }
    }

    // Check if there's a PLUS token before this literal
    if let Some(prev) = node.prev_sibling_or_token() {
        if let Some(token) = prev.as_token() {
            if token.kind() == SyntaxKind::PLUS {
                return true;
            }
        }
    }

    false
}

/// Extract string content from a LITERAL node containing STRING tokens.
///
/// Handles both simple strings and multiline strings:
/// - Simple: "text" → one STRING token
/// - Multiline: "line1\n|line2" → STRING_START + NEWLINE + STRING_PART + ... + STRING_TAIL
fn extract_string_content(node: &SyntaxNode) -> Option<String> {
    let mut result = String::new();
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
            // Remove outer quotes
            let inner = &text[1..text.len() - 1];
            // Unescape quotes (BSL uses "" for escaped ")
            result = inner.replace("\"\"", "\"");
        }
        SyntaxKind::STRING_START => {
            // Multiline string: "line1\n|line2\n|line3"
            // STRING_START contains: "line1
            let text = first_token.text();
            if text.is_empty() {
                return None;
            }
            // Remove opening quote
            result.push_str(&text[1..]);

            // Process remaining tokens
            for token in tokens {
                match token.kind() {
                    SyntaxKind::NEWLINE => {
                        result.push('\n');
                    }
                    SyntaxKind::STRING_PART => {
                        // STRING_PART contains: |line (with | prefix)
                        let text = token.text();
                        // Remove | prefix
                        if let Some(content) = text.strip_prefix('|') {
                            result.push_str(content);
                        }
                    }
                    SyntaxKind::STRING_TAIL => {
                        // STRING_TAIL contains: |line" (with | prefix and closing quote)
                        let text = token.text();
                        // Remove | prefix and closing quote
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

            // Unescape quotes
            result = result.replace("\"\"", "\"");
        }
        _ => return None,
    }

    Some(result)
}

/// Check a single SDBL query for fields without AS keyword.
///
/// This version uses position mapping to convert SDBL positions to BSL positions.
fn check_sdbl_query_with_mapper(
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Try to parse as SDBL
    let parse = parse_sdbl(query_text);

    // If parse has errors, skip (might not be SDBL)
    if parse.has_errors() {
        return;
    }

    let root = parse.syntax_node();

    // CRITICAL: The SDBL parser strips whitespace from the parse tree!
    // TextRange values from the parser are relative to the STRIPPED text, not the original query_text.
    // We MUST use the stripped text for position calculations.
    let sdbl_stripped_text = root.text().to_string();

    // Get query package
    let Some(package) = SdblQueryPackage::cast(root) else {
        return;
    };

    // Check each SELECT query
    for select_query in package.queries() {
        check_select_query_with_mapper(&select_query, &sdbl_stripped_text, mapper, diagnostics);
    }
}

/// Check a SELECT query for fields without AS keyword (with position mapping).
fn check_select_query_with_mapper(
    select_query: &SdblSelectQuery,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(subquery) = select_query.subquery() else {
        return;
    };

    // Check main query fields (parent is subquery ✓)
    if let Some(main_query) = subquery.main_query() {
        check_query_fields_and_subqueries_with_mapper(&main_query, query_text, mapper, diagnostics);
    }

    // Note: UNION queries are NOT checked (parent is union node, not subquery)
}

/// Check query fields AND recursively check subqueries in FROM clause (with mapper).
fn check_query_fields_and_subqueries_with_mapper(
    query: &SdblQuery,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Check fields in this query
    check_query_fields_with_mapper(query, query_text, mapper, diagnostics);

    // Recursively check subqueries in FROM clause
    if let Some(from_clause) = query.from_clause() {
        check_from_clause_for_subqueries_with_mapper(&from_clause, query_text, mapper, diagnostics);
    }
}

/// Check fields in a query for missing AS keyword (with mapper).
fn check_query_fields_with_mapper(
    query: &SdblQuery,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(field_list) = query.field_list() else {
        return;
    };

    for field in field_list.fields() {
        check_field_with_mapper(&field, query_text, mapper, diagnostics);
    }
}

/// Recursively check subqueries in FROM clause (with mapper).
fn check_from_clause_for_subqueries_with_mapper(
    from_clause: &syntax::ast::SdblFromClause,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use syntax::ast::{AstNode, SdblSubquery};
    use syntax::SyntaxKind;

    // Walk descendants looking for SDBL_SUBQUERY nodes
    // These are subqueries in FROM clause like: FROM (SELECT ... ) AS Sub
    for node in from_clause.syntax().descendants() {
        if node.kind() == SyntaxKind::SDBL_SUBQUERY {
            if let Some(subquery) = SdblSubquery::cast(node) {
                // Check the main query in this subquery (parent is subquery ✓)
                if let Some(main_query) = subquery.main_query() {
                    check_query_fields_and_subqueries_with_mapper(
                        &main_query,
                        query_text,
                        mapper,
                        diagnostics,
                    );
                }
            }
        }
    }
}

/// Check a single field for missing AS keyword (with mapper).
fn check_field_with_mapper(
    field: &SdblSelectedField,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Skip asterisk fields (they don't need aliases)
    if field.is_asterisk() {
        return;
    }

    // Check if field has alias
    if let Some(alias) = field.alias() {
        // Alias exists, check if it has AS keyword
        if !alias.has_as_keyword() {
            // ERROR: Alias without AS keyword (implicit alias)
            // Report diagnostic on the whole field expression (not just alias)
            add_diagnostic_for_alias_with_mapper(field, &alias, query_text, mapper, diagnostics);
        }
    } else {
        // ERROR: Field without alias at all
        add_diagnostic_for_field_with_mapper(field, query_text, mapper, diagnostics);
    }
}

/// Add diagnostic for alias without AS keyword (with position mapping).
/// Reports the diagnostic on the whole field expression (column ref + alias).
fn add_diagnostic_for_alias_with_mapper(
    field: &SdblSelectedField,
    alias: &SdblAlias,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let alias_name = alias.name().unwrap_or_else(|| "<unknown>".to_string());

    // Use the whole field's range (expression + alias)
    // Get first token from expression and last token from alias
    let sdbl_range = match (field.expression(), alias.identifier()) {
        (Some(expr), Some(alias_ident)) => {
            // Get first non-whitespace token from expression
            let first_token = expr.first_token();
            let first_non_ws = first_token.and_then(|t| {
                if t.kind() == SyntaxKind::WHITESPACE {
                    t.next_token()
                } else {
                    Some(t)
                }
            });

            // Use alias identifier as last token (it's already trimmed)
            if let Some(first) = first_non_ws {
                TextRange::new(first.text_range().start(), alias_ident.text_range().end())
            } else {
                field.syntax().text_range()
            }
        }
        _ => field.syntax().text_range(),
    };
    let bsl_range = mapper.map_range(sdbl_range, query_text);

    diagnostics.push(Diagnostic {
        code: DiagnosticCode::AssignAliasFieldsInQuery,
        message: format!(
            "Поле '{}' должно иметь явный псевдоним с ключевым словом AS/КАК",
            alias_name
        ),
        severity: Severity::Warning,
        range: bsl_range, // Now BSL-relative!
        tags: vec![],
        fixes: vec![],
    });
}

/// Add diagnostic for field without alias (with position mapping).
fn add_diagnostic_for_field_with_mapper(
    field: &SdblSelectedField,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Get SDBL range - trim leading/trailing whitespace from expression
    let sdbl_range = if let Some(expr) = field.expression() {
        let full_range = expr.text_range();

        // Get first and last non-whitespace tokens to exclude leading/trailing whitespace
        let first_token = expr.first_token();
        let last_token = expr.last_token();

        match (first_token, last_token) {
            (Some(_first), Some(_last)) => {
                // Skip leading whitespace tokens
                let first_non_ws = expr.first_token().and_then(|t| {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        t.next_token()
                    } else {
                        Some(t)
                    }
                });

                // Skip trailing whitespace/comment tokens
                let last_non_trivia = expr.last_token().and_then(|t| {
                    let mut token = t;
                    while matches!(
                        token.kind(),
                        SyntaxKind::WHITESPACE | SyntaxKind::COMMENT | SyntaxKind::NEWLINE
                    ) {
                        token = token.prev_token()?;
                    }
                    Some(token)
                });

                match (first_non_ws, last_non_trivia) {
                    (Some(first_real), Some(last_real)) => TextRange::new(
                        first_real.text_range().start(),
                        last_real.text_range().end(),
                    ),
                    _ => full_range,
                }
            }
            _ => full_range,
        }
    } else {
        field.syntax().text_range()
    };
    let bsl_range = mapper.map_range(sdbl_range, query_text);

    diagnostics.push(Diagnostic {
        code: DiagnosticCode::AssignAliasFieldsInQuery,
        message: "Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК".to_string(),
        severity: Severity::Warning,
        range: bsl_range, // Now BSL-relative!
        tags: vec![],
        fixes: vec![],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        test_utils::{assert_diagnostic_range, range_to_line_col},
        DiagnosticsConfig,
    };
    use std::sync::Arc;

    /// Check a single SDBL query for fields without AS keyword (test version).
    ///
    /// This version doesn't use position mapping and returns SDBL-relative positions.
    /// Used by standalone SDBL tests.
    fn check_sdbl_query(query_text: &str, diagnostics: &mut Vec<Diagnostic>) {
        use syntax::ast::{AstNode, SdblSubquery};
        use syntax::SyntaxKind;

        // Try to parse as SDBL
        let parse = parse_sdbl(query_text);

        // If parse has errors, skip (might not be SDBL)
        if parse.has_errors() {
            return;
        }

        let root = parse.syntax_node();

        // Get query package
        let Some(package) = SdblQueryPackage::cast(root) else {
            return;
        };

        // Check each SELECT query
        for select_query in package.queries() {
            let Some(subquery) = select_query.subquery() else {
                continue;
            };

            // Check main query fields
            if let Some(main_query) = subquery.main_query() {
                // Check fields in this query
                if let Some(field_list) = main_query.field_list() {
                    for field in field_list.fields() {
                        // Skip asterisk fields
                        if field.is_asterisk() {
                            continue;
                        }

                        // Check if field has alias
                        if let Some(alias) = field.alias() {
                            if !alias.has_as_keyword() {
                                // ERROR: Alias without AS keyword
                                let alias_name =
                                    alias.name().unwrap_or_else(|| "<unknown>".to_string());
                                diagnostics.push(Diagnostic {
                                    code: DiagnosticCode::AssignAliasFieldsInQuery,
                                    message: format!(
                                        "Поле '{}' должно иметь явный псевдоним с ключевым словом AS/КАК",
                                        alias_name
                                    ),
                                    severity: Severity::Warning,
                                    range: alias.syntax().text_range(),
                                    tags: vec![],
                                    fixes: vec![],
                                });
                            }
                        } else {
                            // ERROR: Field without alias
                            diagnostics.push(Diagnostic {
                                code: DiagnosticCode::AssignAliasFieldsInQuery,
                                message: "Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК".to_string(),
                                severity: Severity::Warning,
                                range: field.syntax().text_range(),
                                tags: vec![],
                                fixes: vec![],
                            });
                        }
                    }
                }

                // Check subqueries in FROM clause
                if let Some(from_clause) = main_query.from_clause() {
                    for node in from_clause.syntax().descendants() {
                        if node.kind() == SyntaxKind::SDBL_SUBQUERY {
                            if let Some(sub) = SdblSubquery::cast(node) {
                                if let Some(sub_main_query) = sub.main_query() {
                                    if let Some(field_list) = sub_main_query.field_list() {
                                        for field in field_list.fields() {
                                            if field.is_asterisk() {
                                                continue;
                                            }
                                            if let Some(alias) = field.alias() {
                                                if !alias.has_as_keyword() {
                                                    let alias_name = alias
                                                        .name()
                                                        .unwrap_or_else(|| "<unknown>".to_string());
                                                    diagnostics.push(Diagnostic {
                                                        code: DiagnosticCode::AssignAliasFieldsInQuery,
                                                        message: format!(
                                                            "Поле '{}' должно иметь явный псевдоним с ключевым словом AS/КАК",
                                                            alias_name
                                                        ),
                                                        severity: Severity::Warning,
                                                        range: alias.syntax().text_range(),
                                                        tags: vec![],
                                                        fixes: vec![],
                                                    });
                                                }
                                            } else {
                                                diagnostics.push(Diagnostic {
                                                    code: DiagnosticCode::AssignAliasFieldsInQuery,
                                                    message: "Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК".to_string(),
                                                    severity: Severity::Warning,
                                                    range: field.syntax().text_range(),
                                                    tags: vec![],
                                                    fixes: vec![],
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Helper to check a standalone SDBL query (for testing)
    fn check_standalone_query(query_text: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        check_sdbl_query(query_text, &mut diagnostics);
        diagnostics
    }

    /// Helper to run diagnostic on BSL code (like all_function_path_must_have_return)
    fn check_diagnostic(code: &str, config: DiagnosticsConfig) -> (Vec<Diagnostic>, String) {
        use ide_db::base_db::SourceDatabase;
        use ide_db::RootDatabaseImpl;
        use test_fixture::Fixture;

        // Create fixture with test file
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        // Create database
        let mut db = RootDatabaseImpl::new();

        // Set file content in database from fixture
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        // Create diagnostics context
        use ide_db::RootDatabase;
        // RootDatabase trait object is not Send/Sync (Salsa is single-threaded).
        // Arc is used for trait object lifetime management in tests, not thread-safety.
        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let ctx = crate::DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
        };

        // Run diagnostic
        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_field_with_explicit_as() {
        // Should pass - has AS keyword
        let query = "SELECT Name AS ProductName FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_field_without_as_keyword() {
        // Should fail - implicit alias (no AS keyword)
        let query = "SELECT Name ProductName FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("AS/КАК"));
    }

    #[test]
    fn test_field_without_alias() {
        // Should fail - no alias at all
        let query = "SELECT Name FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("псевдоним"));
    }

    #[test]
    fn test_asterisk_field() {
        // Should pass - asterisk doesn't need alias
        let query = "SELECT * FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_table_asterisk() {
        // Should pass - Table.* doesn't need alias
        let query = "SELECT Products.* FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_multiple_fields_mixed() {
        // Mixed: some with AS, some without
        let query = "SELECT Name AS ProductName, Code ProductCode, Price FROM Products";
        let diagnostics = check_standalone_query(query);
        // Should have 2 errors: Code (implicit) and Price (no alias)
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_russian_kak_keyword() {
        // Russian КАК keyword should work
        let query = "ВЫБРАТЬ Имя КАК ИмяПродукта ИЗ Товары";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_union_query() {
        // UNION query - first query checked, UNION query not checked (per Java impl)
        let query = "SELECT Name AS N FROM Products UNION SELECT Title FROM Services";
        let diagnostics = check_standalone_query(query);
        // First query OK (Name AS N), second query (Title without alias) not checked
        // Because we only check main query, not UNION queries
        // NOTE: This matches Java implementation behavior
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_multiline_string_extraction() {
        use ide_db::base_db::{RootQueryDb, SourceDatabase};
        use ide_db::RootDatabaseImpl;
        use test_fixture::Fixture;

        let code = r#"Процедура Тест()
Query = "ВЫБРАТЬ
	|  Ссылка
	|ИЗ
	|  Справочник.Валюты";
КонецПроцедуры"#;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let parse = db.parse(file_id);
        let root = parse.syntax_node();

        // Print tree structure for debugging
        println!("Tree:");
        println!("{:#?}", root);

        // Find LITERAL nodes
        let mut found_literal = false;
        for node in root.descendants() {
            println!("Node: {:?}", node.kind());
            if node.kind() == SyntaxKind::LITERAL {
                found_literal = true;
                if let Some(extracted) = extract_string_content(&node) {
                    println!("Extracted: {:?}", extracted);
                    assert!(extracted.contains("ВЫБРАТЬ"));
                    assert!(extracted.contains("Ссылка"));
                    assert!(extracted.contains("Справочник"));
                }
            }
        }

        assert!(found_literal, "Should have found a LITERAL node");
    }

    #[test]
    fn test_sdbl_russian_query() {
        // Test that SDBL parser handles Russian queries
        let query = "ВЫБРАТЬ Ссылка, Код КАК К ИЗ Справочник.Валюты";

        // Parse SDBL and print debug info
        let parse = parser::parse_sdbl(query);
        println!("Parse has errors: {}", parse.has_errors());
        println!("Parse tree:\n{:#?}", parse.syntax_node());

        let diagnostics = check_standalone_query(query);
        println!("Diagnostics for Russian query: {}", diagnostics.len());
        for d in &diagnostics {
            println!("  - {}", d.message);
        }
        // Should have 1 error: Ссылка without alias
        assert_eq!(diagnostics.len(), 1);
    }

    /// Test from Java: AssignAliasFieldsInQueryDiagnosticTest.java
    ///
    /// Expected 5 diagnostics:
    /// - Line 3, columns 3-16 (Валюты.Ссылка without alias)
    /// - Line 5, columns 3-17 (Валюты.Код Код - implicit alias)
    /// - Line 21, columns 3-16 (Валюты.Ссылка without alias - second query)
    /// - Line 23, columns 3-17 (Валюты.Код Код - implicit alias - second query)
    /// - Line 42, columns 4-17 (Валюты.Ссылка without alias - in subquery)
    #[test]
    fn test_java_diagnostic_compatibility() {
        // Load exact copy of Java test fixture
        let code = include_str!("../../test_data/AssignAliasFieldsInQueryDiagnostic.bsl");
        let config = DiagnosticsConfig::default();

        // Run diagnostic check
        let (diagnostics, file_content) = check_diagnostic(code, config);

        // Java test expects exactly 5 diagnostics at (0-indexed lines):
        // - Line 3, cols 3-16 (Валюты.Ссылка without alias)
        // - Line 5, cols 3-17 (Валюты.Код Код without AS)
        // - Line 21, cols 3-16 (Валюты.Ссылка without alias)
        // - Line 23, cols 3-17 (Валюты.Код Код without AS)
        // - Line 42, cols 4-17 (Валюты.Ссылка in subquery without alias)

        // Debug: print all diagnostics
        for (i, diag) in diagnostics.iter().enumerate() {
            let (start_line, start_col, _end_line, end_col) =
                range_to_line_col(&file_content, diag.range);
            eprintln!(
                "Diagnostic {}: line {}, cols {}-{}, message: {}",
                i, start_line, start_col, end_col, diag.message
            );
        }

        assert_eq!(diagnostics.len(), 5, "Expected 5 diagnostics to match Java implementation");

        // Verify exact positions match Java test expectations
        assert_diagnostic_range(&file_content, &diagnostics[0], 3, 3, 16); // Валюты.Ссылка
        assert_diagnostic_range(&file_content, &diagnostics[1], 5, 3, 17); // Валюты.Код Код
        assert_diagnostic_range(&file_content, &diagnostics[2], 21, 3, 16); // Second query
        assert_diagnostic_range(&file_content, &diagnostics[3], 23, 3, 17); // Second query
        assert_diagnostic_range(&file_content, &diagnostics[4], 42, 4, 17); // Nested subquery
    }
}
