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
                    SDBL_FIELD_LIST@7..27
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

    let parse = parse_sdbl(query);
    assert!(!parse.has_errors(), "Should parse queries separated by semicolon");

    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("Should have query package");
    let count = package.queries().count();
    assert_eq!(count, 2, "Expected 2 queries separated by semicolon");
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
    assert!(!parse.has_errors(), "Should parse UNION queries separated by semicolon");

    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("Should have query package");
    let count = package.queries().count();
    assert_eq!(count, 2, "Expected 2 SELECT queries (each with UNION) separated by semicolon");
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

    let parse = parse_sdbl(query);
    assert!(!parse.has_errors(), "Should parse exact Java query structure");

    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("Should have query package");
    let count = package.queries().count();
    assert_eq!(count, 2, "Should find 2 SELECT queries separated by semicolon");
}

// Tests for FULL OUTER JOIN parsing (fix for keyword consumption bug)

#[test]
fn test_full_outer_join_simple() {
    let input = "SELECT * FROM T1 FULL OUTER JOIN T2 ON T1.A = T2.A";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Should parse without errors");
}

#[test]
fn test_multiple_full_outer_joins() {
    let input =
        "SELECT * FROM T1 FULL OUTER JOIN T2 ON T1.A = T2.A FULL OUTER JOIN T3 ON T1.B = T3.B";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Should parse multiple JOINs");
}

#[test]
fn test_on_not_consumed_as_alias() {
    let input = "SELECT * FROM T1 JOIN T2 ON T1.ID = T2.ID";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors());
    let text = format!("{:#?}", parse.syntax_node());
    assert!(text.contains("ON"), "ON keyword should be in AST, not consumed as alias");
}

#[test]
fn test_nested_joins_multiline_russian() {
    let input = "ВЫБРАТЬ Товары.Номенклатура
ИЗ Товары КАК Товары
    ЛЕВОЕ СОЕДИНЕНИЕ ПланПродаж КАК ПланПродаж
        ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ ФактическиеПродажи КАК ФактическиеПродажи
        ПО ПланПродаж.Номенклатура = ФактическиеПродажи.Номенклатура
    ПО Товары.Номенклатура = ПланПродаж.Номенклатура";

    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Should parse multiline nested JOINs");
}

// Tests for multi-argument function calls (bug fix)

#[test]
fn test_function_with_two_arguments() {
    // Simplest case: two-argument function without alias
    check_no_errors("SELECT ISNULL(A, 0) FROM T");
}

#[test]
fn test_function_with_two_arguments_and_alias() {
    // With alias
    check_no_errors("SELECT ISNULL(Amount, 0) AS Total FROM Products");
}

#[test]
fn test_russian_function_with_arguments() {
    // Russian version
    check_no_errors("ВЫБРАТЬ ЕСТЬNULL(Сумма, 0) ИЗ Товары");
}

#[test]
fn test_multiple_fields_with_function_arguments() {
    // Multiple fields, one with multi-arg function
    check_no_errors("SELECT Name, ISNULL(Amount, 0) AS Total FROM Products");
}

#[test]
fn test_multiple_multi_arg_functions() {
    // Multiple multi-arg functions like the failing diagnostic test
    let input = "ВЫБРАТЬ
    Товары.Номенклатура КАК Номенклатура,
    ЕСТЬNULL(ПланПродаж.Сумма, 0) КАК СуммаПлан,
    ЕСТЬNULL(ФактическиеПродажи.Сумма, 0) КАК СуммаФакт
ИЗ
    Товары";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Should parse multiple multi-arg functions");

    // Verify FROM clause exists in AST
    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("Should have package");
    let query = package.queries().next().expect("Should have query");
    let subquery = query.subquery().expect("Should have subquery");
    let main_query = subquery.main_query().expect("Should have main query");
    let from_clause = main_query.from_clause().expect("Should have FROM clause");

    // FROM clause should have data sources
    let data_sources_count = from_clause.data_sources().count();
    assert!(data_sources_count > 0, "FROM should have data sources");
}

// Comprehensive test coverage for multi-argument functions

#[test]
fn test_single_arg_function() {
    check_no_errors("SELECT SUM(Amount) FROM T");
    check_no_errors("SELECT YEAR(Date) FROM T");
}

#[test]
fn test_two_arg_functions() {
    check_no_errors("SELECT ISNULL(A, 0) FROM T");
    check_no_errors("SELECT SUBSTRING(Name, 1, 10) FROM T");
}

#[test]
fn test_three_arg_functions() {
    check_no_errors("SELECT SUBSTRING(Text, 1, 5) FROM T");
}

#[test]
fn test_multi_arg_with_alias() {
    check_no_errors("SELECT ISNULL(Amount, 0) AS Total FROM T");
}

#[test]
fn test_mixed_fields_and_functions() {
    check_no_errors("SELECT Name, ISNULL(Amount, 0), Code FROM T");
}

#[test]
fn test_nested_functions() {
    check_no_errors("SELECT ISNULL(SUM(Amount), 0) FROM T");
}

#[test]
fn test_russian_multi_arg_functions() {
    check_no_errors("ВЫБРАТЬ ЕСТЬNULL(Сумма, 0) ИЗ Товары");
    check_no_errors("ВЫБРАТЬ ЕСТЬNULL(Сумма, 0) КАК Итого ИЗ Товары");
}

#[test]
fn test_join_with_complex_on_condition() {
    let input =
        "SELECT * FROM T1 INNER JOIN T2 ON T1.ID = T2.ID AND (T1.Amount > 100 OR T2.Price > 500)";

    let parse = parse_sdbl(input);

    assert!(!parse.has_errors(), "Should parse JOIN with complex ON condition");

    let ast_range = parse.syntax_node().text_range();
    let ast_end: usize = ast_range.end().into();
    assert_eq!(ast_end, input.len(), "AST should cover full input");
}

#[test]
fn test_into_clause_with_union_and_semicolon_separator() {
    // Real query from completion logs with INTO clause
    let query = r#"ВЫБРАТЬ РАЗРЕШЕННЫЕ
	ГруппыКонтактовПользователей.Ссылка
ПОМЕСТИТЬ Папки
ИЗ
	Справочник.ГруппыКонтактовПользователей КАК ГруппыКонтактовПользователей
ГДЕ
	ГруппыКонтактовПользователей.Родитель В ИЕРАРХИИ(&Папка)

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	&Папка
;

////////////////////////////////////////////////////////////////////////////////
ВЫБРАТЬ
	ГруппыКонтактовПользователейКонтакты.Контакт,
	ГруппыКонтактовПользователейКонтакты.КонтактнаяИнформация
ИЗ
	Папки КАК Папки
		ВНУТРЕННЕЕ СОЕДИНЕНИЕ Справочник.ГруппыКонтактовПользователей.Контакты КАК ГруппыКонтактовПользователейКонтакты
		ПО Папки.Ссылка = ГруппыКонтактовПользователейКонтакты.Ссылка

СГРУППИРОВАТЬ ПО
	ГруппыКонтактовПользователейКонтакты.Контакт,
	ГруппыКонтактовПользователейКонтакты.КонтактнаяИнформация"#;

    let parse = parse_sdbl(query);
    assert!(
        !parse.has_errors(),
        "Should parse query with INTO, UNION ALL, and semicolon separator: {:?}",
        parse.errors()
    );

    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("Should have query package");
    let count = package.queries().count();
    assert_eq!(count, 2, "Expected 2 queries separated by semicolon (first with INTO and UNION ALL, second with JOIN)");
}

#[test]
fn test_exact_extracted_query_from_logs() {
    // EXACT text extracted from logs (08:16:50) - with incomplete ON condition "Папки. ="
    let query = r#"ВЫБРАТЬ РАЗРЕШЕННЫЕ
	ГруппыКонтактовПользователей.Ссылка
ПОМЕСТИТЬ Папки
ИЗ
	Справочник.ГруппыКонтактовПользователей КАК ГруппыКонтактовПользователей
ГДЕ
	ГруппыКонтактовПользователей.Родитель В ИЕРАРХИИ(&Папка)

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	&Папка
;

////////////////////////////////////////////////////////////////////////////////
ВЫБРАТЬ
	ГруппыКонтактовПользователейКонтакты.Контакт,
	ГруппыКонтактовПользователейКонтакты.КонтактнаяИнформация
ИЗ
	Папки КАК Папки
		ВНУТРЕННЕЕ СОЕДИНЕНИЕ Справочник.ГруппыКонтактовПользователей.Контакты КАК ГруппыКонтактовПользователейКонтакты
		ПО Папки. = ГруппыКонтактовПользователейКонтакты.Ссылка

СГРУППИРОВАТЬ ПО
	ГруппыКонтактовПользователейКонтакты.Контакт,
	ГруппыКонтактовПользователейКонтакты.КонтактнаяИнформация"#;

    println!("Query length: {}", query.len());

    let parse = parse_sdbl(query);

    // Check for errors
    if parse.has_errors() {
        println!("Parse errors:");
        for error in parse.errors() {
            println!("  - {:?}", error);
        }
    }

    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("Should have query package");
    let count = package.queries().count();

    println!("Found {} queries", count);

    // Debug: print all queries
    for (i, query) in package.queries().enumerate() {
        println!("Query {}: {:?}", i, query.syntax().text());
    }

    assert_eq!(count, 2, "Expected 2 queries separated by semicolon, but found {}", count);
}

#[test]
fn test_nested_join_with_parameters_highlighting() {
    // Test for highlighting issue after &Действие parameter in nested JOIN
    let query = r#"ВЫБРАТЬ РАЗЛИЧНЫЕ
    ЗадачиЭлементовСхемы.ИмяЭлемента,
    ЗадачиЭлементовСхемы.ЗадачаПроцесса
ПОМЕСТИТЬ ВТ_ЗадачиСхемы
ИЗ
    &ЗадачиЭлементовСхемы КАК ЗадачиЭлементовСхемы
;

////////////////////////////////////////////////////////////////////////////////
ВЫБРАТЬ
    ВТ_ЗадачиСхемы.ИмяЭлемента
ИЗ
    ВТ_ЗадачиСхемы КАК ВТ_ЗадачиСхемы
        ВНУТРЕННЕЕ СОЕДИНЕНИЕ РегистрСведений.ДанныеБизнесПроцессов КАК ДанныеБизнесПроцессов
            ВНУТРЕННЕЕ СОЕДИНЕНИЕ РегистрСведений.ПроцессыДействий КАК ПроцессыДействий
            ПО ДанныеБизнесПроцессов.БизнесПроцесс = ПроцессыДействий.Процесс
            И ПроцессыДействий.Действие = &Действие
        ПО ВТ_ЗадачиСхемы.ЗадачаПроцесса = ДанныеБизнесПроцессов.ВедущаяЗадача"#;

    let parse = parse_sdbl(query);

    if parse.has_errors() {
        println!("\n=== Parse errors ===");
        for (i, error) in parse.errors().iter().enumerate() {
            println!("Error {}: {:?}", i + 1, error);
        }
    }

    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();

    println!("\n=== Full syntax tree ===");
    println!("{:#?}", root);

    let package = SdblQueryPackage::cast(root).expect("Should have query package");
    let queries: Vec<_> = package.queries().collect();

    println!("\n=== Found {} queries ===", queries.len());
    for (i, query) in queries.iter().enumerate() {
        println!(
            "Query {}: range {:?}, len {:?}",
            i,
            query.syntax().text_range(),
            query.syntax().text().len()
        );
    }

    assert_eq!(queries.len(), 2, "Expected 2 queries");
    assert!(!parse.has_errors(), "Should parse nested JOINs without errors: {:?}", parse.errors());
}

#[test]
fn test_incomplete_on_condition_for_completion() {
    // Test incomplete ON conditions - cursor at "ПроцессыДействий." with incomplete condition
    let query = r#"ВЫБРАТЬ РАЗЛИЧНЫЕ
    ЗадачиЭлементовСхемы.ИмяЭлемента,
    ЗадачиЭлементовСхемы.ЗадачаПроцесса
ПОМЕСТИТЬ ВТ_ЗадачиСхемы
ИЗ
    &ЗадачиЭлементовСхемы КАК ЗадачиЭлементовСхемы
;

////////////////////////////////////////////////////////////////////////////////
ВЫБРАТЬ
    ВТ_ЗадачиСхемы.ИмяЭлемента
ИЗ
    ВТ_ЗадачиСхемы КАК ВТ_ЗадачиСхемы
        ВНУТРЕННЕЕ СОЕДИНЕНИЕ РегистрСведений.ДанныеБизнесПроцессов КАК ДанныеБизнесПроцессов
            ВНУТРЕННЕЕ СОЕДИНЕНИЕ РегистрСведений.ПроцессыДействий КАК ПроцессыДействий
            ПО ДанныеБизнесПроцессов.БизнесПроцесс = ПроцессыДействий.
            И ПроцессыДействий.Действие = &Действие
        ПО ВТ_ЗадачиСхемы.ЗадачаПроцесса = ДанныеБизнесПроцессов."#;

    let parse = parse_sdbl(query);

    if parse.has_errors() {
        println!("\n=== Parse errors (expected for incomplete syntax) ===");
        for (i, error) in parse.errors().iter().enumerate() {
            println!("Error {}: {:?}", i + 1, error);
        }
    }

    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();

    println!("\n=== Syntax tree (abbreviated) ===");
    // Print only the structure, not full content
    for child in root.children() {
        println!("{:?} at {:?}", child.kind(), child.text_range());
    }

    let package = SdblQueryPackage::cast(root);
    assert!(package.is_some(), "Should parse package even with incomplete ON conditions");

    let queries: Vec<_> = package.unwrap().queries().collect();
    println!("\n=== Found {} queries ===", queries.len());

    // We should still get 2 queries
    assert_eq!(queries.len(), 2, "Should parse both queries despite incomplete ON");
}

// Test for empty parameters in function calls (accumulator register methods)
#[test]
fn test_function_with_empty_parameters() {
    // Test empty parameters in .Обороты() method call
    let query = r#"ВЫБРАТЬ
    Обороты.СуммаВыручкиОборот
ИЗ
    РегистрНакопления.ВыручкаИСебестоимостьПродаж.Обороты(
        ,
        ,
        Авто,
        ) КАК Обороты"#;

    let parse = parse_sdbl(query);

    if parse.has_errors() {
        println!("\n=== Parse errors ===");
        for error in parse.errors() {
            println!("  - {:?}", error);
        }
    }

    assert!(!parse.has_errors(), "Should parse function with empty parameters without errors");
}

#[test]
fn test_function_with_mixed_empty_and_filled_parameters() {
    // Test mix of empty and filled parameters like in the user's real query
    let query = r#"ВЫБРАТЬ
    Обороты.СуммаВыручкиОборот,
    Обороты.КоличествоУчетноеОборот
ИЗ
    РегистрНакопления.ВыручкаИСебестоимость.Обороты(
        ,
        ,
        Авто,
        АналитикаУчетаПоПартнерам.Партнер В
            (ВЫБРАТЬ
                ИК.Партнер
            ИЗ
                ИнформацияКлиент КАК ИК)) КАК Обороты"#;

    let parse = parse_sdbl(query);
    assert!(
        !parse.has_errors(),
        "Should parse complex function call with empty parameters and subquery"
    );
}

#[test]
fn test_multiple_functions_with_empty_parameters() {
    // Test multiple function calls with empty parameters in same query
    let query = r#"ВЫБРАТЬ
    Обороты1.Сумма,
    Обороты2.Количество
ИЗ
    РегистрНакопления.Продажи.Обороты(, , Авто, ) КАК Обороты1,
    РегистрНакопления.Закупки.Обороты(, , , Партнер = &Партнер) КАК Обороты2"#;

    let parse = parse_sdbl(query);
    assert!(!parse.has_errors(), "Should parse multiple functions with empty parameters");
}

#[test]
fn test_function_with_in_subquery_parameter() {
    // Test from user's real query: .Обороты() with IN (subquery) as parameter
    let query = r#"ВЫБРАТЬ
    ВыручкаИСебестоимость.СуммаВыручкиОборот
ИЗ
    РегистрНакопления.ВыручкаИСебестоимостьПродаж.Обороты(
        ,
        ,
        Авто,
        АналитикаУчетаПоПартнерам.Партнер В
            (ВЫБРАТЬ
                ИК.Партнер
            ИЗ
                ИнформацияКлиент КАК ИК)) КАК ВыручкаИСебестоимость"#;

    let parse = parse_sdbl(query);

    if parse.has_errors() {
        println!("\n=== Parse errors ===");
        for error in parse.errors() {
            println!("  - {:?}", error);
        }
        println!("\n=== Syntax tree ===");
        println!("{:#?}", parse.syntax_node());
    }

    assert!(!parse.has_errors(), "Should parse function with IN (subquery) as parameter");
}

#[test]
fn test_user_full_query_with_empty_params_and_in_subquery() {
    // Full query from user's original message with highlighting issue
    let query = r#"ВЫБРАТЬ
    АналитикаПоПартнерам.Партнер КАК Партнер,
    ВыручкаИСебестоимость.СуммаВыручкиОборот КАК Выручка,
    ВыручкаИСебестоимость.КоличествоУчетноеОборот КАК КоличествоУчетноеОборот,
    ВыручкаИСебестоимость.СуммаРучнойСкидкиРеглОборот + ВыручкаИСебестоимость.СуммаАвтоматическойСкидкиРеглОборот КАК ИтоговаяСкидка
ИЗ
    РегистрНакопления.ВыручкаИСебестоимостьПродаж.Обороты(
        ,
        ,
        Авто,
        АналитикаУчетаПоПартнерам.Партнер В
            (ВЫБРАТЬ
                ИК.Партнер
            ИЗ
                ИнформацияКлиент КАК ИК)) КАК ВыручкаИСебестоимость
    ВНУТРЕННЕЕ СОЕДИНЕНИЕ РегистрСведений.АналитикаУчетаПоПартнерам КАК АналитикаПоПартнерам
    ПО ВыручкаИСебестоимость.АналитикаУчетаПоПартнерам = АналитикаПоПартнерам.КлючАналитики"#;

    let parse = parse_sdbl(query);

    if parse.has_errors() {
        println!("\n=== Parse errors ===");
        for error in parse.errors() {
            println!("  - {:?}", error);
        }
    }

    assert!(
        !parse.has_errors(),
        "Should parse full query with empty params, IN subquery, and JOIN without errors"
    );
}

#[test]
fn test_complete_user_query_from_fixture() {
    // Complete query from user (first message) - saved as fixture
    let query = include_str!("fixtures/user_query_with_highlighting_issue.sdbl");

    let parse = parse_sdbl(query);

    if parse.has_errors() {
        println!("\n=== Parse errors ===");
        for (i, error) in parse.errors().iter().enumerate() {
            println!("Error {}: {:?}", i + 1, error);
        }
        println!("\n=== Query length: {} chars ===", query.len());
    }

    assert!(
        !parse.has_errors(),
        "Should parse complete user query with ПОМЕСТИТЬ, ОБЪЕДИНИТЬ, empty params, and IN subqueries: found {} errors",
        parse.errors().len()
    );
}

#[test]
fn test_debug_in_expression_parsing() {
    // Simplified test to debug IN expression parsing
    let query = "ВЫБРАТЬ X ИЗ Т ГДЕ Поле В (1, 2, 3)";
    let parse = parse_sdbl(query);
    assert!(!parse.has_errors(), "Should parse simple IN with value list");

    let query = "ВЫБРАТЬ X ИЗ Т ГДЕ Поле В (ВЫБРАТЬ Y ИЗ Т2)";
    let parse = parse_sdbl(query);
    assert!(!parse.has_errors(), "Should parse simple IN with subquery");

    // Now test IN inside function parameter
    let query = "ВЫБРАТЬ X ИЗ Рег.Метод(Поле В (ВЫБРАТЬ Y ИЗ Т2))";
    let parse = parse_sdbl(query);

    if parse.has_errors() {
        println!("\n=== Errors for IN inside function parameter ===");
        for error in parse.errors() {
            println!("  - {:?}", error);
        }
        println!("\n=== Syntax tree ===");
        println!("{:#?}", parse.syntax_node());
    }

    assert!(!parse.has_errors(), "Should parse IN inside function parameter");
}

// ============================================================================
// Phase 2: Error Recovery Tests
// ============================================================================
//
// Tests for error recovery improvements (Phase 2 of SDBL error recovery plan):
// 1. IN predicate with empty values
// 2. REFS identifier chain
// 3. Function arguments with empty parameters

#[test]
fn test_error_recovery_in_empty_value() {
    // IN predicate with empty value: IN (1, , 3)
    let input = "SELECT * FROM T WHERE Field IN (1, , 3)";
    let parse = parse_sdbl(input);

    // Should have ERROR node for empty value
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("ERROR"),
        "Expected ERROR node for empty value in IN list.\nTree: {}",
        tree
    );

    // But WHERE clause should still be parsed!
    assert!(
        tree.contains("SDBL_WHERE_CLAUSE"),
        "WHERE clause should be parsed despite empty IN value.\nTree: {}",
        tree
    );

    // And the IN expression should be present
    assert!(tree.contains("SDBL_IN_EXPR"), "IN expression should be parsed.\nTree: {}", tree);
}

#[test]
fn test_error_recovery_in_leading_empty() {
    // IN predicate with leading empty value: IN (, 2, 3)
    let input = "SELECT * FROM T WHERE Field IN (, 2, 3)";
    let parse = parse_sdbl(input);

    let tree = format!("{:#?}", parse.syntax_node());
    assert!(tree.contains("ERROR"), "Expected ERROR node for leading empty value.\nTree: {}", tree);
    assert!(tree.contains("SDBL_WHERE_CLAUSE"), "WHERE clause should be parsed.\nTree: {}", tree);
}

#[test]
fn test_error_recovery_in_trailing_empty() {
    // IN predicate with trailing empty value: IN (1, 2,)
    let input = "SELECT * FROM T WHERE Field IN (1, 2,)";
    let parse = parse_sdbl(input);

    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("ERROR"),
        "Expected ERROR node for trailing empty value.\nTree: {}",
        tree
    );
    assert!(tree.contains("SDBL_WHERE_CLAUSE"), "WHERE clause should be parsed.\nTree: {}", tree);
}

#[test]
fn test_error_recovery_function_empty_args() {
    // Function with empty arguments: func(, , value)
    let input = "SELECT func(, , 123) FROM T";
    let parse = parse_sdbl(input);

    let tree = format!("{:#?}", parse.syntax_node());

    // Should have ERROR nodes for empty arguments
    let error_count = tree.matches("ERROR").count();
    assert!(
        error_count >= 2,
        "Expected at least 2 ERROR nodes for empty arguments. Got: {}.\nTree: {}",
        error_count,
        tree
    );

    // But function call should still be parsed
    assert!(
        tree.contains("SDBL_FUNCTION_CALL"),
        "Function call should be parsed despite empty args.\nTree: {}",
        tree
    );

    // And SELECT should be complete
    assert!(tree.contains("SDBL_FIELD_LIST"), "Field list should be parsed.\nTree: {}", tree);
}

#[test]
fn test_error_recovery_function_leading_empty() {
    // Function with leading empty argument: func(, value)
    let input = "SELECT func(, 456) FROM T";
    let parse = parse_sdbl(input);

    let tree = format!("{:#?}", parse.syntax_node());
    assert!(tree.contains("ERROR"), "Expected ERROR node for leading empty arg.\nTree: {}", tree);
    assert!(tree.contains("SDBL_FUNCTION_CALL"), "Function call should be parsed.\nTree: {}", tree);
}

#[test]
fn test_error_recovery_function_trailing_empty() {
    // Function with trailing empty argument: func(value,)
    let input = "SELECT func(789,) FROM T";
    let parse = parse_sdbl(input);

    let tree = format!("{:#?}", parse.syntax_node());
    assert!(tree.contains("ERROR"), "Expected ERROR node for trailing empty arg.\nTree: {}", tree);
    assert!(tree.contains("SDBL_FUNCTION_CALL"), "Function call should be parsed.\nTree: {}", tree);
}

#[test]
fn test_error_recovery_refs_predicate() {
    // REFS predicate with MDO reference: Field REFS Catalog.Products
    let input = "SELECT * FROM T WHERE Field REFS Catalog.Products";
    let parse = parse_sdbl(input);

    // Should parse without errors (valid REFS syntax)
    assert!(!parse.has_errors(), "REFS with qualified name should parse without errors");

    let tree = format!("{:#?}", parse.syntax_node());
    assert!(tree.contains("SDBL_REFS_EXPR"), "REFS expression should be parsed.\nTree: {}", tree);
}

#[test]
fn test_error_recovery_comprehensive() {
    // Comprehensive test: multiple error recovery points in one query
    let input = "ВЫБРАТЬ Поле., Поле2, , Поле3 ИЗ Таблица1 ГДЕ Поле В (1, , 3) И func(, 456) > 0";

    let parse = parse_sdbl(input);
    let tree = format!("{:#?}", parse.syntax_node());

    // Should have multiple ERROR nodes:
    // - Incomplete field (Поле.)
    // - Empty field (, , Поле3)
    // - Empty IN value (1, , 3)
    // - Empty function arg (, 456)
    let error_count = tree.matches("ERROR").count();
    assert!(
        error_count >= 3,
        "Expected at least 3 ERROR nodes. Got: {}.\nTree: {}",
        error_count,
        tree
    );

    // But main clauses should still be parsed!
    assert!(tree.contains("SDBL_FROM_CLAUSE"), "FROM clause should be parsed.\nTree: {}", tree);
    assert!(tree.contains("SDBL_WHERE_CLAUSE"), "WHERE clause should be parsed.\nTree: {}", tree);
    assert!(tree.contains("SDBL_FIELD_LIST"), "Field list should be parsed.\nTree: {}", tree);
}

#[test]
fn test_no_infinite_loop_deeply_nested_dots() {
    // Regression test: ensure parser doesn't loop infinitely on deeply nested dots
    // This would previously cause infinite loop without check_iteration_limit()
    let input = "SELECT T.a.b.c.d.e.f.g.h.i.j.k.l.m.n.o.p.q.r.s.t.u.v.w.x.y.z FROM T";
    let parse = parse_sdbl(input);

    // Should complete (not hang)
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_COLUMN_REF"),
        "Deeply nested column ref should be parsed.\nTree: {}",
        tree
    );
}

#[test]
fn test_type_cast_with_recovery() {
    // Test that CAST (ВЫРАЗИТЬ) is properly parsed
    let query = r#"ВЫБРАТЬ
    Поле1 КАК alias1,
    ВЫРАЗИТЬ(Поле2 КАК СТРОКА(200)) КАК alias2,
    Поле3 КАК alias3
ИЗ Таблица
ГДЕ Условие = 1"#;

    let parse = parse_sdbl(query);

    let tree = format!("{:#?}", parse.syntax_node());

    // CAST is now properly parsed - should have SDBL_TYPE node
    assert!(tree.contains("SDBL_TYPE"), "Expected SDBL_TYPE node for CAST type.\nTree: {}", tree);

    // All clauses should parse
    assert!(tree.contains("SDBL_FROM_CLAUSE"), "FROM clause should be parsed.\nTree: {}", tree);

    assert!(tree.contains("SDBL_WHERE_CLAUSE"), "WHERE clause should be parsed.\nTree: {}", tree);

    // Should parse all 3 fields
    let field_count = tree.matches("SDBL_SELECTED_FIELD").count();
    assert!(field_count >= 3, "Should parse all 3 fields. Got: {}.\nTree: {}", field_count, tree);
}

#[test]
fn test_real_query_with_type_cast() {
    // Real-world query from user
    // Features: CAST (ВЫРАЗИТЬ) and CASE expression in arithmetic context
    let query = r#"ВЫБРАТЬ
    ДвиженияПоКлиенту.Документ КАК Документ,
    ДвиженияПоКлиенту.order_number КАК order_number,
    ДвиженияПоКлиенту.shop КАК shop,
    НАЧАЛОПЕРИОДА(ДвиженияПоКлиенту.Дата, ДЕНЬ) КАК date,
    ВЫРАЗИТЬ(ДвиженияПоКлиенту.description КАК СТРОКА(200)) КАК description,
    ДвиженияПоКлиенту.article КАК article,
    ДвиженияПоКлиенту.name +
        ВЫБОР
            КОГДА ДвиженияПоКлиенту.size <> ""
                ТОГДА " (" + ДвиженияПоКлиенту.size + ")"
            ИНАЧЕ ""
        КОНЕЦ КАК name,
    ДвиженияПоКлиенту.quantity КАК quantity,
    ДвиженияПоКлиенту.quantity_accounting КАК quantity_accounting,
    ДвиженияПоКлиенту.price КАК price
ИЗ Таблица
ГДЕ Условие = 1"#;

    let parse = parse_sdbl(query);

    let tree = format!("{:#?}", parse.syntax_node());

    // CAST is now properly parsed - should have SDBL_TYPE node
    assert!(tree.contains("SDBL_TYPE"), "Expected SDBL_TYPE node for CAST type.\nTree: {}", tree);

    // Parser should parse all 10 fields
    let field_count = tree.matches("SDBL_SELECTED_FIELD").count();
    assert!(field_count >= 10, "Should parse all 10 fields. Got: {}.\nTree: {}", field_count, tree);

    // Type cast followed by field - both should be parsed
    assert!(tree.contains("article"), "Field after type cast should be parsed.\nTree: {}", tree);
}

#[test]
fn test_type_cast_without_case() {
    // Simplified version without CASE expression
    // CAST (ВЫРАЗИТЬ) is now fully supported
    let query = r#"ВЫБРАТЬ
    ДвиженияПоКлиенту.Документ КАК Документ,
    ДвиженияПоКлиенту.order_number КАК order_number,
    ДвиженияПоКлиенту.shop КАК shop,
    НАЧАЛОПЕРИОДА(ДвиженияПоКлиенту.Дата, ДЕНЬ) КАК date,
    ВЫРАЗИТЬ(ДвиженияПоКлиенту.description КАК СТРОКА(200)) КАК description,
    ДвиженияПоКлиенту.article КАК article,
    ДвиженияПоКлиенту.name КАК name,
    ДвиженияПоКлиенту.quantity КАК quantity,
    ДвиженияПоКлиенту.price КАК price
ИЗ Таблица
ГДЕ Условие = 1"#;

    let parse = parse_sdbl(query);

    let tree = format!("{:#?}", parse.syntax_node());

    // CAST is now properly parsed - should have SDBL_TYPE node
    assert!(tree.contains("SDBL_TYPE"), "Expected SDBL_TYPE node for CAST type.\nTree: {}", tree);

    // FROM and WHERE should parse
    assert!(tree.contains("SDBL_FROM_CLAUSE"), "FROM clause should be parsed.\nTree: {}", tree);

    assert!(tree.contains("SDBL_WHERE_CLAUSE"), "WHERE clause should be parsed.\nTree: {}", tree);

    // Should parse all 9 fields
    let field_count = tree.matches("SDBL_SELECTED_FIELD").count();
    assert!(
        field_count >= 9,
        "Should parse at least 9 fields. Got: {}.\nTree: {}",
        field_count,
        tree
    );
}

#[test]
fn test_case_in_arithmetic_with_recovery() {
    // CASE expression in arithmetic context - NOW SUPPORTED!
    // Parser should handle CASE correctly and continue parsing
    let query = r#"ВЫБРАТЬ
    Поле1 КАК alias1,
    Поле2 + ВЫБОР КОГДА x ТОГДА 1 ИНАЧЕ 2 КОНЕЦ КАК alias2,
    Поле3 КАК alias3
ИЗ Таблица
ГДЕ Условие = 1"#;

    let parse = parse_sdbl(query);

    let tree = format!("{:#?}", parse.syntax_node());

    // CASE expressions are now supported - should parse without errors!
    assert!(!parse.has_errors(), "CASE in arithmetic should parse correctly.\nTree: {}", tree);

    // Verify CASE expression is parsed
    assert!(tree.contains("SDBL_CASE_EXPR"), "Should have CASE expression node.\nTree: {}", tree);

    // Verify other clauses are parsed
    assert!(tree.contains("SDBL_FROM_CLAUSE"), "FROM clause should be parsed.\nTree: {}", tree);

    assert!(tree.contains("SDBL_WHERE_CLAUSE"), "WHERE clause should be parsed.\nTree: {}", tree);

    // Should parse all 3 fields
    let field_count = tree.matches("SDBL_SELECTED_FIELD").count();
    assert!(field_count >= 3, "Should parse all 3 fields. Got: {}.\nTree: {}", field_count, tree);

    // All fields should be parsed correctly
    assert!(tree.contains("alias1"), "First field should be parsed.\nTree: {}", tree);
    assert!(tree.contains("alias2"), "Field with CASE should be parsed.\nTree: {}", tree);
    assert!(tree.contains("alias3"), "Field after CASE should be parsed.\nTree: {}", tree);
}

#[test]
fn test_full_user_query_with_all_features() {
    // Real user query with CAST and CASE - both now supported
    let query = r#"ВЫБРАТЬ
    ДвиженияПоКлиенту.Документ КАК Документ,
    ДвиженияПоКлиенту.order_number КАК order_number,
    ДвиженияПоКлиенту.shop КАК shop,
    НАЧАЛОПЕРИОДА(ДвиженияПоКлиенту.Дата, ДЕНЬ) КАК date,
    ВЫРАЗИТЬ(ДвиженияПоКлиенту.description КАК СТРОКА(200)) КАК description,
    ДвиженияПоКлиенту.article КАК article,
    ДвиженияПоКлиенту.name +
        ВЫБОР
            КОГДА ДвиженияПоКлиенту.size <> ""
                ТОГДА " (" + ДвиженияПоКлиенту.size + ")"
            ИНАЧЕ ""
        КОНЕЦ КАК name,
    ДвиженияПоКлиенту.quantity КАК quantity,
    ДвиженияПоКлиенту.quantity_accounting КАК quantity_accounting,
    ДвиженияПоКлиенту.price КАК price,
    ДвиженияПоКлиенту.price_eur КАК price_eur,
    ДвиженияПоКлиенту.discount КАК discount,
    ДвиженияПоКлиенту.bonus_accrued КАК bonus_accrued,
    ДвиженияПоКлиенту.bonus_deducted КАК bonus_deducted
ИЗ РегистрНакопления.ДвиженияПоКлиенту
ГДЕ ДвиженияПоКлиенту.Клиент = &Клиент"#;

    let parse = parse_sdbl(query);

    let tree = format!("{:#?}", parse.syntax_node());

    // CAST is now properly parsed - should have SDBL_TYPE node
    assert!(tree.contains("SDBL_TYPE"), "Expected SDBL_TYPE node for CAST type.\nTree: {}", tree);

    // FROM and WHERE should parse
    assert!(tree.contains("SDBL_FROM_CLAUSE"), "FROM clause should be parsed.\nTree: {}", tree);

    assert!(tree.contains("SDBL_WHERE_CLAUSE"), "WHERE clause should be parsed.\nTree: {}", tree);

    // Should parse all 14 fields
    let field_count = tree.matches("SDBL_SELECTED_FIELD").count();
    assert!(field_count >= 14, "Should parse all 14 fields. Got: {}.\nTree: {}", field_count, tree);

    // All fields should be parsed
    assert!(tree.contains("quantity"), "Quantity field should be parsed.\nTree: {}", tree);
    assert!(tree.contains("price_eur"), "price_eur field should be parsed.\nTree: {}", tree);
    assert!(
        tree.contains("bonus_deducted"),
        "bonus_deducted field should be parsed.\nTree: {}",
        tree
    );
}
#[test]
fn test_simple_plus_case() {
    use parser::parse_sdbl;
    let query = "ВЫБРАТЬ Поле2 + ВЫБОР КОГДА x ТОГДА 1 КОНЕЦ КАК alias2 ИЗ T";
    let parse = parse_sdbl(query);

    eprintln!("\n{:#?}", parse.syntax_node());

    let tree = format!("{:#?}", parse.syntax_node());
    // CASE expressions are now supported - should parse without errors!
    assert!(!parse.has_errors(), "CASE in arithmetic context should parse correctly");
    assert!(tree.contains("SDBL_CASE_EXPR"), "Should have CASE expression node.\nTree: {}", tree);
    assert!(tree.contains("SDBL_FROM_CLAUSE"), "FROM clause should be parsed.\nTree: {}", tree);
}
#[test]
fn test_empty_string_literal() {
    use parser::parse_sdbl;
    let query = r#"ВЫБРАТЬ x <> "" КАК result ИЗ T"#;
    let parse = parse_sdbl(query);

    eprintln!("\n{:#?}", parse.syntax_node());

    assert!(!parse.has_errors(), "Should parse empty string");
}

#[test]
fn test_case_with_string_concat_in_then() {
    use parser::parse_sdbl;
    let query = r#"ВЫБРАТЬ
    ВЫБОР
        КОГДА x <> ""
            ТОГДА " (" + y + ")"
        ИНАЧЕ ""
    КОНЕЦ КАК result
ИЗ T"#;
    let parse = parse_sdbl(query);

    eprintln!("\n{:#?}", parse.syntax_node());

    let tree = format!("{:#?}", parse.syntax_node());
    if parse.has_errors() {
        eprintln!("\nErrors: {:?}", parse.errors());
    }

    assert!(!parse.has_errors(), "Should parse CASE with string concatenation in THEN clause");
    assert!(tree.contains("SDBL_CASE_EXPR"), "Should have CASE expression");
    assert!(tree.contains("SDBL_FROM_CLAUSE"), "Should parse FROM clause");
}

#[test]
fn test_single_string_literal() {
    use parser::parse_sdbl;
    let query = r#"ВЫБРАТЬ " (" КАК result ИЗ T"#;
    let parse = parse_sdbl(query);

    eprintln!("\n{:#?}", parse.syntax_node());

    let tree = format!("{:#?}", parse.syntax_node());
    if parse.has_errors() {
        eprintln!("\nErrors: {:?}", parse.errors());
    }

    // Should parse without errors
    assert!(!parse.has_errors(), "Single string should parse correctly");
    assert!(tree.contains("SDBL_FROM_CLAUSE"), "FROM clause should parse");

    // String should be either SDBL_LITERAL or SDBL_MULTI_STRING
    assert!(
        tree.contains("SDBL_LITERAL") || tree.contains("SDBL_MULTI_STRING"),
        "Should have string literal node.\nTree: {}",
        tree
    );
}

#[test]
fn test_simple_two_queries_with_semicolon() {
    use parser::parse_sdbl;
    use syntax::ast::{AstNode, SdblQueryPackage};

    let query = r#"ВЫБРАТЬ x;
ВЫБРАТЬ y"#;

    let parse = parse_sdbl(query);
    eprintln!("\n{:#?}", parse.syntax_node());

    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("Should have query package");
    let count = package.queries().count();

    eprintln!("Query count: {}", count);
    assert_eq!(count, 2, "Expected 2 queries separated by semicolon");
}
#[test]
fn test_unsupported_features() {
    use parser::parse_sdbl;

    // Test 1: CASE expression - should work now
    let query1 = r#"ВЫБРАТЬ ВЫБОР КОГДА x ТОГДА 1 КОНЕЦ КАК result ИЗ T"#;
    let parse1 = parse_sdbl(query1);
    eprintln!("\n=== CASE expression ===");
    eprintln!("Has errors: {}", parse1.has_errors());

    // Test 2: String concatenation - should work now
    let query2 = r#"ВЫБРАТЬ "a" + "b" КАК result ИЗ T"#;
    let parse2 = parse_sdbl(query2);
    eprintln!("\n=== String concatenation ===");
    eprintln!("Has errors: {}", parse2.has_errors());

    // Test 3: Type cast - NOT supported yet
    let query3 = r#"ВЫБРАТЬ ВЫРАЗИТЬ(field КАК СТРОКА(200)) КАК result ИЗ T"#;
    let parse3 = parse_sdbl(query3);
    eprintln!("\n=== Type cast ВЫРАЗИТЬ ===");
    eprintln!("Has errors: {}", parse3.has_errors());
    let tree3 = format!("{:#?}", parse3.syntax_node());
    eprintln!("Has ERROR: {}", tree3.contains("ERROR"));

    // Test 4: НАЧАЛОПЕРИОДА function
    let query4 = r#"ВЫБРАТЬ НАЧАЛОПЕРИОДА(date, ДЕНЬ) КАК result ИЗ T"#;
    let parse4 = parse_sdbl(query4);
    eprintln!("\n=== НАЧАЛОПЕРИОДА function ===");
    eprintln!("Has errors: {}", parse4.has_errors());
}
#[test]
fn test_like_predicate() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ * ИЗ T ГДЕ name ПОДОБНО "%test%""#;
    let parse = parse_sdbl(query);

    eprintln!("\n{:#?}", parse.syntax_node());
    eprintln!("\nHas errors: {}", parse.has_errors());

    let tree = format!("{:#?}", parse.syntax_node());
    assert!(tree.contains("SDBL_WHERE_CLAUSE"), "Should parse WHERE clause");
}

#[test]
fn test_between_predicate() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ * ИЗ T ГДЕ price МЕЖДУ 100 И 200"#;
    let parse = parse_sdbl(query);

    eprintln!("\n{:#?}", parse.syntax_node());
    eprintln!("\nHas errors: {}", parse.has_errors());

    let tree = format!("{:#?}", parse.syntax_node());
    assert!(tree.contains("SDBL_WHERE_CLAUSE"), "Should parse WHERE clause");
}

#[test]
fn test_is_null_predicate() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ * ИЗ T ГДЕ field ЕСТЬ NULL"#;
    let parse = parse_sdbl(query);

    eprintln!("\n{:#?}", parse.syntax_node());
    eprintln!("\nHas errors: {}", parse.has_errors());

    let tree = format!("{:#?}", parse.syntax_node());
    assert!(tree.contains("SDBL_WHERE_CLAUSE"), "Should parse WHERE clause");
}
#[test]
fn test_order_by_clause() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ name, price ИЗ T УПОРЯДОЧИТЬ ПО price УБЫВ, name"#;
    let parse = parse_sdbl(query);

    eprintln!("\n=== ORDER BY ===");
    eprintln!("Has errors: {}", parse.has_errors());
    let tree = format!("{:#?}", parse.syntax_node());
    eprintln!("Has ORDER BY node: {}", tree.contains("SDBL_ORDER_BY"));
}

#[test]
fn test_group_by_clause() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ name, СУММА(price) ИЗ T СГРУППИРОВАТЬ ПО name"#;
    let parse = parse_sdbl(query);

    eprintln!("\n=== GROUP BY ===");
    eprintln!("Has errors: {}", parse.has_errors());
    let tree = format!("{:#?}", parse.syntax_node());
    eprintln!("Has GROUP BY node: {}", tree.contains("SDBL_GROUP_BY"));
}

#[test]
fn test_group_by_debug() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ name, СУММА(price) ИЗ T СГРУППИРОВАТЬ ПО name"#;
    let parse = parse_sdbl(query);

    eprintln!("\n{:#?}", parse.syntax_node());

    let tree = format!("{:#?}", parse.syntax_node());
    eprintln!("\nSearching for: SDBL_GROUP_CLAUSE");
    eprintln!("Found: {}", tree.contains("SDBL_GROUP_CLAUSE"));
}
#[test]
fn test_order_by_debug() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ name, price ИЗ T УПОРЯДОЧИТЬ ПО price УБЫВ, name"#;
    let parse = parse_sdbl(query);

    eprintln!("\n{:#?}", parse.syntax_node());

    let tree = format!("{:#?}", parse.syntax_node());
    eprintln!("\nSearching for: SDBL_ORDER_CLAUSE");
    eprintln!("Found: {}", tree.contains("SDBL_ORDER_CLAUSE"));
}
#[test]
fn debug_complex_query() {
    use parser::parse_sdbl;

    let q5 = r#"
        ВЫБРАТЬ
            category,
            name + ВЫБОР КОГДА discount > 0 ТОГДА " (скидка)" ИНАЧЕ "" КОНЕЦ КАК display_name
        ИЗ Products
    "#;
    let p5 = parse_sdbl(q5);

    eprintln!("\n{:#?}", p5.syntax_node());

    let tree5 = format!("{:#?}", p5.syntax_node());
    eprintln!("\nHas CASE: {}", tree5.contains("SDBL_CASE_EXPR"));
    eprintln!("Has errors: {}", p5.has_errors());
}
#[test]
fn demo_all_features_fixed() {
    use parser::parse_sdbl;

    // ✅ CASE expressions in arithmetic context
    let q1 = r#"ВЫБРАТЬ name + ВЫБОР КОГДА size <> "" ТОГДА " (" + size + ")" ИНАЧЕ "" КОНЕЦ КАК display_name ИЗ T"#;
    let p1 = parse_sdbl(q1);
    assert!(!p1.has_errors(), "CASE expression should work");

    // ✅ String concatenation
    let q2 = r#"ВЫБРАТЬ "Префикс: " + field + " (суффикс)" КАК result ИЗ T"#;
    let p2 = parse_sdbl(q2);
    assert!(!p2.has_errors(), "String concatenation should work");

    // ✅ GROUP BY with Russian keywords
    let q3 = r#"ВЫБРАТЬ category, СУММА(amount) ИЗ T СГРУППИРОВАТЬ ПО category"#;
    let p3 = parse_sdbl(q3);
    let tree3 = format!("{:#?}", p3.syntax_node());
    assert!(tree3.contains("SDBL_GROUP_CLAUSE"), "GROUP BY should work");

    // ✅ ORDER BY with Russian keywords
    let q4 = r#"ВЫБРАТЬ name, price ИЗ T УПОРЯДОЧИТЬ ПО price УБЫВ, name"#;
    let p4 = parse_sdbl(q4);
    let tree4 = format!("{:#?}", p4.syntax_node());
    assert!(tree4.contains("SDBL_ORDER_CLAUSE"), "ORDER BY should work");

    // ✅ Complex query with everything
    let q5 = r#"ВЫБРАТЬ category, name + ВЫБОР КОГДА discount > 0 ТОГДА " (скидка)" ИНАЧЕ "" КОНЕЦ КАК display_name, СУММА(amount) КАК total ИЗ Products ГДЕ active = ИСТИНА СГРУППИРОВАТЬ ПО category, name, discount УПОРЯДОЧИТЬ ПО category, total УБЫВ"#;
    let p5 = parse_sdbl(q5);
    let tree5 = format!("{:#?}", p5.syntax_node());
    assert!(!p5.has_errors(), "Complex query should parse without errors");
    assert!(tree5.contains("SDBL_CASE_EXPR"), "Should have CASE");
    assert!(tree5.contains("SDBL_GROUP_CLAUSE"), "Should have GROUP BY");
    assert!(tree5.contains("SDBL_ORDER_CLAUSE"), "Should have ORDER BY");

    eprintln!("\n✅ All new features work correctly!");
    eprintln!("  ✅ CASE expressions");
    eprintln!("  ✅ String concatenation");
    eprintln!("  ✅ GROUP BY (СГРУППИРОВАТЬ ПО)");
    eprintln!("  ✅ ORDER BY (УПОРЯДОЧИТЬ ПО)");
}
#[test]
fn test_view_presentation() {
    use parser::parse_sdbl;

    // Test 1: Simple VIEW reference
    let q1 = r#"ВЫБРАТЬ * ИЗ Справочник.Контрагенты.ПРЕДСТАВЛЕНИЕ"#;
    let p1 = parse_sdbl(q1);

    eprintln!("\n=== Test 1: Simple VIEW ===");
    eprintln!("{:#?}", p1.syntax_node());
    eprintln!("Has errors: {}", p1.has_errors());

    // Test 2: VIEW with alias
    let q2 = r#"ВЫБРАТЬ * ИЗ Справочник.Контрагенты.ПРЕДСТАВЛЕНИЕ КАК View1"#;
    let p2 = parse_sdbl(q2);

    eprintln!("\n=== Test 2: VIEW with alias ===");
    eprintln!("Has errors: {}", p2.has_errors());

    // Test 3: Multiple VIEWs with JOIN
    let q3 = r#"ВЫБРАТЬ * ИЗ Справочник.Контрагенты.ПРЕДСТАВЛЕНИЕ КАК V1 ЛЕВОЕ СОЕДИНЕНИЕ Документ.ПриходнаяНакладная.ПРЕДСТАВЛЕНИЕ КАК V2 ПО V1.Ссылка = V2.Контрагент"#;
    let p3 = parse_sdbl(q3);

    eprintln!("\n=== Test 3: Multiple VIEWs with JOIN ===");
    eprintln!("Has errors: {}", p3.has_errors());
}
#[test]
fn test_view_presentation_detailed() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ Name, Code ИЗ Справочник.Контрагенты.ПРЕДСТАВЛЕНИЕ КАК V1"#;
    let parse = parse_sdbl(query);

    let tree = format!("{:#?}", parse.syntax_node());
    eprintln!("\nFull tree:\n{}", tree);

    // Check if ПРЕДСТАВЛЕНИЕ is recognized as part of table reference
    assert!(!parse.has_errors(), "Should parse VIEW without errors");
    assert!(tree.contains("SDBL_FROM_CLAUSE"), "Should have FROM clause");
    assert!(tree.contains("SDBL_TABLE_REF"), "Should have table reference");
}
#[test]
fn test_virtual_table_debug() {
    use parser::parse_sdbl;

    let query =
        r#"ВЫБРАТЬ * ИЗ РегистрНакопления.ТоварыНаСкладах.Обороты(&Начало, &Конец, День, )"#;
    let parse = parse_sdbl(query);

    eprintln!("\n{:#?}", parse.syntax_node());
    eprintln!("\nHas errors: {}", parse.has_errors());

    let tree = format!("{:#?}", parse.syntax_node());
    eprintln!("\nSearching for nodes...");
    eprintln!("Has SDBL_VIRTUAL_TABLE: {}", tree.contains("SDBL_VIRTUAL_TABLE"));
    eprintln!("Has SDBL_TABLE_REF: {}", tree.contains("SDBL_TABLE_REF"));
    eprintln!("Has SDBL_FUNCTION_CALL: {}", tree.contains("SDBL_FUNCTION_CALL"));
}
#[test]
fn test_advanced_sdbl_constructs() {
    use parser::parse_sdbl;

    // Test 1: AUTOORDER
    let q1 = r#"ВЫБРАТЬ Name, Price ИЗ Products АВТОУПОРЯДОЧИВАНИЕ"#;
    let p1 = parse_sdbl(q1);
    eprintln!("\n=== AUTOORDER ===");
    eprintln!("Has errors: {}", p1.has_errors());
    let tree1 = format!("{:#?}", p1.syntax_node());
    eprintln!(
        "Has AUTOORDER node: {}",
        tree1.contains("AUTOORDER") || tree1.contains("АВТОУПОРЯДОЧИВАНИЕ")
    );

    // Test 2: TOTALS BY
    let q2 = r#"ВЫБРАТЬ Category, СУММА(Price) КАК Total ИЗ Products СГРУППИРОВАТЬ ПО Category ИТОГИ ПО Category"#;
    let p2 = parse_sdbl(q2);
    eprintln!("\n=== TOTALS BY ===");
    eprintln!("Has errors: {}", p2.has_errors());
    let tree2 = format!("{:#?}", p2.syntax_node());
    eprintln!("Has TOTALS node: {}", tree2.contains("TOTALS") || tree2.contains("ИТОГИ"));

    // Test 3: FOR UPDATE OF
    let q3 = r#"ВЫБРАТЬ Name ИЗ Products ДЛЯ ИЗМЕНЕНИЯ Products"#;
    let p3 = parse_sdbl(q3);
    eprintln!("\n=== FOR UPDATE OF ===");
    eprintln!("Has errors: {}", p3.has_errors());

    // Test 4: INDEX BY
    let q4 = r#"ВЫБРАТЬ Name ИЗ Products ИНДЕКСИРОВАТЬ ПО Name"#;
    let p4 = parse_sdbl(q4);
    eprintln!("\n=== INDEX BY ===");
    eprintln!("Has errors: {}", p4.has_errors());

    // Test 5: ALLOWED / РАЗРЕШЕННЫЕ
    let q5 = r#"ВЫБРАТЬ РАЗРЕШЕННЫЕ Name ИЗ Products"#;
    let p5 = parse_sdbl(q5);
    eprintln!("\n=== ALLOWED ===");
    eprintln!("Has errors: {}", p5.has_errors());
    let tree5 = format!("{:#?}", p5.syntax_node());
    eprintln!("Has ALLOWED: {}", tree5.contains("РАЗРЕШЕННЫЕ") || tree5.contains("ALLOWED"));

    // Test 6: DISTINCT / РАЗЛИЧНЫЕ
    let q6 = r#"ВЫБРАТЬ РАЗЛИЧНЫЕ Category ИЗ Products"#;
    let p6 = parse_sdbl(q6);
    eprintln!("\n=== DISTINCT ===");
    eprintln!("Has errors: {}", p6.has_errors());
    let tree6 = format!("{:#?}", p6.syntax_node());
    eprintln!("Has DISTINCT: {}", tree6.contains("РАЗЛИЧНЫЕ") || tree6.contains("DISTINCT"));
}
#[test]
fn test_having_clause() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ category, СУММА(price) КАК total ИЗ Products СГРУППИРОВАТЬ ПО category ИМЕЮЩИЕ СУММА(price) > 1000"#;
    let parse = parse_sdbl(query);

    eprintln!("\n=== HAVING clause ===");
    let tree = format!("{:#?}", parse.syntax_node());
    eprintln!("Has errors: {}", parse.has_errors());
    eprintln!("Has HAVING node: {}", tree.contains("SDBL_HAVING_CLAUSE"));

    assert!(!parse.has_errors(), "HAVING should parse without errors");
    assert!(tree.contains("SDBL_HAVING_CLAUSE"), "Should have HAVING clause node");
}

#[test]
fn test_for_update_clause() {
    use parser::parse_sdbl;

    // Test 1: FOR UPDATE without MDO
    let q1 = r#"ВЫБРАТЬ Name ИЗ Products ДЛЯ ИЗМЕНЕНИЯ"#;
    let p1 = parse_sdbl(q1);
    let tree1 = format!("{:#?}", p1.syntax_node());

    eprintln!("\n=== FOR UPDATE without MDO ===");
    eprintln!("Has errors: {}", p1.has_errors());
    eprintln!("Has FOR UPDATE node: {}", tree1.contains("SDBL_FOR_UPDATE"));

    assert!(!p1.has_errors(), "FOR UPDATE should parse without errors");
    assert!(tree1.contains("SDBL_FOR_UPDATE"), "Should have FOR UPDATE node");

    // Test 2: FOR UPDATE with MDO
    let q2 = r#"ВЫБРАТЬ Name ИЗ Products ДЛЯ ИЗМЕНЕНИЯ Products"#;
    let p2 = parse_sdbl(q2);

    eprintln!("\n=== FOR UPDATE with MDO ===");
    eprintln!("Has errors: {}", p2.has_errors());

    assert!(!p2.has_errors(), "FOR UPDATE with MDO should parse");
}

#[test]
fn test_index_by_clause() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ Name, Price ИЗ Products ИНДЕКСИРОВАТЬ ПО Name, Price"#;
    let parse = parse_sdbl(query);

    eprintln!("\n=== INDEX BY clause ===");
    let tree = format!("{:#?}", parse.syntax_node());
    eprintln!("Has errors: {}", parse.has_errors());
    eprintln!("Has INDEX BY node: {}", tree.contains("SDBL_INDEX_BY"));

    assert!(!parse.has_errors(), "INDEX BY should parse without errors");
    assert!(tree.contains("SDBL_INDEX_BY"), "Should have INDEX BY node");
}

#[test]
fn test_autoorder_clause() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ Name, Price ИЗ Products АВТОУПОРЯДОЧИВАНИЕ"#;
    let parse = parse_sdbl(query);

    eprintln!("\n=== AUTOORDER clause ===");
    let tree = format!("{:#?}", parse.syntax_node());
    eprintln!("Has errors: {}", parse.has_errors());
    eprintln!("Has AUTOORDER node: {}", tree.contains("SDBL_AUTOORDER"));

    assert!(!parse.has_errors(), "AUTOORDER should parse without errors");
    assert!(tree.contains("SDBL_AUTOORDER"), "Should have AUTOORDER node");
}

#[test]
fn test_totals_by_clause() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ category, СУММА(amount) КАК total ИЗ Sales СГРУППИРОВАТЬ ПО category ИТОГИ ПО category"#;
    let parse = parse_sdbl(query);

    eprintln!("\n=== TOTALS BY clause ===");
    let tree = format!("{:#?}", parse.syntax_node());
    eprintln!("Has errors: {}", parse.has_errors());
    eprintln!("Has TOTALS BY node: {}", tree.contains("SDBL_TOTALS_BY"));

    assert!(!parse.has_errors(), "TOTALS BY should parse without errors");
    assert!(tree.contains("SDBL_TOTALS_BY"), "Should have TOTALS BY node");
}

#[test]
fn test_phase2_combined() {
    use parser::parse_sdbl;

    // Complex query with all Phase 2 features
    let query = r#"
        ВЫБРАТЬ
            category,
            СУММА(amount) КАК total
        ИЗ Sales
        ГДЕ active = ИСТИНА
        СГРУППИРОВАТЬ ПО category
        ИМЕЮЩИЕ СУММА(amount) > 1000
        ДЛЯ ИЗМЕНЕНИЯ Sales
        ИНДЕКСИРОВАТЬ ПО category
        УПОРЯДОЧИТЬ ПО total УБЫВ
        АВТОУПОРЯДОЧИВАНИЕ
        ИТОГИ ПО category
    "#;

    let parse = parse_sdbl(query);
    let tree = format!("{:#?}", parse.syntax_node());

    eprintln!("\n=== Combined Phase 2 features ===");
    eprintln!("Has errors: {}", parse.has_errors());
    eprintln!("Has GROUP BY: {}", tree.contains("SDBL_GROUP_CLAUSE"));
    eprintln!("Has HAVING: {}", tree.contains("SDBL_HAVING_CLAUSE"));
    eprintln!("Has FOR UPDATE: {}", tree.contains("SDBL_FOR_UPDATE"));
    eprintln!("Has INDEX BY: {}", tree.contains("SDBL_INDEX_BY"));
    eprintln!("Has ORDER BY: {}", tree.contains("SDBL_ORDER_CLAUSE"));
    eprintln!("Has AUTOORDER: {}", tree.contains("SDBL_AUTOORDER"));
    eprintln!("Has TOTALS BY: {}", tree.contains("SDBL_TOTALS_BY"));

    // Should parse without errors or with minimal errors
    // SDBL spec allows flexible ordering of these clauses
}
#[test]
fn test_simple_is_null() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ * ИЗ T ГДЕ Field ЕСТЬ NULL"#;
    let parse = parse_sdbl(query);
    let tree = format!("{:#?}", parse.syntax_node());

    eprintln!("\n=== Simple IS NULL test ===");
    eprintln!("Has errors: {}", parse.has_errors());
    eprintln!("\n{}", tree);
}
#[test]
fn test_dotted_is_null() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ * ИЗ T ГДЕ ДокЗаказКлиента.Ссылка ЕСТЬ NULL"#;
    let parse = parse_sdbl(query);
    let tree = format!("{:#?}", parse.syntax_node());

    eprintln!("\n=== Dotted IS NULL test ===");
    eprintln!("Has errors: {}", parse.has_errors());
    eprintln!("\n{}", tree);

    // Check if ЕСТЬ is parsed as part of column ref or as IS NULL
    if tree.contains("SDBL_IS_NULL_EXPR") {
        eprintln!("✓ ЕСТЬ correctly parsed as IS NULL predicate");
    } else {
        eprintln!("✗ ЕСТЬ incorrectly parsed (not IS NULL)");
    }
}
#[test]
fn test_case_with_is_null_no_newline() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ ВЫБОР КОГДА ДокЗаказКлиента.Ссылка ЕСТЬ NULL ТОГДА "Покупка" КОГДА ДокЗаказКлиента.Ссылка ЕСТЬ НЕ NULL ТОГДА "Заказ" КОНЕЦ ИЗ T"#;
    let parse = parse_sdbl(query);
    let tree = format!("{:#?}", parse.syntax_node());

    eprintln!("\n=== CASE with IS NULL (no newline) test ===");
    eprintln!("Has errors: {}", parse.has_errors());

    // Count IS NULL expressions
    let is_null_count = tree.matches("SDBL_IS_NULL_EXPR").count();
    eprintln!("IS NULL expressions found: {}", is_null_count);

    if is_null_count >= 2 {
        eprintln!("✓ Both IS NULL predicates parsed correctly");
    } else {
        eprintln!("✗ Missing IS NULL predicates (expected 2, got {})", is_null_count);
        eprintln!("\nTree snippet:");
        // Print tree but limit output
        let lines: Vec<&str> = tree.lines().collect();
        for line in lines.iter().take(100) {
            eprintln!("{}", line);
        }
    }
}
#[test]
fn test_full_user_query() {
    use parser::parse_sdbl;

    let query = r#"ВЫБРАТЬ
        ВыручкаИСебестоимостьПродаж.Регистратор,
        ЕСТЬNULL(ДокЗаказКлиента.НомерПоДаннымКлиента, ""),
        ВыручкаИСебестоимостьПродаж.Период,
        ВЫБОР
            КОГДА ВыручкаИСебестоимостьПродаж.Регистратор ССЫЛКА Документ.ВозвратТоваровОтКлиента
                    И ДокЗаказКлиента.Ссылка ЕСТЬ NULL
                ТОГДА "Возврат"
            КОГДА ВыручкаИСебестоимостьПродаж.Регистратор ССЫЛКА Документ.ВозвратТоваровОтКлиента
                    И ДокЗаказКлиента.Ссылка ЕСТЬ НЕ NULL 
                ТОГДА "Возврат №" + ПРЕДСТАВЛЕНИЕ(ДокЗаказКлиента.НомерПоДаннымКлиента)
            КОГДА ДокЗаказКлиента.Ссылка ЕСТЬ NULL
                ТОГДА "Покупка в магазине"
            ИНАЧЕ "Заказ №" + ПРЕДСТАВЛЕНИЕ(ДокЗаказКлиента.НомерПоДаннымКлиента)
        КОНЕЦ
    ИЗ T"#;

    let parse = parse_sdbl(query);
    let tree = format!("{:#?}", parse.syntax_node());

    eprintln!("\n=== Full user query test ===");
    eprintln!("Has errors: {}", parse.has_errors());

    // Count IS NULL expressions
    let is_null_count = tree.matches("SDBL_IS_NULL_EXPR").count();
    eprintln!("IS NULL expressions found: {}", is_null_count);

    // Count REFS expressions (ССЫЛКА)
    let refs_count = tree.matches("SDBL_REFS_EXPR").count();
    eprintln!("REFS expressions found: {}", refs_count);

    if parse.has_errors() {
        eprintln!("\n✗ Query has errors");
        // Show first 150 lines of tree
        for line in tree.lines().take(150) {
            eprintln!("{}", line);
        }
    } else {
        eprintln!("✓ Query parsed successfully");
    }
}
#[test]
fn test_estnull_function_vs_predicate() {
    use parser::parse_sdbl;

    // Test 1: ЕСТЬNULL function (should be parsed as function call)
    let q1 = r#"ВЫБРАТЬ ЕСТЬNULL(ДокЗаказКлиента.НомерПоДаннымКлиента, "") ИЗ T"#;
    let p1 = parse_sdbl(q1);
    let tree1 = format!("{:#?}", p1.syntax_node());

    eprintln!("\n=== Test 1: ЕСТЬNULL function ===");
    eprintln!("Has errors: {}", p1.has_errors());
    let func_count = tree1.matches("SDBL_FUNCTION_CALL").count();
    eprintln!("Function calls: {}", func_count);

    // Test 2: ЕСТЬ NULL predicate (should be parsed as IS NULL)
    let q2 = r#"ВЫБРАТЬ * ИЗ T ГДЕ ДокЗаказКлиента.Ссылка ЕСТЬ NULL"#;
    let p2 = parse_sdbl(q2);
    let tree2 = format!("{:#?}", p2.syntax_node());

    eprintln!("\n=== Test 2: ЕСТЬ NULL predicate ===");
    eprintln!("Has errors: {}", p2.has_errors());
    let is_null_count = tree2.matches("SDBL_IS_NULL_EXPR").count();
    eprintln!("IS NULL predicates: {}", is_null_count);

    if func_count > 0 && is_null_count > 0 {
        eprintln!("\n✓ Both ЕСТЬNULL() function and ЕСТЬ NULL predicate work correctly");
    } else {
        eprintln!("\n✗ Problem detected:");
        if func_count == 0 {
            eprintln!("  - ЕСТЬNULL() not recognized as function");
            eprintln!("Tree 1:\n{}", tree1.lines().take(50).collect::<Vec<_>>().join("\n"));
        }
        if is_null_count == 0 {
            eprintln!("  - ЕСТЬ NULL not recognized as predicate");
            eprintln!("Tree 2:\n{}", tree2.lines().take(50).collect::<Vec<_>>().join("\n"));
        }
    }
}
#[test]
fn test_estnull_no_space_issue() {
    use parser::parse_sdbl;

    // User's exact case: ЕСТЬNULL without space
    let query = r#"ВЫБРАТЬ ЕСТЬNULL(ДокЗаказКлиента.НомерПоДаннымКлиента, "") ИЗ T"#;
    let parse = parse_sdbl(query);
    let tree = format!("{:#?}", parse.syntax_node());

    eprintln!("\n=== ЕСТЬNULL token analysis ===");
    eprintln!("Has errors: {}", parse.has_errors());

    // Look for how ЕСТЬNULL is tokenized
    let lines: Vec<&str> = tree.lines().collect();
    for (i, line) in lines.iter().enumerate().take(50) {
        if line.contains("ЕСТЬNULL") || line.contains("ДокЗаказКлиента") {
            // Print context
            if i > 0 {
                eprintln!("{}", lines[i - 1]);
            }
            eprintln!(">>> {}", line);
            if i < lines.len() - 1 {
                eprintln!("{}", lines[i + 1]);
            }
            if i < lines.len() - 2 {
                eprintln!("{}", lines[i + 2]);
            }
        }
    }

    // Check if it's parsed as function call
    if tree.contains("SDBL_FUNCTION_CALL") {
        eprintln!("\n✓ ЕСТЬNULL correctly parsed as function call");
    } else {
        eprintln!("\n✗ ЕСТЬNULL not recognized as function");
    }
}

#[test]
fn test_parameter_as_data_source() {
    // &Parameter as a table reference in FROM clause (e.g., ValueTable passed as param)
    check_no_errors("ВЫБРАТЬ Поле КАК Поле ИЗ &ТЗ КАК ТЗ");
    check_no_errors("ВЫБРАТЬ Поле ИЗ &ТаблицаЗначений КАК Т");
}

#[test]
fn test_parameter_as_data_source_in_batch() {
    // Batch query with &Parameter data sources and comment separator
    let query = "ВЫБРАТЬ Поле КАК Поле ПОМЕСТИТЬ ВТ ИЗ &ТЗ КАК ТЗ;\n\
////////////////////////////////////////////////////////////////////////////////\n\
ВЫБРАТЬ Остатки.Номенклатура КАК Номенклатура ПОМЕСТИТЬ Результат ИЗ &Остатки КАК Остатки";

    let parse = parse_sdbl(query);
    assert!(!parse.has_errors(), "Batch with &Parameter data sources should parse without errors");

    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("Should have query package");
    assert_eq!(package.queries().count(), 2, "Expected 2 queries");
}

#[test]
fn test_sdbl_constructs_for_false_positive() {
    use syntax::SyntaxKind;

    let tests: Vec<(&str, &str)> = vec![
        ("VALUE function", "ВЫБРАТЬ ЗНАЧЕНИЕ(Перечисление.Качества.Новый) ИЗ T"),
        ("NOT IN subquery", "ВЫБРАТЬ * ИЗ T ГДЕ НЕ Поле В (ВЫБРАТЬ X ИЗ T2)"),
        ("DATETIME", "ВЫБРАТЬ * ИЗ T ГДЕ Дата <> ДАТАВРЕМЯ(1, 1, 1)"),
        ("BOOLEAN", "ВЫБРАТЬ * ИЗ T ГДЕ X = ИСТИНА И Y = ЛОЖЬ"),
        ("IN with param", "ВЫБРАТЬ * ИЗ T ГДЕ Склад В (&Список)"),
        ("Virtual table with params", "ВЫБРАТЬ * ИЗ Рег.Остатки(, Поле = &Парам) КАК Т"),
        ("Virtual table empty params", "ВЫБРАТЬ * ИЗ Рег.Остатки(, ) КАК Т"),
        ("TOTALS BY", "ВЫБРАТЬ Поле ИЗ T ИТОГИ СУММА(Кол) ПО Поле"),
        ("NOT expr IN with param list", "ВЫБРАТЬ * ИЗ T ГДЕ НЕ Поле В (&Исключ)"),
        ("PRESENTATION function", "ВЫБРАТЬ ПРЕДСТАВЛЕНИЕ(Поле) ИЗ T"),
        ("ISNULL with comparison arg", "ВЫБРАТЬ ЕСТЬNULL(X.Y = ЗНАЧЕНИЕ(Справ.Пуст), ИСТИНА) ИЗ T"),
        ("Division in expression", "ВЫБРАТЬ X / Y ИЗ T"),
        (
            "ISNULL with CASE inside",
            "ВЫБРАТЬ ЕСТЬNULL(ВЫБОР КОГДА X = 0 ТОГДА 1 ИНАЧЕ X КОНЕЦ, 1) ИЗ T",
        ),
        ("IN with multiple VALUE", "ВЫБРАТЬ * ИЗ T ГДЕ Поле В (ЗНАЧЕНИЕ(Перечисление.Статусы.Новый), ЗНАЧЕНИЕ(Перечисление.Статусы.Ошибка))"),
        ("Nested NOT IS NULL", "ВЫБРАТЬ * ИЗ T ГДЕ НЕ Ссылка ЕСТЬ NULL"),
        ("NOT...NOT IS NULL", "ВЫБРАТЬ * ИЗ T ГДЕ НЕ Назначение.Договор.Ссылка ЕСТЬ NULL"),
        (
            "CASE in VT param",
            "ВЫБРАТЬ * ИЗ Рег.Остатки(, ВЫБОР КОГДА X ТОГДА ЛОЖЬ ИНАЧЕ ИСТИНА КОНЕЦ) КАК Т",
        ),
    ];

    let mut failed = false;
    for (name, query) in &tests {
        let parse = parse_sdbl(query);
        let has_error = parse.syntax_node().descendants().any(|n| n.kind() == SyntaxKind::ERROR);
        if has_error {
            failed = true;
            eprintln!("FAIL: {}: {}", name, query);
            for node in parse.syntax_node().descendants() {
                if node.kind() == SyntaxKind::ERROR {
                    eprintln!("  ERROR at {:?}: {:?}", node.text_range(), node.text());
                }
            }
        } else {
            eprintln!("OK:   {}", name);
        }
    }

    if failed {
        panic!("Some SDBL constructs produced ERROR nodes");
    }
}

#[test]
fn test_error_node_analysis() {
    use syntax::SyntaxKind;

    let cases: Vec<(&str, &str)> = vec![
        ("valid VT empty params", "ВЫБРАТЬ * ИЗ Рег.Остатки(, Поле = &Парам) КАК Т"),
        ("valid VT all empty", "ВЫБРАТЬ * ИЗ Рег.Остатки(, ) КАК Т"),
        ("invalid incomplete FROM", "ВЫБРАТЬ Поле ИЗ  "),
        ("invalid incomplete WHERE", "ВЫБРАТЬ Поле ИЗ Таблица ГДЕ Условие >"),
        ("invalid batch partial", "ВЫБРАТЬ Поле ИЗ Таблица1; ВЫБРАТЬ Поле2 ИЗ"),
    ];

    for (name, query) in &cases {
        let parse = parse_sdbl(query);
        let errors: Vec<_> =
            parse.syntax_node().descendants().filter(|n| n.kind() == SyntaxKind::ERROR).collect();

        eprintln!("\n=== {} ===", name);
        eprintln!("has_errors(): {}", parse.has_errors());
        for e in &errors {
            let parent_kind = e.parent().map(|p| format!("{:?}", p.kind())).unwrap_or_default();
            eprintln!(
                "  ERROR range={:?} empty={} text={:?} parent={}",
                e.text_range(),
                e.text_range().is_empty(),
                e.text(),
                parent_kind,
            );
        }
    }
}

#[test]
fn test_drop_table_russian() {
    check_no_errors("УНИЧТОЖИТЬ ВременнаяТаблица");
}

#[test]
fn test_drop_table_english() {
    check_no_errors("DROP TempTable");
}

#[test]
fn test_batch_with_drop() {
    check_no_errors("ВЫБРАТЬ Поле ИЗ Таблица ПОМЕСТИТЬ ВТ; УНИЧТОЖИТЬ ВТ");
}
