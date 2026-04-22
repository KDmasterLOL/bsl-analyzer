//! Tests for body lowering.

use base_db::{RootQueryDb, SourceDatabase};
use ide_db::RootDatabaseImpl;
use syntax::{SyntaxKind, SyntaxNode};
use vfs::FileId;

use crate::body::BodyDiagnostic;
use crate::hir::{Expr, Literal, Stmt};
use crate::{BindingId, IdConversion};

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
    // (per test fixture comment: "Ошибка быть не должно, есть другая проверка")
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
    let param1 = result.body.binding(BindingId::from_idx(result.body.params[0]));
    assert_eq!(param1.name.as_str(), "А");
    assert!(!param1.is_val);
    assert!(param1.default_value.is_none(), "param А should not have default value");

    // Check second param (Знач)
    let param2 = result.body.binding(BindingId::from_idx(result.body.params[1]));
    assert_eq!(param2.name.as_str(), "Б");
    assert!(param2.is_val);
    assert!(param2.default_value.is_none(), "param Б should not have default value");

    // Check third param (with default value)
    let param3 = result.body.binding(BindingId::from_idx(result.body.params[2]));
    assert_eq!(param3.name.as_str(), "В");
    assert!(!param3.is_val);
    assert!(param3.default_value.is_some(), "param В should have default value");

    // Check that the default value is a number literal 1
    let default_expr_id = param3.default_value.unwrap();
    let default_expr = result.body.expr_idx(default_expr_id);
    assert!(
        matches!(default_expr, Expr::Literal(Literal::Number(_))),
        "default value should be a number literal"
    );
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
    let stmt = result.body.stmt_idx(result.body.body_stmts[0]);
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
    let stmt = result.body.stmt_idx(result.body.body_stmts[0]);
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
    match result.body.expr_idx(*expr_id) {
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

    let result = super::super::lower_module_code(&root, None);

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

// --- ERROR-node recovery lowering -------------------------------------------

#[test]
fn recovery_lowers_bare_field_access_as_stmt_expr() {
    // `Сп.В` without `()` is not a valid BSL statement. Parser wraps the
    // FIELD_EXPR in NodeKind::Error, but we still want an Expr::Field so
    // completion/hover see the receiver type.
    let method = parse_method(
        "Процедура Тест()
            Сп = Новый Массив;
            Сп.В
        КонецПроцедуры",
    );
    let result = super::lower_method(&method, false);

    // Last statement must be a Stmt::Expr for the recovered field access.
    assert!(
        result.body.body_stmts.len() >= 2,
        "expected at least assign + recovered stmt, got {}: {:?}",
        result.body.body_stmts.len(),
        result.body.body_stmts,
    );
    let last_stmt_id = *result.body.body_stmts.last().unwrap();
    let last_stmt = result.body.stmt_idx(last_stmt_id);
    let recovered_expr_id = match last_stmt {
        Stmt::Expr(id) => *id,
        other => panic!("last stmt should be Stmt::Expr (recovered), got {:?}", other),
    };

    let recovered_expr = result.body.expr_idx(recovered_expr_id);
    let base_id = match recovered_expr {
        Expr::Field { base, field } => {
            assert_eq!(
                field.as_str().to_lowercase(),
                "в",
                "field name should round-trip through recovery",
            );
            *base
        }
        other => panic!("recovered expr should be Expr::Field, got {:?}", other),
    };

    let base_expr = result.body.expr_idx(base_id);
    match base_expr {
        Expr::Path(name) => assert_eq!(name.as_str().to_lowercase(), "сп"),
        other => panic!("base should be Expr::Path, got {:?}", other),
    }

    // Both expressions are flagged as recovered so downstream consumers
    // (hir-ty diagnostics, CFG) can opt out.
    use crate::ExprId;
    assert!(
        result.body.is_recovered(ExprId::from_idx(recovered_expr_id)),
        "field-access expr must be recovered",
    );
    assert!(
        result.body.is_recovered(ExprId::from_idx(base_id)),
        "receiver expr must be recovered (mark propagates recursively)",
    );
}

#[test]
fn recovery_does_not_kick_in_for_well_formed_call_stmt() {
    // Sanity: a normal `Сп.Добавить(1)` is a valid CALL_STMT, so lowering
    // must not mark it as recovered.
    let method = parse_method(
        "Процедура Тест()
            Сп = Новый Массив;
            Сп.Добавить(1);
        КонецПроцедуры",
    );
    let result = super::lower_method(&method, false);

    let any_recovered = result.body.exprs_iter().any(|(id, _)| result.body.is_recovered(id));
    assert!(!any_recovered, "well-formed call must not be flagged as recovered");
}
