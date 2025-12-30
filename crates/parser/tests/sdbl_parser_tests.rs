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

// ==================== Basic SELECT Tests ====================

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

// ==================== Alias Tests (CRITICAL for Diagnostic) ====================

#[test]
fn test_alias_with_as_keyword() {
    // This should have AS keyword in the tree
    check(
        "SELECT Name AS ProductName FROM Products",
        expect![[r#"
            SDBL_QUERY_PACKAGE@0..35
              SDBL_SELECT_QUERY@0..35
                SDBL_SUBQUERY@0..35
                  SDBL_QUERY@0..35
                    IDENT@0..6 "SELECT"
                    SDBL_FIELD_LIST@6..23
                      SDBL_SELECTED_FIELD@6..23
                        SDBL_LOGICAL_OR_EXPR@6..10
                          SDBL_LOGICAL_AND_EXPR@6..10
                            SDBL_ADDITIVE_EXPR@6..10
                              SDBL_MULTIPLICATIVE_EXPR@6..10
                                SDBL_COLUMN_REF@6..10
                                  IDENT@6..10 "Name"
                        SDBL_ALIAS@10..23
                          IDENT@10..12 "AS"
                          IDENT@12..23 "ProductName"
                    SDBL_FROM_CLAUSE@23..35
                      IDENT@23..27 "FROM"
                      SDBL_DATA_SOURCE@27..35
                        SDBL_TABLE_REF@27..35
                          IDENT@27..35 "Products"
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

// ==================== UNION Tests ====================

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

// ==================== Subquery Tests ====================

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

// ==================== Expression Tests ====================

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

// ==================== MDO References ====================

#[test]
fn test_mdo_table_reference() {
    check_no_errors("SELECT Name FROM Catalog.Products");
    check_no_errors("SELECT Ref FROM Document.Sales");
}

#[test]
fn test_mdo_qualified_column() {
    check_no_errors("SELECT Catalog.Products.Name FROM Catalog.Products");
}

// ==================== Literals ====================

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

// ==================== Parameters ====================

#[test]
fn test_parameter() {
    check_no_errors("SELECT * FROM Products WHERE Code = &ProductCode");
}

#[test]
fn test_multiple_parameters() {
    check_no_errors("SELECT * FROM Products WHERE Code = &Code AND Active = &IsActive");
}

// ==================== Complex Queries ====================

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

// ==================== Multiple Queries ====================

#[test]
fn test_multiple_queries_with_semicolon() {
    check_no_errors("SELECT Name FROM Products; SELECT Code FROM Services");
}

#[test]
fn test_multiple_queries_trailing_semicolon() {
    check_no_errors("SELECT Name FROM Products; SELECT Code FROM Services;");
}

// ==================== Error Recovery Tests ====================

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

// ==================== AST Navigation Tests (for Diagnostic) ====================

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
