//! Tests for body lowering.

use base_db::{RootQueryDb, SourceDatabase};
use ide_db::RootDatabaseImpl;
use syntax::{SyntaxKind, SyntaxNode};
use vfs::FileId;

use crate::body::BodyDiagnostic;
use crate::hir::{Expr, Literal, Stmt};

use super::lower_method;

fn parse_method(code: &str) -> SyntaxNode {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId::from_raw(0);
    db.set_file_text(file_id, code);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    // Find first method
    root.descendants()
        .find(|n| matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF))
        .expect("No method found in test code")
}

#[test]
fn test_lower_empty_procedure() {
    let method = parse_method("Процедура Тест() КонецПроцедуры");
    let result = lower_method(&method, false);

    assert_eq!(result.body.params.len(), 0);
    // Empty procedure body should NOT emit EmptyCodeBlock diagnostic
    // (per Java fixture comment: "Ошибка быть не должно, есть другая проверка")
    // Empty functions/procedures are handled by FunctionShouldHaveReturn or similar
    assert!(!result.diagnostics.iter().any(|d| matches!(d, BodyDiagnostic::EmptyCodeBlock { .. })));
}

#[test]
fn test_lower_function_without_return() {
    let method = parse_method("Функция Тест() КонецФункции");
    let result = lower_method(&method, true);

    // Function without return should emit FunctionShouldHaveReturn diagnostic
    assert!(result
        .diagnostics
        .iter()
        .any(|d| matches!(d, BodyDiagnostic::FunctionShouldHaveReturn { .. })));
}

#[test]
fn test_lower_function_with_return() {
    let method = parse_method(
        "Функция Тест()
            Возврат 42;
        КонецФункции",
    );
    let result = lower_method(&method, true);

    // Function with return should NOT emit FunctionShouldHaveReturn
    assert!(!result
        .diagnostics
        .iter()
        .any(|d| matches!(d, BodyDiagnostic::FunctionShouldHaveReturn { .. })));
}

#[test]
fn test_lower_procedure_with_params() {
    let method = parse_method("Процедура Тест(А, Знач Б, В = 1) КонецПроцедуры");
    let result = lower_method(&method, false);

    assert_eq!(result.body.params.len(), 3);

    // Check first param
    let param1 = result.body.binding(result.body.params[0]);
    assert_eq!(param1.name.as_str(), "А");
    assert!(!param1.is_val);

    // Check second param (Знач)
    let param2 = result.body.binding(result.body.params[1]);
    assert_eq!(param2.name.as_str(), "Б");
    assert!(param2.is_val);

    // Check third param
    let param3 = result.body.binding(result.body.params[2]);
    assert_eq!(param3.name.as_str(), "В");
    assert!(!param3.is_val);
}

#[test]
fn test_lower_assignment() {
    let method = parse_method(
        "Процедура Тест()
            А = 42;
        КонецПроцедуры",
    );
    let result = lower_method(&method, false);

    assert_eq!(result.body.body_stmts.len(), 1);
    let stmt = result.body.stmt(result.body.body_stmts[0]);
    assert!(matches!(stmt, Stmt::Assign { .. }));
}

#[test]
fn test_lower_self_assign() {
    let method = parse_method(
        "Процедура Тест()
            А = А;
        КонецПроцедуры",
    );
    let result = lower_method(&method, false);

    // Self-assignment should emit diagnostic
    assert!(result.diagnostics.iter().any(|d| matches!(d, BodyDiagnostic::SelfAssign { .. })));
}

#[test]
fn test_lower_if_stmt() {
    let method = parse_method(
        "Процедура Тест()
            Если Истина Тогда
                А = 1;
            КонецЕсли;
        КонецПроцедуры",
    );
    let result = lower_method(&method, false);

    assert_eq!(result.body.body_stmts.len(), 1);
    let stmt = result.body.stmt(result.body.body_stmts[0]);
    assert!(matches!(stmt, Stmt::If { .. }));
}

#[test]
fn test_sdbl_collected_in_hir() {
    let method = parse_method(
        r#"
Процедура Тест()
    Запрос = "SELECT Ссылка FROM Справочник.Валюты";
    Результат = Запрос.Выполнить();
КонецПроцедуры
"#,
    );
    let result = lower_method(&method, false);

    // Should have collected 1 SDBL query
    assert_eq!(result.body.sdbl_exprs.len(), 1);

    let (expr_id, query_info) = &result.body.sdbl_exprs[0];
    assert!(query_info.is_valid());
    assert!(query_info.query_text.contains("SELECT"));

    // Verify ExprId points to a string literal
    match result.body.expr(*expr_id) {
        Expr::Literal(Literal::String(_)) => {}
        _ => panic!("Expected string literal"),
    }
}

#[test]
fn test_sdbl_multiline_query() {
    let method = parse_method(
        r#"
Функция ПолучитьДанные()
    Запрос = "SELECT
             |    Ссылка,
             |    Наименование
             |FROM Справочник.Валюты";
    Возврат Запрос.Выполнить();
КонецФункции
"#,
    );
    let result = lower_method(&method, true);

    assert_eq!(result.body.sdbl_exprs.len(), 1);

    let (_expr_id, query_info) = &result.body.sdbl_exprs[0];
    assert!(query_info.is_valid());
    // Multiline string should be parsed correctly
    assert!(query_info.query_text.contains("Наименование"));
}

#[test]
fn test_short_strings_ignored() {
    let method = parse_method(
        r#"
Процедура Тест()
    Х = "SELECT";
    Y = "Test";
КонецПроцедуры
"#,
    );
    let result = lower_method(&method, false);

    // Should not collect short strings (< 15 chars)
    assert_eq!(result.body.sdbl_exprs.len(), 0);
}

#[test]
fn test_multiple_queries_in_method() {
    let method = parse_method(
        r#"
Процедура МножественныеЗапросы()
    Запрос1 = "SELECT Ссылка FROM Справочник.Валюты";
    Запрос2 = "ВЫБРАТЬ Наименование ИЗ Справочник.Номенклатура";
    Результат1 = Запрос1.Выполнить();
    Результат2 = Запрос2.Выполнить();
КонецПроцедуры
"#,
    );
    let result = lower_method(&method, false);

    // Should collect both queries
    assert_eq!(result.body.sdbl_exprs.len(), 2);

    assert!(result.body.sdbl_exprs[0].1.query_text.contains("SELECT"));
    assert!(result.body.sdbl_exprs[1].1.query_text.contains("ВЫБРАТЬ"));
}

#[test]
fn test_if_else_duplicated_code_block() {
    let method = parse_method(
        r#"Процедура Тест()
    Если x = 1 Тогда
        А = 1;
        Б = 2;
    Иначе
        А = 1;
        Б = 2;
    КонецЕсли;
КонецПроцедуры"#,
    );
    let result = lower_method(&method, false);

    // Should detect duplicated code blocks
    let diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d, BodyDiagnostic::IfElseDuplicatedCodeBlock { .. }))
        .collect();
    assert_eq!(diags.len(), 1, "Should detect 1 duplicated code block");
}

#[test]
fn test_if_else_different_blocks() {
    let method = parse_method(
        r#"Процедура Тест()
    Если x = 1 Тогда
        А = 1;
    Иначе
        А = 2;
    КонецЕсли;
КонецПроцедуры"#,
    );
    let result = lower_method(&method, false);

    // Should NOT detect duplicated code blocks (different values)
    let diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d, BodyDiagnostic::IfElseDuplicatedCodeBlock { .. }))
        .collect();
    assert_eq!(diags.len(), 0, "Different blocks should not trigger diagnostic");
}

#[test]
fn test_if_elsif_duplicated_code_block() {
    let method = parse_method(
        r#"Процедура Тест()
    Если x = 1 Тогда
        А = 1;
    ИначеЕсли x = 2 Тогда
        А = 1;
    КонецЕсли;
КонецПроцедуры"#,
    );
    let result = lower_method(&method, false);

    // Should detect duplicated code blocks in if/elsif
    let diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d, BodyDiagnostic::IfElseDuplicatedCodeBlock { .. }))
        .collect();
    assert_eq!(diags.len(), 1, "Should detect duplicated if/elsif blocks");
}

#[test]
fn test_if_else_empty_blocks_not_duplicated() {
    let method = parse_method(
        r#"Процедура Тест()
    Если x = 1 Тогда
    Иначе
    КонецЕсли;
КонецПроцедуры"#,
    );
    let result = lower_method(&method, false);

    // Empty blocks should NOT be reported as duplicates
    let diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d, BodyDiagnostic::IfElseDuplicatedCodeBlock { .. }))
        .collect();
    assert_eq!(diags.len(), 0, "Empty blocks should not trigger duplicate diagnostic");
}

#[test]
fn test_if_else_duplicated_range_correct() {
    let code = r#"Процедура Тест()
    Если x = 1 Тогда
        А = 1;
    Иначе
        А = 1;
    КонецЕсли;
КонецПроцедуры"#;
    let method = parse_method(code);
    let result = lower_method(&method, false);

    let diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d, BodyDiagnostic::IfElseDuplicatedCodeBlock { .. }))
        .collect();
    assert_eq!(diags.len(), 1);

    // Diagnostic should point to the FIRST block (then-branch)
    if let BodyDiagnostic::IfElseDuplicatedCodeBlock { range } = diags[0] {
        let text = &code[range.start().into()..range.end().into()];
        // The range should cover the STMT_LIST content (А = 1;)
        assert!(text.contains("А = 1"), "Range should cover the duplicated statement");
    }
}

#[test]
fn test_preprocessor_split_expressions() {
    // Test from IdenticalExpressionsDiagnostic.bsl fixture
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId::from_raw(0);
    let code = r#"
// Split expression with region
Результат = Истина
#Область ЕщеОднаОбласть
 ИЛИ Истина;
#КонецОбласти

// Split expression with preprocessor
Результат2 = Истина
#Если ВебКлиент Тогда
 ИЛИ Ложь
#Иначе
 ИЛИ ЗначениеВыражения()
#КонецЕсли
 ИЛИ Истина;
"#;

    db.set_file_text(file_id, code);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    println!("=== PARSE ERRORS ===");
    for error in parse.errors() {
        println!("{:?}", error);
    }

    println!("\n=== SYNTAX TREE ===");
    println!("{:#?}", root);

    let result = super::super::lower_module_code(&root);

    println!("\n=== HIR LOWERING ===");
    println!("Body stmts: {}", result.body.body_stmts.len());
    for (idx, stmt_id) in result.body.body_stmts.iter().enumerate() {
        println!("  Stmt {}: {:?}", idx, result.body.stmts[*stmt_id]);
    }

    println!("\nBody exprs: {}", result.body.exprs.len());
    for (expr_id, expr) in result.body.exprs.iter() {
        println!("  {:?}: {:?}", expr_id, expr);
    }

    println!("\nDiagnostics: {}", result.diagnostics.len());
    for diag in &result.diagnostics {
        println!("  {:?}", diag);
    }

    // Key questions to answer:
    // 1. How many statements are lowered?
    // 2. Are the split expressions represented in HIR?
    // 3. Are there ERROR nodes in the parse tree?

    // The fixture shows this should work, so we expect:
    // - 2 statements (both assignments)
    // - Split expressions should be represented in HIR
    // We're just inspecting for now - assertions will come after we understand the behavior
}
