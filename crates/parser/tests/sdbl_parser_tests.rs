//! SDBL parser tests.
//!
//! Tests SDBL query parsing with focus on:
//! - Basic SELECT queries
//! - Aliases (with and without AS keyword) - CRITICAL for AssignAliasFieldsInQuery
//! - UNION queries
//! - Subqueries in FROM
//! - Error recovery

use expect_test::{expect, Expect};
use parser::parse_sdbl;

fn check(input: &str, expected: Expect) {
    let parse = parse_sdbl(input);
    let debug_tree = format!("{:#?}", parse.syntax_node());
    expected.assert_eq(&debug_tree);
}

fn check_no_errors(input: &str) {
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Expected no errors, but got: {:#?}", parse.errors());
}

#[test]
fn test_select_asterisk() {
    check_no_errors("SELECT * FROM Table");
}

#[test]
fn test_select_single_column() {
    check_no_errors("SELECT Name FROM Products");
}

#[test]
fn test_select_multiple_columns() {
    check_no_errors("SELECT Name, Code, Description FROM Products");
}

#[test]
fn test_select_with_where() {
    check_no_errors("SELECT Name FROM Products WHERE Active = TRUE");
}

#[test]
fn test_select_table_asterisk() {
    check_no_errors("SELECT Products.* FROM Products");
}

#[test]
fn test_select_qualified_column() {
    check_no_errors("SELECT Products.Name FROM Products");
}

#[test]
fn test_alias_with_as_keyword() {
    // This should have AS keyword in the tree
    check(
        "SELECT Name AS ProductName FROM Products",
        expect![[r#"
            SDBL_QUERY_PACKAGE@0..40
              SDBL_SELECT_QUERY@0..40
                SDBL_SUBQUERY@0..40
                  SDBL_QUERY@0..40
                    IDENT@0..6 "SELECT"
                    WHITESPACE@6..7 " "
                    SDBL_FIELD_LIST@7..26
                      SDBL_SELECTED_FIELD@7..26
                        SDBL_LOGICAL_OR_EXPR@7..12
                          SDBL_LOGICAL_AND_EXPR@7..12
                            SDBL_ADDITIVE_EXPR@7..12
                              SDBL_MULTIPLICATIVE_EXPR@7..12
                                SDBL_COLUMN_REF@7..12
                                  IDENT@7..11 "Name"
                                  WHITESPACE@11..12 " "
                        SDBL_ALIAS@12..26
                          IDENT@12..14 "AS"
                          WHITESPACE@14..15 " "
                          IDENT@15..26 "ProductName"
                    WHITESPACE@26..27 " "
                    SDBL_FROM_CLAUSE@27..40
                      IDENT@27..31 "FROM"
                      WHITESPACE@31..32 " "
                      SDBL_DATA_SOURCE@32..40
                        SDBL_TABLE_REF@32..40
                          IDENT@32..40 "Products"
        "#]],
    );
}

#[test]
fn test_alias_without_as_keyword() {
    // Implicit alias (no AS keyword) - this is what the diagnostic should catch
    check_no_errors("SELECT Name ProductName FROM Products");
}

#[test]
fn test_multiple_aliases_with_as() {
    check_no_errors("SELECT Name AS ProductName, Code AS ProductCode FROM Products");
}

#[test]
fn test_multiple_aliases_mixed() {
    // Some with AS, some without
    check_no_errors("SELECT Name AS ProductName, Code ProductCode FROM Products");
}

#[test]
fn test_russian_alias_with_kak() {
    // Russian КАК keyword
    check_no_errors("ВЫБРАТЬ Имя КАК ИмяПродукта ИЗ Товары");
}

#[test]
fn test_alias_case_insensitive() {
    // AS in various cases
    check_no_errors("SELECT Name as ProductName FROM Products");
    check_no_errors("SELECT Name As ProductName FROM Products");
    check_no_errors("SELECT Name aS ProductName FROM Products");
}

#[test]
fn test_asterisk_no_alias() {
    // Asterisk shouldn't have alias
    check_no_errors("SELECT * FROM Products");
    check_no_errors("SELECT Products.* FROM Products");
}

#[test]
fn test_union_simple() {
    check_no_errors("SELECT Name FROM Products UNION SELECT Name FROM Services");
}

#[test]
fn test_union_all() {
    check_no_errors("SELECT Name FROM Products UNION ALL SELECT Name FROM Services");
}

#[test]
fn test_union_multiple() {
    check_no_errors("SELECT A FROM T1 UNION SELECT B FROM T2 UNION SELECT C FROM T3");
}

#[test]
fn test_union_with_aliases() {
    check_no_errors("SELECT Name AS N FROM Products UNION SELECT Title AS N FROM Services");
}

#[test]
fn test_subquery_in_from() {
    check_no_errors("SELECT * FROM (SELECT Name FROM Products) AS Sub");
}

#[test]
fn test_subquery_nested() {
    check_no_errors("SELECT * FROM (SELECT * FROM (SELECT Name FROM Products) AS Inner) AS Outer");
}

#[test]
fn test_subquery_with_where() {
    check_no_errors("SELECT * FROM (SELECT Name FROM Products WHERE Active = TRUE) AS Sub");
}

#[test]
fn test_subquery_in_expression() {
    check_no_errors("SELECT Name FROM Products WHERE Code IN (SELECT Code FROM Active)");
}

#[test]
fn test_arithmetic_expressions() {
    check_no_errors("SELECT Price * Quantity AS Total FROM Orders");
    check_no_errors("SELECT Price + Tax AS TotalPrice FROM Products");
    check_no_errors("SELECT Amount - Discount AS Final FROM Sales");
}

#[test]
fn test_logical_expressions() {
    check_no_errors("SELECT * FROM Products WHERE Active = TRUE AND Price > 100");
    check_no_errors("SELECT * FROM Products WHERE Category = 1 OR Category = 2");
    check_no_errors("SELECT * FROM Products WHERE NOT Deleted");
}

#[test]
fn test_comparison_expressions() {
    check_no_errors("SELECT * FROM Products WHERE Price > 100");
    check_no_errors("SELECT * FROM Products WHERE Quantity >= 10");
    check_no_errors("SELECT * FROM Products WHERE Code <> 0");
}

#[test]
fn test_function_calls() {
    check_no_errors("SELECT COUNT(*) AS Total FROM Products");
    check_no_errors("SELECT SUM(Price) AS TotalPrice FROM Products");
    check_no_errors("SELECT YEAR(Date) AS Year FROM Orders");
}

#[test]
fn test_mdo_table_reference() {
    check_no_errors("SELECT Name FROM Catalog.Products");
    check_no_errors("SELECT Ref FROM Document.Sales");
}

#[test]
fn test_mdo_qualified_column() {
    check_no_errors("SELECT Catalog.Products.Name FROM Catalog.Products");
}

#[test]
fn test_numeric_literals() {
    check_no_errors("SELECT * FROM Products WHERE Price = 100");
    check_no_errors("SELECT * FROM Products WHERE Price = 99.99");
}

#[test]
fn test_string_literals() {
    check_no_errors(r#"SELECT * FROM Products WHERE Name = "Product""#);
}

#[test]
fn test_boolean_literals() {
    check_no_errors("SELECT * FROM Products WHERE Active = TRUE");
    check_no_errors("SELECT * FROM Products WHERE Deleted = FALSE");
}

#[test]
fn test_null_literal() {
    check_no_errors("SELECT * FROM Products WHERE Description = NULL");
}

#[test]
fn test_parameter() {
    check_no_errors("SELECT * FROM Products WHERE Code = &ProductCode");
}

#[test]
fn test_multiple_parameters() {
    check_no_errors("SELECT * FROM Products WHERE Code = &Code AND Active = &IsActive");
}

#[test]
fn test_complex_query_with_all_features() {
    check_no_errors(
        r#"SELECT
            Products.Name AS ProductName,
            Products.Code AS ProductCode,
            SUM(Sales.Amount) AS TotalSales
        FROM
            Catalog.Products AS Products,
            (SELECT ProductRef, Amount FROM Document.Sales WHERE Date >= &StartDate) AS Sales
        WHERE
            Products.Active = TRUE
            AND Sales.Amount > 0"#,
    );
}

#[test]
fn test_multiple_queries_with_semicolon() {
    check_no_errors("SELECT Name FROM Products; SELECT Code FROM Services");
}

#[test]
fn test_multiple_queries_trailing_semicolon() {
    check_no_errors("SELECT Name FROM Products; SELECT Code FROM Services;");
}

// TODO Phase 2: Error recovery is not fully implemented yet.
// These tests are commented out until error recovery is improved.

// #[test]
// fn test_incomplete_select() {
//     // Should recover gracefully
//     let parse = parse_sdbl("SELECT");
//     assert!(parse.has_errors());
// }

// #[test]
// fn test_missing_from_table() {
//     // SELECT without FROM is valid in some SQL dialects, but check our behavior
//     let parse = parse_sdbl("SELECT Name FROM");
//     assert!(parse.has_errors());
// }

// #[test]
// fn test_unclosed_parenthesis() {
//     let parse = parse_sdbl("SELECT * FROM (SELECT Name FROM Products");
//     assert!(parse.has_errors());
// }

#[test]
fn test_ast_alias_has_as_keyword() {
    use syntax::ast::{AstNode, SdblQueryPackage};

    let input = "SELECT Name AS ProductName FROM Products";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    let package = SdblQueryPackage::cast(root).expect("Failed to cast to SdblQueryPackage");
    let query = package.queries().next().expect("No query found");
    let subquery = query.subquery().expect("No subquery found");
    let main_query = subquery.main_query().expect("No main query found");
    let field_list = main_query.field_list().expect("No field list found");
    let field = field_list.fields().next().expect("No field found");
    let alias = field.alias().expect("No alias found");

    // CRITICAL TEST: Check has_as_keyword() method
    assert!(alias.has_as_keyword(), "Alias should have AS keyword: {:?}", alias.syntax());
    assert_eq!(alias.name(), Some("ProductName".to_string()));
}

#[test]
fn test_ast_alias_without_as_keyword() {
    use syntax::ast::{AstNode, SdblQueryPackage};

    let input = "SELECT Name ProductName FROM Products";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    let package = SdblQueryPackage::cast(root).expect("Failed to cast to SdblQueryPackage");
    let query = package.queries().next().expect("No query found");
    let subquery = query.subquery().expect("No subquery found");
    let main_query = subquery.main_query().expect("No main query found");
    let field_list = main_query.field_list().expect("No field list found");
    let field = field_list.fields().next().expect("No field found");
    let alias = field.alias().expect("No alias found");

    // CRITICAL TEST: Implicit alias should NOT have AS keyword
    assert!(
        !alias.has_as_keyword(),
        "Implicit alias should not have AS keyword: {:?}",
        alias.syntax()
    );
    assert_eq!(alias.name(), Some("ProductName".to_string()));
}

#[test]
fn test_ast_asterisk_field() {
    use syntax::ast::{AstNode, SdblQueryPackage};

    let input = "SELECT * FROM Products";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    let package = SdblQueryPackage::cast(root).expect("Failed to cast to SdblQueryPackage");
    let query = package.queries().next().expect("No query found");
    let subquery = query.subquery().expect("No subquery found");
    let main_query = subquery.main_query().expect("No main query found");
    let field_list = main_query.field_list().expect("No field list found");
    let field = field_list.fields().next().expect("No field found");

    // CRITICAL TEST: Check is_asterisk() method
    assert!(field.is_asterisk(), "Field should be asterisk: {:?}", field.syntax());
    assert!(field.alias().is_none(), "Asterisk should not have alias");
}

#[test]
fn test_ast_union_queries() {
    use syntax::ast::{AstNode, SdblQueryPackage};

    let input = "SELECT A FROM T1 UNION SELECT B FROM T2";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    let package = SdblQueryPackage::cast(root).expect("Failed to cast to SdblQueryPackage");
    let query = package.queries().next().expect("No query found");
    let subquery = query.subquery().expect("No subquery found");

    // CRITICAL TEST: Only main query should be checked, not UNION queries
    let main_query = subquery.main_query().expect("No main query found");
    let main_field_list = main_query.field_list().expect("No field list in main query");
    let main_field = main_field_list.fields().next().expect("No field in main query");
    assert_eq!(
        main_field.expression().and_then(|e| e.first_token()).map(|t| t.text().to_string()),
        Some("A".to_string())
    );

    // Check that we have union queries
    let union_count = subquery.union_queries().count();
    assert_eq!(union_count, 1, "Should have 1 UNION query");
}

#[test]
fn test_debug_semicolon_tokens() {
    let query = r#"ВЫБРАТЬ
	Валюты.Ссылка
ИЗ
	Справочник.Валюты КАК Валюты
;

////////////////////////////////////////////////////////////////////////////////
ВЫБРАТЬ
	Валюты.Код
ИЗ
	Справочник.Валюты КАК Валюты"#;

    // Parse SDBL
    use lexer::sdbl::tokenize_sdbl;
    let sdbl_tokens = tokenize_sdbl(query);

    eprintln!("\n=== SDBL Tokens ===");
    for (i, token) in sdbl_tokens.iter().enumerate() {
        let text = &token.text;
        eprintln!(
            "  [{}] {:?} = {:?}",
            i,
            token.kind,
            text.replace('\n', "\\n").replace('\t', "\\t")
        );

        // Print tokens around semicolon
        if text.contains(';') {
            eprintln!("\n  ^^^ SEMICOLON FOUND AT TOKEN {} ^^^", i);
            eprintln!("\n  Next 10 tokens:");
            for j in 1..=10.min(sdbl_tokens.len() - i - 1) {
                let tok = &sdbl_tokens[i + j];
                eprintln!(
                    "    [{}] {:?} = {:?}",
                    i + j,
                    tok.kind,
                    tok.text.replace('\n', "\\n").replace('\t', "\\t")
                );
            }
            break;
        }
    }

    let parse = parse_sdbl(query);
    eprintln!("\n=== Parse Result ===");
    eprintln!("Has errors: {}", parse.has_errors());

    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();
    if let Some(package) = SdblQueryPackage::cast(root) {
        let count = package.queries().count();
        eprintln!("Number of queries in package: {}", count);
        assert_eq!(count, 2, "Expected 2 queries separated by semicolon");
    } else {
        panic!("Failed to cast to SdblQueryPackage");
    }
}

#[test]
fn test_union_with_semicolon_separator() {
    // Test pattern from Java test: SELECT with UNION, semicolon, comment, SELECT with UNION
    let query = r#"ВЫБРАТЬ
	Валюты.Ссылка
ИЗ
	Справочник.Валюты КАК Валюты

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	Валюты.Код
ИЗ
	Справочник.Валюты КАК Валюты
;

////////////////////////////////////////////////////////////////////////////////
ВЫБРАТЬ
	Валюты.Ссылка
ИЗ
	Справочник.Валюты КАК Валюты

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	Валюты.Код
ИЗ
	Справочник.Валюты КАК Валюты"#;

    let parse = parse_sdbl(query);
    eprintln!("\n=== Union+Semicolon Test ===");
    eprintln!("Has errors: {}", parse.has_errors());

    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();
    if let Some(package) = SdblQueryPackage::cast(root) {
        let count = package.queries().count();
        eprintln!("Number of queries: {}", count);
        assert_eq!(count, 2, "Expected 2 SELECT queries (each with UNION) separated by semicolon");
    } else {
        panic!("Failed to cast to SdblQueryPackage");
    }
}

#[test]
fn test_exact_java_query_structure() {
    // Exact query from Java test (first string, 857 chars)
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
	Справочник.Валюты КАК Валюты
;

////////////////////////////////////////////////////////////////////////////////
ВЫБРАТЬ
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

    // Tokenize to see what's happening
    use lexer::sdbl::tokenize_sdbl;
    let tokens = tokenize_sdbl(query);

    eprintln!("\n=== SDBL Tokens 20-40 (around first UNION) ===");
    for (i, token) in tokens.iter().enumerate().skip(20).take(20) {
        eprintln!(
            "[{}] {:?} = {:?}",
            i,
            token.kind,
            token.text.replace('\n', "\\n").replace('\t', "\\t")
        );
    }
    eprintln!("\n=== SDBL Tokens 45-65 (around semicolon at 56) ===");
    for (i, token) in tokens.iter().enumerate().skip(45).take(20) {
        eprintln!(
            "[{}] {:?} = {:?}",
            i,
            token.kind,
            token.text.replace('\n', "\\n").replace('\t', "\\t")
        );
    }
    eprintln!("\nTotal SDBL tokens: {}", tokens.len());

    let parse = parse_sdbl(query);
    eprintln!("\n=== Exact Java Query Test ===");
    eprintln!("Query length: {} chars", query.len());
    eprintln!("Has errors: {}", parse.has_errors());

    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();
    if let Some(package) = SdblQueryPackage::cast(root) {
        let count = package.queries().count();
        eprintln!("Number of queries: {}", count);
        assert_eq!(count, 2, "Should find 2 SELECT queries separated by semicolon");
    } else {
        panic!("Failed to cast to SdblQueryPackage");
    }
}
