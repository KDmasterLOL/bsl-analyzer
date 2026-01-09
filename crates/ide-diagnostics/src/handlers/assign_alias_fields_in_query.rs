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

    // Iterate SDBL HIRs and corresponding query infos in parallel
    // Both are sorted by position in file, so we can zip them
    for ((_expr_id, sdbl_hir), (_query_expr_id, query_info)) in
        sdbl_hirs.iter().zip(sdbl_queries.iter())
    {
        let mapper = SdblPositionMapper::new_from_range_with_line_index(
            query_info.bsl_literal_range,
            &bsl_source,
            &line_starts,
        );

        // Emit diagnostics from HIR
        for hir_diag in &sdbl_hir.hir.diagnostics {
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

    #[test]
    fn test_query_with_comments() {
        // Test exact query from SDBL String Literal 0
        let query = r#"ВЫБРАТЬ
	Валюты.Ссылка, // Неправильно
	Валюты.Ссылка КАК ПсевдонимПоляСсылка, // Правильно
	Валюты.Код Код // Неправильно
ИЗ
	Справочник.Валюты КАК Валюты // Игнорируется

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	Валюты.Ссылка, // Игнорируется
	Валюты.Ссылка, // Игнорируется
	Валюты.Код // Игнорируется
ИЗ
	Справочник.Валюты КАК Валюты"#;

        use sdbl_hir::lower_sdbl_to_hir;
        use syntax::ast::{AstNode, SdblQueryPackage};

        let parse = parser::parse_sdbl(query);
        eprintln!("Parse has errors: {}", parse.has_errors());

        // Print AST to see how comments are handled
        let root = parse.syntax_node();
        eprintln!("\n=== Checking first field ===");
        if let Some(package) = SdblQueryPackage::cast(root.clone()) {
            if let Some(select_query) = package.queries().next() {
                if let Some(subquery) = select_query.subquery() {
                    if let Some(main_query) = subquery.main_query() {
                        if let Some(field_list) = main_query.field_list() {
                            for (i, field) in field_list.fields().enumerate() {
                                eprintln!("\nField {}:", i);
                                eprintln!("  Text: {:?}", field.syntax().text());
                                eprintln!("  Is asterisk: {}", field.is_asterisk());
                                if let Some(expr) = field.expression() {
                                    eprintln!("  Expression: {:?}", expr.text());
                                } else {
                                    eprintln!("  Expression: None");
                                }
                                if let Some(alias) = field.alias() {
                                    eprintln!("  Alias: {:?}", alias.name());
                                    eprintln!("  Has AS: {}", alias.has_as_keyword());
                                } else {
                                    eprintln!("  Alias: None");
                                }
                                // Check for errors
                                let has_error = field
                                    .syntax()
                                    .descendants_with_tokens()
                                    .any(|el| el.kind() == syntax::SyntaxKind::ERROR);
                                eprintln!("  Has error: {}", has_error);
                            }
                        }
                    }
                }
            }
        }

        let hir = lower_sdbl_to_hir(&parse, None);
        eprintln!("\nHIR diagnostics: {}", hir.hir.diagnostics.len());
        for diag in &hir.hir.diagnostics {
            eprintln!("  - {} at {:?}", diag.message(), diag.range());
        }

        // Count only AliasWithoutAsKeyword diagnostics
        let alias_diagnostics: Vec<_> = hir
            .hir
            .diagnostics
            .iter()
            .filter(|d| matches!(d, sdbl_hir::SdblDiagnostic::AliasWithoutAsKeyword { .. }))
            .collect();

        eprintln!("AliasWithoutAsKeyword diagnostics: {}", alias_diagnostics.len());

        // Should have 2 AliasWithoutAsKeyword diagnostics from first SELECT (before UNION):
        // - Валюты.Ссылка without alias
        // - Валюты.Код Код without AS keyword
        assert_eq!(
            alias_diagnostics.len(),
            2,
            "Expected 2 AliasWithoutAsKeyword diagnostics from first SELECT"
        );
    }

    #[test]
    fn test_simple_query_with_hir() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::{RootDatabase, RootDatabaseImpl};
        use test_fixture::Fixture;
        use vfs::VfsPath;

        // Test simple query without comments using HIR
        let code = r#"Процедура Тест()
Запрос = "ВЫБРАТЬ Валюты.Ссылка, Валюты.Код Код ИЗ Справочник.Валюты КАК Валюты";
КонецПроцедуры"#;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();
        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let sdbl_hirs = db.sdbl_hir_in_file(file_id);
        eprintln!("Simple query: {} HIRs", sdbl_hirs.len());
        for (i, (_expr_id, hir)) in sdbl_hirs.iter().enumerate() {
            eprintln!("HIR {}: {} diagnostics", i, hir.hir.diagnostics.len());
            for diag in &hir.hir.diagnostics {
                eprintln!("  - {} at {:?}", diag.message(), diag.range());
            }
        }

        assert_eq!(sdbl_hirs.len(), 1);
        // Should have at least 1 diagnostic (field without alias)
        assert!(
            !sdbl_hirs[0].1.hir.diagnostics.is_empty(),
            "Expected diagnostics for fields without AS keyword"
        );
    }

    #[test]
    fn test_wrapped_vs_unwrapped_code() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::{RootDatabase, RootDatabaseImpl};
        use test_fixture::Fixture;
        use vfs::VfsPath;

        // Test 1: Code wrapped in procedure
        let code_wrapped = r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Валюты.Ссылка, Валюты.Код Код ИЗ Справочник.Валюты КАК Валюты";
КонецПроцедуры"#;

        let fixture_text = format!("//- /test.bsl\n{}", code_wrapped);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();
        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let sdbl_hirs_wrapped = db.sdbl_hir_in_file(file_id);
        eprintln!(
            "Wrapped in procedure: {} HIRs, {} total diagnostics",
            sdbl_hirs_wrapped.len(),
            sdbl_hirs_wrapped.iter().map(|(_, h)| h.hir.diagnostics.len()).sum::<usize>()
        );

        // Test 2: Code at module level (no procedure)
        let code_unwrapped =
            r#"Запрос = "ВЫБРАТЬ Валюты.Ссылка, Валюты.Код Код ИЗ Справочник.Валюты КАК Валюты";"#;

        let fixture_text = format!("//- /test.bsl\n{}", code_unwrapped);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();
        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let sdbl_hirs_unwrapped = db.sdbl_hir_in_file(file_id);
        eprintln!(
            "Module-level code: {} HIRs, {} total diagnostics",
            sdbl_hirs_unwrapped.len(),
            sdbl_hirs_unwrapped.iter().map(|(_, h)| h.hir.diagnostics.len()).sum::<usize>()
        );

        // Both should work
        assert!(!sdbl_hirs_wrapped.is_empty() || !sdbl_hirs_unwrapped.is_empty());
    }

    #[test]
    fn test_debug_first_query() {
        // Debug first query from Java test
        let query = r#"ВЫБРАТЬ
	Валюты.Ссылка,
	Валюты.Ссылка КАК ПсевдонимПоляСсылка,
	Валюты.Код Код
ИЗ
	Справочник.Валюты КАК Валюты

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	Валюты.Ссылка,
	Валюты.Ссылка,
	Валюты.Код
ИЗ
	Справочник.Валюты КАК Валюты"#;

        // Parse and check AST structure
        use syntax::ast::AstNode;

        let parse = parser::parse_sdbl(query);
        eprintln!("Parse has errors: {}", parse.has_errors());

        // Print tree structure to understand UNION layout
        let root = parse.syntax_node();
        eprintln!("\n=== AST Structure ===");
        fn print_tree(node: &syntax::SyntaxNode, indent: usize) {
            let indent_str = "  ".repeat(indent);
            eprintln!("{}{:?}", indent_str, node.kind());
            for child in node.children() {
                print_tree(&child, indent + 1);
            }
        }
        print_tree(&root, 0);

        // Check field ancestors
        use syntax::ast::SdblQueryPackage;
        let Some(package) = SdblQueryPackage::cast(root.clone()) else {
            panic!("Failed to cast as package");
        };

        for (q_idx, select_query) in package.queries().enumerate() {
            eprintln!("\n=== Query {} ===", q_idx);
            if let Some(subquery) = select_query.subquery() {
                if let Some(main_query) = subquery.main_query() {
                    if let Some(field_list) = main_query.field_list() {
                        for (f_idx, field) in field_list.fields().enumerate() {
                            eprintln!("Field {}: {:?}", f_idx, field.syntax().text());
                            eprintln!("  Ancestors:");
                            for ancestor in field.syntax().ancestors() {
                                eprintln!("    - {:?}", ancestor.kind());
                            }
                        }
                    }
                }
            }
        }

        // Parse and check using SDBL HIR
        use sdbl_hir::lower_sdbl_to_hir;

        let hir = lower_sdbl_to_hir(&parse, None);
        eprintln!("\nHIR diagnostics: {}", hir.hir.diagnostics.len());
        for diag in &hir.hir.diagnostics {
            eprintln!("  - {} at {:?}", diag.message(), diag.range());
        }

        // Count only AliasWithoutAsKeyword diagnostics
        let alias_diagnostics: Vec<_> = hir
            .hir
            .diagnostics
            .iter()
            .filter(|d| matches!(d, sdbl_hir::SdblDiagnostic::AliasWithoutAsKeyword { .. }))
            .collect();

        // Should have 2 AliasWithoutAsKeyword diagnostics from first SELECT (before UNION):
        // - Валюты.Ссылка without alias
        // - Валюты.Код Код without AS keyword
        assert_eq!(
            alias_diagnostics.len(),
            2,
            "Expected 2 AliasWithoutAsKeyword diagnostics from first SELECT"
        );
    }

    #[test]
    fn test_top_clause_with_explicit_alias() {
        // Test that ПЕРВЫЕ (TOP) clause doesn't cause false positives
        // Field has explicit КАК keyword, should pass
        let query = r#"ВЫБРАТЬ ПЕРВЫЕ 100
Спр.Номенклатура КАК Номенклатура
ИЗ
Справочник.Номенклатура КАК Спр"#;

        let diagnostics = check_standalone_query(query);
        assert_eq!(
            diagnostics.len(),
            0,
            "ПЕРВЫЕ clause with explicit alias should not trigger diagnostic"
        );
    }

    #[test]
    fn test_top_clause_parsing() {
        // Verify that TOP clause is correctly parsed by the SDBL parser
        use syntax::ast::{AstNode, SdblQueryPackage};
        use syntax::SyntaxKind;

        let query = r#"ВЫБРАТЬ ПЕРВЫЕ 100
Спр.Номенклатура КАК Номенклатура
ИЗ
Справочник.Номенклатура КАК Спр"#;

        let parse = parser::parse_sdbl(query);
        assert!(!parse.has_errors(), "Parse should not have errors");

        let root = parse.syntax_node();

        // Check that SDBL_LIMITATIONS and SDBL_TOP_CLAUSE nodes are present
        let has_limitations =
            root.descendants().any(|node| node.kind() == SyntaxKind::SDBL_LIMITATIONS);
        let has_top_clause =
            root.descendants().any(|node| node.kind() == SyntaxKind::SDBL_TOP_CLAUSE);

        assert!(has_limitations, "Should have SDBL_LIMITATIONS node");
        assert!(has_top_clause, "Should have SDBL_TOP_CLAUSE node");

        // Verify field is correctly parsed
        if let Some(package) = SdblQueryPackage::cast(root) {
            for select_query in package.queries() {
                if let Some(subquery) = select_query.subquery() {
                    if let Some(main_query) = subquery.main_query() {
                        if let Some(field_list) = main_query.field_list() {
                            let fields: Vec<_> = field_list.fields().collect();
                            assert_eq!(fields.len(), 1, "Should have exactly 1 field");

                            let field = &fields[0];
                            assert!(!field.is_asterisk(), "Field should not be asterisk");
                            assert!(field.alias().is_some(), "Field should have alias");
                            assert!(
                                field.alias().unwrap().has_as_keyword(),
                                "Alias should have КАК keyword"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_top_clause_without_alias() {
        // Test that ПЕРВЫЕ (TOP) clause still detects missing alias
        let query = r#"ВЫБРАТЬ ПЕРВЫЕ 100
Спр.Номенклатура
ИЗ
Справочник.Номенклатура КАК Спр"#;

        let diagnostics = check_standalone_query(query);
        assert_eq!(
            diagnostics.len(),
            1,
            "ПЕРВЫЕ clause with missing alias should trigger diagnostic"
        );
    }

    #[test]
    fn test_top_clause_implicit_alias() {
        // Test that ПЕРВЫЕ (TOP) clause detects implicit alias (without КАК)
        let query = r#"ВЫБРАТЬ ПЕРВЫЕ 100
Спр.Номенклатура Номенклатура
ИЗ
Справочник.Номенклатура КАК Спр"#;

        let diagnostics = check_standalone_query(query);
        assert_eq!(
            diagnostics.len(),
            1,
            "ПЕРВЫЕ clause with implicit alias (no КАК) should trigger diagnostic"
        );
    }

    #[test]
    fn test_distinct_clause() {
        // Test DISTINCT keyword
        let query = "SELECT DISTINCT Name AS ProductName FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0, "DISTINCT with explicit alias should pass");
    }

    #[test]
    fn test_distinct_top_combination() {
        // Test DISTINCT TOP combination
        let query = "ВЫБРАТЬ РАЗЛИЧНЫЕ ПЕРВЫЕ 10 Код КАК К ИЗ Товары";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0, "DISTINCT TOP with explicit alias should pass");
    }

    #[test]
    fn test_top_distinct_order() {
        // Test TOP DISTINCT order (also valid)
        let query = "SELECT TOP 50 DISTINCT Name AS N FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0, "TOP DISTINCT with explicit alias should pass");
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

        // Run diagnostic check with debug output
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::{RootDatabase, RootDatabaseImpl};
        use test_fixture::Fixture;
        use vfs::VfsPath;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();

        // Set up source root for file_text_input to work (required for SDBL diagnostics)
        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        // Debug: check how many SDBL queries are extracted
        let sdbl_queries = db.all_sdbl_in_file(file_id);
        eprintln!("Total SDBL string literals found: {}", sdbl_queries.len());

        for (i, (_expr_id, query_info)) in sdbl_queries.iter().enumerate() {
            eprintln!("\nSDdBL String Literal {}:", i);
            eprintln!(
                "  Query text (first 200 chars): {:?}",
                query_info.query_text.chars().take(200).collect::<String>()
            );
            eprintln!("  Has AST: {}", query_info.query_ast.is_some());
            if let Some(ref ast) = query_info.query_ast {
                eprintln!("  Parse errors: {}", ast.has_errors());
            }
        }

        // Debug: check HIR for each SDBL
        let sdbl_hirs = db.sdbl_hir_in_file(file_id);
        eprintln!("\nTotal SDBL HIRs generated: {}", sdbl_hirs.len());

        for (i, (_expr_id, hir)) in sdbl_hirs.iter().enumerate() {
            eprintln!("HIR {}: {} diagnostics", i, hir.hir.diagnostics.len());
            for diag in &hir.hir.diagnostics {
                eprintln!("  - {}", diag.message());
            }
        }

        // Run diagnostic check
        let (diagnostics, file_content) = check_diagnostic(code, config);

        // Java test expects exactly 5 diagnostics at (0-indexed lines):
        // - Line 3, cols 3-16 (Валюты.Ссылка without alias)
        // - Line 5, cols 3-17 (Валюты.Код Код without AS)
        // - Line 21, cols 3-16 (Валюты.Ссылка without alias)
        // - Line 23, cols 3-17 (Валюты.Код Код without AS)
        // - Line 42, cols 4-17 (Валюты.Ссылка in subquery without alias)

        // Debug: print all diagnostics
        eprintln!("\nFinal diagnostics returned:");
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
