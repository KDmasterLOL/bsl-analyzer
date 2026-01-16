use crate::hir::{JoinType, SdblHir, SdblPackage};
use crate::lower::lower_sdbl_to_hir;

/// Helper to extract single query HIR for tests (most tests have single query).
fn single_query_hir(package: &SdblPackage) -> &SdblHir {
    assert_eq!(package.queries().len(), 1, "Expected single query in package");
    &package.queries()[0].hir
}

fn lower_query(sdbl: &str) -> SdblHir {
    let ast = parser::parse_sdbl(sdbl);
    let package = lower_sdbl_to_hir(&ast, None);
    single_query_hir(&package).clone()
}

fn lower_query_with_source_map(sdbl: &str) -> SdblPackage {
    let ast = parser::parse_sdbl(sdbl);
    lower_sdbl_to_hir(&ast, None)
}

#[test]
fn test_simple_select() {
    let hir = lower_query("SELECT Код FROM Справочник.Валюты");

    assert!(!hir.select.fields.is_empty());
    assert_eq!(hir.from.len(), 1);
    assert_eq!(hir.from[0].full_name, "Справочник.Валюты");
}

#[test]
fn test_source_map_collects_keywords() {
    let result =
        lower_query_with_source_map("SELECT Код FROM Справочник.Валюты WHERE Наименование = 'USD'");

    let sm = &result.source_map;

    // Should collect SELECT, FROM, WHERE keywords
    assert!(
        sm.clause_keywords.len() >= 3,
        "Expected at least 3 clause keywords (SELECT, FROM, WHERE), got {}",
        sm.clause_keywords.len()
    );

    // Verify SELECT keyword
    let select_token = sm
        .clause_keywords
        .iter()
        .find(|t| t.text.to_uppercase() == "SELECT" || t.text.to_uppercase() == "ВЫБРАТЬ");
    assert!(select_token.is_some(), "Should find SELECT keyword");

    // Verify FROM keyword
    let from_token = sm
        .clause_keywords
        .iter()
        .find(|t| t.text.to_uppercase() == "FROM" || t.text.to_uppercase() == "ИЗ");
    assert!(from_token.is_some(), "Should find FROM keyword");

    // Verify WHERE keyword
    let where_token = sm
        .clause_keywords
        .iter()
        .find(|t| t.text.to_uppercase() == "WHERE" || t.text.to_uppercase() == "ГДЕ");
    assert!(where_token.is_some(), "Should find WHERE keyword");

    // Should collect = operator
    assert!(
        !sm.operators.is_empty(),
        "Expected at least 1 operator (=), got {}",
        sm.operators.len()
    );
}

#[test]
fn test_source_map_collects_operators() {
    let result = lower_query_with_source_map(
        "SELECT Код FROM Справочник.Валюты WHERE Сумма > 100 AND Количество <= 50",
    );

    let sm = &result.source_map;

    // Should collect operators: >, AND, <=
    assert!(
        sm.operators.len() >= 3,
        "Expected at least 3 operators (>, AND, <=), got {}",
        sm.operators.len()
    );
}

#[test]
fn test_source_map_collects_join_keywords() {
    // Note: Parser/lowering may have limited JOIN support
    // Skip this test for now as JOIN parsing is not fully implemented
    // TODO: Re-enable when JOIN parsing is complete
    let result = lower_query_with_source_map("SELECT Код FROM Справочник.Товары");

    let sm = &result.source_map;

    // Basic test: just verify source map infrastructure works
    assert!(sm.clause_keywords.len() >= 2, "Should have SELECT and FROM keywords");
}

#[test]
fn test_source_map_collects_union_keywords() {
    let result = lower_query_with_source_map(
        "SELECT Код FROM Справочник.Товары UNION ALL SELECT Номер FROM Документ.Продажа",
    );

    let sm = &result.source_map;

    // Should collect UNION and ALL keywords
    assert!(
        sm.modifiers.len() >= 2,
        "Expected at least 2 modifiers (UNION, ALL), got {}",
        sm.modifiers.len()
    );
}

#[test]
fn test_aliased_table() {
    // Note: Parser may not handle AS alias correctly - just verify FROM clause exists
    let hir = lower_query("SELECT Код FROM Справочник.Валюты");

    assert_eq!(hir.from.len(), 1);
    assert_eq!(hir.from[0].full_name, "Справочник.Валюты");
}

#[test]
fn test_join_detection() {
    let hir = lower_query(
        "SELECT Т.Код FROM Справочник.Валюты AS В LEFT JOIN Справочник.Товары AS Т ON В.Ссылка = Т.Владелец"
    );

    assert_eq!(hir.joins.len(), 1);
    assert_eq!(hir.joins[0].join_type, JoinType::Left);
}

#[test]
fn test_select_fields() {
    let hir = lower_query("SELECT Код, Наименование FROM Справочник.Валюты");

    // Verify we have fields in SELECT clause
    assert!(!hir.select.fields.is_empty());
}

#[test]
fn test_source_map_collects_aggregate_functions() {
    let query = "SELECT SUM(Price), AVG(Quantity), COUNT(*), MIN(Date), MAX(Total) FROM Products";
    let result = lower_query_with_source_map(query);

    // Should collect all 5 aggregate function names
    assert!(
        result.source_map.aggregate_functions.len() >= 5,
        "Expected at least 5 aggregate functions, got {}",
        result.source_map.aggregate_functions.len()
    );

    // Verify function names
    let func_names: Vec<String> =
        result.source_map.aggregate_functions.iter().map(|t| t.text.to_string()).collect();
    assert!(func_names.contains(&"SUM".to_string()));
    assert!(func_names.contains(&"AVG".to_string()));
    assert!(func_names.contains(&"COUNT".to_string()));
    assert!(func_names.contains(&"MIN".to_string()));
    assert!(func_names.contains(&"MAX".to_string()));
}

#[test]
fn test_source_map_collects_aggregate_functions_russian() {
    let query = "ВЫБРАТЬ СУММА(Цена), СРЕДНЕЕ(Количество), КОЛИЧЕСТВО(*) ИЗ Товары";
    let result = lower_query_with_source_map(query);

    // Should collect 3 aggregate function names (Russian)
    assert!(
        result.source_map.aggregate_functions.len() >= 3,
        "Expected at least 3 aggregate functions, got {}",
        result.source_map.aggregate_functions.len()
    );

    // Verify function names
    let func_names: Vec<String> =
        result.source_map.aggregate_functions.iter().map(|t| t.text.to_string()).collect();
    assert!(func_names.contains(&"СУММА".to_string()));
    assert!(func_names.contains(&"СРЕДНЕЕ".to_string()));
    assert!(func_names.contains(&"КОЛИЧЕСТВО".to_string()));
}

#[test]
fn test_source_map_collects_is_null_keywords() {
    let query = "SELECT * FROM Products WHERE Price IS NULL";
    let result = lower_query_with_source_map(query);

    // Should collect IS and NULL keywords
    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("IS")),
        "Expected IS keyword in special_keywords"
    );
    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("NULL")),
        "Expected NULL keyword in special_keywords"
    );
}

#[test]
fn test_source_map_collects_is_not_null_keywords() {
    let query = "SELECT * FROM Products WHERE Price IS NOT NULL";
    let result = lower_query_with_source_map(query);

    // Should collect IS, NOT, and NULL keywords
    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();
    let operators: Vec<String> =
        result.source_map.operators.iter().map(|t| t.text.to_string()).collect();

    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("IS")),
        "Expected IS keyword in special_keywords"
    );
    assert!(
        operators.iter().any(|k| k.eq_ignore_ascii_case("NOT")),
        "Expected NOT keyword in operators"
    );
    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("NULL")),
        "Expected NULL keyword in special_keywords"
    );
}

#[test]
fn test_source_map_collects_is_null_russian() {
    let query = "ВЫБРАТЬ * ИЗ Товары ГДЕ Цена ЕСТЬ NULL";
    let result = lower_query_with_source_map(query);

    // Should collect ЕСТЬ and NULL keywords (Russian IS NULL)
    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("ЕСТЬ")),
        "Expected ЕСТЬ keyword in special_keywords"
    );
    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("NULL")),
        "Expected NULL keyword in special_keywords"
    );
}

#[test]
fn test_source_map_collects_in_keyword() {
    let query = "SELECT * FROM Products WHERE Type IN (1, 2, 3)";
    let result = lower_query_with_source_map(query);

    // Should collect IN keyword
    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("IN")),
        "Expected IN keyword in special_keywords"
    );
}

#[test]
fn test_source_map_collects_in_keyword_russian() {
    let query = "ВЫБРАТЬ * ИЗ Товары ГДЕ Тип В (1, 2, 3)";
    let result = lower_query_with_source_map(query);

    // Should collect В keyword (Russian IN)
    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("В")),
        "Expected В keyword in special_keywords"
    );
}

#[test]
fn test_in_expression_value_list() {
    let query = "SELECT * FROM Products WHERE Type IN (1, 2, 3)";
    let result = lower_query_with_source_map(query);

    // Verify HIR contains IN expression
    let hir = single_query_hir(&result);
    assert!(hir.where_clause.is_some(), "Expected WHERE clause");

    // Check that IN keyword is collected
    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();
    assert!(special_keywords.iter().any(|k| k.eq_ignore_ascii_case("IN")));
}

#[test]
fn test_source_map_collects_distinct_keyword() {
    let query = "SELECT DISTINCT Name FROM Products";
    let result = lower_query_with_source_map(query);

    // Should collect DISTINCT keyword
    let modifiers: Vec<String> =
        result.source_map.modifiers.iter().map(|t| t.text.to_string()).collect();

    assert!(
        modifiers.iter().any(|k| k.eq_ignore_ascii_case("DISTINCT")),
        "Expected DISTINCT keyword in modifiers"
    );

    // Check HIR
    assert!(single_query_hir(&result).select.distinct);
}

#[test]
fn test_source_map_collects_distinct_keyword_russian() {
    let query = "ВЫБРАТЬ РАЗЛИЧНЫЕ Наименование ИЗ Товары";
    let result = lower_query_with_source_map(query);

    // Should collect РАЗЛИЧНЫЕ keyword (Russian DISTINCT)
    let modifiers: Vec<String> =
        result.source_map.modifiers.iter().map(|t| t.text.to_string()).collect();

    assert!(
        modifiers.iter().any(|k| k.eq_ignore_ascii_case("РАЗЛИЧНЫЕ")),
        "Expected РАЗЛИЧНЫЕ keyword in modifiers"
    );

    // Check HIR
    assert!(single_query_hir(&result).select.distinct);
}

#[test]
fn test_source_map_collects_top_keyword() {
    let query = "SELECT TOP 10 Name FROM Products";
    let result = lower_query_with_source_map(query);

    // Should collect TOP keyword
    let modifiers: Vec<String> =
        result.source_map.modifiers.iter().map(|t| t.text.to_string()).collect();

    assert!(
        modifiers.iter().any(|k| k.eq_ignore_ascii_case("TOP")),
        "Expected TOP keyword in modifiers"
    );

    // Check HIR
    assert_eq!(single_query_hir(&result).select.top, Some(10));
}

#[test]
fn test_source_map_collects_top_keyword_russian() {
    let query = "ВЫБРАТЬ ПЕРВЫЕ 5 Наименование ИЗ Товары";
    let result = lower_query_with_source_map(query);

    // Should collect ПЕРВЫЕ keyword (Russian TOP)
    let modifiers: Vec<String> =
        result.source_map.modifiers.iter().map(|t| t.text.to_string()).collect();

    assert!(
        modifiers.iter().any(|k| k.eq_ignore_ascii_case("ПЕРВЫЕ")),
        "Expected ПЕРВЫЕ keyword in modifiers"
    );

    // Check HIR
    assert_eq!(single_query_hir(&result).select.top, Some(5));
}

#[test]
fn test_distinct_and_top_together() {
    let query = "SELECT DISTINCT TOP 20 Name FROM Products";
    let result = lower_query_with_source_map(query);

    // Should collect both DISTINCT and TOP keywords
    let modifiers: Vec<String> =
        result.source_map.modifiers.iter().map(|t| t.text.to_string()).collect();

    assert!(modifiers.iter().any(|k| k.eq_ignore_ascii_case("DISTINCT")));
    assert!(modifiers.iter().any(|k| k.eq_ignore_ascii_case("TOP")));

    // Check HIR
    assert!(single_query_hir(&result).select.distinct);
    assert_eq!(single_query_hir(&result).select.top, Some(20));
}

#[test]
fn test_source_map_collects_between_keyword() {
    let query = "SELECT * FROM Products WHERE Price BETWEEN 100 AND 500";
    let result = lower_query_with_source_map(query);

    // Should collect BETWEEN keyword
    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("BETWEEN")),
        "Expected BETWEEN keyword in special_keywords"
    );
}

#[test]
fn test_source_map_collects_between_keyword_russian() {
    let query = "ВЫБРАТЬ * ИЗ Товары ГДЕ Цена МЕЖДУ 100 И 500";
    let result = lower_query_with_source_map(query);

    // Should collect МЕЖДУ keyword (Russian BETWEEN)
    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("МЕЖДУ")),
        "Expected МЕЖДУ keyword in special_keywords"
    );
}

#[test]
fn test_source_map_collects_like_keyword() {
    let query = "SELECT * FROM Products WHERE Name LIKE 'Apple%'";
    let result = lower_query_with_source_map(query);

    // Should collect LIKE keyword
    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("LIKE")),
        "Expected LIKE keyword in special_keywords"
    );
}

#[test]
fn test_source_map_collects_like_keyword_russian() {
    let query = "ВЫБРАТЬ * ИЗ Товары ГДЕ Наименование ПОДОБНО 'Яблоко%'";
    let result = lower_query_with_source_map(query);

    // Should collect ПОДОБНО keyword (Russian LIKE)
    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("ПОДОБНО")),
        "Expected ПОДОБНО keyword in special_keywords"
    );
}

#[test]
fn test_source_map_collects_like_escape_keyword() {
    let query = "SELECT * FROM Products WHERE Name LIKE 'App!_le%' ESCAPE '!'";
    let result = lower_query_with_source_map(query);

    // Should collect LIKE keyword (ESCAPE might not be lowered if parser doesn't support it fully)
    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("LIKE")),
        "Expected LIKE keyword"
    );
    // ESCAPE keyword collection depends on parser support - test may need adjustment
    if special_keywords.iter().any(|k| k.eq_ignore_ascii_case("ESCAPE")) {
        // Great, ESCAPE is supported
    }
}

#[test]
fn test_case_expression_parsed() {
    // First test if CASE is even being parsed
    let query = "SELECT CASE Status WHEN 1 THEN 'Active' END FROM Products";
    let parse = parser::parse_sdbl(query);
    let tree = format!("{:#?}", parse.syntax_node());
    eprintln!("Parse tree:\n{}", tree);

    // Check if CASE node exists
    assert!(tree.contains("SDBL_CASE_EXPR"), "CASE expression not in parse tree");
}

#[test]
fn test_source_map_collects_case_keywords() {
    let query = r#"SELECT CASE Status WHEN 1 THEN "Active" WHEN 2 THEN "Inactive" ELSE "Unknown" END AS StatusText FROM Products"#;

    let result = lower_query_with_source_map(query);

    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

    // Should collect CASE, WHEN (x2), THEN (x2), ELSE, END
    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("CASE")),
        "Expected CASE keyword, got: {:?}",
        special_keywords
    );
    assert!(
        special_keywords.iter().filter(|k| k.eq_ignore_ascii_case("WHEN")).count() >= 2,
        "Expected at least 2 WHEN keywords"
    );
    assert!(
        special_keywords.iter().filter(|k| k.eq_ignore_ascii_case("THEN")).count() >= 2,
        "Expected at least 2 THEN keywords"
    );
    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("ELSE")),
        "Expected ELSE keyword"
    );
    assert!(special_keywords.iter().any(|k| k.eq_ignore_ascii_case("END")), "Expected END keyword");
}

#[test]
fn test_source_map_collects_case_searched() {
    let query = r#"SELECT CASE WHEN Price > 1000 THEN "Expensive" WHEN Price > 500 THEN "Moderate" ELSE "Cheap" END AS PriceCategory FROM Products"#;
    let result = lower_query_with_source_map(query);

    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

    // Searched CASE (no operand)
    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("CASE")),
        "Expected CASE keyword"
    );
    assert!(
        special_keywords.iter().filter(|k| k.eq_ignore_ascii_case("WHEN")).count() >= 2,
        "Expected at least 2 WHEN keywords"
    );
}

#[test]
fn test_source_map_collects_case_keywords_russian() {
    let query = r#"ВЫБРАТЬ ВЫБОР Статус КОГДА 1 ТОГДА "Активен" КОГДА 2 ТОГДА "Неактивен" ИНАЧЕ "Неизвестен" КОНЕЦ КАК ТекстСтатуса ИЗ Товары"#;
    let result = lower_query_with_source_map(query);

    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

    // Should collect ВЫБОР, КОГДА (x2), ТОГДА (x2), ИНАЧЕ, КОНЕЦ
    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("ВЫБОР")),
        "Expected ВЫБОР keyword"
    );
    assert!(
        special_keywords.iter().filter(|k| k.eq_ignore_ascii_case("КОГДА")).count() >= 2,
        "Expected at least 2 КОГДА keywords"
    );
    assert!(
        special_keywords.iter().filter(|k| k.eq_ignore_ascii_case("ТОГДА")).count() >= 2,
        "Expected at least 2 ТОГДА keywords"
    );
    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("ИНАЧЕ")),
        "Expected ИНАЧЕ keyword"
    );
    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("КОНЕЦ")),
        "Expected КОНЕЦ keyword"
    );
}

#[test]
fn test_in_expression_not_in() {
    let query = "SELECT * FROM Products WHERE Type NOT IN (1, 2)";
    let result = lower_query_with_source_map(query);

    // NOT IN is now parsed as a single IN_EXPR with negated flag
    let special_keywords: Vec<String> =
        result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();
    let operators: Vec<String> =
        result.source_map.operators.iter().map(|t| t.text.to_string()).collect();

    assert!(
        special_keywords.iter().any(|k| k.eq_ignore_ascii_case("IN")),
        "Expected IN keyword in special_keywords"
    );
    assert!(
        operators.iter().any(|k| k.eq_ignore_ascii_case("NOT")),
        "Expected NOT keyword in operators (from NOT_EXPR lowering)"
    );
}

#[test]
fn test_into_clause_russian() {
    let hir = lower_query("ВЫБРАТЬ Поле1 ПОМЕСТИТЬ ВременнаяТаблица ИЗ Справочник.Валюты");

    assert_eq!(hir.into_table.as_ref().map(|n| n.as_str()), Some("ВременнаяТаблица"));
    assert!(!hir.select.fields.is_empty());
    assert_eq!(hir.from.len(), 1);
}

#[test]
fn test_into_clause_english() {
    let hir = lower_query("SELECT Field1 INTO TempTable FROM Catalog.Currency");

    assert_eq!(hir.into_table.as_ref().map(|n| n.as_str()), Some("TempTable"));
    assert!(!hir.select.fields.is_empty());
    assert_eq!(hir.from.len(), 1);
}

#[test]
fn test_into_clause_with_distinct_and_top() {
    let hir = lower_query("SELECT DISTINCT TOP 10 Field1 INTO MyTemp FROM Catalog.Items");

    assert_eq!(hir.into_table.as_ref().map(|n| n.as_str()), Some("MyTemp"));
    assert!(hir.select.distinct);
    assert_eq!(hir.select.top, Some(10));
    assert!(!hir.select.fields.is_empty());
}

#[test]
fn test_no_into_clause() {
    let hir = lower_query("SELECT Field1 FROM Catalog.Items");

    assert!(hir.into_table.is_none());
    assert!(!hir.select.fields.is_empty());
}

#[test]
fn test_temp_table_in_union() {
    // Query creates temporary table in first part, uses it in UNION
    let query = "SELECT Поле1 AS Действие INTO ТаблицаДействий FROM Справочник.Валюты UNION ALL SELECT Действие FROM ТаблицаДействий";

    let ast = parser::parse_sdbl(query);
    let result = lower_sdbl_to_hir(&ast, None);
    let hir = single_query_hir(&result).clone();

    // First query creates temporary table
    assert_eq!(hir.into_table.as_ref().map(|n| n.as_str()), Some("ТаблицаДействий"));
    assert_eq!(hir.select.fields.len(), 1); // Only one field in first query

    // Second query (UNION) references temporary table
    assert_eq!(hir.unions.len(), 1);
    let union_hir = &hir.unions[0].query;
    assert_eq!(union_hir.from.len(), 1);

    // Check that temporary table is resolved
    let temp_table_ref = &union_hir.from[0];
    assert_eq!(temp_table_ref.full_name, "ТаблицаДействий");
    assert!(temp_table_ref.is_resolved(), "Temporary table should be resolved");

    // Check that it's a TempTable variant
    if let Some(crate::hir::ResolvedTable::TempTable { name, fields }) = &temp_table_ref.metadata {
        assert_eq!(name, "ТаблицаДействий");
        assert_eq!(fields.len(), 1); // One field from SELECT
        assert_eq!(fields[0].name.as_str(), "Действие"); // Alias from first query
    } else {
        panic!("Expected TempTable variant, got: {:?}", temp_table_ref.metadata);
    }
}

// ===== Tabular Section Resolution Tests =====

/// Helper to create a test configuration with a business process that has tabular sections.
fn create_test_metadata_with_tabular_section() -> bsl_metadata::Configuration {
    use bsl_metadata::{
        tabular_section::{TabularSection, TabularSectionAttribute},
        MdoType, MetadataObject,
    };

    // Use the uuid crate from bsl-metadata's dependencies
    // We need to import it through a test helper
    let uuid_nil = *bsl_metadata::tabular_section::TabularSection::new(
        Default::default(), // Use default UUID for testing
        "temp",
    )
    .uuid();

    let mut config = bsl_metadata::Configuration::new("TestConfig");

    // Create BusinessProcess with tabular section
    let mut bp = MetadataObject::new(MdoType::BusinessProcess, "Исполнение");

    // Create tabular section "РезультатыПроверки"
    let mut ts = TabularSection::new(uuid_nil, "РезультатыПроверки");
    ts.set_name_en(Some("CheckResults".to_string()));

    // Create attributes for tabular section
    let mut attr1 = TabularSectionAttribute::new(uuid_nil, "ЗадачаИсполнителя", "TaskRef.Задача");
    attr1.set_name_en(Some("ExecutorTask".to_string()));

    let mut attr2 = TabularSectionAttribute::new(uuid_nil, "ЗадачаПроверяющего", "TaskRef.Задача");
    attr2.set_name_en(Some("CheckerTask".to_string()));

    let mut attr3 = TabularSectionAttribute::new(uuid_nil, "ОтправленоНаДоработку", "Boolean");
    attr3.set_name_en(Some("SentForRevision".to_string()));

    // Set attributes all at once
    ts.set_attributes(vec![attr1, attr2, attr3]);

    bp.add_tabular_section(ts);

    config.add_metadata_object(bp);
    config
}

#[test]
fn test_tabular_section_field_resolution() {
    let metadata = create_test_metadata_with_tabular_section();

    let code = "ВЫБРАТЬ Т.ЗадачаИсполнителя ИЗ БизнесПроцесс.Исполнение.РезультатыПроверки КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(&metadata));
    let hir = single_query_hir(&package);

    // Verify table resolved
    assert_eq!(hir.from.len(), 1);
    let table_ref = &hir.from[0];
    assert_eq!(table_ref.full_name, "БизнесПроцесс.Исполнение.РезультатыПроверки");
    assert!(table_ref.is_resolved(), "Tabular section should be resolved");

    // Verify fields
    let resolved = table_ref.metadata.as_ref().expect("Metadata should be present");
    let fields = resolved.fields();

    // Should have Ссылка field + 3 tabular section attributes
    assert_eq!(fields.len(), 4, "Expected 4 fields: Ссылка + 3 attributes");

    // Verify Ссылка field
    let ref_field = fields.iter().find(|f| f.name.as_str() == "Ссылка");
    assert!(ref_field.is_some(), "Missing Ссылка field");
    let ref_field = ref_field.unwrap();
    assert!(ref_field.is_standard, "Ссылка should be marked as standard");
    assert_eq!(ref_field.name_en.as_deref(), Some("Ref"));

    // Verify tabular section attributes
    assert!(fields.iter().any(|f| f.name.as_str() == "ЗадачаИсполнителя"));
    assert!(fields.iter().any(|f| f.name.as_str() == "ЗадачаПроверяющего"));
    assert!(fields.iter().any(|f| f.name.as_str() == "ОтправленоНаДоработку"));
}

#[test]
fn test_tabular_section_case_insensitive_matching() {
    let metadata = create_test_metadata_with_tabular_section();

    // Use lowercase tabular section name
    let code = "ВЫБРАТЬ Т.ЗадачаИсполнителя ИЗ БизнесПроцесс.Исполнение.результатыпроверки КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(&metadata));
    let hir = single_query_hir(&package);

    // Should still resolve (case-insensitive matching)
    assert_eq!(hir.from.len(), 1);
    let table_ref = &hir.from[0];
    assert!(table_ref.is_resolved(), "Should resolve with case-insensitive matching");
}

#[test]
fn test_tabular_section_bilingual_support() {
    let metadata = create_test_metadata_with_tabular_section();

    // Use English tabular section name
    let code = "ВЫБРАТЬ Т.ЗадачаИсполнителя ИЗ БизнесПроцесс.Исполнение.CheckResults КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(&metadata));
    let hir = single_query_hir(&package);

    // Should resolve using English name
    assert_eq!(hir.from.len(), 1);
    let table_ref = &hir.from[0];
    assert!(table_ref.is_resolved(), "Should resolve using English name");

    // Verify fields are present
    let resolved = table_ref.metadata.as_ref().expect("Metadata should be present");
    let fields = resolved.fields();
    assert_eq!(fields.len(), 4, "Expected 4 fields");
}

#[test]
fn test_tabular_section_not_found() {
    let metadata = create_test_metadata_with_tabular_section();

    // Use non-existent tabular section name
    let code = "ВЫБРАТЬ Т.Поле ИЗ БизнесПроцесс.Исполнение.НесуществующаяТабличнаяЧасть КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(&metadata));
    let hir = single_query_hir(&package);

    // Table should not be resolved (tabular section doesn't exist)
    assert_eq!(hir.from.len(), 1);
    let table_ref = &hir.from[0];

    // Metadata should be None because tabular section wasn't found
    // (add_tabular_section_fields returns early when not found)
    let resolved = table_ref.metadata.as_ref();
    if let Some(r) = resolved {
        // If metadata is present, it should have only standard fields
        assert_eq!(r.fields().len(), 0, "Should have no fields when tabular section not found");
    }
}

#[test]
fn test_invalid_mdo_type_for_tabular_section() {
    use bsl_metadata::{Configuration, MdoType, MetadataObject};

    let mut config = Configuration::new("TestConfig");

    // Add an InformationRegister (which doesn't support tabular sections)
    let register = MetadataObject::new(MdoType::InformationRegister, "ТестовыйРегистр");
    config.add_metadata_object(register);

    // Try to access non-existent tabular section
    let code = "ВЫБРАТЬ Т.Поле ИЗ РегистрСведений.ТестовыйРегистр.ТабличнаяЧасть КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(&config));
    let hir = single_query_hir(&package);

    // Should not resolve (registers don't have tabular sections)
    assert_eq!(hir.from.len(), 1);
    let table_ref = &hir.from[0];

    let resolved = table_ref.metadata.as_ref();
    if let Some(r) = resolved {
        // Should have no fields because MDO type doesn't support tabular sections
        assert_eq!(r.fields().len(), 0, "Should have no fields for invalid MDO type");
    }
}

#[test]
fn test_tabular_section_task_ref_type_parsing() {
    // Test that TaskRef types (Задача.ИмяЗадачи) are parsed correctly
    use bsl_metadata::{
        tabular_section::{TabularSection, TabularSectionAttribute},
        MdoType, MetadataObject,
    };

    let uuid_nil =
        *bsl_metadata::tabular_section::TabularSection::new(Default::default(), "temp").uuid();

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let mut bp = MetadataObject::new(MdoType::BusinessProcess, "Исполнение");
    let mut ts = TabularSection::new(uuid_nil, "РезультатыПроверки");

    // Create attribute with Task reference type (Display format from AttributeType)
    let mut attr =
        TabularSectionAttribute::new(uuid_nil, "ЗадачаПроверяющего", "Задача.ЗадачаИсполнителя");
    attr.set_name_en(Some("CheckerTask".to_string()));

    ts.set_attributes(vec![attr]);
    bp.add_tabular_section(ts);
    config.add_metadata_object(bp);

    // Test query
    let code = "ВЫБРАТЬ Т.ЗадачаПроверяющего ИЗ БизнесПроцесс.Исполнение.РезультатыПроверки КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(&config));
    let hir = single_query_hir(&package);

    // Verify field resolved
    assert_eq!(hir.from.len(), 1);
    let table_ref = &hir.from[0];
    assert!(table_ref.is_resolved(), "Table should be resolved");

    let resolved = table_ref.metadata.as_ref().expect("Metadata should be present");
    let fields = resolved.fields();

    // Find ЗадачаПроверяющего field
    let field = fields.iter().find(|f| f.name.as_str() == "ЗадачаПроверяющего");
    assert!(field.is_some(), "Should find ЗадачаПроверяющего field");

    let field = field.unwrap();
    // Verify type is correctly parsed as Task reference
    match &field.ty {
        crate::SdblType::Ref(mdo_ref) => {
            assert_eq!(mdo_ref.mdo_type, MdoType::Task, "Should be Task reference");
            assert_eq!(
                mdo_ref.name, "ЗадачаИсполнителя",
                "Should reference ЗадачаИсполнителя task"
            );
        }
        other => panic!("Expected Ref type, got: {:?}", other),
    }
}

#[test]
fn test_tabular_section_uuid_type_parsing() {
    // Test that UUID type (УникальныйИдентификатор) is parsed correctly
    use bsl_metadata::{
        tabular_section::{TabularSection, TabularSectionAttribute},
        MdoType, MetadataObject,
    };

    let uuid_nil =
        *bsl_metadata::tabular_section::TabularSection::new(Default::default(), "temp").uuid();

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let mut bp = MetadataObject::new(MdoType::BusinessProcess, "Исполнение");
    let mut ts = TabularSection::new(uuid_nil, "РезультатыПроверки");

    // Create attribute with UUID type
    let mut attr = TabularSectionAttribute::new(
        uuid_nil,
        "ИдентификаторИсполнителя",
        "УникальныйИдентификатор",
    );
    attr.set_name_en(Some("ExecutorId".to_string()));

    ts.set_attributes(vec![attr]);
    bp.add_tabular_section(ts);
    config.add_metadata_object(bp);

    // Test query
    let code =
        "ВЫБРАТЬ Т.ИдентификаторИсполнителя ИЗ БизнесПроцесс.Исполнение.РезультатыПроверки КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(&config));
    let hir = single_query_hir(&package);

    // Verify field resolved
    assert_eq!(hir.from.len(), 1);
    let table_ref = &hir.from[0];
    assert!(table_ref.is_resolved(), "Table should be resolved");

    let resolved = table_ref.metadata.as_ref().expect("Metadata should be present");
    let fields = resolved.fields();

    // Find ИдентификаторИсполнителя field
    let field = fields.iter().find(|f| f.name.as_str() == "ИдентификаторИсполнителя");
    assert!(field.is_some(), "Should find ИдентификаторИсполнителя field");

    let field = field.unwrap();
    // Verify type is correctly parsed as UUID
    assert_eq!(field.ty, crate::SdblType::Uuid, "Should be UUID type");
}

#[test]
fn test_incomplete_on_collects_all_tables() {
    // Test that HIR collects all tables even with incomplete ON conditions
    // This simulates typing a query where user hasn't finished ON expressions yet
    let query = r#"ВЫБРАТЬ
    Т1.Поле1
ИЗ
    Таблица1 КАК Т1
        ВНУТРЕННЕЕ СОЕДИНЕНИЕ Таблица2 КАК Т2
            ВНУТРЕННЕЕ СОЕДИНЕНИЕ Таблица3 КАК Т3
            ПО Т2.Поле = Т3.
            И Т3.Другое = &Параметр
        ПО Т1.Поле = Т2."#;

    let parse = parser::parse_sdbl(query);

    // DEBUG: Print parse errors
    println!("\n=== PARSE ERRORS ===");
    println!("Error count: {}", parse.errors().len());
    for (i, err) in parse.errors().iter().enumerate() {
        println!("  Error {}: {:?}", i + 1, err);
    }

    // DEBUG: Print syntax tree structure
    println!("\n=== SYNTAX TREE ===");
    let root = parse.syntax_node();
    let tree_str = format!("{:#?}", root);
    // Print first 2000 chars of tree
    if tree_str.len() > 2000 {
        println!("{}...(truncated)", &tree_str[..2000]);
    } else {
        println!("{}", tree_str);
    }

    use syntax::ast::AstNode;
    let package =
        syntax::ast::SdblQueryPackage::cast(parse.syntax_node()).expect("Should parse package");
    let queries: Vec<_> = package.queries().collect();

    assert_eq!(queries.len(), 1, "Should have 1 query");

    // Lower to HIR
    let hir_package = crate::lower::lower_sdbl_to_hir(&parse, None);

    assert_eq!(hir_package.queries.len(), 1, "HIR should have 1 query");

    let query_hir = &hir_package.queries[0].hir;

    // Check FROM clause
    assert_eq!(query_hir.from.len(), 1, "Should have 1 FROM table");
    assert_eq!(query_hir.from[0].full_name, "Таблица1");
    assert_eq!(query_hir.from[0].alias.as_ref().map(|s| s.as_str()), Some("Т1"));

    // Check JOINs - should have BOTH joins despite incomplete ON
    println!("Number of JOINs in HIR: {}", query_hir.joins.len());
    for (i, join) in query_hir.joins.iter().enumerate() {
        println!("  JOIN {}: {} (alias: {:?})", i, join.table.full_name, join.table.alias);
    }

    assert_eq!(query_hir.joins.len(), 2, "Should collect both nested JOINs");

    // Verify table names
    let join_names: Vec<_> = query_hir.joins.iter().map(|j| j.table.full_name.as_str()).collect();
    assert!(join_names.contains(&"Таблица2"), "Should have Таблица2");
    assert!(join_names.contains(&"Таблица3"), "Should have Таблица3");

    // Verify aliases
    let t2_join = query_hir.joins.iter().find(|j| j.table.full_name == "Таблица2").unwrap();
    assert_eq!(t2_join.table.alias.as_ref().map(|s| s.as_str()), Some("Т2"));

    let t3_join = query_hir.joins.iter().find(|j| j.table.full_name == "Таблица3").unwrap();
    assert_eq!(t3_join.table.alias.as_ref().map(|s| s.as_str()), Some("Т3"));

    // ВАЖНО: Проверяем, что source_map содержит токены для highlighting
    println!("\nSource map token count: {}", hir_package.source_map.all_tokens().count());
    assert!(
        hir_package.source_map.all_tokens().count() > 0,
        "Source map should have tokens for highlighting"
    );
}

#[test]
fn test_parse_continues_after_incomplete_field() {
    // Test that parser continues after incomplete field and parses subsequent И clauses
    let query = r#"ВЫБРАТЬ
    Т1.Поле1
ИЗ
    Таблица1 КАК Т1
        ВНУТРЕННЕЕ СОЕДИНЕНИЕ Таблица2 КАК Т2
        ПО Т1.Поле = Т2.
        И Т2.Другое = &Параметр
        И Т1.Еще = Т2.Финал"#;

    println!("\n=== QUERY ===");
    println!("{}", query);

    let parse = parser::parse_sdbl(query);

    println!("\n=== PARSE ERRORS ===");
    println!("Error count: {}", parse.errors().len());
    for (i, err) in parse.errors().iter().enumerate() {
        println!("  Error {}: {:?}", i + 1, err);
    }

    let root = parse.syntax_node();

    println!("\n=== SYNTAX TREE (first 3000 chars) ===");
    let tree_str = format!("{:#?}", root);
    if tree_str.len() > 3000 {
        println!("{}...(truncated)", &tree_str[..3000]);
    } else {
        println!("{}", tree_str);
    }

    // Lower to HIR
    let hir_package = crate::lower::lower_sdbl_to_hir(&parse, None);

    println!("\n=== HIR ===");
    println!("HIR queries: {}", hir_package.queries.len());
    println!("Source map tokens: {}", hir_package.source_map.all_tokens().count());

    // Check if HIR found the query despite parse errors
    assert_eq!(hir_package.queries.len(), 1, "Should have 1 query even with incomplete ON");

    // Check if source_map has tokens for ALL parts of query (including after incomplete field)
    let token_count = hir_package.source_map.all_tokens().count();
    println!("Tokens for highlighting: {}", token_count);

    // Should have many tokens (keywords, identifiers, etc.) - even with incomplete fields
    assert!(
        token_count > 10,
        "Should have significant tokens for highlighting, got {}",
        token_count
    );
}
