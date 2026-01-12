//! Cognitive complexity calculation based on HIR.
//!
//! This module provides HIR-based cognitive complexity calculation
//! following the SonarSource Cognitive Complexity specification v1.4.
//!
//! ## Algorithm
//!
//! **Structural increment** (if, for, while, foreach, except, ternary):
//! - Add: 1 + current_nesting_level
//! - Then increase nesting for children
//!
//! **Hybrid increment** (elsif, else):
//! - Add: 1 (no nesting penalty on the keyword itself)
//! - But increase nesting for children
//!
//! **Fundamental increment** (goto, AND/OR operators):
//! - Add: 1 per construct (no nesting, no nesting increase)
//!
//! ## Usage
//!
//! ```ignore
//! use hir_def::cognitive_complexity::calculate_complexity;
//!
//! let complexity = calculate_complexity(&body);
//! ```
//!
//! This can be used for:
//! - Diagnostics (when complexity exceeds threshold)
//! - Code lens (showing complexity in editor)
//! - Metrics collection

use crate::hir::{BinaryOp, Expr, ExprId, Stmt, StmtId};
use crate::Body;

/// Calculate cognitive complexity for a method body.
///
/// Returns the total cognitive complexity score based on HIR representation.
/// This is a pure function that can be cached by Salsa when called through a query.
pub fn calculate_complexity(body: &Body) -> u32 {
    let mut complexity = 0;

    // Process top-level statements
    for &stmt_id in body.body_stmts.iter() {
        count_stmt_complexity(body, stmt_id, &mut complexity, 0);
    }

    complexity
}

/// Recursively count complexity for a statement.
fn count_stmt_complexity(body: &Body, stmt_id: StmtId, complexity: &mut u32, nesting: u32) {
    let stmt = body.stmt(stmt_id);

    match stmt {
        // Structural increment: +1 + nesting, then increase nesting for children
        Stmt::If(if_stmt) => {
            // If itself: +1 + nesting
            *complexity += 1 + nesting;

            // Count complexity in condition (for AND/OR)
            count_expr_complexity(body, if_stmt.condition, complexity);

            // Then branch: IF increases nesting by 1, THEN_CLAUSE doesn't add more
            for &child_stmt in if_stmt.then_branch.iter() {
                count_stmt_complexity(body, child_stmt, complexity, nesting + 1);
            }

            // Elsif branches: +1 each (hybrid), then nested body
            // ELSIF adds +1 to nesting on top of IF's +1, so children get nesting+2
            for (elsif_condition, elsif_body) in if_stmt.elsif_branches.iter() {
                *complexity += 1; // Hybrid increment for elsif

                // Count complexity in elsif condition
                count_expr_complexity(body, *elsif_condition, complexity);

                for &child_stmt in elsif_body.iter() {
                    count_stmt_complexity(body, child_stmt, complexity, nesting + 2);
                }
            }

            // Else branch: +1 (hybrid), then nested body
            // ELSE adds +1 to nesting on top of IF's +1, so children get nesting+2
            if let Some(ref else_body) = if_stmt.else_branch {
                *complexity += 1; // Hybrid increment for else

                for &child_stmt in else_body.iter() {
                    count_stmt_complexity(body, child_stmt, complexity, nesting + 2);
                }
            }
        }

        Stmt::While { condition, body: loop_body } => {
            // Structural increment: +1 + nesting
            *complexity += 1 + nesting;

            // Count complexity in condition
            count_expr_complexity(body, *condition, complexity);

            // Body is nested
            for &child_stmt in loop_body.iter() {
                count_stmt_complexity(body, child_stmt, complexity, nesting + 1);
            }
        }

        Stmt::For { body: loop_body, .. } => {
            // Structural increment: +1 + nesting
            *complexity += 1 + nesting;

            // Body is nested
            for &child_stmt in loop_body.iter() {
                count_stmt_complexity(body, child_stmt, complexity, nesting + 1);
            }
        }

        Stmt::ForEach { body: loop_body, .. } => {
            // Structural increment: +1 + nesting
            *complexity += 1 + nesting;

            // Body is nested
            for &child_stmt in loop_body.iter() {
                count_stmt_complexity(body, child_stmt, complexity, nesting + 1);
            }
        }

        Stmt::Try { body: try_body, except } => {
            // Try block doesn't add complexity by itself
            for &child_stmt in try_body.iter() {
                count_stmt_complexity(body, child_stmt, complexity, nesting);
            }

            // Except clause: structural increment +1 + nesting
            *complexity += 1 + nesting;

            for &child_stmt in except.iter() {
                count_stmt_complexity(body, child_stmt, complexity, nesting + 1);
            }
        }

        // Fundamental increment: +1 only
        Stmt::Goto(_) => {
            *complexity += 1;
        }

        // Expression statement - check for ternary and logical operators
        Stmt::Expr(expr_id) => {
            count_expr_complexity(body, *expr_id, complexity);
        }

        // Assignment - check expression for complexity
        Stmt::Assign { value, .. } => {
            count_expr_complexity(body, *value, complexity);
        }

        // Return - check expression if present
        Stmt::Return { value } => {
            if let Some(expr_id) = value {
                count_expr_complexity(body, *expr_id, complexity);
            }
        }

        // Raise - check expression if present
        Stmt::Raise { value } => {
            if let Some(expr_id) = value {
                count_expr_complexity(body, *expr_id, complexity);
            }
        }

        // Execute - check expression
        Stmt::Execute { expr } => {
            count_expr_complexity(body, *expr, complexity);
        }

        // Handler statements - check expressions
        Stmt::AddHandler { event, handler } | Stmt::RemoveHandler { event, handler } => {
            count_expr_complexity(body, *event, complexity);
            count_expr_complexity(body, *handler, complexity);
        }

        // No complexity for these
        Stmt::VarDecl { .. } | Stmt::Break | Stmt::Continue | Stmt::Label(_) => {}
    }
}

/// Count complexity in an expression (for ternary and logical operators).
fn count_expr_complexity(body: &Body, expr_id: ExprId, complexity: &mut u32) {
    let expr = body.expr(expr_id);

    match expr {
        // Ternary: structural increment (we don't track nesting inside expressions)
        // This matches the AST-based implementation behavior
        Expr::Ternary { condition, then_expr, else_expr } => {
            // Note: In the original AST implementation, ternary gets +1 + nesting
            // But for expressions we typically don't have statement-level nesting context
            // So we add +1 (matching the behavior when nesting=0 in expression context)
            *complexity += 1;

            // Recursively check sub-expressions
            count_expr_complexity(body, *condition, complexity);
            count_expr_complexity(body, *then_expr, complexity);
            count_expr_complexity(body, *else_expr, complexity);
        }

        // Logical AND/OR: fundamental increment +1
        Expr::BinaryOp { lhs, rhs, op } => {
            if matches!(op, BinaryOp::And | BinaryOp::Or) {
                *complexity += 1;
            }

            // Recursively check sub-expressions
            count_expr_complexity(body, *lhs, complexity);
            count_expr_complexity(body, *rhs, complexity);
        }

        // Unary - check sub-expression
        Expr::UnaryOp { expr, .. } => {
            count_expr_complexity(body, *expr, complexity);
        }

        // Call - check callee and arguments
        Expr::Call { callee, args } => {
            count_expr_complexity(body, *callee, complexity);
            for &arg in args.iter() {
                count_expr_complexity(body, arg, complexity);
            }
        }

        // Method call - check receiver and arguments
        Expr::MethodCall { receiver, args, .. } => {
            count_expr_complexity(body, *receiver, complexity);
            for &arg in args.iter() {
                count_expr_complexity(body, arg, complexity);
            }
        }

        // Index - check base and index
        Expr::Index { base, index } => {
            count_expr_complexity(body, *base, complexity);
            count_expr_complexity(body, *index, complexity);
        }

        // Field - check base
        Expr::Field { base, .. } => {
            count_expr_complexity(body, *base, complexity);
        }

        // New - check arguments
        Expr::New { args, .. } => {
            for &arg in args.iter() {
                count_expr_complexity(body, arg, complexity);
            }
        }

        // Array - check elements
        Expr::Array(elements) => {
            for &elem in elements.iter() {
                count_expr_complexity(body, elem, complexity);
            }
        }

        // Await - check expression
        Expr::Await { expr } => {
            count_expr_complexity(body, *expr, complexity);
        }

        // No complexity for these
        Expr::Missing | Expr::Literal(_) | Expr::Path(_) | Expr::QualifiedPath(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::lower_method;
    use syntax::SyntaxKind;

    fn parse_and_lower(code: &str) -> Body {
        let parse = parser::parse(code);
        let root = parse.syntax_node();

        // Find the first method
        let method_node = root
            .descendants()
            .find(|n| matches!(n.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF))
            .expect("Should have a method");

        let is_function = method_node.kind() == SyntaxKind::FUNCTION_DEF;
        let result = lower_method(&method_node, is_function);
        result.body
    }

    #[test]
    fn test_simple_function() {
        let code = r#"Функция ПростаяФункция(Параметр)
    Возврат Параметр + 1;
КонецФункции"#;

        let body = parse_and_lower(code);
        let complexity = calculate_complexity(&body);

        assert_eq!(complexity, 0, "Simple function should have complexity 0");
    }

    #[test]
    fn test_single_if() {
        let code = r#"Функция Тест(А)
    Если А > 0 Тогда
        Возврат 1;
    КонецЕсли;
    Возврат 0;
КонецФункции"#;

        let body = parse_and_lower(code);
        let complexity = calculate_complexity(&body);

        assert_eq!(complexity, 1, "Single if should have complexity 1");
    }

    #[test]
    fn test_nested_if() {
        let code = r#"Функция ВложенныеУсловия(А, Б)
    Если А > 0 Тогда
        Если Б > 0 Тогда
            Возврат А + Б;
        КонецЕсли;
    КонецЕсли;
    Возврат 0;
КонецФункции"#;

        let body = parse_and_lower(code);
        let complexity = calculate_complexity(&body);

        // Outer if: +1 (nesting=0)
        // Inner if: +1+1 = +2 (nesting=1)
        // Total: 3
        assert_eq!(complexity, 3, "Nested if should have complexity 3");
    }

    #[test]
    fn test_if_elsif_else() {
        let code = r#"Функция СМножественнымиУсловиями(Х)
    Если Х = 1 Тогда
        Возврат "один";
    ИначеЕсли Х = 2 Тогда
        Возврат "два";
    ИначеЕсли Х = 3 Тогда
        Возврат "три";
    Иначе
        Возврат "другое";
    КонецЕсли;
КонецФункции"#;

        let body = parse_and_lower(code);
        let complexity = calculate_complexity(&body);

        // If: +1
        // ElsIf: +1 (hybrid)
        // ElsIf: +1 (hybrid)
        // Else: +1 (hybrid)
        // Total: 4
        assert_eq!(complexity, 4, "If with elsif/else should have complexity 4");
    }

    #[test]
    fn test_while_loop() {
        let code = r#"Процедура Тест()
    Пока Истина Цикл
        // тело
    КонецЦикла;
КонецПроцедуры"#;

        let body = parse_and_lower(code);
        let complexity = calculate_complexity(&body);

        assert_eq!(complexity, 1, "While loop should have complexity 1");
    }

    #[test]
    fn test_for_loop() {
        let code = r#"Процедура Тест()
    Для Сч = 1 По 10 Цикл
        // тело
    КонецЦикла;
КонецПроцедуры"#;

        let body = parse_and_lower(code);
        let complexity = calculate_complexity(&body);

        assert_eq!(complexity, 1, "For loop should have complexity 1");
    }

    #[test]
    fn test_foreach_loop() {
        let code = r#"Процедура Тест(Массив)
    Для Каждого Элемент Из Массив Цикл
        // тело
    КонецЦикла;
КонецПроцедуры"#;

        let body = parse_and_lower(code);
        let complexity = calculate_complexity(&body);

        assert_eq!(complexity, 1, "ForEach loop should have complexity 1");
    }

    #[test]
    fn test_try_except() {
        let code = r#"Процедура Тест()
    Попытка
        // тело
    Исключение
        // обработка
    КонецПопытки;
КонецПроцедуры"#;

        let body = parse_and_lower(code);
        let complexity = calculate_complexity(&body);

        // Except clause: +1 (structural, nesting=0)
        assert_eq!(complexity, 1, "Try-except should have complexity 1");
    }

    #[test]
    fn test_goto() {
        let code = r#"Процедура Тест()
    Перейти ~Метка;
    ~Метка:
КонецПроцедуры"#;

        let body = parse_and_lower(code);
        let complexity = calculate_complexity(&body);

        // Goto: +1 (fundamental)
        assert_eq!(complexity, 1, "Goto should have complexity 1");
    }

    #[test]
    fn test_logical_and_or() {
        let code = r#"Функция Тест(А, Б)
    Если А И Б Тогда
        Возврат 1;
    КонецЕсли;
    Возврат 0;
КонецФункции"#;

        let body = parse_and_lower(code);
        let complexity = calculate_complexity(&body);

        // If: +1
        // AND: +1 (fundamental)
        // Total: 2
        assert_eq!(complexity, 2, "If with AND should have complexity 2");
    }

    #[test]
    fn test_multiple_logical_operators() {
        let code = r#"Функция Тест(А, Б, В)
    Если А И Б ИЛИ В Тогда
        Возврат 1;
    КонецЕсли;
    Возврат 0;
КонецФункции"#;

        let body = parse_and_lower(code);
        let complexity = calculate_complexity(&body);

        // If: +1
        // AND: +1
        // OR: +1
        // Total: 3
        assert_eq!(complexity, 3, "If with AND and OR should have complexity 3");
    }

    #[test]
    fn test_deeply_nested() {
        let code = r#"Функция ГлубокаяВложенность(П1, П2, П3)
    Если П1 > 0 Тогда
        Если П2 > 0 Тогда
            Для Каждого Э Из П3 Цикл
                Если Э > 5 Тогда
                    Возврат 1;
                КонецЕсли;
            КонецЦикла;
        КонецЕсли;
    КонецЕсли;
    Возврат 0;
КонецФункции"#;

        let body = parse_and_lower(code);
        let complexity = calculate_complexity(&body);

        // If (nesting=0): +1
        // If (nesting=1): +2
        // ForEach (nesting=2): +3
        // If (nesting=3): +4
        // Total: 10
        assert_eq!(complexity, 10, "Deeply nested should have complexity 10");
    }
}
