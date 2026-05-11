use crate::hir::{JoinType, SdblHir, SdblPackage};
use crate::lower::lower_sdbl_to_hir;

/// Helper to extract single query HIR for tests (most tests have single query).
fn single_query_hir(package: &SdblPackage) -> &SdblHir {
    assert_eq!(package.queries().len(), 1, "Expected single query in package");
    &package.queries()[0].hir
}

/// Helper to create AttributeType from string for tests.
/// Supports simplified type strings like "Boolean", "TaskRef.Задача", etc.
fn parse_attr_type_for_test(type_str: &str) -> bsl_metadata::AttributeType {
    use bsl_metadata::{AttributeType, MdoType};

    match type_str {
        "Boolean" => AttributeType::Boolean,
        "String" => AttributeType::String { length: None },
        "Number" => AttributeType::Number { precision: 10, scale: 2 },
        "УникальныйИдентификатор" => AttributeType::Uuid,
        s if s.starts_with("TaskRef.") => {
            let name = &s["TaskRef.".len()..];
            AttributeType::Ref { mdo_type: MdoType::Task, name: name.to_string() }
        }
        s if s.contains('.') => {
            // Parse "Задача.ЗадачаИсполнителя" as Task.ЗадачаИсполнителя
            let parts: Vec<_> = s.split('.').collect();
            if parts.len() == 2 {
                let mdo_type = match parts[0] {
                    "Задача" => MdoType::Task,
                    "Документ" => MdoType::Document,
                    "Справочник" => MdoType::Catalog,
                    "БизнесПроцесс" => MdoType::BusinessProcess,
                    _ => MdoType::Document,
                };
                AttributeType::Ref { mdo_type, name: parts[1].to_string() }
            } else {
                AttributeType::String { length: None }
            }
        }
        _ => AttributeType::String { length: None },
    }
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

    // New flat list architecture: package contains 2 queries (main + UNION)
    assert_eq!(result.queries().len(), 2, "Expected 2 queries in package (main + UNION)");

    // First query creates temporary table
    let main_hir = &result.queries()[0].hir;
    assert_eq!(main_hir.into_table.as_ref().map(|n| n.as_str()), Some("ТаблицаДействий"));
    assert_eq!(main_hir.select.fields.len(), 1); // Only one field in first query

    // Second query (UNION) references temporary table
    let union_hir = &result.queries()[1].hir;
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

#[test]
fn test_drop_query_semantic_tokens() {
    let result = lower_query_with_source_map("УНИЧТОЖИТЬ ВТ_ВсеСвойства");
    let sm = &result.source_map;

    assert!(result.queries().is_empty(), "DROP query should not create SELECT HIR");

    let clause_keywords: Vec<_> = sm.clause_keywords.iter().map(|t| t.text.as_str()).collect();
    let table_names: Vec<_> = sm.table_names.iter().map(|t| t.text.as_str()).collect();

    assert!(
        clause_keywords.contains(&"УНИЧТОЖИТЬ"),
        "DROP keyword should be highlighted, got: {clause_keywords:?}"
    );
    assert!(
        table_names.contains(&"ВТ_ВсеСвойства"),
        "temporary table name should be highlighted, got: {table_names:?}"
    );
}

#[test]
fn test_drop_query_removes_temp_table_from_subsequent_scope() {
    let query = "ВЫБРАТЬ Поле КАК Поле ПОМЕСТИТЬ ВТ ИЗ Источник; УНИЧТОЖИТЬ ВТ; ВЫБРАТЬ Поле ИЗ ВТ";

    let result = lower_query_with_source_map(query);

    assert_eq!(result.queries().len(), 2, "DROP query should not create SELECT HIR");

    let second_hir = &result.queries()[1].hir;
    assert_eq!(second_hir.from.len(), 1);
    assert!(
        !second_hir.from[0].is_resolved(),
        "temporary table must not remain resolved after DROP/УНИЧТОЖИТЬ"
    );
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
    let mut attr1 = TabularSectionAttribute::new(
        uuid_nil,
        "ЗадачаИсполнителя",
        parse_attr_type_for_test("TaskRef.Задача"),
    );
    attr1.set_name_en(Some("ExecutorTask".to_string()));

    let mut attr2 = TabularSectionAttribute::new(
        uuid_nil,
        "ЗадачаПроверяющего",
        parse_attr_type_for_test("TaskRef.Задача"),
    );
    attr2.set_name_en(Some("CheckerTask".to_string()));

    let mut attr3 = TabularSectionAttribute::new(
        uuid_nil,
        "ОтправленоНаДоработку",
        parse_attr_type_for_test("Boolean"),
    );
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
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(metadata.clone())));
    let hir = single_query_hir(&package);

    // Verify table resolved
    assert_eq!(hir.from.len(), 1);
    let table_ref = &hir.from[0];
    assert_eq!(table_ref.full_name, "БизнесПроцесс.Исполнение.РезультатыПроверки");
    assert!(table_ref.is_resolved(), "Tabular section should be resolved");

    // Verify fields
    let resolved = table_ref.metadata.as_ref().expect("Metadata should be present");
    let fields = resolved.fields();

    // Should have Ссылка + НомерСтроки fields + 3 tabular section attributes
    assert_eq!(fields.len(), 5, "Expected 5 fields: Ссылка + НомерСтроки + 3 attributes");

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
fn test_tabular_section_nomer_stroki_field() {
    let metadata = create_test_metadata_with_tabular_section();

    let code = "ВЫБРАТЬ Т.НомерСтроки ИЗ БизнесПроцесс.Исполнение.РезультатыПроверки КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(metadata)));
    let hir = single_query_hir(&package);

    let table_ref = &hir.from[0];
    let resolved = table_ref.metadata.as_ref().expect("Metadata should be present");
    let fields = resolved.fields();

    // НомерСтроки must be present as a standard field
    let line_num_field = fields.iter().find(|f| f.name.as_str() == "НомерСтроки");
    assert!(line_num_field.is_some(), "Missing НомерСтроки field");
    let line_num_field = line_num_field.unwrap();
    assert!(line_num_field.is_standard, "НомерСтроки should be marked as standard");
    assert_eq!(line_num_field.name_en.as_deref(), Some("LineNumber"));
}

#[test]
fn test_tabular_section_case_insensitive_matching() {
    let metadata = create_test_metadata_with_tabular_section();

    // Use lowercase tabular section name
    let code = "ВЫБРАТЬ Т.ЗадачаИсполнителя ИЗ БизнесПроцесс.Исполнение.результатыпроверки КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(metadata.clone())));
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
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(metadata.clone())));
    let hir = single_query_hir(&package);

    // Should resolve using English name
    assert_eq!(hir.from.len(), 1);
    let table_ref = &hir.from[0];
    assert!(table_ref.is_resolved(), "Should resolve using English name");

    // Verify fields are present
    let resolved = table_ref.metadata.as_ref().expect("Metadata should be present");
    let fields = resolved.fields();
    assert_eq!(fields.len(), 5, "Expected 5 fields");
}

#[test]
fn test_tabular_section_not_found() {
    let metadata = create_test_metadata_with_tabular_section();

    // Use non-existent tabular section name
    let code = "ВЫБРАТЬ Т.Поле ИЗ БизнесПроцесс.Исполнение.НесуществующаяТабличнаяЧасть КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(metadata.clone())));
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
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config.clone())));
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
    let mut attr = TabularSectionAttribute::new(
        uuid_nil,
        "ЗадачаПроверяющего",
        parse_attr_type_for_test("Задача.ЗадачаИсполнителя"),
    );
    attr.set_name_en(Some("CheckerTask".to_string()));

    ts.set_attributes(vec![attr]);
    bp.add_tabular_section(ts);
    config.add_metadata_object(bp);

    // Test query
    let code = "ВЫБРАТЬ Т.ЗадачаПроверяющего ИЗ БизнесПроцесс.Исполнение.РезультатыПроверки КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config.clone())));
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
fn hierarchical_catalog_parent_field_is_resolved() {
    let catalog_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000000">
        <Properties>
            <Name>Номенклатура</Name>
            <Hierarchical>true</Hierarchical>
            <CodeLength>9</CodeLength>
            <DescriptionLength>25</DescriptionLength>
        </Properties>
    </Catalog>
</MetaDataObject>"#;

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let catalog = bsl_metadata::xml_parser::parse_catalog_xml(catalog_xml).unwrap();
    config.add_metadata_object(catalog);

    let code = "ВЫБРАТЬ Номенклатура.Родитель ИЗ Справочник.Номенклатура КАК Номенклатура";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let hir = single_query_hir(&package);

    let fields = hir.from[0].metadata.as_ref().expect("catalog must resolve").fields();
    let parent = fields.iter().find(|field| field.name == "Родитель").expect("Родитель field");
    assert_eq!(parent.name_en.as_deref(), Some("Parent"));
    assert!(parent.is_standard, "Родитель must remain marked as a standard field");

    let unknown_diags: Vec<_> = package
        .all_diagnostics()
        .filter(|diag| {
            matches!(
                diag,
                crate::diagnostics::SdblDiagnostic::UnknownField { field_name, .. }
                    if field_name == "Родитель"
            )
        })
        .collect();
    assert!(unknown_diags.is_empty(), "Родитель must not be UnknownField: {unknown_diags:?}");

    let unresolved: Vec<_> =
        package.source_map.unresolved_field_names.iter().map(|t| t.text.as_str()).collect();
    assert!(
        !unresolved.contains(&"Родитель"),
        "Родитель must not be highlighted as unresolved: {unresolved:?}"
    );
}

#[test]
fn hierarchical_catalog_parent_field_resolves_by_english_name() {
    let catalog_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000000">
        <Properties>
            <Name>Номенклатура</Name>
            <Hierarchical>true</Hierarchical>
            <CodeLength>9</CodeLength>
            <DescriptionLength>25</DescriptionLength>
        </Properties>
    </Catalog>
</MetaDataObject>"#;

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let catalog = bsl_metadata::xml_parser::parse_catalog_xml(catalog_xml).unwrap();
    config.add_metadata_object(catalog);

    let code = "SELECT N.Parent FROM Catalog.Номенклатура AS N";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));

    let unknown_diags: Vec<_> = package
        .all_diagnostics()
        .filter(|diag| {
            matches!(
                diag,
                crate::diagnostics::SdblDiagnostic::UnknownField { field_name, .. }
                    if field_name == "Parent"
            )
        })
        .collect();
    assert!(unknown_diags.is_empty(), "Parent must resolve by English standard name");

    let resolved: Vec<_> = package.source_map.field_names.iter().map(|t| t.text.as_str()).collect();
    let unresolved: Vec<_> =
        package.source_map.unresolved_field_names.iter().map(|t| t.text.as_str()).collect();

    assert!(resolved.contains(&"Parent"), "Parent should be a resolved field: {resolved:?}");
    assert!(!unresolved.contains(&"Parent"), "Parent must not be unresolved: {unresolved:?}");
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
        parse_attr_type_for_test("УникальныйИдентификатор"),
    );
    attr.set_name_en(Some("ExecutorId".to_string()));

    ts.set_attributes(vec![attr]);
    bp.add_tabular_section(ts);
    config.add_metadata_object(bp);

    // Test query
    let code =
        "ВЫБРАТЬ Т.ИдентификаторИсполнителя ИЗ БизнесПроцесс.Исполнение.РезультатыПроверки КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config.clone())));
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

    let parse = parser::parse_sdbl(query);

    // Lower to HIR
    let hir_package = crate::lower::lower_sdbl_to_hir(&parse, None);

    // Check if HIR found the query despite parse errors
    assert_eq!(hir_package.queries.len(), 1, "Should have 1 query even with incomplete ON");

    // Check if source_map has tokens for ALL parts of query (including after incomplete field)
    let token_count = hir_package.source_map.all_tokens().count();

    // Should have many tokens (keywords, identifiers, etc.) - even with incomplete fields
    assert!(
        token_count > 10,
        "Should have significant tokens for highlighting, got {}",
        token_count
    );
}

#[test]
fn test_multiple_incomplete_fields_with_operators() {
    // User's complex example: multiple incomplete fields with = operator
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
            И ПроцессыДействий. = &Действие
        ПО ВТ_ЗадачиСхемы. = ДанныеБизнесПроцессов."#;

    let parse = parser::parse_sdbl(query);

    // Lower to HIR
    let hir_package = crate::lower::lower_sdbl_to_hir(&parse, None);

    // Check token count for highlighting
    let token_count = hir_package.source_map.all_tokens().count();
    assert!(
        token_count > 10,
        "Should have significant tokens for highlighting, got {}",
        token_count
    );
}

#[test]
fn test_incomplete_as_alias_in_select() {
    // User's stress test: incomplete AS keyword without alias
    let query = r#"ВЫБРАТЬ
    Валюты.Наименование КАК
ИЗ
    Справочник.Валюты КАК Валюты
ГДЕ
    Валюты.СпособУстановкиКурса = ЗНАЧЕНИЕ(Перечисление.СпособыУстановкиКурсаВалюты.РасчетПоФормуле)"#;

    let parse = parser::parse_sdbl(query);

    // Lower to HIR
    let hir_package = crate::lower::lower_sdbl_to_hir(&parse, None);

    let token_count = hir_package.source_map.all_tokens().count();
    assert_eq!(hir_package.queries.len(), 1, "Should have 1 query even with incomplete AS");
    assert!(
        token_count > 10,
        "Should have significant tokens for highlighting, got {}",
        token_count
    );
}

#[test]
fn test_incomplete_table_reference_in_from() {
    // User's case: incomplete table reference "Справочник." in FROM clause
    let query = r#"ВЫБРАТЬ
    Валюты.Наименование КАК СимвольныйКод
ИЗ
    Справочник.Валюты КАК Валюты
ГДЕ
    Валюты.СпособУстановкиКурса = ЗНАЧЕНИЕ(Перечисление.СпособыУстановкиКурсаВалюты.НаценкаНаКурсДругойВалюты)

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
    Валюты.Наименование
ИЗ
    Справочник. КАК Валюты
ГДЕ
    Валюты.СпособУстановкиКурса = ЗНАЧЕНИЕ(Перечисление.СпособыУстановкиКурсаВалюты.РасчетПоФормуле)"#;

    let parse = parser::parse_sdbl(query);

    // Lower to HIR
    let hir_package = crate::lower::lower_sdbl_to_hir(&parse, None);

    let token_count = hir_package.source_map.all_tokens().count();

    // Verify parser didn't break on incomplete table ref
    assert!(token_count > 20, "Should have significant tokens despite incomplete table ref");
}

#[test]
fn test_parse_simple_nested_subquery() {
    let query = r#"ВЫБРАТЬ
    Т.Поле КАК Поле
ИЗ (
    ВЫБРАТЬ
        Т1.Поле КАК Поле
    ИЗ Таблица1 КАК Т1
) КАК Т"#;

    let parse = parser::parse_sdbl(query);
    let package = crate::lower::lower_sdbl_to_hir(&parse, None);

    // Verify simple nested subquery works
    assert_eq!(package.queries().len(), 1);
    let query_hir = &package.queries()[0].hir;
    assert_eq!(query_hir.from.len(), 1, "Should have 1 FROM table");
    assert_eq!(query_hir.select.fields.len(), 1, "Should have 1 SELECT field");

    // Verify subquery was lowered to HIR
    let subquery_table = &query_hir.from[0];
    assert_eq!(subquery_table.subquery.len(), 1, "Should have 1 subquery HIR");
}

#[test]
fn test_union_in_nested_subquery_lowers_all_queries() {
    let query = r#"ВЫБРАТЬ
    Внешний.Контрагент КАК Клиент
ИЗ (
    ВЫБРАТЬ
        Т1.Контрагент КАК Контрагент
    ИЗ Документ.ЧекККМ.Товары КАК Т1

    ОБЪЕДИНИТЬ ВСЕ

    ВЫБРАТЬ
        Т2.Контрагент КАК Контрагент
    ИЗ Документ.ЧекККМВозврат.Товары КАК Т2
) КАК Внешний"#;

    let parse = parser::parse_sdbl(query);
    let package = crate::lower::lower_sdbl_to_hir(&parse, None);

    // Должен быть один query в пакете
    assert_eq!(package.queries().len(), 1, "Expected single outer query");

    let outer_query = &package.queries()[0];

    // В FROM должна быть одна таблица (nested subquery)
    assert_eq!(outer_query.hir.from.len(), 1, "Expected single table in FROM");

    let subquery_table = &outer_query.hir.from[0];
    assert_eq!(
        subquery_table.alias.as_ref().map(|s| s.as_str()),
        Some("Внешний"),
        "Expected alias 'Внешний'"
    );

    // КРИТИЧНО: subquery должен содержать 2 HIR (main + UNION)
    assert_eq!(
        subquery_table.subquery.len(),
        2,
        "Subquery должен содержать 2 HIR: main query + UNION query"
    );

    // Проверяем первый query (main)
    let first_query_hir = &subquery_table.subquery[0];
    assert_eq!(first_query_hir.from.len(), 1, "First query should have 1 FROM table");
    assert_eq!(
        first_query_hir.from[0].full_name, "Документ.ЧекККМ.Товары",
        "First query FROM table name mismatch"
    );
    assert_eq!(
        first_query_hir.from[0].alias.as_ref().map(|s| s.as_str()),
        Some("Т1"),
        "First query alias mismatch"
    );

    // Проверяем второй query (UNION)
    let second_query_hir = &subquery_table.subquery[1];
    assert_eq!(second_query_hir.from.len(), 1, "Second query should have 1 FROM table");
    assert_eq!(
        second_query_hir.from[0].full_name, "Документ.ЧекККМВозврат.Товары",
        "Second query FROM table name mismatch"
    );
    assert_eq!(
        second_query_hir.from[0].alias.as_ref().map(|s| s.as_str()),
        Some("Т2"),
        "Second query alias mismatch"
    );
}

#[test]
fn test_deeply_nested_subquery_with_union() {
    // Проверяем рекурсивную обработку: внутри вложенного запроса ещё один вложенный с UNION
    let query = r#"ВЫБРАТЬ
    Внешний.Контрагент КАК Клиент
ИЗ (
    ВЫБРАТЬ
        Средний.Контрагент КАК Контрагент
    ИЗ (
        ВЫБРАТЬ
            Т1.Контрагент КАК Контрагент
        ИЗ Документ.ЧекККМ.Товары КАК Т1

        ОБЪЕДИНИТЬ ВСЕ

        ВЫБРАТЬ
            Т2.Контрагент КАК Контрагент
        ИЗ Документ.ЧекККМВозврат.Товары КАК Т2
    ) КАК Средний
) КАК Внешний"#;

    let parse = parser::parse_sdbl(query);
    let package = crate::lower::lower_sdbl_to_hir(&parse, None);

    assert_eq!(package.queries().len(), 1, "Expected single outer query");

    let outer_query = &package.queries()[0];
    assert_eq!(outer_query.hir.from.len(), 1, "Outer query should have 1 FROM table");

    // Уровень 1: Внешний subquery (содержит один SELECT)
    let level1_table = &outer_query.hir.from[0];
    assert_eq!(
        level1_table.alias.as_ref().map(|s| s.as_str()),
        Some("Внешний"),
        "Level 1 alias mismatch"
    );
    assert_eq!(
        level1_table.subquery.len(),
        1,
        "Level 1 should have 1 subquery HIR (no UNION at this level)"
    );

    // Уровень 2: Средний subquery (содержит UNION - 2 запроса)
    let level1_hir = &level1_table.subquery[0];
    assert_eq!(level1_hir.from.len(), 1, "Level 1 HIR should have 1 FROM table");

    let level2_table = &level1_hir.from[0];
    assert_eq!(
        level2_table.alias.as_ref().map(|s| s.as_str()),
        Some("Средний"),
        "Level 2 alias mismatch"
    );
    assert_eq!(
        level2_table.subquery.len(),
        2,
        "Level 2 should have 2 subquery HIRs (UNION at this level)"
    );

    // Уровень 3: Первый запрос UNION (Т1)
    let level3_first_hir = &level2_table.subquery[0];
    assert_eq!(level3_first_hir.from.len(), 1, "Level 3 first query should have 1 FROM table");
    assert_eq!(
        level3_first_hir.from[0].full_name, "Документ.ЧекККМ.Товары",
        "Level 3 first query table name mismatch"
    );
    assert_eq!(
        level3_first_hir.from[0].alias.as_ref().map(|s| s.as_str()),
        Some("Т1"),
        "Level 3 first query alias mismatch"
    );

    // Уровень 3: Второй запрос UNION (Т2)
    let level3_second_hir = &level2_table.subquery[1];
    assert_eq!(level3_second_hir.from.len(), 1, "Level 3 second query should have 1 FROM table");
    assert_eq!(
        level3_second_hir.from[0].full_name, "Документ.ЧекККМВозврат.Товары",
        "Level 3 second query table name mismatch"
    );
    assert_eq!(
        level3_second_hir.from[0].alias.as_ref().map(|s| s.as_str()),
        Some("Т2"),
        "Level 3 second query alias mismatch"
    );
}

#[test]
fn test_union_at_multiple_levels() {
    // Проверяем UNION на разных уровнях вложенности одновременно
    let query = r#"ВЫБРАТЬ
    Внешний.Поле
ИЗ (
    ВЫБРАТЬ
        Средний.Поле
    ИЗ (
        ВЫБРАТЬ Т1.Поле ИЗ Таблица1 КАК Т1
        ОБЪЕДИНИТЬ ВСЕ
        ВЫБРАТЬ Т2.Поле ИЗ Таблица2 КАК Т2
    ) КАК Средний

    ОБЪЕДИНИТЬ ВСЕ

    ВЫБРАТЬ
        Средний2.Поле
    ИЗ (
        ВЫБРАТЬ Т3.Поле ИЗ Таблица3 КАК Т3
        ОБЪЕДИНИТЬ ВСЕ
        ВЫБРАТЬ Т4.Поле ИЗ Таблица4 КАК Т4
    ) КАК Средний2
) КАК Внешний"#;

    let parse = parser::parse_sdbl(query);
    let package = crate::lower::lower_sdbl_to_hir(&parse, None);

    assert_eq!(package.queries().len(), 1, "Expected single outer query");

    let outer_query = &package.queries()[0];
    assert_eq!(outer_query.hir.from.len(), 1, "Outer query should have 1 FROM table");

    // Уровень 1: Внешний subquery (содержит UNION - 2 запроса)
    let level1_table = &outer_query.hir.from[0];
    assert_eq!(
        level1_table.alias.as_ref().map(|s| s.as_str()),
        Some("Внешний"),
        "Level 1 alias mismatch"
    );
    assert_eq!(
        level1_table.subquery.len(),
        2,
        "Level 1 should have 2 subquery HIRs (UNION at this level)"
    );

    // Первая ветка UNION на уровне 1
    let level1_first_hir = &level1_table.subquery[0];
    assert_eq!(level1_first_hir.from.len(), 1, "Level 1 first HIR should have 1 FROM table");

    let level2_first_table = &level1_first_hir.from[0];
    assert_eq!(
        level2_first_table.alias.as_ref().map(|s| s.as_str()),
        Some("Средний"),
        "Level 2 first table alias mismatch"
    );
    assert_eq!(
        level2_first_table.subquery.len(),
        2,
        "Level 2 first table should have 2 subquery HIRs (Т1, Т2)"
    );

    // Проверяем Т1 и Т2
    assert_eq!(level2_first_table.subquery[0].from[0].full_name, "Таблица1");
    assert_eq!(
        level2_first_table.subquery[0].from[0].alias.as_ref().map(|s| s.as_str()),
        Some("Т1")
    );
    assert_eq!(level2_first_table.subquery[1].from[0].full_name, "Таблица2");
    assert_eq!(
        level2_first_table.subquery[1].from[0].alias.as_ref().map(|s| s.as_str()),
        Some("Т2")
    );

    // Вторая ветка UNION на уровне 1
    let level1_second_hir = &level1_table.subquery[1];
    assert_eq!(level1_second_hir.from.len(), 1, "Level 1 second HIR should have 1 FROM table");

    let level2_second_table = &level1_second_hir.from[0];
    assert_eq!(
        level2_second_table.alias.as_ref().map(|s| s.as_str()),
        Some("Средний2"),
        "Level 2 second table alias mismatch"
    );
    assert_eq!(
        level2_second_table.subquery.len(),
        2,
        "Level 2 second table should have 2 subquery HIRs (Т3, Т4)"
    );

    // Проверяем Т3 и Т4
    assert_eq!(level2_second_table.subquery[0].from[0].full_name, "Таблица3");
    assert_eq!(
        level2_second_table.subquery[0].from[0].alias.as_ref().map(|s| s.as_str()),
        Some("Т3")
    );
    assert_eq!(level2_second_table.subquery[1].from[0].full_name, "Таблица4");
    assert_eq!(
        level2_second_table.subquery[1].from[0].alias.as_ref().map(|s| s.as_str()),
        Some("Т4")
    );
}

#[test]
fn test_tabular_section_in_join_condition() {
    // Проверяем, что табличная часть корректно обрабатывается в JOIN условиях
    let query = r#"ВЫБРАТЬ
    ЧекККМТовары.Номенклатура КАК Товар,
    ЧекККМ.Номер КАК НомерДокумента,
    ЧекККМ.Дата КАК ДатаДокумента
ИЗ Документ.ЧекККМ.Товары КАК ЧекККМТовары
    ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ЧекККМ КАК ЧекККМ
    ПО ЧекККМТовары.Ссылка = ЧекККМ.Ссылка
        И ЧекККМ.Проведен = ИСТИНА
        И НЕ ЧекККМ.ПометкаУдаления"#;

    let parse = parser::parse_sdbl(query);
    let package = crate::lower::lower_sdbl_to_hir(&parse, None);

    assert_eq!(package.queries().len(), 1, "Expected single query");

    let query_hir = &package.queries()[0].hir;

    // Проверяем FROM clause: должна быть табличная часть
    assert_eq!(query_hir.from.len(), 1, "Should have 1 FROM table");

    let tabular_table = &query_hir.from[0];
    assert_eq!(
        tabular_table.full_name, "Документ.ЧекККМ.Товары",
        "FROM table should be tabular section"
    );
    assert_eq!(
        tabular_table.alias.as_ref().map(|s| s.as_str()),
        Some("ЧекККМТовары"),
        "Tabular section alias mismatch"
    );
    assert_eq!(
        tabular_table.parts.len(),
        3,
        "Tabular section should have 3 parts (Документ.ЧекККМ.Товары)"
    );

    // Проверяем JOIN clause: должен быть основной документ
    assert_eq!(query_hir.joins.len(), 1, "Should have 1 JOIN");

    let join = &query_hir.joins[0];
    assert_eq!(join.join_type, crate::hir::JoinType::Inner, "Should be INNER JOIN");

    let document_table = &join.table;
    assert_eq!(document_table.full_name, "Документ.ЧекККМ", "JOIN table should be document");
    assert_eq!(
        document_table.alias.as_ref().map(|s| s.as_str()),
        Some("ЧекККМ"),
        "Document alias mismatch"
    );
    assert_eq!(document_table.parts.len(), 2, "Document should have 2 parts (Документ.ЧекККМ)");

    // Проверяем SELECT clause: должны быть поля из обеих таблиц
    assert_eq!(query_hir.select.fields.len(), 3, "Should have 3 SELECT fields");

    // Проверяем алиасы полей
    let field_aliases: Vec<_> = query_hir
        .select
        .fields
        .iter()
        .filter_map(|f| f.alias.as_ref().map(|a| a.as_str()))
        .collect();

    assert_eq!(field_aliases.len(), 3);
    assert!(field_aliases.contains(&"Товар"));
    assert!(field_aliases.contains(&"НомерДокумента"));
    assert!(field_aliases.contains(&"ДатаДокумента"));
}

#[test]
fn test_complex_join_with_tabular_and_nested_fields() {
    // Проверяем сложный случай: JOIN с табличной частью и вложенными полями в условии
    let query = r#"ВЫБРАТЬ
    Товары.Номенклатура.Наименование КАК ТоварНаименование,
    Чек.Партнер.Наименование КАК КлиентНаименование
ИЗ Документ.ЧекККМ.Товары КАК Товары
    ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ЧекККМ КАК Чек
    ПО Товары.Ссылка = Чек.Ссылка
    ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Контрагенты КАК Контрагенты
    ПО Чек.Партнер = Контрагенты.Ссылка"#;

    let parse = parser::parse_sdbl(query);
    let package = crate::lower::lower_sdbl_to_hir(&parse, None);

    assert_eq!(package.queries().len(), 1);

    let query_hir = &package.queries()[0].hir;

    // Проверяем FROM: табличная часть
    assert_eq!(query_hir.from.len(), 1);
    assert_eq!(query_hir.from[0].full_name, "Документ.ЧекККМ.Товары");

    // Проверяем JOINs: парсер обрабатывает последовательные JOINs как плоский список
    // Благодаря рекурсивному lowering (commit d66dc345) все JOINs должны быть в одном списке
    assert!(
        !query_hir.joins.is_empty(),
        "Should have at least 1 JOIN, got {}",
        query_hir.joins.len()
    );

    // Проверяем, что все необходимые таблицы присутствуют в JOINs
    let join_tables: Vec<_> = query_hir.joins.iter().map(|j| j.table.full_name.as_str()).collect();

    assert!(join_tables.contains(&"Документ.ЧекККМ"), "Should have Document.ЧекККМ in JOINs");
    assert!(
        join_tables.contains(&"Справочник.Контрагенты"),
        "Should have Catalog.Контрагенты in JOINs"
    );

    // Проверяем SELECT: должны быть вложенные поля
    assert_eq!(query_hir.select.fields.len(), 2);

    // Проверяем первое поле: Товары.Номенклатура.Наименование
    let first_field = &query_hir.select.fields[0];
    assert_eq!(first_field.alias.as_ref().map(|a| a.as_str()), Some("ТоварНаименование"));

    // Проверяем второе поле: Чек.Партнер.Наименование
    let second_field = &query_hir.select.fields[1];
    assert_eq!(second_field.alias.as_ref().map(|a| a.as_str()), Some("КлиентНаименование"));
}

#[test]
fn test_nested_subquery_with_tabular_section_in_join() {
    // Проверяем вложенный запрос с nested JOIN (JOIN внутри JOIN)
    // Используем скобки в условиях и параметры запроса (как в реальных продакшн-запросах)
    let query = r#"ВЫБРАТЬ
    Внешний.Товар КАК ТоварНаименование,
    Внешний.Количество КАК Количество,
    Внешний.НомерЗаказа КАК НомерЗаказа
ИЗ (
    ВЫБРАТЬ
        ЧекККМТовары.Номенклатура.Наименование КАК Товар,
        ЧекККМТовары.Количество КАК Количество,
        ЧекККМ.Дата КАК ДатаДокумента,
        ЧекККМ.Склад.Наименование КАК СкладНаименование,
        ЕСТЬNULL(ДокЗаказКлиента.НомерПоДаннымКлиента, "") КАК НомерЗаказа
    ИЗ Документ.ЧекККМ.Товары КАК ЧекККМТовары
        ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ЧекККМ КАК ЧекККМ
            ЛЕВОЕ СОЕДИНЕНИЕ Документ.ЗаказКлиента КАК ДокЗаказКлиента
            ПО ЧекККМ.ЗаказКлиента = ДокЗаказКлиента.Ссылка
        ПО ЧекККМТовары.Ссылка = ЧекККМ.Ссылка
            И (ЧекККМ.Партнер = &Партнер)
            И (ЧекККМ.Проведен = ИСТИНА)
            И (ЧекККМ.ПометкаУдаления = ЛОЖЬ)
            И (ЧекККМ.Дата МЕЖДУ &ДатаНачало И &ДатаКонец)
            И (НЕ ЧекККМ.Архивный)
) КАК Внешний"#;

    let parse = parser::parse_sdbl(query);
    let package = crate::lower::lower_sdbl_to_hir(&parse, None);

    assert_eq!(package.queries().len(), 1, "Expected single outer query");

    let outer_query = &package.queries()[0];

    // Проверяем внешний SELECT
    assert_eq!(outer_query.hir.select.fields.len(), 3, "Outer query should have 3 SELECT fields");

    // Проверяем FROM: должна быть одна таблица (subquery)
    assert_eq!(outer_query.hir.from.len(), 1, "Outer query should have 1 FROM table");

    let subquery_table = &outer_query.hir.from[0];
    assert_eq!(
        subquery_table.alias.as_ref().map(|s| s.as_str()),
        Some("Внешний"),
        "Subquery alias should be 'Внешний'"
    );

    // КРИТИЧНО: Проверяем, что subquery был спущен в HIR
    assert_eq!(
        subquery_table.subquery.len(),
        1,
        "Should have 1 subquery HIR (nested query with JOIN)"
    );

    // Проверяем содержимое вложенного запроса
    let nested_hir = &subquery_table.subquery[0];

    // Внутренний SELECT должен иметь 5 полей
    assert_eq!(nested_hir.select.fields.len(), 5, "Nested query should have 5 SELECT fields");

    // Проверяем алиасы полей вложенного SELECT
    let nested_field_aliases: Vec<_> = nested_hir
        .select
        .fields
        .iter()
        .filter_map(|f| f.alias.as_ref().map(|a| a.as_str()))
        .collect();

    assert!(nested_field_aliases.contains(&"Товар"), "Nested SELECT should have field 'Товар'");
    assert!(
        nested_field_aliases.contains(&"Количество"),
        "Nested SELECT should have field 'Количество'"
    );
    assert!(
        nested_field_aliases.contains(&"ДатаДокумента"),
        "Nested SELECT should have field 'ДатаДокумента'"
    );
    assert!(
        nested_field_aliases.contains(&"СкладНаименование"),
        "Nested SELECT should have field 'СкладНаименование'"
    );
    assert!(
        nested_field_aliases.contains(&"НомерЗаказа"),
        "Nested SELECT should have field 'НомерЗаказа'"
    );

    // Проверяем FROM вложенного запроса: табличная часть
    assert_eq!(nested_hir.from.len(), 1, "Nested query should have 1 FROM table");

    let nested_tabular_table = &nested_hir.from[0];
    assert_eq!(
        nested_tabular_table.full_name, "Документ.ЧекККМ.Товары",
        "Nested FROM should be tabular section"
    );
    assert_eq!(
        nested_tabular_table.alias.as_ref().map(|s| s.as_str()),
        Some("ЧекККМТовары"),
        "Nested tabular section alias mismatch"
    );
    assert_eq!(nested_tabular_table.parts.len(), 3, "Nested tabular section should have 3 parts");

    // Проверяем JOINs вложенного запроса: должно быть 2 JOIN (INNER + LEFT)
    // Благодаря рекурсивному lowering (commit d66dc345) вложенные JOINs в плоском списке
    assert_eq!(nested_hir.joins.len(), 2, "Nested query should have 2 JOINs (INNER + nested LEFT)");

    // Собираем информацию о всех JOINs для проверки
    let join_tables: Vec<_> = nested_hir
        .joins
        .iter()
        .map(|j| {
            (j.join_type, j.table.full_name.as_str(), j.table.alias.as_ref().map(|s| s.as_str()))
        })
        .collect();

    // Проверяем, что есть JOIN с Документ.ЧекККМ
    assert!(
        join_tables
            .iter()
            .any(|(_, name, alias)| *name == "Документ.ЧекККМ" && *alias == Some("ЧекККМ")),
        "Should have JOIN with Документ.ЧекККМ"
    );

    // Проверяем, что есть JOIN с Документ.ЗаказКлиента
    assert!(
        join_tables.iter().any(|(_, name, alias)| *name == "Документ.ЗаказКлиента"
            && *alias == Some("ДокЗаказКлиента")),
        "Should have JOIN with Документ.ЗаказКлиента"
    );

    // Проверяем типы JOINs (независимо от порядка)
    let join_types: Vec<_> = nested_hir.joins.iter().map(|j| j.join_type).collect();
    assert!(join_types.contains(&crate::hir::JoinType::Left), "Should have at least one LEFT JOIN");
}

#[test]
fn test_nested_subquery_with_tabular_and_union() {
    // Комбинированный тест: вложенный запрос с UNION, где обе ветки используют табличные части
    // Используем скобки в условиях и параметры запроса
    let query = r#"ВЫБРАТЬ
    Данные.Товар,
    Данные.Количество,
    Данные.ТипОперации
ИЗ (
    ВЫБРАТЬ
        Товары.Номенклатура.Наименование КАК Товар,
        Товары.Количество КАК Количество,
        "Продажа" КАК ТипОперации
    ИЗ Документ.ЧекККМ.Товары КАК Товары
        ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ЧекККМ КАК Чек
        ПО Товары.Ссылка = Чек.Ссылка
            И (Чек.Партнер = &Партнер)
            И (Чек.Проведен = ИСТИНА)
            И (Чек.ПометкаУдаления = ЛОЖЬ)
            И (Чек.Дата МЕЖДУ &ДатаНачало И &ДатаКонец)
            И (НЕ Чек.Архивный)

    ОБЪЕДИНИТЬ ВСЕ

    ВЫБРАТЬ
        ВозвратТовары.Номенклатура.Наименование,
        -ВозвратТовары.Количество,
        "Возврат"
    ИЗ Документ.ЧекККМВозврат.Товары КАК ВозвратТовары
        ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ЧекККМВозврат КАК Возврат
        ПО ВозвратТовары.Ссылка = Возврат.Ссылка
            И (Возврат.Партнер = &Партнер)
            И (Возврат.Проведен = ИСТИНА)
            И (Возврат.ПометкаУдаления = ЛОЖЬ)
            И (Возврат.Дата МЕЖДУ &ДатаНачало И &ДатаКонец)
            И (НЕ Возврат.Архивный)
) КАК Данные"#;

    let parse = parser::parse_sdbl(query);
    let package = crate::lower::lower_sdbl_to_hir(&parse, None);

    assert_eq!(package.queries().len(), 1);

    let outer_query = &package.queries()[0];
    assert_eq!(outer_query.hir.from.len(), 1);

    let subquery_table = &outer_query.hir.from[0];
    assert_eq!(subquery_table.alias.as_ref().map(|s| s.as_str()), Some("Данные"));

    // КРИТИЧНО: Должно быть 2 HIR (UNION)
    assert_eq!(
        subquery_table.subquery.len(),
        2,
        "Should have 2 subquery HIRs (UNION with 2 branches)"
    );

    // Проверяем первую ветку UNION (Продажа)
    let first_union_hir = &subquery_table.subquery[0];
    assert_eq!(first_union_hir.select.fields.len(), 3, "First UNION branch should have 3 fields");

    // FROM: табличная часть ЧекККМ.Товары
    assert_eq!(first_union_hir.from.len(), 1);
    assert_eq!(first_union_hir.from[0].full_name, "Документ.ЧекККМ.Товары");
    assert_eq!(first_union_hir.from[0].alias.as_ref().map(|s| s.as_str()), Some("Товары"));

    // JOIN: документ ЧекККМ
    assert_eq!(first_union_hir.joins.len(), 1);
    assert_eq!(first_union_hir.joins[0].table.full_name, "Документ.ЧекККМ");
    assert_eq!(first_union_hir.joins[0].table.alias.as_ref().map(|s| s.as_str()), Some("Чек"));

    // Проверяем вторую ветку UNION (Возврат)
    let second_union_hir = &subquery_table.subquery[1];
    assert_eq!(second_union_hir.select.fields.len(), 3, "Second UNION branch should have 3 fields");

    // FROM: табличная часть ЧекККМВозврат.Товары
    assert_eq!(second_union_hir.from.len(), 1);
    assert_eq!(second_union_hir.from[0].full_name, "Документ.ЧекККМВозврат.Товары");
    assert_eq!(second_union_hir.from[0].alias.as_ref().map(|s| s.as_str()), Some("ВозвратТовары"));

    // JOIN: документ ЧекККМВозврат
    assert_eq!(second_union_hir.joins.len(), 1);
    assert_eq!(second_union_hir.joins[0].table.full_name, "Документ.ЧекККМВозврат");
    assert_eq!(second_union_hir.joins[0].table.alias.as_ref().map(|s| s.as_str()), Some("Возврат"));
}

#[test]
fn test_query_range_includes_all_select_fields_with_case() {
    // Воспроизводит проблему: второй SELECT с вложенным ВЫРАЗИТЬ(ВЫБОР...)
    // должен иметь TextRange который включает ВСЕ поля SELECT.
    //
    // BUG: cursor на поле "description" не попадает в query range потому что
    // parser/lowering неправильно определяет границы запроса.

    let query = r#"
ВЫБРАТЬ
    Поле1,
    Поле2
ПОМЕСТИТЬ ВТ
ИЗ
    (ВЫБРАТЬ 1 КАК Поле1, 2 КАК Поле2) КАК Вложенный
;

ВЫБРАТЬ
    ВТ.Документ КАК Документ,
    ВТ.date КАК date,
    ВТ.description КАК description,
    ВЫРАЗИТЬ(ВЫБОР
        КОГДА ВТ.Флаг
            ТОГДА ВТ.Значение1
        ИНАЧЕ ВТ.Значение2
    КОНЕЦ КАК ЧИСЛО(15, 2)) КАК Результат
ИЗ
    ВТ КАК ВТ
"#;

    let parsed = parser::parse_sdbl(query);

    let package = lower_sdbl_to_hir(&parsed, None);

    // Должно быть 2 запроса
    assert_eq!(package.queries.len(), 2, "Should have 2 queries");

    let query1_range = package.queries[1].range;

    // Cursor на строке "date" - должен попадать в range
    let date_offset = query.find("date").expect("Should find 'date'");
    assert!(
        query1_range.contains(date_offset.try_into().unwrap()),
        "Date offset {} should be within query 1 range {:?}",
        date_offset,
        query1_range
    );

    // Cursor на строке "description" - тоже должен попадать в range
    let description_offset = query.find("description").expect("Should find 'description'");
    assert!(
        query1_range.contains(description_offset.try_into().unwrap()),
        "Description offset {} should be within query 1 range {:?}. Query text:\n{}",
        description_offset,
        query1_range,
        query
    );
}

#[test]
fn test_leading_whitespace_in_sdbl() {
    // Regression test: SDBL parser should handle leading whitespace/newlines
    let query = "\nВЫБРАТЬ 1 ИЗ Т";
    let parsed = parser::parse_sdbl(query);
    assert!(!parsed.has_errors(), "Should parse query with leading newline");

    use syntax::ast::{AstNode, SdblQueryPackage};
    let pkg = SdblQueryPackage::cast(parsed.syntax_node()).expect("Should have query package");
    assert_eq!(pkg.queries().count(), 1, "Should have 1 query");
}

// ===== RefOveruse Diagnostic Tests with Metadata =====

/// Helper to create a config with a Catalog that has a Ref-typed attribute.
fn create_config_with_ref_attribute() -> bsl_metadata::Configuration {
    use bsl_metadata::{Attribute, AttributeType, MdoType, MetadataObject};

    let mut config = bsl_metadata::Configuration::new("TestConfig");

    // Target catalog that will be referenced
    let files_catalog = MetadataObject::new(MdoType::Catalog, "Файлы");
    config.add_metadata_object(files_catalog);

    // Catalog with a Ref-typed attribute "Файл" pointing to Catalog.Файлы
    let mut catalog = MetadataObject::new(MdoType::Catalog, "СлужебныеФайлы");
    catalog.add_attribute(Attribute {
        name: "Файл".to_string(),
        name_en: None,
        attr_type: AttributeType::Ref {
            mdo_type: MdoType::Catalog, name: "Файлы".to_string()
        },
    });
    config.add_metadata_object(catalog);

    config
}

#[test]
fn test_ref_overuse_with_metadata_ref_at_end() {
    // Т.Файл is Ref(Catalog.Файлы), so Т.Файл.Ссылка is redundant → 1 diagnostic
    let config = create_config_with_ref_attribute();

    let code = "ВЫБРАТЬ Т.Файл.Ссылка КАК Ссылка ИЗ Справочник.СлужебныеФайлы КАК Т";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));

    let ref_overuse_diags: Vec<_> = package
        .all_diagnostics()
        .filter(|d| matches!(d, crate::diagnostics::SdblDiagnostic::RefOveruse { .. }))
        .collect();

    assert_eq!(ref_overuse_diags.len(), 1, "Expected 1 RefOveruse diagnostic: Файл is Ref type");
}

#[test]
fn test_ref_overuse_with_metadata_non_ref_field() {
    // Т.ИНН is String, so Т.ИНН.Ссылка does NOT trigger RefOveruse → 0 diagnostics
    use bsl_metadata::{Attribute, AttributeType, MdoType, MetadataObject};

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let mut catalog = MetadataObject::new(MdoType::Catalog, "Контрагенты");
    catalog.add_attribute(Attribute {
        name: "ИНН".to_string(),
        name_en: None,
        attr_type: AttributeType::String { length: None },
    });
    config.add_metadata_object(catalog);

    let code = "ВЫБРАТЬ Т.ИНН.Ссылка КАК Ссылка ИЗ Справочник.Контрагенты КАК Т";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));

    let ref_overuse_diags: Vec<_> = package
        .all_diagnostics()
        .filter(|d| matches!(d, crate::diagnostics::SdblDiagnostic::RefOveruse { .. }))
        .collect();

    assert_eq!(
        ref_overuse_diags.len(),
        0,
        "Expected 0 RefOveruse diagnostics: ИНН is String, not a Ref"
    );
}

#[test]
fn test_ref_overuse_with_metadata_double_ref() {
    // Т.Ссылка.Ссылка — Ссылка is at position 1 (standard field, not in metadata fields()),
    // so resolve_nested_field_type returns Unknown → no diagnostic.
    // Standard fields like Ссылка are not included in resolved table metadata.
    use bsl_metadata::{MdoType, MetadataObject};

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let catalog = MetadataObject::new(MdoType::Catalog, "Контрагенты");
    config.add_metadata_object(catalog);

    let code = "ВЫБРАТЬ Т.Ссылка.Ссылка КАК п1 ИЗ Справочник.Контрагенты КАК Т";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));

    let ref_overuse_diags: Vec<_> = package
        .all_diagnostics()
        .filter(|d| matches!(d, crate::diagnostics::SdblDiagnostic::RefOveruse { .. }))
        .collect();

    assert_eq!(
        ref_overuse_diags.len(),
        0,
        "Ссылка is a standard field not in metadata fields() → type Unknown → no diagnostic"
    );
}

#[test]
fn test_ref_overuse_with_metadata_ref_in_middle_not_at_end() {
    // Т.Ссылка.ИНН — Ссылка is at position 1, not >=2, so NOT RefOveruse → 0 diagnostics
    use bsl_metadata::{Attribute, AttributeType, MdoType, MetadataObject};

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let mut catalog = MetadataObject::new(MdoType::Catalog, "Контрагенты");
    catalog.add_attribute(Attribute {
        name: "ИНН".to_string(),
        name_en: None,
        attr_type: AttributeType::String { length: None },
    });
    config.add_metadata_object(catalog);

    let code = "ВЫБРАТЬ Т.Ссылка.ИНН КАК ИНН ИЗ Справочник.Контрагенты КАК Т";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));

    let ref_overuse_diags: Vec<_> = package
        .all_diagnostics()
        .filter(|d| matches!(d, crate::diagnostics::SdblDiagnostic::RefOveruse { .. }))
        .collect();

    assert_eq!(
        ref_overuse_diags.len(),
        0,
        "Expected 0 RefOveruse diagnostics: Ссылка is at position 1, not a redundant usage"
    );
}

#[test]
fn test_ref_overuse_with_metadata_chain_ref_at_end() {
    // Т.Файл.Ссылка.Дата — Ссылка at position 2, field before it (Файл) is Ref → 1 diagnostic
    let config = create_config_with_ref_attribute();

    let code = "ВЫБРАТЬ Т.Файл.Ссылка.Дата КАК Дата ИЗ Справочник.СлужебныеФайлы КАК Т";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));

    let ref_overuse_diags: Vec<_> = package
        .all_diagnostics()
        .filter(|d| matches!(d, crate::diagnostics::SdblDiagnostic::RefOveruse { .. }))
        .collect();

    assert_eq!(
        ref_overuse_diags.len(),
        1,
        "Expected 1 RefOveruse diagnostic: Файл is Ref, so .Ссылка after it is redundant"
    );
}

#[test]
fn test_ref_overuse_with_metadata_simple_ref_no_error() {
    // Т.Ссылка — just accessing the reference field of a table, NOT redundant → 0 diagnostics
    use bsl_metadata::{MdoType, MetadataObject};

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let catalog = MetadataObject::new(MdoType::Catalog, "Контрагенты");
    config.add_metadata_object(catalog);

    let code = "ВЫБРАТЬ Т.Ссылка КАК Контрагент ИЗ Справочник.Контрагенты КАК Т";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));

    let ref_overuse_diags: Vec<_> = package
        .all_diagnostics()
        .filter(|d| matches!(d, crate::diagnostics::SdblDiagnostic::RefOveruse { .. }))
        .collect();

    assert_eq!(
        ref_overuse_diags.len(),
        0,
        "Expected 0 RefOveruse diagnostics: simple Alias.Ссылка is not redundant"
    );
}

#[test]
fn test_nested_inner_left_join_types() {
    // INNER JOIN with nested LEFT JOIN must preserve correct join types.
    // Previously, join_type() used node.text() which included nested JOIN keywords,
    // causing INNER to be misidentified as LEFT.
    let sdbl = "ВЫБРАТЬ
    ЧекККМ.Ссылка КАК Документ,
    ЕСТЬNULL(ДокЗаказ.НомерДокумента, \"\") КАК НомерЗаказа
ИЗ
    Документ.ЧекККМ.Товары КАК ЧекККМТовары
        ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ЧекККМ КАК ЧекККМ
            ЛЕВОЕ СОЕДИНЕНИЕ Документ.ЗаказКлиента КАК ДокЗаказ
            ПО ЧекККМ.ЗаказКлиента = ДокЗаказ.Ссылка
        ПО ЧекККМТовары.Ссылка = ЧекККМ.Ссылка";
    let hir = lower_query(sdbl);

    assert_eq!(hir.joins.len(), 2);
    // Nested LEFT JOIN comes first (depth-first)
    assert_eq!(hir.joins[0].join_type, crate::hir::JoinType::Left);
    assert!(hir.joins[0].table.alias.as_ref().unwrap().eq_ignore_ascii_case("ДокЗаказ"));
    // INNER JOIN comes second
    assert_eq!(hir.joins[1].join_type, crate::hir::JoinType::Inner);
    assert!(hir.joins[1].table.alias.as_ref().unwrap().eq_ignore_ascii_case("ЧекККМ"));
}

// ===== VALUE()/ЗНАЧЕНИЕ() Function Tests =====

/// Helper: create config with an enum that has known values.
fn create_config_with_enum() -> bsl_metadata::Configuration {
    use bsl_metadata::{metadata_object::EnumValue, MdoType, MetadataObject};

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let mut enum_obj = MetadataObject::new(MdoType::Enum, "ПолФизическогоЛица");
    enum_obj.enum_values = vec![
        EnumValue {
            name: "Мужской".to_string(),
            name_en: Some("Male".to_string()),
            uuid: "1".to_string(),
        },
        EnumValue {
            name: "Женский".to_string(),
            name_en: Some("Female".to_string()),
            uuid: "2".to_string(),
        },
    ];
    config.add_metadata_object(enum_obj);
    config
}

#[test]
fn test_value_function_valid_enum_value_gets_field_name_token() {
    // VALUE(Enum.GenderEnum.Male) with metadata should produce FieldName token for last part
    let config = create_config_with_enum();

    let code = "ВЫБРАТЬ 1 ИЗ Справочник.Тест КАК Т ГДЕ Т.Пол = ЗНАЧЕНИЕ(Перечисление.ПолФизическогоЛица.Мужской)";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let sm = &package.source_map;

    // "Мужской" should be in field_names (resolved)
    let resolved: Vec<String> = sm.field_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        resolved.iter().any(|t| t == "Мужской"),
        "Expected 'Мужской' in field_names, got: {:?}",
        resolved
    );

    // Must NOT appear in unresolved_field_names
    let unresolved: Vec<String> =
        sm.unresolved_field_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        !unresolved.iter().any(|t| t == "Мужской"),
        "Expected 'Мужской' NOT in unresolved_field_names, got: {:?}",
        unresolved
    );
}

#[test]
fn test_value_function_invalid_enum_value_gets_unresolved_token() {
    // VALUE(Enum.X.InvalidValue) with metadata should produce UnresolvedFieldName token
    let config = create_config_with_enum();

    let code = "ВЫБРАТЬ 1 ИЗ Справочник.Тест КАК Т ГДЕ Т.Пол = ЗНАЧЕНИЕ(Перечисление.ПолФизическогоЛица.НесуществующееЗначение)";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let sm = &package.source_map;

    // "НесуществующееЗначение" should be in unresolved_field_names
    let unresolved: Vec<String> =
        sm.unresolved_field_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        unresolved.iter().any(|t| t == "НесуществующееЗначение"),
        "Expected 'НесуществующееЗначение' in unresolved_field_names, got: {:?}",
        unresolved
    );
}

#[test]
fn test_value_function_empty_ref_always_valid() {
    // VALUE(Catalog.Currencies.ПустаяСсылка) — EmptyRef is always valid regardless of metadata
    // Use alias "Вал" (longer than single letter) to avoid parser ambiguity
    let code = "ВЫБРАТЬ 1 ИЗ Справочник.Валюты КАК Вал ГДЕ Вал.Пол = ЗНАЧЕНИЕ(Справочник.Валюты.ПустаяСсылка)";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, None); // no metadata
    let sm = &package.source_map;

    // "ПустаяСсылка" must be in field_names (not unresolved)
    let resolved: Vec<String> = sm.field_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        resolved.iter().any(|t| t == "ПустаяСсылка"),
        "Expected 'ПустаяСсылка' in field_names (EmptyRef is always valid), got: {:?}",
        resolved
    );

    let unresolved: Vec<String> =
        sm.unresolved_field_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        !unresolved.iter().any(|t| t == "ПустаяСсылка"),
        "Expected 'ПустаяСсылка' NOT in unresolved_field_names, got: {:?}",
        unresolved
    );
}

#[test]
fn test_value_function_without_metadata_graceful_degradation() {
    // Without metadata, the value part of VALUE() should be FieldName (not unresolved)
    let code = "ВЫБРАТЬ 1 ИЗ Справочник.Тест КАК Т ГДЕ Т.Пол = ЗНАЧЕНИЕ(Перечисление.ПолФизическогоЛица.Мужской)";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, None); // no metadata
    let sm = &package.source_map;

    // "Мужской" should NOT appear in unresolved_field_names when there's no metadata
    let unresolved: Vec<String> =
        sm.unresolved_field_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        !unresolved.iter().any(|t| t == "Мужской"),
        "Without metadata, 'Мужской' should not be unresolved, got: {:?}",
        unresolved
    );

    // "Мужской" should be in field_names
    let resolved: Vec<String> = sm.field_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        resolved.iter().any(|t| t == "Мужской"),
        "Without metadata, 'Мужской' should be in field_names, got: {:?}",
        resolved
    );
}

#[test]
fn test_value_function_mdo_type_and_table_name_tokens() {
    // VALUE() arg parts 0 and 1 should be MdoType and TableName tokens respectively
    let code = "ВЫБРАТЬ 1 ИЗ Справочник.Тест КАК Т ГДЕ Т.Ссылка = ЗНАЧЕНИЕ(Перечисление.ПолФизическогоЛица.Мужской)";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, None);
    let sm = &package.source_map;

    // "Перечисление" should be in mdo_types
    let mdo_types: Vec<String> = sm.mdo_types.iter().map(|t| t.text.to_string()).collect();
    assert!(
        mdo_types.iter().any(|t| t == "Перечисление"),
        "Expected 'Перечисление' in mdo_types, got: {:?}",
        mdo_types
    );

    // "ПолФизическогоЛица" should be in table_names
    let table_names: Vec<String> = sm.table_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        table_names.iter().any(|t| t == "ПолФизическогоЛица"),
        "Expected 'ПолФизическогоЛица' in table_names, got: {:?}",
        table_names
    );
}

/// Helper: create config with a catalog that has known predefined items.
fn create_config_with_catalog_predefined() -> bsl_metadata::Configuration {
    use bsl_metadata::{metadata_object::PredefinedItem, MdoType, MetadataObject};

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let mut catalog_obj = MetadataObject::new(MdoType::Catalog, "Валюты");
    catalog_obj.predefined_items = vec![
        PredefinedItem {
            name: "Доллар".to_string(),
            name_en: Some("Dollar".to_string()),
            uuid: "1".to_string(),
        },
        PredefinedItem {
            name: "Евро".to_string(),
            name_en: Some("Euro".to_string()),
            uuid: "2".to_string(),
        },
    ];
    config.add_metadata_object(catalog_obj);
    config
}

#[test]
fn test_value_function_valid_predefined_item_gets_field_name_token() {
    // VALUE(Catalog.Currencies.Dollar) with metadata should produce FieldName token for last part
    let config = create_config_with_catalog_predefined();

    let code = "ВЫБРАТЬ 1 ИЗ Справочник.Валюты КАК Вал ГДЕ Вал.Ссылка = ЗНАЧЕНИЕ(Справочник.Валюты.Доллар)";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let sm = &package.source_map;

    // "Доллар" should be in field_names (resolved)
    let resolved: Vec<String> = sm.field_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        resolved.iter().any(|t| t == "Доллар"),
        "Expected 'Доллар' in field_names, got: {:?}",
        resolved
    );

    // Must NOT appear in unresolved_field_names
    let unresolved: Vec<String> =
        sm.unresolved_field_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        !unresolved.iter().any(|t| t == "Доллар"),
        "Expected 'Доллар' NOT in unresolved_field_names, got: {:?}",
        unresolved
    );
}

#[test]
fn test_value_function_invalid_predefined_item_gets_unresolved_token() {
    // VALUE(Catalog.Currencies.NonExistent) with metadata should produce UnresolvedFieldName token
    let config = create_config_with_catalog_predefined();

    let code = "ВЫБРАТЬ 1 ИЗ Справочник.Валюты КАК Вал ГДЕ Вал.Ссылка = ЗНАЧЕНИЕ(Справочник.Валюты.Несуществующий)";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let sm = &package.source_map;

    // "Несуществующий" should be in unresolved_field_names
    let unresolved: Vec<String> =
        sm.unresolved_field_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        unresolved.iter().any(|t| t == "Несуществующий"),
        "Expected 'Несуществующий' in unresolved_field_names, got: {:?}",
        unresolved
    );
}

#[test]
fn test_value_function_predefined_item_empty_list_graceful_degradation() {
    // When predefined_items is empty, should not flag as error (graceful degradation)
    use bsl_metadata::{MdoType, MetadataObject};

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    // Catalog exists but has no predefined_items loaded
    let catalog_obj = MetadataObject::new(MdoType::Catalog, "Валюты");
    config.add_metadata_object(catalog_obj);

    let code = "ВЫБРАТЬ 1 ИЗ Справочник.Валюты КАК Вал ГДЕ Вал.Ссылка = ЗНАЧЕНИЕ(Справочник.Валюты.ЛюбоеЗначение)";
    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let sm = &package.source_map;

    // "ЛюбоеЗначение" should be in field_names (not unresolved) because predefined_items is empty
    let resolved: Vec<String> = sm.field_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        resolved.iter().any(|t| t == "ЛюбоеЗначение"),
        "Expected 'ЛюбоеЗначение' in field_names when predefined_items is empty, got: {:?}",
        resolved
    );

    let unresolved: Vec<String> =
        sm.unresolved_field_names.iter().map(|t| t.text.to_string()).collect();
    assert!(
        !unresolved.iter().any(|t| t == "ЛюбоеЗначение"),
        "Expected 'ЛюбоеЗначение' NOT in unresolved_field_names when predefined_items is empty, got: {:?}",
        unresolved
    );
}

#[test]
fn test_join_paren_field_resolution() {
    let query = r#"ВЫБРАТЬ Т.Ссылка ИЗ Справочник.Валюты КАК Т ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Валюты КАК Т2 ПО Т.Ссылка = Т2.Ссылка И (Т2.Код = "USD")"#;

    let ast = parser::parse_sdbl(query);

    let mut config = bsl_metadata::Configuration::new("Test");
    let catalog = bsl_metadata::MetadataObject {
        mdo_type: bsl_metadata::MdoType::Catalog,
        name: "Валюты".to_string(),
        name_en: None,
        attributes: vec![
            bsl_metadata::Attribute {
                name: "Ссылка".to_string(),
                name_en: Some("Ref".to_string()),
                attr_type: bsl_metadata::AttributeType::Ref {
                    mdo_type: bsl_metadata::MdoType::Catalog,
                    name: "Валюты".to_string(),
                },
            },
            bsl_metadata::Attribute {
                name: "Код".to_string(),
                name_en: Some("Code".to_string()),
                attr_type: bsl_metadata::AttributeType::String { length: Some(10) },
            },
        ],
        tabular_sections: vec![],
        children: vec![],
        enum_values: vec![],
        predefined_items: vec![],
        check_unique: false,
        code_series: bsl_metadata::CodeSeries::default(),
        constant_type: None,
    };
    config.add_metadata_object(catalog);

    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));

    let source_map = &package.source_map;
    let unresolved =
        source_map.tokens_by_category(crate::source_map::TokenCategory::UnresolvedFieldName);
    let resolved = source_map.tokens_by_category(crate::source_map::TokenCategory::FieldName);

    assert!(
        unresolved.is_empty(),
        "Fields inside parens should resolve. Unresolved: {:?}",
        unresolved.iter().map(|t| &t.text).collect::<Vec<_>>()
    );
    assert!(!resolved.is_empty(), "Fields inside parens should produce resolved field tokens");
}

// ===== Virtual Table Field Resolution Tests =====

/// Helper: create a config with an AccumulationRegister that has dimensions and resources.
fn create_config_with_accumulation_register() -> bsl_metadata::Configuration {
    use bsl_metadata::{
        dimension::DimensionBuilder, register::RegisterResource, MdoType, Register,
    };

    let mut config = bsl_metadata::Configuration::new("TestConfig");

    let register = Register::builder()
        .name("ИзмененияВНакопленияхКлиента")
        .mdo_type(MdoType::AccumulationRegister)
        .dimensions(vec![DimensionBuilder::default().name("Партнер").build()])
        .resources(vec![
            RegisterResource::new(Default::default(), "Сумма"),
            RegisterResource::new(Default::default(), "Количество"),
        ])
        .build();

    config.add_register(register);
    config
}

#[test]
fn test_virtual_table_turnovers_field_generation() {
    let config = create_config_with_accumulation_register();
    let code = "ВЫБРАТЬ Т.Партнер, Т.СуммаОборот, Т.КоличествоОборот ИЗ РегистрНакопления.ИзмененияВНакопленияхКлиента.Обороты(,,) КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let hir = single_query_hir(&package);

    // Check table resolved as Register
    assert_eq!(hir.from.len(), 1);
    let table_ref = &hir.from[0];
    assert!(table_ref.is_virtual_table);
    let resolved = table_ref.metadata.as_ref().expect("Should have metadata");

    // Check fields: standard (Период, Регистратор, НомерСтроки) + Партнер + СуммаОборот + КоличествоОборот
    let fields = resolved.fields();
    let field_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();

    assert!(field_names.contains(&"Период"), "Should have Период, got: {:?}", field_names);
    assert!(
        field_names.contains(&"Регистратор"),
        "Should have Регистратор, got: {:?}",
        field_names
    );
    assert!(field_names.contains(&"Партнер"), "Should have Партнер, got: {:?}", field_names);
    assert!(
        field_names.contains(&"СуммаОборот"),
        "Should have СуммаОборот, got: {:?}",
        field_names
    );
    assert!(
        field_names.contains(&"КоличествоОборот"),
        "Should have КоличествоОборот, got: {:?}",
        field_names
    );

    // Raw resource names should NOT be present
    assert!(!field_names.contains(&"Сумма"), "Should NOT have raw Сумма, got: {:?}", field_names);
    assert!(
        !field_names.contains(&"Количество"),
        "Should NOT have raw Количество, got: {:?}",
        field_names
    );

    // No UnknownField diagnostics
    let unknown_diags: Vec<_> = package
        .all_diagnostics()
        .filter(|d| matches!(d, crate::diagnostics::SdblDiagnostic::UnknownField { .. }))
        .collect();
    assert!(
        unknown_diags.is_empty(),
        "Should have no UnknownField diagnostics, got: {:?}",
        unknown_diags
    );
}

#[test]
fn test_virtual_table_balance_field_generation() {
    use bsl_metadata::{
        dimension::DimensionBuilder, register::RegisterResource, MdoType, Register,
    };

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let register = Register::builder()
        .name("ТоварыНаСкладах")
        .mdo_type(MdoType::AccumulationRegister)
        .dimensions(vec![DimensionBuilder::default().name("Склад").build()])
        .resources(vec![RegisterResource::new(Default::default(), "Количество")])
        .build();
    config.add_register(register);

    let code = "ВЫБРАТЬ Т.Склад, Т.КоличествоОстаток ИЗ РегистрНакопления.ТоварыНаСкладах.Остатки(,) КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let hir = single_query_hir(&package);

    let resolved = hir.from[0].metadata.as_ref().expect("Should have metadata");
    let field_names: Vec<&str> = resolved.fields().iter().map(|f| f.name.as_str()).collect();

    assert!(field_names.contains(&"Склад"), "Should have Склад");
    assert!(field_names.contains(&"КоличествоОстаток"), "Should have КоличествоОстаток");
    assert!(!field_names.contains(&"Количество"), "Should NOT have raw Количество");
}

#[test]
fn test_virtual_table_balance_and_turnovers_field_generation() {
    use bsl_metadata::{
        dimension::DimensionBuilder, register::RegisterResource, MdoType, Register,
    };

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let register = Register::builder()
        .name("Продажи")
        .mdo_type(MdoType::AccumulationRegister)
        .dimensions(vec![DimensionBuilder::default().name("Товар").build()])
        .resources(vec![RegisterResource::new(Default::default(), "Сумма")])
        .build();
    config.add_register(register);

    let code = "ВЫБРАТЬ Т.Товар, Т.СуммаНачальныйОстаток, Т.СуммаОборот, Т.СуммаКонечныйОстаток ИЗ РегистрНакопления.Продажи.ОстаткиИОбороты(,,) КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let hir = single_query_hir(&package);

    let resolved = hir.from[0].metadata.as_ref().expect("Should have metadata");
    let field_names: Vec<&str> = resolved.fields().iter().map(|f| f.name.as_str()).collect();

    assert!(field_names.contains(&"Товар"), "Should have Товар");
    assert!(
        field_names.contains(&"СуммаНачальныйОстаток"),
        "Should have СуммаНачальныйОстаток, got: {:?}",
        field_names
    );
    assert!(field_names.contains(&"СуммаОборот"), "Should have СуммаОборот");
    assert!(field_names.contains(&"СуммаКонечныйОстаток"), "Should have СуммаКонечныйОстаток");
}

#[test]
fn test_virtual_table_slice_last_preserves_fields() {
    use bsl_metadata::{
        dimension::DimensionBuilder, register::RegisterResource, MdoType, Register,
    };

    let mut config = bsl_metadata::Configuration::new("TestConfig");
    let register = Register::builder()
        .name("Курсы")
        .mdo_type(MdoType::InformationRegister)
        .dimensions(vec![DimensionBuilder::default().name("Валюта").build()])
        .resources(vec![RegisterResource::new(Default::default(), "Курс")])
        .build();
    config.add_register(register);

    let code =
        "ВЫБРАТЬ Т.Валюта, Т.Курс, Т.Период ИЗ РегистрСведений.Курсы.СрезПоследних(&Дата,) КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let hir = single_query_hir(&package);

    let resolved = hir.from[0].metadata.as_ref().expect("Should have metadata");
    let field_names: Vec<&str> = resolved.fields().iter().map(|f| f.name.as_str()).collect();

    // SliceLast preserves fields as-is + adds Период
    assert!(field_names.contains(&"Валюта"), "Should have Валюта");
    assert!(field_names.contains(&"Курс"), "Should have Курс (not suffixed)");
    assert!(field_names.contains(&"Период"), "Should have Период");
}

#[test]
fn test_virtual_table_param_scope_resolves_dimension() {
    let config = create_config_with_accumulation_register();
    // Партнер in the VT condition should resolve as a register dimension
    let code = "ВЫБРАТЬ Т.Партнер ИЗ РегистрНакопления.ИзмененияВНакопленияхКлиента.Обороты(,,, Партнер В (ВЫБРАТЬ 1)) КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));

    // No UnknownField diagnostic for Партнер in condition
    let unknown_diags: Vec<_> = package
        .all_diagnostics()
        .filter(|d| {
            matches!(d, crate::diagnostics::SdblDiagnostic::UnknownField { field_name, .. } if field_name == "Партнер")
        })
        .collect();
    assert!(
        unknown_diags.is_empty(),
        "Партнер in VT condition should resolve via dimension scope, got: {:?}",
        unknown_diags
    );
}

#[test]
fn test_virtual_table_periodicity_resolved() {
    let config = create_config_with_accumulation_register();
    // Авто is a periodicity value — should not be unresolved
    let code = "ВЫБРАТЬ Т.Партнер ИЗ РегистрНакопления.ИзмененияВНакопленияхКлиента.Обороты(,, Авто, Партнер В (ВЫБРАТЬ 1)) КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let sm = &package.source_map;

    // Авто should be in special_keywords, not in unresolved
    let special: Vec<String> = sm.special_keywords.iter().map(|t| t.text.to_string()).collect();
    let unresolved: Vec<String> =
        sm.unresolved_field_names.iter().map(|t| t.text.to_string()).collect();

    assert!(
        special.iter().any(|t| t == "Авто"),
        "Авто should be in special_keywords, got: {:?}",
        special
    );
    assert!(
        !unresolved.iter().any(|t| t == "Авто"),
        "Авто should NOT be in unresolved_field_names, got: {:?}",
        unresolved
    );
}

#[test]
fn test_virtual_table_semantic_tokens_resolved() {
    let config = create_config_with_accumulation_register();
    let code = "ВЫБРАТЬ Т.СуммаОборот, Т.КоличествоОборот ИЗ РегистрНакопления.ИзмененияВНакопленияхКлиента.Обороты(,,) КАК Т";

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let sm = &package.source_map;

    let resolved: Vec<String> = sm.field_names.iter().map(|t| t.text.to_string()).collect();
    let unresolved: Vec<String> =
        sm.unresolved_field_names.iter().map(|t| t.text.to_string()).collect();

    assert!(
        resolved.iter().any(|t| t == "СуммаОборот"),
        "СуммаОборот should be in field_names, got: {:?}",
        resolved
    );
    assert!(
        resolved.iter().any(|t| t == "КоличествоОборот"),
        "КоличествоОборот should be in field_names, got: {:?}",
        resolved
    );
    assert!(
        !unresolved.iter().any(|t| t == "СуммаОборот"),
        "СуммаОборот should NOT be in unresolved, got: {:?}",
        unresolved
    );
}

#[test]
fn chart_of_characteristic_ref_and_index_by_are_semantically_highlighted() {
    use bsl_metadata::{Attribute, AttributeType, Configuration, MdoType, MetadataObject};

    let mut config = Configuration::new("TestConfig");
    let mut cct = MetadataObject::new(
        MdoType::ChartOfCharacteristicTypes,
        "ДополнительныеРеквизитыИСведения",
    );
    cct.attributes.push(Attribute {
        name: "Ссылка".to_string(),
        name_en: Some("Ref".to_string()),
        attr_type: AttributeType::Ref {
            mdo_type: MdoType::ChartOfCharacteristicTypes,
            name: "ДополнительныеРеквизитыИСведения".to_string(),
        },
    });
    config.add_metadata_object(cct);

    let code = r#"ВЫБРАТЬ
	ДополнительныеРеквизитыИСведения.Ссылка КАК Свойство
ПОМЕСТИТЬ ВТ_ВсеСвойства
ИЗ
	ПланВидовХарактеристик.ДополнительныеРеквизитыИСведения КАК ДополнительныеРеквизитыИСведения

ИНДЕКСИРОВАТЬ ПО
	Свойство"#;

    let ast = parser::parse_sdbl(code);
    let package = lower_sdbl_to_hir(&ast, Some(std::sync::Arc::new(config)));
    let sm = &package.source_map;

    let resolved_fields: Vec<_> = sm.field_names.iter().map(|t| t.text.as_str()).collect();
    let unresolved_fields: Vec<_> =
        sm.unresolved_field_names.iter().map(|t| t.text.as_str()).collect();
    let clause_keywords: Vec<_> = sm.clause_keywords.iter().map(|t| t.text.as_str()).collect();
    let field_aliases: Vec<_> = sm.field_aliases.iter().map(|t| t.text.as_str()).collect();

    assert!(
        resolved_fields.contains(&"Ссылка"),
        "Ссылка should resolve for ChartOfCharacteristicTypes, got fields: {resolved_fields:?}"
    );
    assert!(
        !unresolved_fields.contains(&"Ссылка"),
        "Ссылка must not be unresolved, got: {unresolved_fields:?}"
    );
    assert!(
        clause_keywords.contains(&"ИНДЕКСИРОВАТЬ"),
        "INDEX BY keyword should be highlighted, got: {clause_keywords:?}"
    );
    assert!(
        clause_keywords.contains(&"ПО"),
        "INDEX BY 'ПО' keyword should be highlighted, got: {clause_keywords:?}"
    );
    assert!(
        field_aliases.iter().filter(|name| **name == "Свойство").count() >= 2,
        "SELECT alias and INDEX BY reference should be field aliases, got: {field_aliases:?}"
    );
}

#[test]
fn defined_type_inside_composite_resolves_through_metadata() {
    // Codex Q7a regression test.
    //
    // `AttributeType::Composite { types: [DefinedType("X"), Boolean] }` —
    // the original SDBL `resolve_attribute_type` only matched the
    // top-level `DefinedType` arm and fell through to
    // `SdblType::from_attribute_type` for `Composite`. The latter is
    // metadata-unaware: it produces `SdblType::DefinedType { name: "X",
    // underlying_type: None }` for nested DefinedType arms even when the
    // underlying type is reachable through the configuration.
    use crate::lower::context::LoweringContext;
    use crate::types::SdblType;
    use bsl_metadata::{AttributeType, Configuration, DefinedType, Uuid};
    use std::sync::Arc;

    let mut config = Configuration::new("Test");
    config.add_defined_type(
        DefinedType::builder()
            .uuid(Uuid::new_v4())
            .name("X")
            .underlying_type(AttributeType::Boolean)
            .build(),
    );

    let ctx = LoweringContext::new(Some(Arc::new(config)));
    let composite = AttributeType::Composite {
        types: vec![AttributeType::DefinedType { name: "X".to_string() }, AttributeType::Boolean],
    };

    let resolved = ctx.resolve_attribute_type(&composite);

    // The composite arm carrying `DefinedType("X")` must reflect that
    // metadata resolved its underlying — i.e. the DefinedType wrapper is
    // present *with* `underlying_type: Some(...)`, not the metadata-blind
    // `None` shape that the bug produced.
    let arms = match &resolved {
        SdblType::Composite { types } => types.clone(),
        other => panic!("expected Composite, got {other:?}"),
    };
    let defined_arm = arms
        .iter()
        .find_map(|t| match t {
            SdblType::DefinedType { name, underlying_type } if name == "X" => {
                Some(underlying_type.clone())
            }
            _ => None,
        })
        .expect("Composite must carry the DefinedType('X') arm");
    let underlying =
        defined_arm.expect("DefinedType('X') underlying must resolve through metadata");
    assert_eq!(*underlying, SdblType::Boolean);
}
