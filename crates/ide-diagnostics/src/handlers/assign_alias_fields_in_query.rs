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
    SyntaxKind, SyntaxNode,
};

/// Runs the AssignAliasFieldsInQuery diagnostic.
///
/// **Current implementation (MVP):**
/// For now, this is a placeholder that returns no diagnostics.
/// Full implementation requires extracting SDBL queries from BSL string literals.
///
/// **TODO Phase 2:**
/// 1. Walk BSL AST to find STRING_LITERAL nodes
/// 2. Try to parse each string as SDBL
/// 3. Check for fields without AS keyword
/// 4. Map diagnostic positions back to BSL source
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Check if diagnostic is disabled
    if ctx.config.is_disabled(DiagnosticCode::AssignAliasFieldsInQuery) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    // TODO: Extract SDBL queries from BSL string literals
    // For now, we'll look for any SDBL query packages in the syntax tree
    // (This is a simplified version - full implementation needs string extraction)

    // Walk the tree looking for string literals that might contain SDBL
    for node in root.descendants() {
        if node.kind() == SyntaxKind::STRING {
            // Try to extract and parse as SDBL
            if let Some(query_text) = extract_string_content(&node) {
                check_sdbl_query(&query_text, &mut diagnostics);
            }
        }
    }

    diagnostics
}

/// Extract string content from a STRING node.
///
/// Handles multiline strings and unescapes quotes.
fn extract_string_content(node: &SyntaxNode) -> Option<String> {
    let text = node.text().to_string();

    // Remove quotes and handle multiline strings
    // BSL strings can be:
    // - "simple string"
    // - "multiline
    //    |continued"

    if text.len() < 2 {
        return None;
    }

    // Remove outer quotes
    let inner = &text[1..text.len() - 1];

    // Handle multiline continuation (remove | at start of lines)
    let result = inner
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            trimmed.strip_prefix('|').unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Unescape quotes (BSL uses "" for escaped ")
    let unescaped = result.replace("\"\"", "\"");

    Some(unescaped)
}

/// Check a single SDBL query for fields without AS keyword.
fn check_sdbl_query(query_text: &str, diagnostics: &mut Vec<Diagnostic>) {
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
        check_select_query(&select_query, diagnostics);
    }
}

/// Check a SELECT query for fields without AS keyword.
fn check_select_query(select_query: &SdblSelectQuery, diagnostics: &mut Vec<Diagnostic>) {
    let Some(subquery) = select_query.subquery() else {
        return;
    };

    // We need to determine if this is a subquery (nested in another query)
    // For MVP, we'll check all queries
    // TODO: Better subquery detection - check if parent is FROM clause

    // Check main query fields
    if let Some(main_query) = subquery.main_query() {
        check_query_fields(&main_query, diagnostics);
    }

    // TODO: Also check subqueries in FROM clause recursively
}

/// Check fields in a query for missing AS keyword.
fn check_query_fields(query: &SdblQuery, diagnostics: &mut Vec<Diagnostic>) {
    let Some(field_list) = query.field_list() else {
        return;
    };

    for field in field_list.fields() {
        check_field(&field, diagnostics);
    }
}

/// Check a single field for missing AS keyword.
fn check_field(field: &SdblSelectedField, diagnostics: &mut Vec<Diagnostic>) {
    // Skip asterisk fields (they don't need aliases)
    if field.is_asterisk() {
        return;
    }

    // Check if field has alias
    if let Some(alias) = field.alias() {
        // Alias exists, check if it has AS keyword
        if !alias.has_as_keyword() {
            // ERROR: Alias without AS keyword (implicit alias)
            add_diagnostic_for_alias(&alias, diagnostics);
        }
    } else {
        // ERROR: Field without alias at all
        add_diagnostic_for_field(field, diagnostics);
    }
}

/// Add diagnostic for alias without AS keyword.
fn add_diagnostic_for_alias(alias: &SdblAlias, diagnostics: &mut Vec<Diagnostic>) {
    let alias_name = alias.name().unwrap_or_else(|| "<unknown>".to_string());

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

/// Add diagnostic for field without alias.
fn add_diagnostic_for_field(field: &SdblSelectedField, diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.push(Diagnostic {
        code: DiagnosticCode::AssignAliasFieldsInQuery,
        message: "Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК".to_string(),
        severity: Severity::Warning,
        range: field.syntax().text_range(),
        tags: vec![],
        fixes: vec![],
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to check a standalone SDBL query (for testing)
    fn check_standalone_query(query_text: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        check_sdbl_query(query_text, &mut diagnostics);
        diagnostics
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
}
