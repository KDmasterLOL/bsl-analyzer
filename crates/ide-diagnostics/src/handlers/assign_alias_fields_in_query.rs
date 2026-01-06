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
//! Now uses SDBL HIR with diagnostics collected during lowering.

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use sdbl_hir;
use tracing::debug;

/// Runs the AssignAliasFieldsInQuery diagnostic.
///
/// Uses SDBL HIR with diagnostics collected during lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::AssignAliasFieldsInQuery) {
        return Vec::new();
    }

    // Get SDBL HIR with collected diagnostics
    let sdbl_hirs = ctx.db.sdbl_hir_in_file(ctx.file_id);

    let input = ctx.db.file_text_input(ctx.file_id);
    let bsl_source = input.text(ctx.db);

    // Get cached SDBL queries for position mapping
    let sdbl_queries = ctx.db.all_sdbl_in_file(ctx.file_id);

    // Build shared line index
    use crate::sdbl_utils::build_line_index_shared;
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    // Iterate SDBL HIRs and emit diagnostics
    for (expr_id, sdbl_hir) in sdbl_hirs.iter() {
        // Find corresponding query info for position mapping
        let query_info = sdbl_queries.iter().find(|(id, _)| id == expr_id).map(|(_, info)| info);

        let Some(query_info) = query_info else {
            continue;
        };

        let mapper = SdblPositionMapper::new_from_range_with_line_index(
            query_info.bsl_literal_range,
            &bsl_source,
            &line_starts,
        );

        // Emit diagnostics from HIR
        for hir_diag in &sdbl_hir.diagnostics {
            if let sdbl_hir::SdblDiagnostic::AliasWithoutAsKeyword { field_name, range } = hir_diag
            {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

                let message = if let Some(name) = field_name {
                    format!("Поле '{}' должно иметь явный псевдоним с ключевым словом AS/КАК", name)
                } else {
                    "Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК".to_string()
                };

                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::AssignAliasFieldsInQuery,
                    message,
                    severity: Severity::Warning,
                    range: bsl_range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "AssignAliasFieldsInQuery completed"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        sdbl_utils::extract_string_content,
        test_utils::{assert_diagnostic_range, range_to_line_col},
        DiagnosticsConfig,
    };
    use parser::parse_sdbl;
    use syntax::ast::SdblQueryPackage;
    use syntax::SyntaxKind;

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

    /// Helper to run diagnostic on BSL code
    fn check_diagnostic(code: &str, config: DiagnosticsConfig) -> (Vec<Diagnostic>, String) {
        use crate::test_utils::check_sdbl_diagnostic_with_config;

        let diagnostics = check_sdbl_diagnostic_with_config(code, config, check);
        // Extract content from the code
        let content = code.strip_prefix("//- /test.bsl\n").unwrap_or(code).to_string();
        (diagnostics, content)
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
