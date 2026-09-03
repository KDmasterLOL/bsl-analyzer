use base_db::{RootQueryDb, SourceDatabase};
use ide_db::RootDatabaseImpl;
use stdx::case::CaseExt;
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

    root.descendants()
        .find(|n| matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF))
        .expect("No method found in test code")
}

#[test]
fn test_lower_empty_procedure() {
    let method = parse_method("Процедура Тест() КонецПроцедуры");
    let result = lower_method(&method, false);

    assert_eq!(result.body.params.len(), 0);
    assert!(!result.diagnostics.iter().any(|d| matches!(d, BodyDiagnostic::EmptyCodeBlock { .. })));
}

#[test]
fn test_lower_function_without_return() {
    let method = parse_method("Функция Тест() КонецФункции");
    let result = lower_method(&method, true);

    assert!(result
        .diagnostics
        .iter()
        .any(|d| matches!(d, BodyDiagnostic::FunctionShouldHaveReturn { .. })));
}

#[test]
fn test_if_branch_with_extension_directives_is_not_empty() {
    let method = parse_method(
        "Процедура Тест()
            Если Условие Тогда
                #Удаление
                А = 1;
                #КонецУдаления
                #Вставка
                Б = 2;
                #КонецВставки
            КонецЕсли;
        КонецПроцедуры",
    );
    let result = lower_method(&method, false);

    assert!(
        !result.diagnostics.iter().any(|d| matches!(d, BodyDiagnostic::EmptyCodeBlock { .. })),
        "an Если branch holding #Вставка/#Удаление directives is not an empty code block"
    );
}

#[test]
fn test_genuinely_empty_if_branch_is_still_flagged() {
    let method = parse_method(
        "Процедура Тест()
            Если Условие Тогда
            КонецЕсли;
        КонецПроцедуры",
    );
    let result = lower_method(&method, false);

    assert!(
        result.diagnostics.iter().any(|d| matches!(d, BodyDiagnostic::EmptyCodeBlock { .. })),
        "a branch with no statements and no extension directives is still an empty code block"
    );
}

#[test]
fn test_lower_function_with_return() {
    let method = parse_method(
        "Функция Тест()
            Возврат 42;
        КонецФункции",
    );
    let result = lower_method(&method, true);

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

    let param1 = result.body.binding(BindingId::from_idx(result.body.params[0]));
    assert_eq!(param1.name.as_str(), "А");
    assert!(!param1.is_val);
    assert!(param1.default_value.is_none(), "param А should not have default value");

    let param2 = result.body.binding(BindingId::from_idx(result.body.params[1]));
    assert_eq!(param2.name.as_str(), "Б");
    assert!(param2.is_val);
    assert!(param2.default_value.is_none(), "param Б should not have default value");

    let param3 = result.body.binding(BindingId::from_idx(result.body.params[2]));
    assert_eq!(param3.name.as_str(), "В");
    assert!(!param3.is_val);
    assert!(param3.default_value.is_some(), "param В should have default value");

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
fn statements_inside_region_are_lowered() {
    // Flat region markers must not swallow the statements between them.
    let method = parse_method(
        "Процедура Тест()
            #Область Р
            А = 1;
            Б = 2;
            #КонецОбласти
        КонецПроцедуры",
    );
    let result = lower_method(&method, false);

    assert_eq!(result.body.body_stmts.len(), 2, "both assignments must reach the HIR body");
    for &id in result.body.body_stmts.iter() {
        assert!(matches!(result.body.stmt_idx(id), Stmt::Assign { .. }));
    }
}

#[test]
fn region_crossing_if_keeps_if_in_hir() {
    let method = parse_method(
        "Процедура Тест()
            #Область Р
            Если Истина Тогда
                А = 1;
            #КонецОбласти
            КонецЕсли;
        КонецПроцедуры",
    );
    let result = lower_method(&method, false);

    assert_eq!(result.body.body_stmts.len(), 1);
    assert!(matches!(result.body.stmt_idx(result.body.body_stmts[0]), Stmt::If { .. }));
}

#[test]
fn function_return_inside_region_is_seen() {
    // A trailing region marker after Возврат must not hide the return.
    let method = parse_method(
        "Функция Тест()
            #Область Р
            Возврат 1;
            #КонецОбласти
        КонецФункции",
    );
    let result = lower_method(&method, true);

    assert!(!result
        .diagnostics
        .iter()
        .any(|d| matches!(d, BodyDiagnostic::FunctionShouldHaveReturn { .. })));
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

    assert_eq!(result.body.sdbl_exprs.len(), 1);

    let (expr_id, _literal, query_info) = &result.body.sdbl_exprs[0];
    assert!(query_info.is_valid());
    assert!(query_info.query_text.contains("SELECT"));

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

    let (_expr_id, _literal, query_info) = &result.body.sdbl_exprs[0];
    assert!(query_info.is_valid());
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

    assert_eq!(result.body.sdbl_exprs.len(), 2);

    assert!(result.body.sdbl_exprs[0].2.query_text.contains("SELECT"));
    assert!(result.body.sdbl_exprs[1].2.query_text.contains("ВЫБРАТЬ"));
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

    if let BodyDiagnostic::IfElseDuplicatedCodeBlock { range } = diags[0] {
        let base = cfg_types::MethodOffset::new(method.text_range().start());
        let text = &code[range.lift(base)];
        assert!(text.contains("А = 1"), "Range should cover the duplicated statement");
    }
}

#[test]
fn test_preprocessor_split_expressions() {
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
}

#[test]
fn recovery_lowers_bare_field_access_as_stmt_expr() {
    let method = parse_method(
        "Процедура Тест()
            Сп = Новый Массив;
            Сп.В
        КонецПроцедуры",
    );
    let result = super::lower_method(&method, false);

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
                field.as_str().fold_lower(),
                "в",
                "field name should round-trip through recovery",
            );
            *base
        }
        other => panic!("recovered expr should be Expr::Field, got {:?}", other),
    };

    let base_expr = result.body.expr_idx(base_id);
    match base_expr {
        Expr::Path(name) => assert_eq!(name.as_str().fold_lower(), "сп"),
        other => panic!("base should be Expr::Path, got {:?}", other),
    }

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

fn find_preproc_if(body: &crate::body::Body) -> &crate::hir::PreprocIfStmt {
    body.body_stmts
        .iter()
        .find_map(|stmt_id| match body.stmt_idx(*stmt_id) {
            Stmt::PreprocIf(preproc) => Some(preproc.as_ref()),
            _ => None,
        })
        .expect("body must contain a #Если statement")
}

fn assert_single_recovered_field_stmt(
    body: &crate::body::Body,
    branch: &[crate::hir::StmtIdx],
    expected_field: &str,
) {
    assert_eq!(branch.len(), 1, "branch must hold exactly the recovered stmt");
    let expr_id = match body.stmt_idx(branch[0]) {
        Stmt::Expr(id) => *id,
        other => panic!("branch stmt should be Stmt::Expr (recovered), got {:?}", other),
    };
    match body.expr_idx(expr_id) {
        Expr::Field { base, field } => {
            assert_eq!(field.as_str().fold_lower(), expected_field);
            match body.expr_idx(*base) {
                Expr::Path(name) => assert_eq!(name.as_str().fold_lower(), "сп"),
                other => panic!("base should be Expr::Path, got {:?}", other),
            }
        }
        other => panic!("recovered expr should be Expr::Field, got {:?}", other),
    }
    use crate::ExprId;
    assert!(
        body.is_recovered(ExprId::from_idx(expr_id)),
        "field-access expr inside the branch must be recovered",
    );
}

#[test]
fn recovery_lifts_bare_field_access_inside_preproc_then_branch() {
    let method = parse_method(
        "Процедура Тест()
            Сп = Новый Массив;
            #Если Сервер Тогда
            Сп.В
            #КонецЕсли
        КонецПроцедуры",
    );
    let result = super::lower_method(&method, false);

    let preproc = find_preproc_if(&result.body);
    assert_single_recovered_field_stmt(&result.body, &preproc.then_branch, "в");
}

#[test]
fn recovery_lifts_bare_field_access_inside_preproc_elsif_and_else_branches() {
    let method = parse_method(
        "Процедура Тест()
            Сп = Новый Массив;
            #Если Сервер Тогда
                А = 1;
            #ИначеЕсли Клиент Тогда
                Сп.В
            #Иначе
                Сп.Д
            #КонецЕсли
        КонецПроцедуры",
    );
    let result = super::lower_method(&method, false);

    let preproc = find_preproc_if(&result.body);
    assert_eq!(preproc.elsif_branches.len(), 1);
    assert_single_recovered_field_stmt(&result.body, &preproc.elsif_branches[0].2, "в");
    let else_branch = preproc.else_branch.as_ref().expect("fixture has #Иначе");
    assert_single_recovered_field_stmt(&result.body, else_branch, "д");
}

#[test]
fn recovery_does_not_lift_header_garbage_when_then_is_missing() {
    // Без `Тогда` восстановление заголовка съедает начало следующей строки:
    // ERROR с идентификатором стоит в позиции заголовка, не тела ветки, и
    // подниматься как statement не должен.
    let method = parse_method(
        "Процедура Тест()
            #Если Сервер
            А = 1;
            #КонецЕсли
        КонецПроцедуры",
    );
    let result = super::lower_method(&method, false);

    let preproc = find_preproc_if(&result.body);
    let lifted_header_ident =
        preproc.then_branch.iter().any(|stmt_id| match result.body.stmt_idx(*stmt_id) {
            Stmt::Expr(expr_id) => matches!(
                result.body.expr_idx(*expr_id),
                Expr::Path(name) if name.as_str().fold_lower() == "а"
            ),
            _ => false,
        });
    assert!(!lifted_header_ident, "header-position ERROR must not be lifted into the branch body",);
}

#[test]
fn recovery_ignores_malformed_condition_inside_pre_expr() {
    let method = parse_method(
        "Процедура Тест()
            #Если Сервер И Тогда
            А = 1;
            #КонецЕсли
        КонецПроцедуры",
    );
    let result = super::lower_method(&method, false);

    let preproc = find_preproc_if(&result.body);
    assert_eq!(
        preproc.then_branch.len(),
        1,
        "branch must hold only the assignment, nothing lifted from the header",
    );
    assert!(
        matches!(result.body.stmt_idx(preproc.then_branch[0]), Stmt::Assign { .. }),
        "the only branch stmt must be the assignment",
    );
}

#[test]
fn recovery_does_not_kick_in_for_well_formed_call_stmt() {
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
