//! SDBL parser tests.
//!
//! Tests SDBL query parsing with focus on:
//! - Basic SELECT queries
//! - Aliases (with and without AS keyword) - CRITICAL for AssignAliasFieldsInQuery
//! - UNION queries
//! - Subqueries in FROM
//! - Error recovery
//!
//! # Provenance and bucket policy
//!
//! Tests in this file follow the A/B/C classification defined in
//! `docs/legal/sdbl-test-corpus-slice0.md`:
//!
//! - Bucket A — generic language-acceptance coverage: keep as-is; example
//!   queries are owned and may be rewritten freely from official 1C docs.
//! - Bucket B — valuable behavioral coverage whose current query text is
//!   bulky or historically shaped: keep the behavioral assertion, rewrite
//!   the query text from the minimal local scenario needed to prove it.
//! - Bucket C — historical regression archives kept for compatibility: do
//!   not treat as specification input for clean-room work. Marked inline
//!   with `// Bucket C:` comments.
//!
//! New SDBL tests should be written from 1C query-language documentation
//! (https://its.1c.ru/db/pubqlang) without consulting third-party grammar
//! files. See `docs/legal/sdbl-clean-room-slices.md` for the full policy.

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

// Bucket A.
#[test]
fn test_select_asterisk() {
    check_no_errors("SELECT * FROM Table");
}

// Bucket A.
#[test]
fn test_select_single_column() {
    check_no_errors("SELECT Name FROM Products");
}

// Bucket A.
#[test]
fn test_select_multiple_columns() {
    check_no_errors("SELECT Name, Code, Description FROM Products");
}

// Bucket A.
#[test]
fn test_select_with_where() {
    check_no_errors("SELECT Name FROM Products WHERE Active = TRUE");
}

// Bucket A.
#[test]
fn test_select_table_asterisk() {
    check_no_errors("SELECT Products.* FROM Products");
}

// Bucket A.
#[test]
fn test_select_qualified_column() {
    check_no_errors("SELECT Products.Name FROM Products");
}

// Bucket A.
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

// Bucket A.
#[test]
fn test_alias_without_as_keyword() {
    // Implicit alias (no AS keyword) - this is what the diagnostic should catch
    check_no_errors("SELECT Name ProductName FROM Products");
}

// Bucket A.
#[test]
fn test_multiple_aliases_with_as() {
    check_no_errors("SELECT Name AS ProductName, Code AS ProductCode FROM Products");
}

// Bucket A.
#[test]
fn test_multiple_aliases_mixed() {
    // Some with AS, some without
    check_no_errors("SELECT Name AS ProductName, Code ProductCode FROM Products");
}

// Bucket A.
#[test]
fn test_russian_alias_with_kak() {
    // Russian КАК keyword
    check_no_errors("ВЫБРАТЬ Имя КАК ИмяПродукта ИЗ Товары");
}

// Bucket A.
#[test]
fn test_alias_case_insensitive() {
    // AS in various cases
    check_no_errors("SELECT Name as ProductName FROM Products");
    check_no_errors("SELECT Name As ProductName FROM Products");
    check_no_errors("SELECT Name aS ProductName FROM Products");
}

// Bucket A.
#[test]
fn test_asterisk_no_alias() {
    // Asterisk shouldn't have alias
    check_no_errors("SELECT * FROM Products");
    check_no_errors("SELECT Products.* FROM Products");
}

// Bucket A.
#[test]
fn test_russian_table_asterisk() {
    // Russian identifier + .* — parallel to English `Products.*`.
    // Locks that `is_asterisk_start` accepts Russian Ident in the Ident.* form.
    check_no_errors("ВЫБРАТЬ Товары.* ИЗ Товары");
}

// Bucket A.
#[test]
fn test_russian_into_simple() {
    // Minimal Russian ПОМЕСТИТЬ (INTO) in canonical field-then-INTO-then-FROM order.
    // Complementary to the complex Bucket-B fixture at test_into_clause_with_union_...;
    // adds a spec-shaped minimal gate for Slice 7 INTO parsing.
    check_no_errors("ВЫБРАТЬ Поле ПОМЕСТИТЬ ВремТаблица ИЗ Товары");
}

// Bucket A.
#[test]
fn test_union_simple() {
    check_no_errors("SELECT Name FROM Products UNION SELECT Name FROM Services");
}

// Bucket A.
#[test]
fn test_union_all() {
    check_no_errors("SELECT Name FROM Products UNION ALL SELECT Name FROM Services");
}

// Bucket A.
#[test]
fn test_union_multiple() {
    check_no_errors("SELECT A FROM T1 UNION SELECT B FROM T2 UNION SELECT C FROM T3");
}

// Bucket A.
#[test]
fn test_union_with_aliases() {
    check_no_errors("SELECT Name AS N FROM Products UNION SELECT Title AS N FROM Services");
}

// Bucket A.
#[test]
fn test_subquery_in_from() {
    check_no_errors("SELECT * FROM (SELECT Name FROM Products) AS Sub");
}

// Bucket A.
#[test]
fn test_subquery_nested() {
    check_no_errors("SELECT * FROM (SELECT * FROM (SELECT Name FROM Products) AS Inner) AS Outer");
}

// Bucket A.
#[test]
fn test_subquery_with_where() {
    check_no_errors("SELECT * FROM (SELECT Name FROM Products WHERE Active = TRUE) AS Sub");
}

// Bucket A.
#[test]
fn test_subquery_in_expression() {
    check_no_errors("SELECT Name FROM Products WHERE Code IN (SELECT Code FROM Active)");
}

// Bucket A.
#[test]
fn test_arithmetic_expressions() {
    check_no_errors("SELECT Price * Quantity AS Total FROM Orders");
    check_no_errors("SELECT Price + Tax AS TotalPrice FROM Products");
    check_no_errors("SELECT Amount - Discount AS Final FROM Sales");
}

// Bucket A.
#[test]
fn test_logical_expressions() {
    check_no_errors("SELECT * FROM Products WHERE Active = TRUE AND Price > 100");
    check_no_errors("SELECT * FROM Products WHERE Category = 1 OR Category = 2");
    check_no_errors("SELECT * FROM Products WHERE NOT Deleted");
}

// Bucket A.
#[test]
fn test_comparison_expressions() {
    check_no_errors("SELECT * FROM Products WHERE Price > 100");
    check_no_errors("SELECT * FROM Products WHERE Quantity >= 10");
    check_no_errors("SELECT * FROM Products WHERE Code <> 0");
}

// Bucket A.
#[test]
fn test_function_calls() {
    check_no_errors("SELECT COUNT(*) AS Total FROM Products");
    check_no_errors("SELECT SUM(Price) AS TotalPrice FROM Products");
    check_no_errors("SELECT YEAR(Date) AS Year FROM Orders");
}

// Bucket A.
#[test]
fn test_mdo_table_reference() {
    check_no_errors("SELECT Name FROM Catalog.Products");
    check_no_errors("SELECT Ref FROM Document.Sales");
}

// Bucket A.
#[test]
fn test_mdo_qualified_column() {
    check_no_errors("SELECT Catalog.Products.Name FROM Catalog.Products");
}

// Bucket A.
#[test]
fn test_numeric_literals() {
    check_no_errors("SELECT * FROM Products WHERE Price = 100");
    check_no_errors("SELECT * FROM Products WHERE Price = 99.99");
}

// Bucket A.
#[test]
fn test_string_literals() {
    check_no_errors(r#"SELECT * FROM Products WHERE Name = "Product""#);
}

// Bucket A.
#[test]
fn test_boolean_literals() {
    check_no_errors("SELECT * FROM Products WHERE Active = TRUE");
    check_no_errors("SELECT * FROM Products WHERE Deleted = FALSE");
}

// Bucket A.
#[test]
fn test_null_literal() {
    check_no_errors("SELECT * FROM Products WHERE Description = NULL");
}

// Bucket A.
#[test]
fn test_parameter() {
    check_no_errors("SELECT * FROM Products WHERE Code = &ProductCode");
}

// Bucket A.
#[test]
fn test_multiple_parameters() {
    check_no_errors("SELECT * FROM Products WHERE Code = &Code AND Active = &IsActive");
}

// Bucket A.
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

// Bucket A.
#[test]
fn test_multiple_queries_with_semicolon() {
    check_no_errors("SELECT Name FROM Products; SELECT Code FROM Services");
}

// Bucket A.
#[test]
fn test_multiple_queries_trailing_semicolon() {
    check_no_errors("SELECT Name FROM Products; SELECT Code FROM Services;");
}

// Bucket A: AST API contract.
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

// Bucket A: AST API contract.
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

// Bucket A: AST API contract.
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

// Bucket A: AST API contract.
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

// Bucket B: banner-separator semicolon pattern with historically shaped query text.
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

// Bucket B: banner-separator UNION pattern with historically shaped query text.
#[test]
fn test_union_with_semicolon_separator() {
    // Pattern: SELECT … ОБЪЕДИНИТЬ ВСЕ … ; <comment line> SELECT … ОБЪЕДИНИТЬ ВСЕ …
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

// Bucket B: multi-column UNION ALL across a two-statement package boundary
// with mixed alias forms (bare / AS-style / implicit). Query text authored
// from ITS pubqlang/10 catalog-reference examples; spec-shaped, no log
// provenance.
#[test]
fn test_double_union_all_queries_with_aliases() {
    let query = r#"ВЫБРАТЬ
	Товары.Ссылка,
	Товары.Ссылка КАК ПсевдонимПоляСсылка,
	Товары.Код Код
ИЗ
	Справочник.Товары КАК Товары

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	Товары.Ссылка,
	Товары.Ссылка,
	Товары.Код
ИЗ
	Справочник.Товары КАК Товары
;

////////////////////////////////////////////////////////////////////////////////
ВЫБРАТЬ
	Товары.Ссылка,
	Товары.Ссылка КАК ПсевдонимПоляСсылка,
	Товары.Код Код
ИЗ
	Справочник.Товары КАК Товары

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	Товары.Ссылка,
	Товары.Ссылка,
	Товары.Код
ИЗ
	Справочник.Товары КАК Товары"#;

    let parse = parse_sdbl(query);
    assert!(!parse.has_errors(), "Should parse without errors");

    use syntax::ast::{AstNode, SdblQueryPackage};
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("Should have query package");
    let count = package.queries().count();
    assert_eq!(count, 2, "Should find 2 SELECT queries separated by semicolon");
}

// Tests for FULL OUTER JOIN parsing (fix for keyword consumption bug)

// Bucket A.
#[test]
fn test_full_outer_join_simple() {
    let input = "SELECT * FROM T1 FULL OUTER JOIN T2 ON T1.A = T2.A";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Should parse without errors");
}

// Bucket A.
#[test]
fn test_multiple_full_outer_joins() {
    let input =
        "SELECT * FROM T1 FULL OUTER JOIN T2 ON T1.A = T2.A FULL OUTER JOIN T3 ON T1.B = T3.B";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Should parse multiple JOINs");
}

// Bucket A.
#[test]
fn test_on_not_consumed_as_alias() {
    let input = "SELECT * FROM T1 JOIN T2 ON T1.ID = T2.ID";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors());
    let text = format!("{:#?}", parse.syntax_node());
    assert!(text.contains("ON"), "ON keyword should be in AST, not consumed as alias");
}

// Bucket B: multiline RU join with historically shaped schema names (Товары/ПланПродаж).
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

// Bucket A.
#[test]
fn test_function_with_two_arguments() {
    // Simplest case: two-argument function without alias
    check_no_errors("SELECT ISNULL(A, 0) FROM T");
}

// Bucket A.
#[test]
fn test_function_with_two_arguments_and_alias() {
    // With alias
    check_no_errors("SELECT ISNULL(Amount, 0) AS Total FROM Products");
}

// Bucket A.
#[test]
fn test_russian_function_with_arguments() {
    // Russian version
    check_no_errors("ВЫБРАТЬ ЕСТЬNULL(Сумма, 0) ИЗ Товары");
}

// Bucket A.
#[test]
fn test_multiple_fields_with_function_arguments() {
    // Multiple fields, one with multi-arg function
    check_no_errors("SELECT Name, ISNULL(Amount, 0) AS Total FROM Products");
}

// Bucket B: multiline RU query with historically shaped schema (Товары/ПланПродаж/ФактическиеПродажи).
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

// Bucket A.
#[test]
fn test_single_arg_function() {
    check_no_errors("SELECT SUM(Amount) FROM T");
    check_no_errors("SELECT YEAR(Date) FROM T");
}

// Bucket A.
#[test]
fn test_two_arg_functions() {
    check_no_errors("SELECT ISNULL(A, 0) FROM T");
    check_no_errors("SELECT SUBSTRING(Name, 1, 10) FROM T");
}

// Bucket A.
#[test]
fn test_three_arg_functions() {
    check_no_errors("SELECT SUBSTRING(Text, 1, 5) FROM T");
}

// Bucket A.
#[test]
fn test_multi_arg_with_alias() {
    check_no_errors("SELECT ISNULL(Amount, 0) AS Total FROM T");
}

// Bucket A.
#[test]
fn test_mixed_fields_and_functions() {
    check_no_errors("SELECT Name, ISNULL(Amount, 0), Code FROM T");
}

// Bucket A.
#[test]
fn test_nested_functions() {
    check_no_errors("SELECT ISNULL(SUM(Amount), 0) FROM T");
}

// Bucket A.
#[test]
fn test_russian_multi_arg_functions() {
    check_no_errors("ВЫБРАТЬ ЕСТЬNULL(Сумма, 0) ИЗ Товары");
    check_no_errors("ВЫБРАТЬ ЕСТЬNULL(Сумма, 0) КАК Итого ИЗ Товары");
}

// Bucket A.
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

// Bucket B: two-statement package where the first query uses ПОМЕСТИТЬ (INTO)
// + UNION ALL and the second uses INNER JOIN + GROUP BY over the temp table.
// Exercises package-boundary + UNION ALL bundling + cross-statement temp-table
// reference. Query text authored from ITS pubqlang/10 temporary-table example
// shape (ПОМЕСТИТЬ Папки ... ВНУТРЕННЕЕ СОЕДИНЕНИЕ Папки).
#[test]
fn test_into_clause_with_union_and_semicolon_separator() {
    let query = r#"ВЫБРАТЬ РАЗРЕШЕННЫЕ
	Товары.Ссылка
ПОМЕСТИТЬ ВыбранныеТовары
ИЗ
	Справочник.Товары КАК Товары
ГДЕ
	Товары.Родитель В ИЕРАРХИИ(&Группа)

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	&Группа
;

////////////////////////////////////////////////////////////////////////////////
ВЫБРАТЬ
	ЦеныТоваров.Цена,
	ЦеныТоваров.Валюта
ИЗ
	ВыбранныеТовары КАК ВыбранныеТовары
		ВНУТРЕННЕЕ СОЕДИНЕНИЕ РегистрСведений.ЦеныТоваров КАК ЦеныТоваров
		ПО ВыбранныеТовары.Ссылка = ЦеныТоваров.Товар

СГРУППИРОВАТЬ ПО
	ЦеныТоваров.Цена,
	ЦеныТоваров.Валюта"#;

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

// Bucket C (preserved): exact text extracted from runtime logs with an
// incomplete inner JOIN ON condition ("Папки. ="). Encodes a specific
// error-recovery invariant that is not spec-derivable: the package boundary
// (';' separator) must still yield two SdblSelectQuery children even when
// the first query has a malformed ON expression. Listed in the Slice 6
// attestation §Preserved pre-refactor behaviours.
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

// Bucket C: user-extracted highlighting bug query (БизнесПроцессы schema).
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

// Bucket C: user-extracted completion scenario (incomplete ON condition).
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
// Bucket B: register virtual-table access pattern with historically shaped schema.
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

// Bucket C: user's real query (comment: "like in the user's real query").
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

// Bucket A.
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

// Bucket C: from user's real query (comment: "Test from user's real query").
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

// Bucket C: full query from user's original bug report.
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

// Bucket C: fixture-backed regression (see in-body note).
#[test]
fn test_complete_user_query_from_fixture() {
    // Bucket C: historical regression from a user report, preserved as a
    // fixture (ПОМЕСТИТЬ, ОБЪЕДИНИТЬ, empty params, IN subqueries). Not a
    // specification input. See docs/legal/sdbl-test-corpus-slice0.md.
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

// Bucket A.
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

// Bucket A.
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

// Bucket A.
#[test]
fn test_error_recovery_in_leading_empty() {
    // IN predicate with leading empty value: IN (, 2, 3)
    let input = "SELECT * FROM T WHERE Field IN (, 2, 3)";
    let parse = parse_sdbl(input);

    let tree = format!("{:#?}", parse.syntax_node());
    assert!(tree.contains("ERROR"), "Expected ERROR node for leading empty value.\nTree: {}", tree);
    assert!(tree.contains("SDBL_WHERE_CLAUSE"), "WHERE clause should be parsed.\nTree: {}", tree);
}

// Bucket A.
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

// Bucket A.
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

// Bucket A.
#[test]
fn test_error_recovery_function_leading_empty() {
    // Function with leading empty argument: func(, value)
    let input = "SELECT func(, 456) FROM T";
    let parse = parse_sdbl(input);

    let tree = format!("{:#?}", parse.syntax_node());
    assert!(tree.contains("ERROR"), "Expected ERROR node for leading empty arg.\nTree: {}", tree);
    assert!(tree.contains("SDBL_FUNCTION_CALL"), "Function call should be parsed.\nTree: {}", tree);
}

// Bucket A.
#[test]
fn test_error_recovery_function_trailing_empty() {
    // Function with trailing empty argument: func(value,)
    let input = "SELECT func(789,) FROM T";
    let parse = parse_sdbl(input);

    let tree = format!("{:#?}", parse.syntax_node());
    assert!(tree.contains("ERROR"), "Expected ERROR node for trailing empty arg.\nTree: {}", tree);
    assert!(tree.contains("SDBL_FUNCTION_CALL"), "Function call should be parsed.\nTree: {}", tree);
}

// Bucket A.
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

// Bucket A.
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

// Bucket A.
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

// Bucket A.
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

// Bucket C: real-world query from user (comment: "Real-world query from user").
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

// Bucket C: simplified slice of same user query (ДвиженияПоКлиенту schema).
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

// Bucket A.
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

// Bucket C: full user query (14 fields, comment: "Real user query with CAST and CASE").
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
// Bucket A.
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
// Bucket A.
#[test]
fn test_empty_string_literal() {
    use parser::parse_sdbl;
    let query = r#"ВЫБРАТЬ x <> "" КАК result ИЗ T"#;
    let parse = parse_sdbl(query);

    eprintln!("\n{:#?}", parse.syntax_node());

    assert!(!parse.has_errors(), "Should parse empty string");
}

// Bucket A.
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

// Bucket A.
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

// Bucket A.
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
// Bucket A.
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
// Bucket A.
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

// Bucket A.
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

// Bucket A.
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
// Bucket A.
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

// Bucket A.
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

// Bucket A.
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
// Bucket A.
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
// Bucket A.
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
// Bucket A.
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
// Bucket A.
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
// Bucket A.
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
// Bucket A.
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
// Bucket A.
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
// Bucket A.
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

// Bucket A.
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

// Bucket A.
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

// Bucket A.
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

// Bucket A.
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

// Bucket A.
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
// Bucket A.
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
// Bucket B: dotted IS NULL using specific user schema name (ДокЗаказКлиента.Ссылка).
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
// Bucket C: user's CASE/IS NULL repro using lifted schema (ДокЗаказКлиента).
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
// Bucket C: user-extracted multiline query with CASE/REFS/IS NULL.
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
// Bucket B: ЕСТЬNULL function vs IS NULL predicate disambiguation using lifted schema name.
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
// Bucket C: user's exact case (comment: "User's exact case").
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

// Bucket A.
#[test]
fn test_parameter_as_data_source() {
    // &Parameter as a table reference in FROM clause (e.g., ValueTable passed as param)
    check_no_errors("ВЫБРАТЬ Поле КАК Поле ИЗ &ТЗ КАК ТЗ");
    check_no_errors("ВЫБРАТЬ Поле ИЗ &ТаблицаЗначений КАК Т");
}

// Bucket A.
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

// Bucket A.
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

// Bucket A.
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

// Bucket A.
#[test]
fn test_drop_table_russian() {
    check_no_errors("УНИЧТОЖИТЬ ВременнаяТаблица");
}

// Bucket A.
#[test]
fn test_drop_table_english() {
    check_no_errors("DROP TempTable");
}

// Bucket A.
#[test]
fn test_batch_with_drop() {
    check_no_errors("ВЫБРАТЬ Поле ИЗ Таблица ПОМЕСТИТЬ ВТ; УНИЧТОЖИТЬ ВТ");
}

// ============================================================================
// Slice 6 surface coverage — added by C0 audit to close gaps before the
// clean-room rewrite of query_package / queries / drop_table_query /
// select_query / subquery / union_clause in C2.
// ============================================================================

// Bucket A: three-SELECT package. Locks the queries() + query_package shape
// for N > 2. (SdblQueryPackage::queries() iterates SdblSelectQuery children
// only; DROP statements are separate — see the mid-package DROP test below.)
#[test]
fn test_package_with_three_statements() {
    use syntax::ast::{AstNode, SdblQueryPackage};
    let input = "SELECT Name FROM Products; \
                 SELECT Code FROM Services; \
                 SELECT Price FROM Prices";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Should parse 3-SELECT package: {:?}", parse.errors());
    let package = SdblQueryPackage::cast(parse.syntax_node()).expect("query package");
    assert_eq!(package.queries().count(), 3, "Expected 3 SELECT queries in the package");
}

// Bucket A: subquery in a WHERE predicate followed by UNION in the outer
// query. Guards the subquery()/union_clause() boundary: the UNION belongs to
// the outer SdblSubquery, not to the IN-subquery. Asserts UNION-clause count
// on BOTH subqueries — a count-only package check would pass even if the
// UNION were wrongly attached to the inner subquery.
#[test]
fn test_subquery_in_where_with_outer_union() {
    use syntax::{
        ast::{AstNode, SdblQueryPackage, SdblSubquery},
        SyntaxKind,
    };
    let input = "SELECT Name FROM Products WHERE Id IN (SELECT Id FROM Archive) \
                 UNION ALL SELECT Name FROM Services";
    let parse = parse_sdbl(input);
    assert!(
        !parse.has_errors(),
        "Should parse subquery-in-WHERE + outer UNION: {:?}",
        parse.errors()
    );
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root.clone()).expect("query package");
    assert_eq!(package.queries().count(), 1, "Single outer SELECT statement");
    let outer_subquery =
        package.queries().next().and_then(|q| q.subquery()).expect("outer SdblSubquery");
    assert_eq!(
        outer_subquery.union_clauses().count(),
        1,
        "UNION ALL must attach to the outer subquery",
    );
    let mut subquery_nodes: Vec<_> =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_SUBQUERY).collect();
    assert_eq!(subquery_nodes.len(), 2, "One outer + one IN-subquery");
    subquery_nodes.sort_by_key(|n| usize::from(n.text_range().start()));
    let inner_subquery =
        SdblSubquery::cast(subquery_nodes.pop().expect("inner")).expect("cast inner SdblSubquery");
    assert_eq!(
        inner_subquery.union_clauses().count(),
        0,
        "The IN-subquery must not own the UNION clause",
    );
}

// Bucket A: DROP dispatch mid-package after a UNION ALL statement. Three
// statements: (1) SELECT ОБЪЕДИНИТЬ ВСЕ SELECT, (2) УНИЧТОЖИТЬ ВТ, (3)
// SELECT. The queries() dispatcher must pick the DROP branch at statement 2
// without letting the preceding UNION ALL bleed into the package boundary,
// and statement 3 must appear as a fresh outer SdblSelectQuery.
#[test]
fn test_drop_mid_package_after_union() {
    use syntax::{
        ast::{AstNode, SdblQueryPackage},
        SyntaxKind,
    };
    let input = "ВЫБРАТЬ Поле ИЗ Таблица1 ОБЪЕДИНИТЬ ВСЕ ВЫБРАТЬ Поле ИЗ Таблица2; \
                 УНИЧТОЖИТЬ ВТ; \
                 ВЫБРАТЬ Поле ИЗ Таблица3";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Should parse DROP-after-union package: {:?}", parse.errors());
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root.clone()).expect("query package");
    assert_eq!(package.queries().count(), 2, "Two SELECT statements surround the DROP");
    let first_outer_subquery = package
        .queries()
        .next()
        .and_then(|q| q.subquery())
        .expect("outer subquery of first SELECT");
    assert_eq!(
        first_outer_subquery.union_clauses().count(),
        1,
        "UNION ALL lives inside the first SELECT statement, not the package",
    );
    let drop_count = root.children().filter(|n| n.kind() == SyntaxKind::SDBL_DROP_QUERY).count();
    assert_eq!(drop_count, 1, "Exactly one DROP statement in the package");
}

// ============================================================================
// Slice 8 surface coverage — added by C0 audit to close gaps before the
// clean-room rewrite of is_data_source_start / from_clause / data_source /
// table_ref / source_alias in C2. Authored from 1C query-language docs
// (pubqlang/10 §query-body, /12 identifier + ampersand lexis) and the local
// mini-spec at docs/legal/sdbl-select-mini-spec.md §FROM clause.
// ============================================================================

// Bucket A: multi-source FROM with commas and a bare implicit alias on the
// second source — locks the data_source list shape, the comma separator, and
// the bare-alias branch of source_alias (no AS / КАК keyword between `Т2`
// and `А`). Asserts two SdblDataSource nodes AND that the second one carries
// an SdblAlias whose name is `А` with has_as_keyword() == false, so a
// regression that drops bare-identifier consumption cannot silently pass.
#[test]
fn test_slice8_from_multi_source_with_bare_alias() {
    use syntax::{
        ast::{AstNode, SdblFromClause},
        SyntaxKind,
    };
    let input = "ВЫБРАТЬ * ИЗ Т1, Т2 А";
    let parse = parse_sdbl(input);
    assert!(
        !parse.has_errors(),
        "Multi-source FROM with bare alias should parse: {:?}",
        parse.errors()
    );
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input (no trailing drop)");
    let from = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE)
        .and_then(SdblFromClause::cast)
        .expect("SdblFromClause");
    let sources: Vec<_> = from.data_sources().collect();
    assert_eq!(sources.len(), 2, "Expected two SdblDataSource nodes in the FROM list");
    let second_alias = sources[1].alias().expect("Second data source must carry SdblAlias");
    assert_eq!(second_alias.name().as_deref(), Some("А"), "Bare alias name mismatch");
    assert!(
        !second_alias.has_as_keyword(),
        "Bare alias must not carry AS/КАК keyword (implicit-alias branch of source_alias)",
    );
}

// Bucket A: subquery as a data source with the Russian КАК alias form. The
// English subquery-in-FROM shape is already exercised by test_subquery_in_from
// (:202); this gap covers the bilingual alias site of data_source. Asserts
// the SdblSubquery sits directly under SdblDataSource and that the alias is
// attached at the data-source level with has_as_keyword() == true.
#[test]
fn test_slice8_russian_subquery_source_with_alias() {
    use syntax::{
        ast::{AstNode, SdblFromClause},
        SyntaxKind,
    };
    let input = "ВЫБРАТЬ * ИЗ (ВЫБРАТЬ 1) КАК С";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Russian subquery source should parse: {:?}", parse.errors());
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input (no trailing drop)");
    let from = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE)
        .and_then(SdblFromClause::cast)
        .expect("SdblFromClause");
    let source = from.data_sources().next().expect("single SdblDataSource");
    assert!(
        source.subquery().is_some(),
        "SdblSubquery must sit as a direct child of SdblDataSource",
    );
    let alias = source.alias().expect("data source must carry SdblAlias");
    assert!(alias.has_as_keyword(), "КАК alias must set has_as_keyword() = true");
    assert_eq!(alias.name().as_deref(), Some("С"));
}

// Bucket A: temporary-table source crossing a package boundary — the first
// statement creates ВТ via ПОМЕСТИТЬ (INTO), the second statement consumes
// it as a table_ref. Exercises the identifier-only table_ref path (no MDO
// prefix, no VT args) and its interaction with the SdblQueryPackage
// statement loop. Asserts the second query's FROM data source is a single
// SdblTableRef whose text is `ВремТаблица`.
#[test]
fn test_slice8_temp_table_source_across_package_boundary() {
    use syntax::ast::{AstNode, SdblQueryPackage};
    let input = "ВЫБРАТЬ Поле ПОМЕСТИТЬ ВремТаблица ИЗ Товары; \
                 ВЫБРАТЬ Поле ИЗ ВремТаблица";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Temp-table package should parse: {:?}", parse.errors());
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input (no trailing drop)");
    let package = SdblQueryPackage::cast(root).expect("query package");
    let queries: Vec<_> = package.queries().collect();
    assert_eq!(queries.len(), 2, "Two SELECT statements around the temp table");
    let second_from = queries[1]
        .subquery()
        .and_then(|sq| sq.main_query())
        .and_then(|q| q.from_clause())
        .expect("second query must have FROM clause");
    let sources: Vec<_> = second_from.data_sources().collect();
    assert_eq!(sources.len(), 1, "Second query FROM must have a single data source");
    let table_ref = sources[0].table_ref().expect("identifier-only table_ref");
    assert_eq!(
        table_ref.syntax().text().to_string().trim(),
        "ВремТаблица",
        "Second query must reference the temp table created by the first",
    );
}

// Bucket A: parameter source without an alias — existing
// test_parameter_as_data_source (:2205) always pairs &Parameter with КАК;
// this gap locks the alias?-optional branch of data_source on the parameter
// path. Asserts SdblParameter sits inside SdblTableRef inside SdblDataSource
// and that the data source carries NO SdblAlias.
#[test]
fn test_slice8_parameter_source_without_alias() {
    use syntax::{
        ast::{AstNode, SdblFromClause},
        SyntaxKind,
    };
    let input = "ВЫБРАТЬ * ИЗ &ТЗ";
    let parse = parse_sdbl(input);
    assert!(
        !parse.has_errors(),
        "Parameter source without alias should parse: {:?}",
        parse.errors()
    );
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input (no trailing drop)");
    let from = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE)
        .and_then(SdblFromClause::cast)
        .expect("SdblFromClause");
    let source = from.data_sources().next().expect("single SdblDataSource");
    assert!(source.alias().is_none(), "Parameter source without alias must carry no SdblAlias");
    let table_ref = source.table_ref().expect("SdblTableRef for parameter source");
    let has_parameter =
        table_ref.syntax().children().any(|n| n.kind() == SyntaxKind::SDBL_PARAMETER);
    assert!(
        has_parameter,
        "SdblParameter must be a direct child of SdblTableRef on the &Ident path"
    );
}

// ============================================================================
// Slice 10a surface coverage — added by C0b audit to close gaps in the
// operator-chain + atoms + parens/tuple/subquery surface that the Slice
// 10a clean-room rewrite must satisfy. Authored from
// docs/legal/sdbl-expressions-mini-spec.md (the C0a clean-room
// reference for Slice 10a + 10b) and the local 1C ITS pubqlang dump
// at /home/itrous/src/tools_migration/its/dump/, specifically chapters
// 22 (WHERE / logical-operator precedence ladder), 40 (literal forms,
// arithmetic operators, ВЫБОР / ВЫРАЗИТЬ / ССЫЛКА / МЕЖДУ), and 60
// (`&Identifier` parameter prefix, ПОДОБНО). The companion overview
// chapters /10 and /12 (intro paragraph + bilingual-keywords
// principle) are referenced for surrounding context.
//
// **Oracle:** the assertions below derive from the mini-spec §AST-shape
// invariants and §Operator-binding pin list, NOT from any pre-rewrite
// parser implementation. Each per-test comment cites the relevant
// mini-spec section / ITS chapter so the post-rewrite parser is
// validated against the mini-spec contract rather than against
// accidental implementation shape.
// ============================================================================

// Bucket A: nested NOT — tests right-recursive multi-NOT body of
// `not_expr`. Mini-spec §Operator-binding pin list item 1 + AST-shape
// invariant for SdblNotExpr (operator token first child, single operand
// second child). ITS pubqlang/22 §Условие отбора (`И`, `ИЛИ`, `НЕ`).
#[test]
fn test_slice10a_nested_not() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ НЕ НЕ А";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Nested NOT should parse: {:?}", parse.errors());
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input");
    let outer_not = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_NOT_EXPR)
        .expect("outer SdblNotExpr");
    let inner_not = outer_not
        .children()
        .find(|n| n.kind() == SyntaxKind::SDBL_NOT_EXPR)
        .expect("inner SdblNotExpr nested directly inside outer (right-recursive shape)");
    assert!(
        inner_not.text().to_string().contains("НЕ"),
        "Inner SdblNotExpr text must include the second НЕ token",
    );
}

// Bucket A: NOT-AND binding — `НЕ А И Б` parses as
// `LogicalAndExpr( NotExpr(А), AND, Б )`, NOT as `NotExpr( А И Б )`.
// Mini-spec §Operator-binding pin list item 3 (NOT binds tightest under
// AND because not_expr is the logical_and_expr operand).
#[test]
fn test_slice10a_not_and_binding() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ НЕ А И Б";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "NOT-AND binding should parse: {:?}", parse.errors());
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input");
    let and_expr = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_LOGICAL_AND_EXPR)
        .expect("SdblLogicalAndExpr at the top");
    let not_descendants_under_and: Vec<_> =
        and_expr.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_NOT_EXPR).collect();
    assert_eq!(
        not_descendants_under_and.len(),
        1,
        "Exactly one SdblNotExpr nested under the SdblLogicalAndExpr (NOT binds А, not the AND-pair)",
    );
    let not_text = not_descendants_under_and[0].text().to_string();
    assert!(
        not_text.contains('А') && !not_text.contains('Б'),
        "SdblNotExpr must wrap А alone — not include Б; got {not_text:?}",
    );
}

// Bucket A: nested unary minus — `- - А` parses as
// `UnaryExpr( - , UnaryExpr( - , А ) )` per mini-spec §Operator-binding
// pin list item 2 (right-recursive multi-unary).
#[test]
fn test_slice10a_nested_unary_minus() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ - - А ИЗ Т";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Nested unary minus should parse: {:?}", parse.errors());
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input");
    let outer_unary = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_UNARY_EXPR)
        .expect("outer SdblUnaryExpr");
    let inner_unary = outer_unary
        .children()
        .find(|n| n.kind() == SyntaxKind::SDBL_UNARY_EXPR)
        .expect("inner SdblUnaryExpr nested directly inside outer");
    assert!(
        inner_unary.text().to_string().contains('-'),
        "Inner SdblUnaryExpr text must include the second minus token",
    );
}

// Bucket A: unary minus inside additive right operand — `А + - Б` parses
// as `AdditiveExpr( А, +, UnaryExpr( -, Б ) )` per mini-spec §Operator-
// binding pin list item 4. Verifies that unary nests cleanly under the
// flat-additive wrapper.
#[test]
fn test_slice10a_unary_inside_additive_right_operand() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ А + - Б ИЗ Т";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Unary in additive right operand: {:?}", parse.errors());
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input");
    let additive = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_ADDITIVE_EXPR)
        .expect("SdblAdditiveExpr");
    let unary_under_additive =
        additive.descendants().find(|n| n.kind() == SyntaxKind::SDBL_UNARY_EXPR);
    assert!(
        unary_under_additive.is_some(),
        "SdblUnaryExpr must appear under SdblAdditiveExpr — got tree {additive:#?}",
    );
}

// Bucket A: Russian NOT with nested AND inside parens — `НЕ (А И Б)`
// parses as `NotExpr( НЕ, ParenExpr( LogicalAndExpr( А, И, Б ) ) )`.
// Mini-spec §Atoms paren dispatch + §Operator-binding pin list item 1.
#[test]
fn test_slice10a_russian_not_with_paren_and() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ НЕ (А И Б)";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "НЕ (А И Б) should parse: {:?}", parse.errors());
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input");
    let not_expr =
        root.descendants().find(|n| n.kind() == SyntaxKind::SDBL_NOT_EXPR).expect("SdblNotExpr");
    let paren_in_not = not_expr.descendants().find(|n| n.kind() == SyntaxKind::SDBL_PAREN_EXPR);
    assert!(paren_in_not.is_some(), "SdblParenExpr must sit inside SdblNotExpr for `НЕ (А И Б)`",);
    let and_in_paren =
        paren_in_not.unwrap().descendants().find(|n| n.kind() == SyntaxKind::SDBL_LOGICAL_AND_EXPR);
    assert!(and_in_paren.is_some(), "SdblLogicalAndExpr must sit inside the SdblParenExpr",);
}

// Bucket A: a single user-visible string `"X"` produces 3 internal
// STRING tokens (opening `"`, content, closing `"`) at the lexer
// level, and `string_literal_or_multi` collects every consecutive
// run of STRING tokens into one wrapper. Because count > 1 the
// wrapper is `SdblMultiString` rather than `SdblLiteral` — even
// for what the user sees as one literal. The 3-token internal
// shape is the lexer-level invariant; the input
// `ВЫБРАТЬ "a" "b" "c" ИЗ Т` produces an SdblMultiString wrapping
// `"a"` (3 STRING tokens) at the SELECT-field-head position; the
// trailing `"b" "c"` are recovery noise (whitespace breaks
// `string_literal_or_multi`'s consecutive-only collector). Mini-
// spec §Atoms — string literal multi-part IDE recovery + §Lexical
// assumptions; ITS pubqlang/40 §Литералы string lexical shape.
#[test]
fn test_slice10a_multi_string_three_tokens() {
    use syntax::SyntaxKind;
    let input = r#"ВЫБРАТЬ "a" "b" "c" ИЗ Т"#;
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input");
    let multi = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_MULTI_STRING)
        .expect("SdblMultiString for the first user-visible string");
    let string_token_count = multi
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .filter(|t| t.kind() == SyntaxKind::STRING)
        .count();
    assert_eq!(
        string_token_count, 3,
        "First SdblMultiString must wrap exactly 3 STRING tokens (the lexer's open/content/close split for one user-visible string)",
    );
}

// Bucket A: precedence with newline trivia between operator and operand.
// Mini-spec §Trivia handling convention: `p.skip_trivia()` BEFORE the
// operator probe so operator tokens preceded by whitespace / comments /
// newlines are recognised. Verifies `1\n+\n2 * 3` parses as
// `AdditiveExpr( 1, +, MultiplicativeExpr( 2, *, 3 ) )` — strong
// assertion: SdblAdditiveExpr has a DIRECT PLUS token child (not just
// some descendant), and the SdblMultiplicativeExpr is a DIRECT child of
// the additive wrapper with a DIRECT STAR token covering `2 * 3`.
// Mini-spec §AST-shape invariants #1 (FLAT) and #2 (trivia-before-probe).
#[test]
fn test_slice10a_precedence_with_newline_trivia() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ 1\n+\n2 * 3 ИЗ Т";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Newline trivia in precedence: {:?}", parse.errors());
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input");

    // Find the additive wrapper that has a DIRECT PLUS token child. The
    // parser opens single-child wrappers unconditionally; only the
    // wrapper whose direct token children include PLUS owns the operator.
    let additive_with_plus = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::SDBL_ADDITIVE_EXPR)
        .find(|n| {
            n.children_with_tokens()
                .filter_map(|c| c.into_token())
                .any(|t| t.kind() == SyntaxKind::PLUS)
        })
        .expect("SdblAdditiveExpr with a DIRECT PLUS token child — mini-spec §AST-shape #1");

    // The right operand of `+` is a SdblMultiplicativeExpr direct child
    // with a DIRECT STAR token. Note: the LEFT operand `1` is also wrapped
    // in an SdblMultiplicativeExpr (single-child empty-operator wrapper —
    // mini-spec §AST-shape invariant #1 + empty-wrapper unwrapping note).
    // The wrapper that owns the actual `*` operator is the one whose
    // direct token children include STAR.
    let mul_with_star = additive_with_plus
        .children()
        .filter(|n| n.kind() == SyntaxKind::SDBL_MULTIPLICATIVE_EXPR)
        .find(|n| {
            n.children_with_tokens()
                .filter_map(|c| c.into_token())
                .any(|t| t.kind() == SyntaxKind::STAR)
        })
        .expect(
            "SdblMultiplicativeExpr with a DIRECT STAR token child — the `2 * 3` wrapper sits as a direct child of the additive node",
        );
    let mul_text = mul_with_star.text().to_string();
    assert!(
        mul_text.contains('2') && mul_text.contains('3'),
        "SdblMultiplicativeExpr must cover `2 * 3` (got {mul_text:?})",
    );

    // FLAT-shape guard for additive: exactly ONE PLUS direct token child
    // (mini-spec §AST-shape invariant #1 — flat wrapper for `1 + (2*3)`).
    let plus_count = additive_with_plus
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .filter(|t| t.kind() == SyntaxKind::PLUS)
        .count();
    assert_eq!(
        plus_count, 1,
        "FLAT additive wrapper for `1\\n+\\n2 * 3` must have exactly 1 PLUS direct token child"
    );
}

// Bucket A: flat-associativity guard — `А + Б + В` parses as a SINGLE
// SdblAdditiveExpr with 3 expression children + 2 `+` tokens, NOT a
// nested left-associative tree. Mini-spec §AST-shape invariant #1
// (FLAT operator wrappers) and §Operator-binding pin list item
// "flat-wrapper rule".
#[test]
fn test_slice10a_flat_additive_associativity() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ А + Б + В ИЗ Т";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Flat additive should parse: {:?}", parse.errors());
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input");
    let additives: Vec<_> =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_ADDITIVE_EXPR).collect();
    assert_eq!(additives.len(), 1, "Exactly one SdblAdditiveExpr — wrapper is FLAT, not nested",);
    let additive = &additives[0];
    let plus_tokens = additive
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .filter(|t| t.kind() == SyntaxKind::PLUS)
        .count();
    assert_eq!(
        plus_tokens, 2,
        "FLAT SdblAdditiveExpr must have exactly 2 `+` direct token children for `А + Б + В`",
    );
}

// Bucket A: tuple vs paren distinction at expression level — single
// expression in parens emits SdblParenExpr; 2+ comma-separated
// expressions emit SdblTupleExpr. Mini-spec §Atoms paren dispatch +
// §AST-shape invariant #5.
#[test]
fn test_slice10a_paren_single_vs_tuple_two() {
    use syntax::SyntaxKind;
    let single_input = "ВЫБРАТЬ (1) ИЗ Т";
    let single_parse = parse_sdbl(single_input);
    assert!(
        !single_parse.has_errors(),
        "Single-element paren should parse: {:?}",
        single_parse.errors()
    );
    let single_root = single_parse.syntax_node();
    assert_eq!(single_root.text().to_string(), single_input, "Single root coverage");
    let single_paren = single_root.descendants().find(|n| n.kind() == SyntaxKind::SDBL_PAREN_EXPR);
    let single_tuple = single_root.descendants().find(|n| n.kind() == SyntaxKind::SDBL_TUPLE_EXPR);
    assert!(single_paren.is_some(), "(1) must emit SdblParenExpr");
    assert!(single_tuple.is_none(), "(1) must NOT emit SdblTupleExpr");

    let tuple_input = "ВЫБРАТЬ (1, 2) ИЗ Т";
    let tuple_parse = parse_sdbl(tuple_input);
    assert!(
        !tuple_parse.has_errors(),
        "Two-element tuple should parse: {:?}",
        tuple_parse.errors()
    );
    let tuple_root = tuple_parse.syntax_node();
    assert_eq!(tuple_root.text().to_string(), tuple_input, "Tuple root coverage");
    let tuple = tuple_root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_TUPLE_EXPR)
        .expect("(1, 2) must emit SdblTupleExpr");
    let comma_count = tuple
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .filter(|t| t.kind() == SyntaxKind::COMMA)
        .count();
    assert_eq!(comma_count, 1, "SdblTupleExpr for (1, 2) must have 1 COMMA direct token child");
}

// Bucket A: newline-separated logical operators — `А\nИ\nБ` parses as a
// SdblLogicalAndExpr with a DIRECT KW_AND token child and at least two
// operand subtrees as direct children (mini-spec §AST-shape invariants #1
// + #2 + §IDE-recovery allowances #8). Note: HIR text-based operator
// detection at sdbl-hir/src/lower/expr/ops.rs:64-67 looks for " И " (with
// surrounding spaces) and may fall back to default BinaryOp::Eq when
// newlines replace spaces; that's a Slice 13 follow-up. This test only
// locks the parser-side wrapper shape, not HIR's lowering.
#[test]
fn test_slice10a_newline_separated_logical_and() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ А\nИ\nБ";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Newline-separated AND should parse: {:?}", parse.errors());
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input including newlines");

    // Find the SdblLogicalAndExpr that has a DIRECT KW_AND token child —
    // single-atom wrappers exist throughout the chain, only the wrapper
    // owning the operator has KW_AND as a direct child.
    let and_with_kw = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::SDBL_LOGICAL_AND_EXPR)
        .find(|n| {
            n.children_with_tokens()
                .filter_map(|c| c.into_token())
                .any(|t| t.kind() == SyntaxKind::KW_AND)
        })
        .expect(
            "SdblLogicalAndExpr with a DIRECT KW_AND token child even when separated by newlines",
        );

    // The wrapper must contain at least two operand subtrees (one per side
    // of the AND). Direct children that are nodes — not trivia tokens —
    // are the operands. Slice 10a's chain wraps every operand, so the
    // operand-children count is at least 2 (one for А, one for Б).
    let operand_node_count = and_with_kw.children().count();
    assert!(
        operand_node_count >= 2,
        "SdblLogicalAndExpr must have at least 2 operand subtrees as direct children; got {operand_node_count}",
    );

    let and_text = and_with_kw.text().to_string();
    assert!(
        and_text.contains('А') && and_text.contains('Б'),
        "SdblLogicalAndExpr text must cover both operands (got {and_text:?})",
    );
}

// Bucket A — Slice 10a NULL dispatch regression gate (WHERE side).
//
// Pre-Slice-10a-C2, bare `NULL` was routed through `column_or_function`
// because `sdbl_token_converter.rs:57` maps `LitNull → TokenKind::Ident`
// and the historical `Some(TokenKind::KwNull)` arm in the parser was
// unreachable dead code — bare `NULL` was silently consumed as
// `SdblColumnRef`. Slice 10a C2 added an `at_keyword("NULL")` probe
// before the generic `Ident → column_or_function` arm so bare `NULL`
// now emits `SdblLiteral` wrapping the `Ident` token.
//
// `check_no_errors` alone (as in the existing `test_null_literal` at
// line 290) cannot detect the buggy shape because the pre-fix shape
// was a parse-tree shape bug, not a parse-error bug. This test gates
// the fix structurally. Mini-spec §Atoms primary dispatch + ITS
// pubqlang/40 §Литералы.
#[test]
fn test_slice10a_bare_null_emits_literal_not_column_ref() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ Поле = NULL";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "WHERE …= NULL should parse: {:?}", parse.errors());
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input");

    let null_token = root
        .descendants_with_tokens()
        .filter_map(|c| c.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT && t.text().eq_ignore_ascii_case("NULL"))
        .expect("NULL Ident token must be present");

    let null_parent_kind = null_token.parent().map(|p| p.kind());
    assert_eq!(
        null_parent_kind,
        Some(SyntaxKind::SDBL_LITERAL),
        "Bare NULL must emit SdblLiteral wrapping the Ident token; got parent {null_parent_kind:?}. Pre-Slice-10a-C2 bug placed NULL inside SdblColumnRef.",
    );

    // Defensive cross-angle check: no SdblColumnRef in the tree may
    // contain the NULL text (catches future refactors that change
    // the wrapper layout in a different direction).
    let column_refs_with_null = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::SDBL_COLUMN_REF)
        .filter(|n| n.text().to_string().to_uppercase().contains("NULL"))
        .count();
    assert_eq!(
        column_refs_with_null, 0,
        "No SdblColumnRef may contain the NULL token; got {column_refs_with_null} occurrences.",
    );
}

// Bucket A — Slice 10a NULL at SELECT-field-head position.
//
// Stronger gate: `SELECT NULL FROM Т` places NULL at the head of an
// expression position via the SELECT field list (no comparison
// context). Pre-Slice-10a-C2 this also routed NULL to SdblColumnRef
// via primary_expr's match arm `Some(TokenKind::Ident) =>
// column_or_function(p)`. Mini-spec §Atoms primary dispatch + ITS
// pubqlang/40 §Литералы.
#[test]
fn test_slice10a_select_field_null_emits_literal() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ NULL ИЗ Т";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "SELECT NULL should parse: {:?}", parse.errors());
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input);

    let null_token = root
        .descendants_with_tokens()
        .filter_map(|c| c.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT && t.text().eq_ignore_ascii_case("NULL"))
        .expect("NULL Ident token must be present");
    assert_eq!(
        null_token.parent().map(|p| p.kind()),
        Some(SyntaxKind::SDBL_LITERAL),
        "Bare NULL at SELECT-field position must emit SdblLiteral",
    );
}

// ============================================================================
// Slice 10b C0b Bucket-A gap additions
// ============================================================================
//
// Pre-rewrite regression gate for the Slice 10b clean-room rewrite of
// predicates / comparison / column-or-function / CAST / CASE.
// Authored from `docs/legal/sdbl-expressions-mini-spec.md`
// (C0a-extended) and ITS pubqlang chapters 22, 23, 27, 32, 40 via the
// local dump at `/home/itrous/src/tools_migration/its/dump/`. See
// `docs/legal/sdbl-clean-room-slices.md` §Slice 10b for the slice
// scope.
//
// Tests (a)-(l) and (n.1)-(n.5) MUST pass on the pre-Slice-10b parser:
// they document existing behaviour that the C2 clean-room rewrite
// preserves bit-for-bit. Tests (m) EN/RU are `#[ignore]`-ed in C0b —
// they are the regression gate for the C2 fix to
// `column_or_function`'s clause-keyword recovery (codex Round-1
// finding 2). Slice 10b C2 unignores them in the same atomic commit
// as the fix.

// (a) Empty IN list recovery — `IN ()` accepted as a recoverable
// parse. Mini-spec §Predicates §SdblInExpr + §IDE-recovery
// allowances #10. ITS pubqlang/22 documents IN with a non-empty
// value-list; the empty form is preserved as IDE-recovery for
// mid-typing.
#[test]
fn test_slice10b_empty_in_list_recovery() {
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ Поле В ()";
    let parse = parse_sdbl(input);
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_IN_EXPR"),
        "Empty `IN ()` must still emit SdblInExpr (recoverable parse).\nTree: {}",
        tree
    );
    assert!(
        tree.contains("SDBL_WHERE_CLAUSE"),
        "WHERE clause must be parsed despite empty IN.\nTree: {}",
        tree
    );
}

// (b) NOT IN with subquery — `НЕ В (ВЫБРАТЬ ...)` emits SdblInExpr
// with KwNot before KwIn, and SdblSubquery inside the parens.
// Mini-spec §Predicates §SdblInExpr.
#[test]
fn test_slice10b_not_in_subquery() {
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ Поле НЕ В (ВЫБРАТЬ Х ИЗ С)";
    let parse = parse_sdbl(input);
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_IN_EXPR"),
        "NOT IN with subquery must emit SdblInExpr.\nTree: {}",
        tree
    );
    assert!(
        tree.contains("SDBL_SUBQUERY"),
        "IN-subquery must produce SdblSubquery inside the parens.\nTree: {}",
        tree
    );
}

// (c) IN HIERARCHY Russian variant — `В ИЕРАРХИИ (...)` emits
// SdblInHierarchyExpr. Mini-spec §Predicates §SdblInHierarchyExpr +
// ITS pubqlang/32 canonical example.
#[test]
fn test_slice10b_in_hierarchy_russian() {
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ Поле В ИЕРАРХИИ (&Корень)";
    let parse = parse_sdbl(input);
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_IN_HIERARCHY_EXPR"),
        "В ИЕРАРХИИ must emit SdblInHierarchyExpr.\nTree: {}",
        tree
    );
}

// (d) IS NOT NULL shape — `ЕСТЬ НЕ NULL` emits SdblIsNullExpr with
// KwNot between IS and NULL. Mini-spec §Predicates §SdblIsNullExpr
// + ITS pubqlang/27 canonical example.
#[test]
fn test_slice10b_is_not_null_russian() {
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ Поле ЕСТЬ НЕ NULL";
    let parse = parse_sdbl(input);
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_IS_NULL_EXPR"),
        "ЕСТЬ НЕ NULL must emit SdblIsNullExpr.\nTree: {}",
        tree
    );
}

// (e) BETWEEN missing AND recovery — `МЕЖДУ 1` (no AND high-bound)
// emits SdblBetweenExpr with only the low bound. Mini-spec
// §Predicates §SdblBetweenExpr + §IDE-recovery allowances #12.
#[test]
fn test_slice10b_between_missing_and_recovery() {
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ Поле МЕЖДУ 1";
    let parse = parse_sdbl(input);
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_BETWEEN_EXPR"),
        "МЕЖДУ without AND must still emit SdblBetweenExpr (recovery).\nTree: {}",
        tree
    );
}

// (f) LIKE pattern ESCAPE char — `ПОДОБНО "..." СПЕЦСИМВОЛ "\"`
// emits SdblLikeExpr. ESCAPE/СПЕЦСИМВОЛ is a local IDE-recovery
// allowance (mini-spec §IDE-recovery allowances #13 — not in dumped
// ITS chapters).
#[test]
fn test_slice10b_like_with_escape() {
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ Поле ПОДОБНО \"abc%\" СПЕЦСИМВОЛ \"!\"";
    let parse = parse_sdbl(input);
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_LIKE_EXPR"),
        "ПОДОБНО ... СПЕЦСИМВОЛ must emit SdblLikeExpr.\nTree: {}",
        tree
    );
}

// (g) REFS MDO chain — `ССЫЛКА Документ.ПриходнаяНакладная` emits
// SdblRefsExpr with the MDO chain as direct token children.
// Mini-spec §Predicates §SdblRefsExpr + ITS pubqlang/40 canonical
// example.
#[test]
fn test_slice10b_refs_mdo_chain_russian() {
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ Регистратор ССЫЛКА Документ.ПриходнаяНакладная";
    let parse = parse_sdbl(input);
    assert!(
        !parse.has_errors(),
        "REFS with MDO chain must parse without errors: {:?}",
        parse.errors()
    );
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_REFS_EXPR"),
        "ССЫЛКА Документ.ПриходнаяНакладная must emit SdblRefsExpr.\nTree: {}",
        tree
    );
}

// (h) CASE simple form — `ВЫБОР Т.Х КОГДА ... КОНЕЦ` emits
// SdblCaseExpr whose first child node is the operand expression
// (NOT SdblWhenClause). Mini-spec §CASE expressions
// §Child-order invariant + HIR consumer
// `crates/sdbl-hir/src/lower/expr/case_expr.rs:40-45`.
#[test]
fn test_slice10b_case_simple_form_operand_first() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ ВЫБОР Т.Х КОГДА 1 ТОГДА \"А\" ИНАЧЕ \"Б\" КОНЕЦ ИЗ Т";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Simple CASE must parse: {:?}", parse.errors());

    let case = parse
        .syntax_node()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_CASE_EXPR)
        .expect("Tree must contain SdblCaseExpr");

    let first_child_kind = case.children().next().map(|n| n.kind());
    assert_ne!(
        first_child_kind,
        Some(SyntaxKind::SDBL_WHEN_CLAUSE),
        "Simple CASE first child node must be the operand expression, not SdblWhenClause. Got: {:?}",
        first_child_kind
    );
}

// (i) CASE searched form — `ВЫБОР КОГДА ... КОНЕЦ` (no operand)
// emits SdblCaseExpr whose first child node is SdblWhenClause.
// Mini-spec §CASE expressions §Child-order invariant.
#[test]
fn test_slice10b_case_searched_form_when_first() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ ВЫБОР КОГДА Т.Х = 1 ТОГДА \"А\" КОНЕЦ ИЗ Т";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "Searched CASE must parse: {:?}", parse.errors());

    let case = parse
        .syntax_node()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_CASE_EXPR)
        .expect("Tree must contain SdblCaseExpr");

    let first_child_kind = case.children().next().map(|n| n.kind());
    assert_eq!(
        first_child_kind,
        Some(SyntaxKind::SDBL_WHEN_CLAUSE),
        "Searched CASE first child node must be SdblWhenClause (no operand). Got: {:?}",
        first_child_kind
    );
}

// (j) CAST primitive parameterised type — `ВЫРАЗИТЬ(Поле КАК
// СТРОКА(200))` emits SdblFunctionCall containing SdblType with the
// primitive type Ident plus the (decimal) parameter list. Mini-spec
// §CAST type specification + ITS pubqlang/40 canonical example.
#[test]
fn test_slice10b_cast_primitive_parameterised() {
    let input = "ВЫБРАТЬ ВЫРАЗИТЬ(Поле КАК СТРОКА(200)) ИЗ Т";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "CAST(... AS STRING(200)) must parse: {:?}", parse.errors());
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_FUNCTION_CALL"),
        "CAST is dispatched as SdblFunctionCall.\nTree: {}",
        tree
    );
    assert!(tree.contains("SDBL_TYPE"), "CAST type spec must emit SdblType.\nTree: {}", tree);
}

// (k) CAST MDO type and member access — `ВЫРАЗИТЬ(Регистратор КАК
// Документ.ПриходнаяНакладная).Поставщик` emits SdblFunctionCall
// containing SdblType (MDO chain) AND a post-RParen Dot/Ident chain
// (member access). Mini-spec §CAST type specification +
// §SdblFunctionCall member access + ITS pubqlang/40.
#[test]
fn test_slice10b_cast_mdo_with_member_access() {
    let input = "ВЫБРАТЬ ВЫРАЗИТЬ(Регистратор КАК Документ.ПриходнаяНакладная).Поставщик ИЗ Т";
    let parse = parse_sdbl(input);
    assert!(!parse.has_errors(), "CAST(MDO).Поле must parse: {:?}", parse.errors());
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_FUNCTION_CALL"),
        "CAST is dispatched as SdblFunctionCall.\nTree: {}",
        tree
    );
    assert!(tree.contains("SDBL_TYPE"), "MDO CAST type must emit SdblType.\nTree: {}", tree);
    assert!(
        tree.contains("Поставщик"),
        "Member access on CAST result must be preserved in the tree.\nTree: {}",
        tree
    );
}

// (l) Inline tabular field syntax — `Т.ТабЧасть.(Поле1, Поле2)`
// emits SdblColumnRef containing SdblInlineTableFields wrapping
// SdblSelectedField children. Mini-spec §Inline tabular field
// syntax. The Slice-10b → Slice-7 dispatch boundary.
#[test]
fn test_slice10b_inline_tabular_field_syntax() {
    let input = "ВЫБРАТЬ Т.ТабЧасть.(Поле1, Поле2) ИЗ Т";
    let parse = parse_sdbl(input);
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_INLINE_TABLE_FIELDS"),
        "Inline tabular fields must emit SdblInlineTableFields.\nTree: {}",
        tree
    );
    assert!(
        tree.contains("SDBL_SELECTED_FIELD"),
        "Inline tabular fields must wrap SdblSelectedField children.\nTree: {}",
        tree
    );
}

// (m) Function-call clause-keyword recovery — `func(x, FROM T)`
// must NOT consume FROM as an Ident-shaped argument; the FROM
// clause must remain detectable for the outer SELECT. Codex
// Round-1 finding 2 → Slice 10b C2 FIX. The C2 commit lands a
// `&& !is_clause_keyword` clause at both arg-start probes in
// `column_or_function`. Mini-spec §Column references and function
// calls §SdblFunctionCall + §IDE-recovery allowances #15.
//
// `#[ignore]`-ed in C0b: the pre-C2 parser hijacks FROM as an
// Ident-shaped argument, so this test FAILS on the pre-rewrite
// parser. Slice 10b C2 unignores it in the same atomic commit as
// the fix.
#[test]
fn test_func_call_clause_keyword_recovery() {
    use syntax::SyntaxKind;
    let input = "SELECT func(x, FROM T)";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    // Outer SELECT must still recognise FROM T as the FROM clause —
    // i.e. SDBL_FROM_CLAUSE must appear in the tree.
    let from_clauses =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE).count();
    assert!(
        from_clauses >= 1,
        "Outer SELECT must keep its FROM clause despite the unbalanced func call.\nTree: {:#?}",
        root
    );

    // The function call must NOT contain FROM as a direct
    // argument-position Ident — the keyword filter at the
    // arg-start probe should reject FROM.
    let func_call = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_FUNCTION_CALL)
        .expect("Tree must contain SdblFunctionCall");
    let func_text = func_call.text().to_string();
    assert!(
        !func_text.to_uppercase().contains("FROM"),
        "Function call must NOT consume FROM as an argument: got `{}`",
        func_text
    );
}

// (m, RU) Russian variant of the function-call clause-keyword
// recovery regression gate. Same contract as
// `test_func_call_clause_keyword_recovery` for ИЗ.
#[test]
fn test_russian_func_call_clause_keyword_recovery() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ функ(х, ИЗ Т)";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    let from_clauses =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE).count();
    assert!(
        from_clauses >= 1,
        "Outer ВЫБРАТЬ must keep its ИЗ clause despite the unbalanced func call.\nTree: {:#?}",
        root
    );

    let func_call = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_FUNCTION_CALL)
        .expect("Tree must contain SdblFunctionCall");
    let func_text = func_call.text().to_string();
    assert!(
        !func_text.to_uppercase().contains("ИЗ"),
        "Function call must NOT consume ИЗ as an argument: got `{}`",
        func_text
    );
}

// ----------------------------------------------------------------------------
// (n.1)-(n.5) SELECT-field predicate descendant guards.
//
// Producer-side invariant: `expression(p)` always wraps in
// `logical_or_expr` (Slice 10a) so consumer-side
// `SdblSelectedField::expression()` (which directly matches only 3
// of the 13 Slice-10b kinds — COLUMN_REF, FUNCTION_CALL,
// COMPARISON_EXPR) reaches the predicate / CASE node via
// descendant traversal. Codex Round-1 finding 3 + Round-3 expansion.
//
// Each guard test asserts:
//  1. SdblSelectedField direct child is SdblLogicalOrExpr;
//  2. SdblSelectedField direct child is NOT a bare predicate /
//     CASE / comparison node.
// ----------------------------------------------------------------------------

fn first_selected_field_direct_child_kinds(input: &str) -> Vec<syntax::SyntaxKind> {
    use syntax::SyntaxKind;
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    let field = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_SELECTED_FIELD)
        .expect("Tree must contain SdblSelectedField");
    field.children().map(|n| n.kind()).collect()
}

// (n.1) SELECT-field comparison descendant guard.
#[test]
fn test_select_field_comparison_descendant_guard() {
    use syntax::SyntaxKind;
    let kinds = first_selected_field_direct_child_kinds("ВЫБРАТЬ Поле = 1 ИЗ Т");
    assert!(
        kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR),
        "SelectedField must have SdblLogicalOrExpr as direct child. Got: {:?}",
        kinds
    );
    assert!(
        !kinds.contains(&SyntaxKind::SDBL_COMPARISON_EXPR),
        "SelectedField must NOT have bare SdblComparisonExpr — it must be wrapped in LogicalOrExpr. Got: {:?}",
        kinds
    );
}

// (n.2) SELECT-field IN descendant guard.
#[test]
fn test_select_field_in_descendant_guard() {
    use syntax::SyntaxKind;
    let kinds = first_selected_field_direct_child_kinds("ВЫБРАТЬ Поле В (1, 2) ИЗ Т");
    assert!(
        kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR),
        "SelectedField must have SdblLogicalOrExpr as direct child. Got: {:?}",
        kinds
    );
    assert!(
        !kinds.contains(&SyntaxKind::SDBL_IN_EXPR),
        "SelectedField must NOT have bare SdblInExpr — it must be wrapped in LogicalOrExpr. Got: {:?}",
        kinds
    );
}

// (n.3) SELECT-field BETWEEN descendant guard.
#[test]
fn test_select_field_between_descendant_guard() {
    use syntax::SyntaxKind;
    let kinds = first_selected_field_direct_child_kinds("ВЫБРАТЬ Поле МЕЖДУ 1 И 5 ИЗ Т");
    assert!(
        kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR),
        "SelectedField must have SdblLogicalOrExpr as direct child. Got: {:?}",
        kinds
    );
    assert!(
        !kinds.contains(&SyntaxKind::SDBL_BETWEEN_EXPR),
        "SelectedField must NOT have bare SdblBetweenExpr — it must be wrapped in LogicalOrExpr. Got: {:?}",
        kinds
    );
}

// (n.4) SELECT-field IS NULL descendant guard.
#[test]
fn test_select_field_is_null_descendant_guard() {
    use syntax::SyntaxKind;
    let kinds = first_selected_field_direct_child_kinds("ВЫБРАТЬ Поле ЕСТЬ NULL ИЗ Т");
    assert!(
        kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR),
        "SelectedField must have SdblLogicalOrExpr as direct child. Got: {:?}",
        kinds
    );
    assert!(
        !kinds.contains(&SyntaxKind::SDBL_IS_NULL_EXPR),
        "SelectedField must NOT have bare SdblIsNullExpr — it must be wrapped in LogicalOrExpr. Got: {:?}",
        kinds
    );
}

// (n.5) SELECT-field CASE descendant guard.
#[test]
fn test_select_field_case_descendant_guard() {
    use syntax::SyntaxKind;
    let kinds =
        first_selected_field_direct_child_kinds("ВЫБРАТЬ ВЫБОР КОГДА 1 = 1 ТОГДА \"А\" КОНЕЦ ИЗ Т");
    assert!(
        kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR),
        "SelectedField must have SdblLogicalOrExpr as direct child. Got: {:?}",
        kinds
    );
    assert!(
        !kinds.contains(&SyntaxKind::SDBL_CASE_EXPR),
        "SelectedField must NOT have bare SdblCaseExpr — it must be wrapped in LogicalOrExpr. Got: {:?}",
        kinds
    );
}

// ----------------------------------------------------------------------------
// Slice 9 (JOIN family) Bucket-A gap tests — parser-side AST-shape guards.
//
// Pin parser-side invariants that downstream consumers (sdbl-hir,
// ide-diagnostics) read. Per `sdbl-clean-room-slice9` plan v9, all 15
// pass on the pre-rewrite parser. Tier classification per test:
//   #1-#4  Tier A1 — ITS chapters 44/45/46/47 listings (RU canonical).
//   #5-#6  Tier A2 OR Tier D candidates — bare ПОЛНОЕ/ЛЕВОЕ without
//          ВНЕШНЕЕ; final tier set by C2 author after chapter prose
//          verification.
//   #7-#8  Tier C — SELECT mini-spec §JOIN clauses + chapter 44
//          standalone СОЕДИНЕНИЕ.
//   #9-#10 Tier A1 — chapter 48 chained / nested JOINs.
//   #11-#13 Parser-side AST-shape guards for the three HIR diagnostics
//          (JoinWithSubQuery / JoinWithVirtualTable / LogicalOrInJoin).
//   #14-#15 Audit-gate tests for the two `Parser::error()`-bumps in
//          `join_clause`. Locked to current behavior so C2 can either
//          flip them (Option A FIX) or preserve them (Option B).
// ----------------------------------------------------------------------------

/// Assert a clean parse: both the parser-error list AND the syntax tree
/// must be free of `ERROR` recovery nodes. `Parser::error()` inserts
/// `SyntaxKind::ERROR` into the tree without populating `Parse::errors()`,
/// so checking only `has_errors()` would let recovered parses slip through.
fn assert_clean_parse(parse: &syntax::Parse<syntax::SyntaxNode>, input: &str) {
    use syntax::SyntaxKind;
    assert!(
        !parse.has_errors(),
        "Expected clean parse for `{}`; got errors: {:#?}",
        input,
        parse.errors(),
    );
    let error_nodes: Vec<_> =
        parse.syntax_node().descendants().filter(|n| n.kind() == SyntaxKind::ERROR).collect();
    assert!(
        error_nodes.is_empty(),
        "Expected clean parse for `{}` — but tree contains {} ERROR recovery node(s): {:#?}",
        input,
        error_nodes.len(),
        error_nodes,
    );
}

fn find_first_join_clause(input: &str) -> syntax::SyntaxNode {
    use syntax::SyntaxKind;
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    parse
        .syntax_node()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_JOIN_CLAUSE)
        .expect("Tree must contain SdblJoinClause")
}

// (1) Tier A1 — chapter 44 ВНУТРЕННЕЕ СОЕДИНЕНИЕ canonical RU listing.
#[test]
fn test_slice9_canonical_inner_join_ru() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join_node =
        find_first_join_clause("ВЫБРАТЬ * ИЗ Т1 ВНУТРЕННЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А");
    let join = SdblJoinClause::cast(join_node).expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Inner);
    assert!(join.data_source().is_some(), "JOIN must carry a joined SdblDataSource child");
}

// (2) Tier A1 — chapter 45 ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ canonical RU listing.
#[test]
fn test_slice9_canonical_left_outer_join_ru() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join_node =
        find_first_join_clause("ВЫБРАТЬ * ИЗ Т1 ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А");
    let join = SdblJoinClause::cast(join_node).expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Left);
}

// (3) Tier A1 — chapter 46 ПРАВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ canonical RU listing.
#[test]
fn test_slice9_canonical_right_outer_join_ru() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join_node =
        find_first_join_clause("ВЫБРАТЬ * ИЗ Т1 ПРАВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А");
    let join = SdblJoinClause::cast(join_node).expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Right);
}

// (4) Tier A1 — chapter 47 ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ canonical RU listing.
#[test]
fn test_slice9_canonical_full_outer_join_ru() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join_node =
        find_first_join_clause("ВЫБРАТЬ * ИЗ Т1 ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А");
    let join = SdblJoinClause::cast(join_node).expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Full);
}

// (5) Bare ПОЛНОЕ — FULL without ВНЕШНЕЕ. Tier classification by C2
// author (Tier A2 if chapter 47 prose attests OUTER optionality, else
// Tier D local-allowance guard). Locks current parser behavior:
// `is_join_keyword` accepts ПОЛНОЕ as a starter and `join_type()`
// substring-matches it back to JoinType::Full.
#[test]
fn test_slice9_bare_full_join_ru() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join_node = find_first_join_clause("ВЫБРАТЬ * ИЗ Т1 ПОЛНОЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А");
    let join = SdblJoinClause::cast(join_node).expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Full);
}

// (6) Bare ЛЕВОЕ — LEFT without ВНЕШНЕЕ. Tier classification by C2
// author (Tier A2 if chapter 45 prose-note attests ВНЕШНЕЕ optionality,
// else Tier D).
#[test]
fn test_slice9_bare_left_join_ru() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join_node = find_first_join_clause("ВЫБРАТЬ * ИЗ Т1 ЛЕВОЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А");
    let join = SdblJoinClause::cast(join_node).expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Left);
}

// (7) Bare JOIN (implicit INNER, EN). Tier C SELECT mini-spec §JOIN
// clauses (line 318).
#[test]
fn test_slice9_bare_join_en() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join_node = find_first_join_clause("SELECT * FROM T1 JOIN T2 ON T1.A = T2.A");
    let join = SdblJoinClause::cast(join_node).expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Inner);
}

// (8) Bare СОЕДИНЕНИЕ (implicit INNER, RU). Tier C / chapter 44
// standalone (final classification at C2 author time).
#[test]
fn test_slice9_bare_join_ru() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join_node = find_first_join_clause("ВЫБРАТЬ * ИЗ Т1 СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А");
    let join = SdblJoinClause::cast(join_node).expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Inner);
}

// (9) Chained JOINs at the same data source — chapter 48 listing.
// Both JOIN clauses attach as direct children of T1's SdblDataSource.
#[test]
fn test_slice9_chained_joins_same_source() {
    use syntax::ast::{AstNode, SdblQueryPackage};
    let input = "SELECT * FROM T1 JOIN T2 ON T1.A = T2.A JOIN T3 ON T1.B = T3.B";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("query package");
    let select_query = package.queries().next().expect("query");
    let main = select_query.subquery().and_then(|s| s.main_query()).expect("main query");
    let from = main.from_clause().expect("FROM clause");
    let t1_source = from.data_sources().next().expect("first data source");
    let join_count = t1_source.join_clauses().count();
    assert_eq!(
        join_count, 2,
        "Both chained JOINs must attach as direct children of T1's SdblDataSource",
    );
}

// (10) Nested JOIN inside JOIN'ed source — chapter 48 nested example.
// Asserts the placement invariant: outer LEFT JOIN attaches to T1's
// SdblDataSource; the inner bare JOIN attaches to the OUTER JOIN's
// data_source (i.e. T2's SdblDataSource), NOT to T1's. The inner
// `join_type()` walks up to T2's data source for parent-tokens
// fallback — that source does NOT carry LEFT, so the default
// JoinType::Inner fires (Invariant #6).
#[test]
fn test_slice9_nested_join_inside_join() {
    use syntax::ast::{AstNode, JoinType, SdblQueryPackage};
    let input = "SELECT * FROM T1 LEFT JOIN T2 JOIN T3 ON T2.B = T3.B ON T1.A = T2.A";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("query package");
    let select_query = package.queries().next().expect("query");
    let main = select_query.subquery().and_then(|s| s.main_query()).expect("main query");
    let from = main.from_clause().expect("FROM clause");
    let t1_source = from.data_sources().next().expect("T1 source");
    let outer_join = t1_source.join_clauses().next().expect("outer LEFT JOIN");
    assert_eq!(outer_join.join_type(), JoinType::Left);
    let t2_source = outer_join.data_source().expect("T2 source under outer JOIN");
    let inner_join =
        t2_source.join_clauses().next().expect("inner JOIN attached to T2's data source");
    assert_eq!(
        inner_join.join_type(),
        JoinType::Inner,
        "Inner bare JOIN must default to Inner via parent-tokens fallback over T2's data source",
    );
}

// (11) FROM-side subquery + JOIN AST-shape guard.
// Pins Invariant #7: outer SdblDataSource carries BOTH subquery() Some
// AND join_clauses().next() Some. The JoinWithSubQuery HIR diagnostic
// (`crates/ide-diagnostics/src/handlers/join_with_sub_query.rs`)
// reads exactly this shape.
#[test]
fn test_slice9_from_subquery_with_join_ast_shape() {
    use syntax::ast::{AstNode, SdblQueryPackage};
    let input = "SELECT * FROM (SELECT * FROM T1) AS S LEFT JOIN T2 ON S.A = T2.A";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("query package");
    let select_query = package.queries().next().expect("query");
    let main = select_query.subquery().and_then(|s| s.main_query()).expect("main query");
    let from = main.from_clause().expect("FROM clause");
    let s_source = from.data_sources().next().expect("subquery data source");
    assert!(
        s_source.subquery().is_some(),
        "Outer SdblDataSource must carry SdblSubquery as direct child",
    );
    assert!(
        s_source.join_clauses().next().is_some(),
        "Outer SdblDataSource must also carry the LEFT JOIN as direct child",
    );
}

// (12) FROM-side virtual-table + JOIN AST-shape guard.
// Pins Invariant #7 for the JoinWithVirtualTable HIR diagnostic
// (`crates/ide-diagnostics/src/handlers/join_with_virtual_table.rs`).
#[test]
fn test_slice9_from_virtual_table_with_join_ast_shape() {
    use syntax::ast::{AstNode, SdblQueryPackage};
    let input = "ВЫБРАТЬ * ИЗ РегистрНакопления.ТоварыНаСкладах.Остатки(&Дата) КАК Р \
                 ЛЕВОЕ СОЕДИНЕНИЕ Т2 ПО Р.Х = Т2.Х";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("query package");
    let select_query = package.queries().next().expect("query");
    let main = select_query.subquery().and_then(|s| s.main_query()).expect("main query");
    let from = main.from_clause().expect("FROM clause");
    let r_source = from.data_sources().next().expect("virtual-table data source");
    assert!(
        r_source.table_ref().is_some(),
        "Outer SdblDataSource must carry SdblTableRef (virtual table) as direct child",
    );
    assert!(
        r_source.join_clauses().next().is_some(),
        "Outer SdblDataSource must also carry the JOIN as direct child",
    );
}

// (13) OR-in-ON parser-side AST-shape guard.
// Pins the AST shape that LogicalOrInJoin reads
// (`crates/sdbl-hir/src/lower/join_clause.rs:188`): SdblJoinClause's
// ON-condition is wrapped in SdblLogicalOrExpr (Slice 10a), which then
// holds the OR.
#[test]
fn test_slice9_or_in_on_ast_shape() {
    use syntax::SyntaxKind;
    let input = "SELECT * FROM T1 JOIN T2 ON T1.A = T2.A OR T1.B = T2.B";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let join = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_JOIN_CLAUSE)
        .expect("Tree must contain SdblJoinClause");
    let direct_kinds: Vec<SyntaxKind> = join.children().map(|c| c.kind()).collect();
    assert!(
        direct_kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR),
        "ON-condition must wrap in SdblLogicalOrExpr direct child of SdblJoinClause. Got: {:?}",
        direct_kinds,
    );
}

// (14) Audit-gate: missing JOIN keyword after LEFT.
// Locks pre-rewrite behavior — `Parser::error()` at select.rs:984
// BUMPS the next token (T2) into an ERROR node attached as a direct
// child of SdblJoinClause, then `m.complete()` runs anyway. The
// outer parse does NOT raise `has_errors()` (the error lives only as
// a syntax-tree ERROR node). At C2 the author chooses Option A FIX
// (mirror Slice 10b column_or_function: zero-width ERROR, do NOT
// bump T2 — flip this test in the same atomic commit) or Option B
// PRESERVE (this test stays).
#[test]
fn test_slice9_missing_join_keyword_current_behavior() {
    use syntax::SyntaxKind;
    let parse = parse_sdbl("SELECT * FROM T1 LEFT T2 ON A = B");
    let root = parse.syntax_node();
    let join = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_JOIN_CLAUSE)
        .expect("SdblJoinClause marker must still be completed on missing JOIN keyword");
    let error_children: Vec<_> =
        join.children().filter(|c| c.kind() == SyntaxKind::ERROR).collect();
    assert_eq!(
        error_children.len(),
        1,
        "Current behavior: exactly one ERROR node attaches as direct child of SdblJoinClause",
    );
    let error_text = error_children[0].text().to_string();
    assert!(
        error_text.contains("T2"),
        "Current behavior: `p.error()` BUMPS T2 into the ERROR node. Got: `{}`",
        error_text,
    );
}

// (15) Audit-gate: missing ON keyword between JOIN'ed source and
// condition. Same locking pattern as #14 (`Parser::error()` at
// select.rs:997). Bumps the `=` token into an ERROR node that
// attaches as a direct child of SdblJoinClause (after the joined
// SdblDataSource).
#[test]
fn test_slice9_missing_on_current_behavior() {
    use syntax::SyntaxKind;
    let parse = parse_sdbl("SELECT * FROM T1 JOIN T2 A = B");
    let root = parse.syntax_node();
    let join = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_JOIN_CLAUSE)
        .expect("SdblJoinClause marker must still be completed on missing ON keyword");
    let error_children: Vec<_> =
        join.children().filter(|c| c.kind() == SyntaxKind::ERROR).collect();
    assert_eq!(
        error_children.len(),
        1,
        "Current behavior: exactly one ERROR node attaches as direct child of SdblJoinClause",
    );
    let error_text = error_children[0].text().to_string();
    assert!(
        error_text.contains('='),
        "Current behavior: `p.error()` BUMPS the `=` token (after T2's alias `A` was consumed) into the ERROR node. Got: `{}`",
        error_text,
    );
}

// ============================================================
// Slice 11 (clauses-after-FROM) C0b Bucket-A gap tests.
//
// These 14 tests pin pre-Slice-11-C2 parser behaviour for the
// post-FROM clause family (WHERE / GROUP BY / HAVING / ORDER BY /
// AUTOORDER / TOTALS BY / FOR UPDATE / INDEX BY plus the two
// dispatchers and `is_clause_keyword`). All but test (g) pass on
// the pre-rewrite parser; test (g) is the regression-gate for the
// MANDATORY C2 FIX (HIERARCHY consumption per ITS chapter 27 —
// `chapter_027.html:39, 51` `УПОРЯДОЧИТЬ ПО Наименование
// ИЕРАРХИЯ`) and lands `#[ignore]`-ed at C0b. C2 unignores it
// atomically with the `order_by_item` HIERARCHY consumption fix.
//
// Coverage maps onto the four §IDE-recovery allowances and the
// AST-shape invariants enumerated in the Slice 11 plan
// (serialized-moseying-orbit.md).
// ============================================================

/// Recursive-walk replica of
/// `crates/sdbl-hir/src/lower/clauses.rs:170-192`
/// `collect_or_tokens_excluding_subqueries` — counts KW_OR tokens
/// reachable from `node` via `children_with_tokens()` recursion,
/// skipping `SDBL_SUBQUERY` / `SDBL_SUBQUERY_EXPR` /
/// `SDBL_SELECT_QUERY` descendants. Pins the consumer-side
/// recursive-walk reachability invariant for the
/// `LogicalOrInWhere` IDE diagnostic.
fn count_kw_or_excluding_subqueries(node: &syntax::SyntaxNode) -> usize {
    use syntax::{NodeOrToken, SyntaxKind};
    let mut total = 0usize;
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::KW_OR => {
                total += 1;
            }
            NodeOrToken::Node(n)
                if !matches!(
                    n.kind(),
                    SyntaxKind::SDBL_SUBQUERY
                        | SyntaxKind::SDBL_SUBQUERY_EXPR
                        | SyntaxKind::SDBL_SELECT_QUERY,
                ) =>
            {
                total += count_kw_or_excluding_subqueries(&n);
            }
            _ => {}
        }
    }
    total
}

// (a) Slice 11 — WHERE with KW_OR token reachable via recursive
// walk from SdblWhereClause (LogicalOrInWhere producer-side gate;
// the token sits as a direct child of the inner SdblLogicalOrExpr
// wrapper, NOT as a direct token child of SdblWhereClause).
#[test]
fn test_slice11_where_kw_or_recursive_walk_reachable() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ A = 1 ИЛИ B = 2";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let where_clause = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_WHERE_CLAUSE)
        .expect("Tree must contain SdblWhereClause");

    let direct_kw_or: Vec<_> = where_clause
        .children_with_tokens()
        .filter_map(|c| c.as_token().filter(|t| t.kind() == SyntaxKind::KW_OR).cloned())
        .collect();
    assert_eq!(
        direct_kw_or.len(),
        0,
        "KW_OR must NOT be a direct token child of SdblWhereClause — \
         it sits inside the SdblLogicalOrExpr wrapper. Got direct KW_OR tokens: {:?}",
        direct_kw_or,
    );

    let recursive_count = count_kw_or_excluding_subqueries(&where_clause);
    assert_eq!(
        recursive_count, 1,
        "Recursive walk (mirroring \
         collect_or_tokens_excluding_subqueries) must find exactly one \
         KW_OR token reachable from SdblWhereClause through non-subquery \
         descendants. Got: {}",
        recursive_count,
    );
}

// (b) Slice 11 — WHERE with subquery: outer recursive walk does
// NOT descend through SDBL_SUBQUERY to count inner KW_OR tokens.
// The outer walk finds zero, the inner subquery's own SdblWhereClause
// recursive walk finds exactly one.
#[test]
fn test_slice11_where_recursive_walk_skips_subquery() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ * ИЗ Т ГДЕ A В (ВЫБРАТЬ X ИЗ С ГДЕ X = 1 ИЛИ X = 2)";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();

    let where_clauses: Vec<_> =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_WHERE_CLAUSE).collect();
    assert_eq!(
        where_clauses.len(),
        2,
        "Expected outer + inner SdblWhereClause; got {}",
        where_clauses.len(),
    );

    // Sort by subquery-ancestor depth: the outer WHERE has zero
    // subquery ancestors, the inner WHERE has at least one.
    let mut sorted = where_clauses.clone();
    sorted.sort_by_key(|w| {
        w.ancestors()
            .filter(|a| {
                matches!(
                    a.kind(),
                    SyntaxKind::SDBL_SUBQUERY
                        | SyntaxKind::SDBL_SUBQUERY_EXPR
                        | SyntaxKind::SDBL_SELECT_QUERY,
                )
            })
            .count()
    });
    let outer = &sorted[0];
    let inner = &sorted[1];

    let outer_count = count_kw_or_excluding_subqueries(outer);
    let inner_count = count_kw_or_excluding_subqueries(inner);
    assert_eq!(
        outer_count, 0,
        "Outer SdblWhereClause recursive walk (skipping subquery kinds) \
         must find zero KW_OR — the only OR is inside the subquery. Got: {}",
        outer_count,
    );
    assert_eq!(
        inner_count, 1,
        "Inner subquery's SdblWhereClause recursive walk must find \
         exactly one KW_OR (the inner OR). Got: {}",
        inner_count,
    );
}

/// Strong bare-keyword recovery assertion: a missing-BY clause
/// must contain ONLY the leading keyword token, no direct child
/// nodes at all, and no other non-trivia tokens. Used by tests
/// (c), (d), and (e) to lock the strict bare-keyword recovery
/// shape — a regression that bumps the trailing `A` as a raw
/// IDENT token, or wraps it in any other node, would fail this
/// assertion.
fn assert_bare_keyword_clause(clause: &syntax::SyntaxNode, expected_keyword: &str) {
    use syntax::{NodeOrToken, SyntaxKind};
    assert_eq!(
        clause.children().count(),
        0,
        "Bare-keyword shape: `{}` clause must have zero direct child \
         nodes; got: {:?}",
        expected_keyword,
        clause.children().map(|c| c.kind()).collect::<Vec<_>>(),
    );
    let non_trivia_tokens: Vec<(SyntaxKind, String)> = clause
        .children_with_tokens()
        .filter_map(|c| match c {
            NodeOrToken::Token(t)
                if !matches!(
                    t.kind(),
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT
                ) =>
            {
                Some((t.kind(), t.text().to_string()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        non_trivia_tokens.len(),
        1,
        "Bare-keyword shape: `{}` clause must contain exactly one \
         non-trivia token (the leading keyword). Got: {:?}",
        expected_keyword,
        non_trivia_tokens,
    );
    let leading = &non_trivia_tokens[0].1.to_uppercase();
    assert_eq!(
        leading,
        &expected_keyword.to_uppercase(),
        "Bare-keyword shape: leading keyword text mismatch",
    );
    assert!(
        clause.descendants().all(|n| n == *clause || n.kind() != SyntaxKind::ERROR),
        "Bare-keyword shape: `{}` clause must contain no ERROR descendants",
        expected_keyword,
    );
}

// (c) Slice 11 — GROUP BY missing-BY recovery (§IDE-recovery
// allowance #3). The leading СГРУППИРОВАТЬ keyword is consumed
// via eat_sdbl_keyword BEFORE the BY check; the early-return
// emits a bare SdblGroupClause containing only the leading
// keyword (NO direct child nodes, NO non-trivia tokens beyond
// the keyword — the trailing `A` falls through outside the
// clause).
#[test]
fn test_slice11_group_missing_by_recovery() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ A ИЗ Т СГРУППИРОВАТЬ A";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    let group = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_GROUP_CLAUSE)
        .expect("Tree must contain SdblGroupClause even when BY is missing");

    assert_bare_keyword_clause(&group, "СГРУППИРОВАТЬ");
}

// (d) Slice 11 — ORDER BY missing-BY recovery (§IDE-recovery
// allowance #3, parallel shape to GROUP — strict bare-keyword
// node).
#[test]
fn test_slice11_order_missing_by_recovery() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ A ИЗ Т УПОРЯДОЧИТЬ A";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    let order = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_ORDER_CLAUSE)
        .expect("Tree must contain SdblOrderClause even when BY is missing");
    assert_bare_keyword_clause(&order, "УПОРЯДОЧИТЬ");
}

// (e) Slice 11 — INDEX BY missing-BY recovery (§IDE-recovery
// allowance #3 — strict bare-keyword node).
#[test]
fn test_slice11_index_missing_by_recovery() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ A ИЗ Т ИНДЕКСИРОВАТЬ A";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    let index = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_INDEX_BY)
        .expect("Tree must contain SdblIndexBy even when BY is missing");
    assert_bare_keyword_clause(&index, "ИНДЕКСИРОВАТЬ");
}

// (f) Slice 11 — TOTALS missing-BY recovery (§IDE-recovery
// allowance #3, TOTALS variant). Unlike GROUP/ORDER/INDEX, the
// pre-BY aggregate-expression loop runs FIRST at
// select.rs:1359-1386, so `ИТОГИ A` (no BY) produces a
// SdblTotalsBy containing the leading ИТОГИ token PLUS A as a
// pre-BY expression child.
#[test]
fn test_slice11_totals_missing_by_recovery() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ A ИЗ Т ИТОГИ A";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    let totals = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_TOTALS_BY)
        .expect("Tree must contain SdblTotalsBy even when BY is missing");

    let has_by_token = totals.children_with_tokens().any(|c| {
        c.as_token().is_some_and(|t| {
            let s = t.text().to_uppercase();
            s == "BY" || s == "ПО"
        })
    });
    assert!(
        !has_by_token,
        "Missing-BY recovery: SdblTotalsBy must NOT contain a BY/ПО token \
         when BY is absent in the input",
    );
    // Pre-BY aggregate loop consumed `A` before the BY check failed.
    let pre_by_expr_kinds: Vec<_> = totals
        .children()
        .map(|c| c.kind())
        .filter(|k| {
            matches!(
                k,
                SyntaxKind::SDBL_LOGICAL_OR_EXPR
                    | SyntaxKind::SDBL_COLUMN_REF
                    | SyntaxKind::SDBL_FUNCTION_CALL,
            )
        })
        .collect();
    assert!(
        !pre_by_expr_kinds.is_empty(),
        "Pre-BY aggregate-expression loop must consume `A` BEFORE the \
         missing-BY check; SdblTotalsBy direct children should include \
         at least one expression kind. Got direct child kinds: {:?}",
        totals.children().map(|c| c.kind()).collect::<Vec<_>>(),
    );
}

// (g) Slice 11 — ORDER BY with HIERARCHY modifier consumed as an
// order-by-item modifier (regression gate for the MANDATORY C2
// FIX promoted per codex Round-1 finding 2; ITS chapter 27
// attestation `chapter_027.html:39, 51` —
// `УПОРЯДОЧИТЬ ПО Наименование ИЕРАРХИЯ`).
//
// LANDED `#[ignore]`-ED IN C0b. C2 atomically (a) extended
// `order_by_item` to consume the optional HIERARCHY/ИЕРАРХИЯ
// modifier after ASC/DESC (per ITS chapter 27 mandatory fix —
// `chapter_027.html:39, 51`), AND (b) removed the `#[ignore]`.
// This test is now an ACTIVE regression gate.
#[test]
fn test_slice11_order_by_hierarchy_consumed() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ A ИЗ Т УПОРЯДОЧИТЬ ПО A ИЕРАРХИЯ";
    let parse = parse_sdbl(input);
    // Once C2 unignores this gate, the canonical ITS-attested
    // input must parse cleanly with no ERROR descendants and
    // no parser errors anywhere in the tree.
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let order = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_ORDER_CLAUSE)
        .expect("Tree must contain SdblOrderClause");

    // The ИЕРАРХИЯ token must end up INSIDE SdblOrderClause as a
    // flat sibling token (no per-item wrapper), NOT left in the
    // outer token stream after the clause.
    let has_hierarchy_token = order.children_with_tokens().any(|c| {
        c.as_token().is_some_and(|t| {
            let s = t.text().to_uppercase();
            s == "HIERARCHY" || s == "ИЕРАРХИЯ"
        })
    });
    assert!(
        has_hierarchy_token,
        "C2 fix: ИЕРАРХИЯ must be consumed by order_by_item as a flat \
         sibling token of SdblOrderClause. Direct children/tokens: {:?}",
        order
            .children_with_tokens()
            .map(|c| match c {
                syntax::NodeOrToken::Node(n) => format!("Node({:?})", n.kind()),
                syntax::NodeOrToken::Token(t) => format!("Token({:?}: {:?})", t.kind(), t.text()),
            })
            .collect::<Vec<_>>(),
    );
}

// (h) Slice 11 — order_by_item flat children: no per-item
// wrapper. SdblOrderClause direct children include the
// expression nodes and ASC/DESC IDENT tokens as flat siblings,
// NOT wrapped in any SdblOrderByItem-style node (which does not
// exist as a NodeKind anyway). The HIR consumer at
// sdbl-hir/src/lower/clauses.rs:114-156 reads ВОЗР/УБЫВ direction
// tokens as direct children of SdblOrderClause to derive sort
// direction — both tokens MUST appear as flat siblings.
#[test]
fn test_slice11_order_by_flat_children_no_wrapper() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ A, B ИЗ Т УПОРЯДОЧИТЬ ПО A ВОЗР, B УБЫВ";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let order = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_ORDER_CLAUSE)
        .expect("Tree must contain SdblOrderClause");

    // Both expression-node children must be DIRECT (not wrapped).
    let direct_expr_kinds: Vec<_> = order
        .children()
        .map(|c| c.kind())
        .filter(|k| {
            matches!(
                k,
                SyntaxKind::SDBL_LOGICAL_OR_EXPR
                    | SyntaxKind::SDBL_COLUMN_REF
                    | SyntaxKind::SDBL_FUNCTION_CALL,
            )
        })
        .collect();
    assert_eq!(
        direct_expr_kinds.len(),
        2,
        "Two flat direct expression children expected (no per-item \
         wrapper). Got: {:?}",
        direct_expr_kinds,
    );

    // Both ВОЗР and УБЫВ direction tokens must appear as flat
    // direct-child IDENT tokens — not buried inside a wrapper.
    let direct_ident_texts: Vec<String> = order
        .children_with_tokens()
        .filter_map(|c| c.as_token().cloned())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_uppercase())
        .collect();
    assert!(
        direct_ident_texts.iter().any(|s| s == "ВОЗР" || s == "ASC"),
        "Direction token ВОЗР must be a flat IDENT direct token of \
         SdblOrderClause. Got direct IDENT texts: {:?}",
        direct_ident_texts,
    );
    assert!(
        direct_ident_texts.iter().any(|s| s == "УБЫВ" || s == "DESC"),
        "Direction token УБЫВ must be a flat IDENT direct token of \
         SdblOrderClause. Got direct IDENT texts: {:?}",
        direct_ident_texts,
    );

    // No per-item wrapper node may sit between the expressions
    // and the ORDER BY clause — i.e. no unknown direct-child
    // node kinds beyond the expression kinds and trivia.
    let unexpected_node_kinds: Vec<_> = order
        .children()
        .map(|c| c.kind())
        .filter(|k| {
            !matches!(
                k,
                SyntaxKind::SDBL_LOGICAL_OR_EXPR
                    | SyntaxKind::SDBL_COLUMN_REF
                    | SyntaxKind::SDBL_FUNCTION_CALL,
            )
        })
        .collect();
    assert!(
        unexpected_node_kinds.is_empty(),
        "Flat-children invariant: SdblOrderClause must have only \
         expression-node direct children — no per-item wrapper node. \
         Got unexpected direct node kinds: {:?}",
        unexpected_node_kinds,
    );
}

// (i) Slice 11 — HAVING calls expression(p) (NOT
// logical_expression(p)), but the consumer-side wrapper is still
// SdblLogicalOrExpr because Slice 10a wraps both entry points.
#[test]
fn test_slice11_having_logical_expression_wrapping() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ A ИЗ Т СГРУППИРОВАТЬ ПО A ИМЕЮЩИЕ A > 0";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let having = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_HAVING_CLAUSE)
        .expect("Tree must contain SdblHavingClause");

    let direct_expr_kinds: Vec<_> = having.children().map(|c| c.kind()).collect();
    assert!(
        direct_expr_kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR),
        "HAVING calls expression(p) but Slice 10a wraps the result in \
         SdblLogicalOrExpr — the consumer-side wrapper kind must still \
         appear as a direct child of SdblHavingClause. Got: {:?}",
        direct_expr_kinds,
    );
}

// (j) Slice 11 — FOR UPDATE without UPDATE keyword recovery: the
// FOR token alone (without UPDATE/ИЗМЕНЕНИЯ) emits SdblForUpdate,
// the MDO chain follows.
#[test]
fn test_slice11_for_without_update_recovery() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ A ИЗ Т ДЛЯ Справочник.X";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    let for_update = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_FOR_UPDATE)
        .expect("Tree must contain SdblForUpdate even when UPDATE keyword is missing");

    let has_update_token = for_update.children_with_tokens().any(|c| {
        c.as_token().is_some_and(|t| {
            let s = t.text().to_uppercase();
            s == "UPDATE" || s == "ИЗМЕНЕНИЯ"
        })
    });
    assert!(
        !has_update_token,
        "Missing-UPDATE recovery: SdblForUpdate must NOT contain an \
         UPDATE/ИЗМЕНЕНИЯ token when input has just `ДЛЯ <MDO>`",
    );
}

// (k) Slice 11 — FOR UPDATE deep MDO chain (greedy until the
// post-Dot lookahead fails to be an Ident).
#[test]
fn test_slice11_for_update_deep_mdo_chain() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ A ИЗ Т ДЛЯ ИЗМЕНЕНИЯ Справочник.X.Y.Z";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let for_update = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_FOR_UPDATE)
        .expect("Tree must contain SdblForUpdate");

    // Count Dot tokens at the FLAT direct-child token level —
    // there should be 3 dots for `Справочник.X.Y.Z`.
    let dot_count = for_update
        .children_with_tokens()
        .filter(|c| c.as_token().is_some_and(|t| t.text() == "."))
        .count();
    assert_eq!(
        dot_count, 3,
        "Greedy MDO chain expected to flatten `Справочник.X.Y.Z` into \
         3 Dot tokens at SdblForUpdate's direct-child level. Got: {}",
        dot_count,
    );
}

// (l) Slice 11 — TOTALS BY OVERALL fallthrough (§IDE-recovery
// allowance #1 — flat-Ident parser shape; OVERALL/ОБЩИЕ falls
// through is_expression_start and is consumed as a bare
// SdblColumnRef expression).
#[test]
fn test_slice11_totals_overall_fallthrough_shape() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ СУММА(A) ИЗ Т ИТОГИ ПО ОБЩИЕ";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let totals = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_TOTALS_BY)
        .expect("Tree must contain SdblTotalsBy");

    // OVERALL/ОБЩИЕ must end up as a direct expression-node child
    // of SdblTotalsBy (NOT a structured TOTALS-marker NodeKind).
    let has_expr_child = totals.children().any(|c| {
        matches!(
            c.kind(),
            SyntaxKind::SDBL_COLUMN_REF
                | SyntaxKind::SDBL_LOGICAL_OR_EXPR
                | SyntaxKind::SDBL_FUNCTION_CALL,
        )
    });
    assert!(
        has_expr_child,
        "OVERALL/ОБЩИЕ must fall through is_expression_start and be \
         consumed as a bare-expression direct child of SdblTotalsBy. \
         Direct child kinds: {:?}",
        totals.children().map(|c| c.kind()).collect::<Vec<_>>(),
    );
}

// (m) Slice 11 — tail-clause any-order across a multi-query
// package: each query's tail clauses must attach within that
// query's SdblSelectQuery scope, with no leakage across `;`
// boundaries (§AST-shape invariant #2). Query 1 has TOTALS BY +
// AUTOORDER (any-order acceptance); query 2 has ORDER BY; query 3
// has AUTOORDER. The test asserts each tail-clause node's
// nearest SdblSelectQuery ancestor is the correct query.
#[test]
fn test_slice11_tail_any_order_no_cross_query_leak() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ A ИЗ Т1 ИТОГИ ПО A АВТОУПОРЯДОЧИВАНИЕ; \
                 SELECT B FROM T2 ORDER BY B; \
                 SELECT C FROM T3 АВТОУПОРЯДОЧИВАНИЕ";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();

    // Walk the root's package structure and collect ONLY top-level
    // SdblSelectQuery nodes — i.e. SdblSelectQuery nodes that are
    // NOT nested inside another SdblSelectQuery. This catches a
    // regression where one query's range incorrectly spans across
    // a semicolon boundary and ends up containing the other queries
    // as descendants.
    let all_select_queries: Vec<_> =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_SELECT_QUERY).collect();
    let top_level_queries: Vec<_> = all_select_queries
        .iter()
        .filter(|q| !q.ancestors().skip(1).any(|a| a.kind() == SyntaxKind::SDBL_SELECT_QUERY))
        .cloned()
        .collect();
    assert_eq!(
        top_level_queries.len(),
        3,
        "Three top-level SdblSelectQuery nodes expected (one per `;` \
         segment, not nested inside another query); got {} top-level \
         out of {} total SdblSelectQuery nodes",
        top_level_queries.len(),
        all_select_queries.len(),
    );
    // No SdblSelectQuery may contain another SdblSelectQuery as a
    // descendant — package segments are disjoint.
    for q in &all_select_queries {
        let nested_count =
            q.descendants().filter(|d| d != q && d.kind() == SyntaxKind::SDBL_SELECT_QUERY).count();
        assert_eq!(
            nested_count,
            0,
            "SdblSelectQuery at offset {:?} must NOT contain another \
             SdblSelectQuery as a descendant (semicolon boundary \
             violation). Nested count: {}",
            q.text_range().start(),
            nested_count,
        );
    }

    // Identify each top-level query by its source-text identifier.
    // ALL three identification predicates require the marker to be
    // present AND the other two markers to be absent — this catches
    // any single-query span that crosses a semicolon boundary.
    let q_t1 = top_level_queries
        .iter()
        .find(|q| {
            let s = q.text().to_string();
            s.contains("Т1") && !s.contains("T2") && !s.contains("T3")
        })
        .expect(
            "Query 1 (Т1) top-level SdblSelectQuery must exist and not span across `;` boundaries",
        );
    let q_t2 = top_level_queries
        .iter()
        .find(|q| {
            let s = q.text().to_string();
            s.contains("T2") && !s.contains("Т1") && !s.contains("T3")
        })
        .expect(
            "Query 2 (T2) top-level SdblSelectQuery must exist and not span across `;` boundaries",
        );
    let q_t3 = top_level_queries
        .iter()
        .find(|q| {
            let s = q.text().to_string();
            s.contains("T3") && !s.contains("T2") && !s.contains("Т1")
        })
        .expect(
            "Query 3 (T3) top-level SdblSelectQuery must exist and not span across `;` boundaries",
        );

    // Fail-closed owner attribution: collect raw tail-clause nodes
    // first; for each, assert it has a SdblSelectQuery ancestor
    // (no root/package-level leak) AND return that ancestor by
    // identity (text_range), NOT by text substring. A regression
    // where one query node's range incorrectly spans across a
    // semicolon would make the wrong owner match by substring but
    // would be caught here because `text_range()` is unique per
    // node identity.
    fn owners_or_fail(
        root: &syntax::SyntaxNode,
        kind: SyntaxKind,
        kind_label: &str,
    ) -> Vec<syntax::SyntaxNode> {
        let nodes: Vec<_> = root.descendants().filter(|n| n.kind() == kind).collect();
        nodes
            .iter()
            .map(|n| {
                let ancestor = n.ancestors().find(|a| a.kind() == SyntaxKind::SDBL_SELECT_QUERY);
                assert!(
                    ancestor.is_some(),
                    "{} node at offset {:?} has no SdblSelectQuery \
                     ancestor — that is a cross-query leak at the \
                     package/root level. Node text: `{}`",
                    kind_label,
                    n.text_range().start(),
                    n.text(),
                );
                ancestor.unwrap()
            })
            .collect()
    }

    fn ranges_eq(a: &syntax::SyntaxNode, b: &syntax::SyntaxNode) -> bool {
        a.text_range() == b.text_range()
    }

    let totals_owners = owners_or_fail(&root, SyntaxKind::SDBL_TOTALS_BY, "SdblTotalsBy");
    assert_eq!(
        totals_owners.len(),
        1,
        "Exactly one SdblTotalsBy node expected across the package; got {}",
        totals_owners.len(),
    );
    assert!(
        ranges_eq(&totals_owners[0], q_t1),
        "The single SdblTotalsBy must attach inside the Т1 query \
         (matched by text_range identity, not substring). Got \
         owner range {:?}, expected Т1 range {:?}",
        totals_owners[0].text_range(),
        q_t1.text_range(),
    );

    let order_owners = owners_or_fail(&root, SyntaxKind::SDBL_ORDER_CLAUSE, "SdblOrderClause");
    assert_eq!(
        order_owners.len(),
        1,
        "Exactly one SdblOrderClause node expected across the package; got {}",
        order_owners.len(),
    );
    assert!(
        ranges_eq(&order_owners[0], q_t2),
        "The single SdblOrderClause must attach inside the T2 query \
         (matched by text_range identity). Got owner range {:?}, \
         expected T2 range {:?}",
        order_owners[0].text_range(),
        q_t2.text_range(),
    );

    // Exactly two AUTOORDER nodes — query 1 (Т1) and query 3 (T3),
    // no extras. Match by text_range identity, not text substring.
    let autoorder_owners = owners_or_fail(&root, SyntaxKind::SDBL_AUTOORDER, "SdblAutoorder");
    assert_eq!(
        autoorder_owners.len(),
        2,
        "Exactly two SdblAutoorder nodes expected (Т1 + T3); got {}",
        autoorder_owners.len(),
    );
    let t1_count = autoorder_owners.iter().filter(|o| ranges_eq(o, q_t1)).count();
    let t3_count = autoorder_owners.iter().filter(|o| ranges_eq(o, q_t3)).count();
    assert_eq!(
        t1_count, 1,
        "Exactly one SdblAutoorder must attach inside the Т1 query \
         (any-order after TOTALS) — matched by text_range identity",
    );
    assert_eq!(
        t3_count, 1,
        "Exactly one SdblAutoorder must attach inside the T3 query \
         — matched by text_range identity",
    );
}

// (n) Slice 11 — is_clause_keyword preserves the JOIN family
// delegation (§Child-attachment invariant #10). The
// `is_clause_keyword` predicate delegates to `is_join_keyword`
// (LEFT/RIGHT/FULL/INNER/JOIN/ON family) so JOIN starters
// terminate alias / source / clause-body scans. The observable
// signal: after the FROM data source `Т1`, the source-alias scan
// must NOT consume `ВНУТРЕННЕЕ` (an INNER JOIN type keyword)
// as a source alias — instead, the source completes alias-less
// and the JOIN clause attaches as a sibling. Without the
// `is_join_keyword` delegation, alias scan would swallow
// `ВНУТРЕННЕЕ` as an alias and the JOIN would never form.
#[test]
fn test_slice11_is_clause_keyword_join_delegation() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ A ИЗ Т1 ВНУТРЕННЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.X = Т2.Y ГДЕ Т1.X > 0";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();

    // (a) The JOIN-family delegation guarantees the JOIN actually
    // forms: ВНУТРЕННЕЕ СОЕДИНЕНИЕ is recognised as a JOIN keyword
    // boundary, so the parser builds an SdblJoinClause attached to
    // the Т1 SdblDataSource as a sibling/child relationship.
    let join_clauses: Vec<_> =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_JOIN_CLAUSE).collect();
    assert_eq!(
        join_clauses.len(),
        1,
        "is_join_keyword delegation must let alias scan terminate at \
         ВНУТРЕННЕЕ so the JOIN clause forms; expected exactly one \
         SdblJoinClause, got {}",
        join_clauses.len(),
    );

    // (b) The first SdblDataSource (Т1) must NOT carry an alias —
    // ВНУТРЕННЕЕ must not have been consumed as Т1's alias. Without
    // is_join_keyword delegation, the parser would consume
    // ВНУТРЕННЕЕ as an alias for Т1 and the test would fail.
    let first_data_source = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_DATA_SOURCE)
        .expect("Tree must contain SdblDataSource for Т1");
    let has_alias = first_data_source.children().any(|c| c.kind() == SyntaxKind::SDBL_ALIAS);
    assert!(
        !has_alias,
        "is_join_keyword delegation must terminate Т1's alias scan \
         at ВНУТРЕННЕЕ — first SdblDataSource must have NO SdblAlias \
         direct child. Direct children: {:?}",
        first_data_source.children().map(|c| c.kind()).collect::<Vec<_>>(),
    );

    // (c) After the JOIN ON-condition, ГДЕ is recognised as a
    // clause boundary by is_clause_keyword's direct
    // at_sdbl_keyword(p, "WHERE", "ГДЕ") branch, so the WHERE
    // clause attaches at the SdblQuery level above the JOIN, NOT
    // as a descendant of the JOIN.
    let has_where = root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_WHERE_CLAUSE);
    assert!(has_where, "SdblWhereClause must appear in the tree");
    let where_inside_join = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::SDBL_WHERE_CLAUSE)
        .any(|w| w.ancestors().skip(1).any(|a| a.kind() == SyntaxKind::SDBL_JOIN_CLAUSE));
    assert!(
        !where_inside_join,
        "is_clause_keyword must terminate JOIN parsing at ГДЕ — \
         SdblWhereClause must NOT be a descendant of SdblJoinClause",
    );
}

// ============================================================
// Slice 7-addendum (SELECT prefix qualifiers) C0 Bucket-A gap
// tests.
//
// These 5 tests pin pre-Slice-7-addendum-C2 parser behaviour
// for the limitations helper family (DISTINCT / TOP / ALLOWED +
// `is_identifier_token` predicate). All 5 must pass on the
// pre-rewrite parser (audit-gate semantics — they pin current
// behaviour before C2 touches it).
//
// Provenance per Slice 7-addendum plan §Tier classification
// (post-Round-3 v8327doc Глава 8 reclassification):
//   - DISTINCT — Tier A1. Primary source: v8327doc Глава 8
//                §<Описание запроса> at
//                its_db_v8327doc_bookmark_dev_TI000000453/page.html:1320
//                canonical EBNF + :1346-1348 prose. Secondary
//                corroborating: pubqlang chapter 20.
//   - TOP      — Tier A1. Primary source: v8327doc Глава 8 at
//                page.html:1320 (`[ПЕРВЫЕ <Количество>]` slot)
//                + :1350-1356 prose. Secondary corroborating:
//                pubqlang chapter 19.
//   - ALLOWED  — Tier A1. Primary source: v8327doc Глава 8 at
//                page.html:1320 (`[РАЗРЕШЕННЫЕ]` first
//                SELECT-prefix slot in canonical EBNF) +
//                :1331-1344 prose (RLS scope, top-level-only
//                constraint, propagation into subqueries,
//                ЧТЕНИЕ-rights interaction). Bilingual
//                word-list at :1038-1046 РАЗРЕШЕННЫЕ ↔ ALLOWED.
//                Secondary corroborating: pubqlang
//                chapter_057.html:50 UI-checkbox prose. Test
//                name uses `_canonical_ru` per the post-Round-3
//                Tier A1 elevation; the codex Round-2 finding 1
//                "no `_canonical_` for ALLOWED" rule is now
//                satisfied because the canonical source DOES
//                exist.
//
// Coverage maps onto §IDE-recovery allowances Q1/Q2/Q3:
//   - Q1 (any-order qualifier acceptance) pinned by test (d).
//   - Q3 (missing-TOP-count recovery) pinned by test (e).
//   - Q2 (duplicate-qualifier loop tolerance) is documented in
//     the mini-spec §IDE-recovery allowances but NOT directly
//     tested in C0 (test (d)'s input does not contain a
//     duplicate qualifier). A dedicated duplicate-qualifier
//     acceptance test lands in C3 alongside the
//     `sdbl_slice7_addendum_limitations.rs` acceptance suite.
// ============================================================

// (a) Slice 7-addendum — DISTINCT canonical RU form. **Tier A1
// per v8327doc Глава 8 §<Описание запроса> at
// `its_db_v8327doc_bookmark_dev_TI000000453/page.html:1320`**
// (canonical EBNF places РАЗЛИЧНЫЕ in the second SELECT-prefix
// slot) + `:1346-1348` (duplicate-elimination prose). Pubqlang
// `chapter_020.html:18, 29` provides the demonstrative
// `ВЫБРАТЬ РАЗЛИЧНЫЕ` example (secondary). Pins SdblLimitations
// as a direct child of SdblQuery, containing the РАЗЛИЧНЫЕ
// Ident token.
#[test]
fn test_slice7adn_distinct_canonical_ru() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ РАЗЛИЧНЫЕ A ИЗ Т";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let query = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_QUERY)
        .expect("Tree must contain SdblQuery");
    let limitations: Vec<_> =
        query.children().filter(|c| c.kind() == SyntaxKind::SDBL_LIMITATIONS).collect();
    assert_eq!(
        limitations.len(),
        1,
        "SdblQuery must have exactly one SdblLimitations direct child for \
         `ВЫБРАТЬ РАЗЛИЧНЫЕ A ИЗ Т`. Got: {:?}",
        query.children().map(|c| c.kind()).collect::<Vec<_>>(),
    );
    let kw_text: String = limitations[0]
        .children_with_tokens()
        .filter_map(|c| c.as_token().map(|t| t.text().to_string()))
        .collect::<Vec<_>>()
        .join("|");
    assert!(
        kw_text.to_uppercase().contains("РАЗЛИЧНЫЕ"),
        "SdblLimitations must contain РАЗЛИЧНЫЕ token. Got tokens: {}",
        kw_text,
    );
}

// (b) Slice 7-addendum — TOP canonical RU form. **Tier A1 per
// v8327doc Глава 8 §<Описание запроса> at
// `its_db_v8327doc_bookmark_dev_TI000000453/page.html:1320`**
// (canonical EBNF `[ПЕРВЫЕ <Количество>]` slot) + `:1350-1356`
// (limit / ordering / nested-query prose). Pubqlang
// `chapter_019.html:19, 28` provides the demonstrative
// `ВЫБРАТЬ ПЕРВЫЕ 3` example (secondary). Pins SdblTopClause as
// a direct child node of SdblLimitations, containing the
// ПЕРВЫЕ Ident + Decimal `3` tokens.
#[test]
fn test_slice7adn_top_canonical_ru() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ ПЕРВЫЕ 3 A ИЗ Т";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let limitations = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_LIMITATIONS)
        .expect("Tree must contain SdblLimitations");
    let top_clauses: Vec<_> =
        limitations.children().filter(|c| c.kind() == SyntaxKind::SDBL_TOP_CLAUSE).collect();
    assert_eq!(
        top_clauses.len(),
        1,
        "SdblLimitations must have exactly one SdblTopClause direct child \
         for `ВЫБРАТЬ ПЕРВЫЕ 3 A ИЗ Т`. Got direct children: {:?}",
        limitations.children().map(|c| c.kind()).collect::<Vec<_>>(),
    );
    let token_text: Vec<(SyntaxKind, String)> = top_clauses[0]
        .children_with_tokens()
        .filter_map(|c| c.as_token().map(|t| (t.kind(), t.text().to_string())))
        .filter(|(k, _)| {
            !matches!(k, SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT,)
        })
        .collect();
    let has_top_keyword = token_text
        .iter()
        .any(|(_, t)| t.to_uppercase().contains("ПЕРВЫЕ") || t.to_uppercase().contains("TOP"));
    let has_decimal = token_text.iter().any(|(k, t)| *k == SyntaxKind::DECIMAL && t == "3");
    assert!(
        has_top_keyword,
        "SdblTopClause must contain ПЕРВЫЕ/TOP keyword token. Got non-trivia tokens: {:?}",
        token_text,
    );
    assert!(
        has_decimal,
        "SdblTopClause must contain a Decimal `3` token. Got non-trivia tokens: {:?}",
        token_text,
    );
}

// (c) Slice 7-addendum — ALLOWED canonical RU form. **Tier A1
// per v8327doc Глава 8 §<Описание запроса> at
// `its_db_v8327doc_bookmark_dev_TI000000453/page.html:1320`**
// — the canonical EBNF skeleton
// `ВЫБРАТЬ [РАЗРЕШЕННЫЕ] [РАЗЛИЧНЫЕ] [ПЕРВЫЕ <Количество>]`
// places ALLOWED in the canonical first-qualifier slot, with
// full prose semantics at lines 1331-1344 covering RLS scope
// (top-level only, propagates into subqueries) and
// interaction with ЧТЕНИЕ rights. The pubqlang dump's
// `chapter_057.html:50` UI-checkbox prose is the secondary
// (textbook-companion) reference; v8327doc Глава 8 is the
// primary specification.
#[test]
fn test_slice7adn_allowed_canonical_ru() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ РАЗРЕШЕННЫЕ A ИЗ Т";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let query = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_QUERY)
        .expect("Tree must contain SdblQuery");
    let limitations: Vec<_> =
        query.children().filter(|c| c.kind() == SyntaxKind::SDBL_LIMITATIONS).collect();
    assert_eq!(
        limitations.len(),
        1,
        "SdblQuery must have exactly one SdblLimitations direct child for \
         `ВЫБРАТЬ РАЗРЕШЕННЫЕ A ИЗ Т`. Got: {:?}",
        query.children().map(|c| c.kind()).collect::<Vec<_>>(),
    );
    let kw_text: String = limitations[0]
        .children_with_tokens()
        .filter_map(|c| c.as_token().map(|t| t.text().to_string()))
        .collect::<Vec<_>>()
        .join("|");
    assert!(
        kw_text.to_uppercase().contains("РАЗРЕШЕННЫЕ"),
        "SdblLimitations must contain РАЗРЕШЕННЫЕ token (canonical Tier A1 \
         per v8327doc Глава 8). Got tokens: {}",
        kw_text,
    );
}

// (d) Slice 7-addendum — combined any-order acceptance pinning
// IDE-recovery allowance Q1. Per codex Round-2 finding 3, the
// input must include all three qualifiers (TOP + ALLOWED +
// DISTINCT). The parser must consume all three under a single
// SdblLimitations wrapper without enforcing canonical
// permutation. SDBL canonical-order normalisation is a
// semantic-layer concern, not a parser concern.
#[test]
fn test_slice7adn_combined_any_order() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ ПЕРВЫЕ 5 РАЗРЕШЕННЫЕ РАЗЛИЧНЫЕ A ИЗ Т";
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let query = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_QUERY)
        .expect("Tree must contain SdblQuery");
    let limitations: Vec<_> =
        query.children().filter(|c| c.kind() == SyntaxKind::SDBL_LIMITATIONS).collect();
    assert_eq!(
        limitations.len(),
        1,
        "SdblQuery must have exactly one SdblLimitations wrapper for the \
         any-order combined input. Got direct children: {:?}",
        query.children().map(|c| c.kind()).collect::<Vec<_>>(),
    );
    let top_count =
        limitations[0].children().filter(|c| c.kind() == SyntaxKind::SDBL_TOP_CLAUSE).count();
    assert_eq!(
        top_count, 1,
        "SdblLimitations must have exactly one SdblTopClause direct child \
         (the `ПЕРВЫЕ 5` qualifier). Got: {}",
        top_count,
    );
    let kw_text: String = limitations[0]
        .children_with_tokens()
        .filter_map(|c| c.as_token().map(|t| t.text().to_string()))
        .collect::<Vec<_>>()
        .join("|")
        .to_uppercase();
    assert!(
        kw_text.contains("РАЗРЕШЕННЫЕ"),
        "SdblLimitations must contain the РАЗРЕШЕННЫЕ keyword. Tokens: {}",
        kw_text,
    );
    assert!(
        kw_text.contains("РАЗЛИЧНЫЕ"),
        "SdblLimitations must contain the РАЗЛИЧНЫЕ keyword. Tokens: {}",
        kw_text,
    );
}

// (e) Slice 7-addendum — TOP missing-decimal recovery pinning
// IDE-recovery allowance Q3. `top_clause` calls
// `p.expect(TokenKind::Decimal)` and `Parser::expect` calls
// `Parser::error` on failure, which BUMPS the next token into
// an ERROR sub-node. So for input `ВЫБРАТЬ ПЕРВЫЕ A ИЗ Т`:
//   - SdblTopClause is still emitted (no Decimal child);
//   - the next Ident `A` is consumed into an ERROR sub-node
//     attached as a direct child of SdblTopClause;
//   - the trailing `ИЗ Т` is NOT recognised as a FROM clause:
//     the SDBL_FROM_CLAUSE node is absent, and `ИЗ` falls
//     through to `selected_fields` as a bare SdblColumnRef
//     while `Т` becomes its SdblAlias. This is the **current**
//     Q3 recovery shape — a known IDE-recovery quality issue
//     pinned here so any change to the recovery boundary is
//     visible. Slice 12 owns the recovery-quality fix.
//
// The parse-level `Parser::error()` call produces an ERROR
// NodeKind in the tree, NOT a `Parse::errors()` SyntaxError —
// `parse.has_errors()` returns false even though there is an
// ERROR node in the tree. The test asserts on the tree shape,
// not on `parse.errors()`.
#[test]
fn test_slice7adn_top_missing_decimal_recovery() {
    use syntax::SyntaxKind;
    let input = "ВЫБРАТЬ ПЕРВЫЕ A ИЗ Т";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    let limitations = root.descendants().find(|n| n.kind() == SyntaxKind::SDBL_LIMITATIONS).expect(
        "SdblLimitations marker must still be completed when the \
             Decimal after ПЕРВЫЕ is missing — IDE-recovery allowance Q3",
    );
    let top_clauses: Vec<_> =
        limitations.children().filter(|c| c.kind() == SyntaxKind::SDBL_TOP_CLAUSE).collect();
    assert_eq!(
        top_clauses.len(),
        1,
        "SdblLimitations must still have exactly one SdblTopClause direct \
         child even when the Decimal is missing. Got: {}",
        top_clauses.len(),
    );
    let decimal_count = top_clauses[0]
        .children_with_tokens()
        .filter_map(|c| c.as_token().filter(|t| t.kind() == SyntaxKind::DECIMAL).cloned())
        .count();
    assert_eq!(
        decimal_count, 0,
        "SdblTopClause must have NO Decimal token child when the count is \
         missing — Q3 preserved-parser-support recovery shape (Parser::expect \
         calls Parser::error which bumps the next token into ERROR sub-node).",
    );
    // Pre-rewrite parser shape: Parser::error() bumps the next
    // token (`A`) into an ERROR sub-node attached as a direct
    // child of SdblTopClause. The ERROR sub-node contains the
    // bumped Ident token.
    let error_children: Vec<_> =
        top_clauses[0].children().filter(|c| c.kind() == SyntaxKind::ERROR).collect();
    assert_eq!(
        error_children.len(),
        1,
        "SdblTopClause must have exactly one ERROR sub-node child (the \
         bumped `A` Ident) per Parser::error() recovery contract. Got: {}",
        error_children.len(),
    );
    let error_text = error_children[0].text().to_string();
    assert!(
        error_text.contains('A'),
        "ERROR sub-node must contain the bumped `A` Ident. Got text: {:?}",
        error_text,
    );
    // Q3 IDE-recovery boundary: the trailing `ИЗ Т` is NOT
    // recognised as a FROM clause in the current parser. `ИЗ`
    // falls through to `selected_fields` as a bare
    // SdblColumnRef and `Т` becomes its SdblAlias. This is a
    // known recovery-quality issue documented in the Slice
    // 7-addendum plan §IDE-recovery allowance Q3 and deferred
    // to Slice 12.
    let from_clauses_count =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE).count();
    assert_eq!(
        from_clauses_count, 0,
        "Pre-rewrite parser current shape: NO SdblFromClause emitted for \
         `ВЫБРАТЬ ПЕРВЫЕ A ИЗ Т` because `ИЗ` falls through to \
         selected_fields as a bare SdblColumnRef. Got: {} SdblFromClause \
         nodes (a regression that promotes recovery quality must update \
         this test plus the §IDE-recovery allowance Q3 documentation).",
        from_clauses_count,
    );
}
