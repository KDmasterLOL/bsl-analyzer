//! BSL grammar rules.

pub mod expressions;
pub mod items;
pub mod statements;

use lexer::TokenKind;

use crate::event::NodeKind;
use crate::parser::Parser;

/// Parses a source file.
pub fn source_file(p: &mut Parser) {
    let m = p.start();

    while !p.at_end() {
        p.check_iteration_limit();
        p.skip_trivia();

        if p.at_end() {
            break;
        }

        match p.current() {
            Some(TokenKind::KwAsync) => {
                // Look ahead to determine if it's procedure or function
                match p.nth(1) {
                    Some(TokenKind::KwProcedure) => items::procedure_def(p),
                    Some(TokenKind::KwFunction) => items::function_def(p),
                    _ => p.error(),
                }
            }
            Some(TokenKind::KwProcedure) => items::procedure_def(p),
            Some(TokenKind::KwFunction) => items::function_def(p),
            Some(TokenKind::KwVar) => items::var_declaration(p),
            Some(TokenKind::PreRegion) => preprocessor_region(p),
            Some(TokenKind::PreIf) => preprocessor_if(p),
            Some(TokenKind::PreDelete) => preprocessor_delete(p),
            Some(TokenKind::PreInsert) => preprocessor_insert(p),
            // Compiler directives (&НаКлиенте и т.д.)
            Some(TokenKind::AnnAtClient)
            | Some(TokenKind::AnnAtServer)
            | Some(TokenKind::AnnAtServerNoContext)
            | Some(TokenKind::AnnAtClientAtServer)
            | Some(TokenKind::AnnAtClientAtServerNoContext) => {
                items::compiler_directive(p);
                // After compiler directive, can be more directives/annotations or procedure/function
                p.skip_trivia();
                match p.current() {
                    Some(TokenKind::KwAsync) => match p.nth(1) {
                        Some(TokenKind::KwProcedure) => items::procedure_def(p),
                        Some(TokenKind::KwFunction) => items::function_def(p),
                        _ => p.error(),
                    },
                    Some(TokenKind::KwProcedure) => items::procedure_def(p),
                    Some(TokenKind::KwFunction) => items::function_def(p),
                    _ => p.error(),
                }
            }
            // Custom annotations (&До, &После и т.д.)
            Some(TokenKind::AnnBefore)
            | Some(TokenKind::AnnAfter)
            | Some(TokenKind::AnnAround)
            | Some(TokenKind::AnnChangeAndValidate)
            | Some(TokenKind::AnnCustom) => {
                items::annotation(p);
                // After annotation, can be more annotations or procedure/function
                p.skip_trivia();
                match p.current() {
                    Some(TokenKind::KwAsync) => match p.nth(1) {
                        Some(TokenKind::KwProcedure) => items::procedure_def(p),
                        Some(TokenKind::KwFunction) => items::function_def(p),
                        _ => p.error(),
                    },
                    Some(TokenKind::KwProcedure) => items::procedure_def(p),
                    Some(TokenKind::KwFunction) => items::function_def(p),
                    _ => p.error(),
                }
            }
            _ => {
                p.error();
            }
        }
    }

    m.complete(p, NodeKind::SourceFile);
}

pub(super) fn preprocessor_region(p: &mut Parser) {
    let m = p.start();
    p.bump(); // #Область
    p.skip_trivia();

    if p.at(TokenKind::Ident) {
        p.bump();
    }

    // Parse content until #КонецОбласти
    while !p.at_end() && !p.at(TokenKind::PreEndRegion) {
        p.check_iteration_limit();
        p.skip_trivia();
        if p.at_end() || p.at(TokenKind::PreEndRegion) {
            break;
        }

        match p.current() {
            Some(TokenKind::KwAsync) => {
                // Look ahead to determine if it's procedure or function
                match p.nth(1) {
                    Some(TokenKind::KwProcedure) => items::procedure_def(p),
                    Some(TokenKind::KwFunction) => items::function_def(p),
                    _ => p.error(),
                }
            }
            Some(TokenKind::KwProcedure) => items::procedure_def(p),
            Some(TokenKind::KwFunction) => items::function_def(p),
            Some(TokenKind::KwVar) => items::var_declaration(p),
            Some(TokenKind::PreRegion) => preprocessor_region(p),
            Some(TokenKind::PreIf) => preprocessor_if(p),
            Some(TokenKind::PreDelete) => preprocessor_delete(p),
            Some(TokenKind::PreInsert) => preprocessor_insert(p),
            Some(TokenKind::AnnAtClient)
            | Some(TokenKind::AnnAtServer)
            | Some(TokenKind::AnnAtServerNoContext)
            | Some(TokenKind::AnnAtClientAtServer)
            | Some(TokenKind::AnnAtClientAtServerNoContext) => {
                items::compiler_directive(p);
                p.skip_trivia();
                match p.current() {
                    Some(TokenKind::KwAsync) | Some(TokenKind::KwProcedure) => {
                        items::procedure_def(p)
                    }
                    Some(TokenKind::KwFunction) => items::function_def(p),
                    _ => p.error(),
                }
            }
            Some(TokenKind::AnnBefore)
            | Some(TokenKind::AnnAfter)
            | Some(TokenKind::AnnAround)
            | Some(TokenKind::AnnChangeAndValidate)
            | Some(TokenKind::AnnCustom) => {
                items::annotation(p);
                p.skip_trivia();
                match p.current() {
                    Some(TokenKind::KwAsync) | Some(TokenKind::KwProcedure) => {
                        items::procedure_def(p)
                    }
                    Some(TokenKind::KwFunction) => items::function_def(p),
                    _ => p.error(),
                }
            }
            _ => p.bump(),
        }
    }

    p.eat(TokenKind::PreEndRegion);
    m.complete(p, NodeKind::PreRegionDir);
}

pub(super) fn preprocessor_if(p: &mut Parser) {
    let m = p.start();
    p.bump(); // #Если
    p.skip_trivia();

    // Parse condition expression
    preproc_expression(p);
    p.skip_trivia();

    p.expect(TokenKind::KwThen);

    // Parse content until ElsIf, Else, or EndIf
    preproc_content(p);

    // Handle ElsIf clauses
    while p.at(TokenKind::PreElsIf) {
        p.check_iteration_limit();
        let elsif_m = p.start();
        p.bump(); // #ИначеЕсли
        p.skip_trivia();

        preproc_expression(p);
        p.skip_trivia();

        p.expect(TokenKind::KwThen);

        preproc_content(p);

        elsif_m.complete(p, NodeKind::PreElsIfClause);
    }

    // Handle Else clause
    if p.at(TokenKind::PreElse) {
        let else_m = p.start();
        p.bump(); // #Иначе

        preproc_content(p);

        else_m.complete(p, NodeKind::PreElseClause);
    }

    p.expect(TokenKind::PreEndIf);
    m.complete(p, NodeKind::PreIfDir);
}

/// Parses preprocessor content (code between #Если/#ИначеЕсли/#Иначе and #КонецЕсли)
/// This recursively parses the content using the same logic as preprocessor_region
fn preproc_content(p: &mut Parser) {
    while !p.at_end()
        && !p.at(TokenKind::PreElsIf)
        && !p.at(TokenKind::PreElse)
        && !p.at(TokenKind::PreEndIf)
    {
        p.check_iteration_limit();
        p.skip_trivia();
        if p.at_end()
            || p.at(TokenKind::PreElsIf)
            || p.at(TokenKind::PreElse)
            || p.at(TokenKind::PreEndIf)
        {
            break;
        }

        // Parse content recursively like in preprocessor_region
        match p.current() {
            Some(TokenKind::KwAsync) => match p.nth(1) {
                Some(TokenKind::KwProcedure) => items::procedure_def(p),
                Some(TokenKind::KwFunction) => items::function_def(p),
                _ => p.error(),
            },
            Some(TokenKind::KwProcedure) => items::procedure_def(p),
            Some(TokenKind::KwFunction) => items::function_def(p),
            Some(TokenKind::KwVar) => items::var_declaration(p),
            Some(TokenKind::PreRegion) => preprocessor_region(p),
            Some(TokenKind::PreIf) => preprocessor_if(p),
            Some(TokenKind::PreDelete) => preprocessor_delete(p),
            Some(TokenKind::PreInsert) => preprocessor_insert(p),
            Some(TokenKind::AnnAtClient)
            | Some(TokenKind::AnnAtServer)
            | Some(TokenKind::AnnAtServerNoContext)
            | Some(TokenKind::AnnAtClientAtServer)
            | Some(TokenKind::AnnAtClientAtServerNoContext) => {
                items::compiler_directive(p);
                p.skip_trivia();
                match p.current() {
                    Some(TokenKind::KwAsync) | Some(TokenKind::KwProcedure) => {
                        items::procedure_def(p)
                    }
                    Some(TokenKind::KwFunction) => items::function_def(p),
                    _ => p.error(),
                }
            }
            Some(TokenKind::AnnBefore)
            | Some(TokenKind::AnnAfter)
            | Some(TokenKind::AnnAround)
            | Some(TokenKind::AnnChangeAndValidate)
            | Some(TokenKind::AnnCustom) => {
                items::annotation(p);
                p.skip_trivia();
                match p.current() {
                    Some(TokenKind::KwAsync) | Some(TokenKind::KwProcedure) => {
                        items::procedure_def(p)
                    }
                    Some(TokenKind::KwFunction) => items::function_def(p),
                    _ => p.error(),
                }
            }
            _ => p.bump(),
        }
    }
}

/// Parses a preprocessor expression: NOT? (LPAREN expr RPAREN) | logical_expr
fn preproc_expression(p: &mut Parser) {
    let m = p.start();

    // Optional NOT
    let _has_not = p.eat(TokenKind::KwNot);
    p.skip_trivia();

    // Check for parenthesized expression
    if p.at(TokenKind::LParen) {
        p.bump(); // (
        p.skip_trivia();
        preproc_expression(p);
        p.skip_trivia();
        p.expect(TokenKind::RParen);
    } else {
        // Parse logical expression (handles the expression after NOT if present)
        preproc_logical_expression(p);
    }

    m.complete(p, NodeKind::PreExpr);
}

/// Parses a preprocessor logical expression: operand (AND/OR operand)*
fn preproc_logical_expression(p: &mut Parser) {
    let m = p.start();

    preproc_logical_operand(p);

    while matches!(p.current(), Some(TokenKind::KwAnd) | Some(TokenKind::KwOr)) {
        p.check_iteration_limit();
        p.skip_trivia();
        let op_m = p.start();
        p.bump(); // AND/OR
        op_m.complete(p, NodeKind::PreBoolOp);
        p.skip_trivia();
        preproc_logical_operand(p);
    }

    m.complete(p, NodeKind::PreLogicalExpr);
}

/// Parses a preprocessor logical operand: (NOT? operand) | symbol | (logical_expr)
fn preproc_logical_operand(p: &mut Parser) {
    let m = p.start();

    if p.at(TokenKind::LParen) {
        p.bump(); // (
        p.skip_trivia();

        // Can be NOT? operand or logical expression
        if p.at(TokenKind::KwNot) {
            p.bump(); // NOT
            p.skip_trivia();
            preproc_logical_operand(p);
        } else {
            preproc_logical_expression(p);
        }

        p.skip_trivia();
        p.expect(TokenKind::RParen);
    } else if p.at(TokenKind::KwNot) {
        p.bump(); // NOT
        p.skip_trivia();
        preproc_symbol(p);
    } else {
        preproc_symbol(p);
    }

    m.complete(p, NodeKind::PreLogicalOperand);
}

pub(super) fn preprocessor_delete(p: &mut Parser) {
    let m = p.start();
    p.bump(); // #Удаление
    p.skip_trivia();

    // Skip all content until #КонецУдаления
    while !p.at_end() && !p.at(TokenKind::PreEndDelete) {
        p.check_iteration_limit();
        p.bump();
    }

    p.eat(TokenKind::PreEndDelete);
    m.complete(p, NodeKind::PreDeleteDir);
}

pub(super) fn preprocessor_insert(p: &mut Parser) {
    let m = p.start();
    p.bump(); // #Вставка
    p.skip_trivia();

    // Skip all content until #КонецВставки
    while !p.at_end() && !p.at(TokenKind::PreEndInsert) {
        p.check_iteration_limit();
        p.bump();
    }

    p.eat(TokenKind::PreEndInsert);
    m.complete(p, NodeKind::PreInsertDir);
}

/// Parses a preprocessor symbol (platform/OS symbols)
/// Platform/OS symbols (Клиент, Сервер, Linux, etc.) are now regular Idents.
/// The parser accepts any Ident as a preprocessor symbol.
fn preproc_symbol(p: &mut Parser) {
    let m = p.start();

    match p.current() {
        // Preprocessor symbols are recognized as Ident by the lexer
        // (Клиент, НаКлиенте, Сервер, Linux, Windows, etc.)
        Some(TokenKind::Ident) => {
            p.bump();
        }
        _ => {
            // Error: expected a preprocessor symbol (identifier)
            p.error();
        }
    }

    m.complete(p, NodeKind::PreSymbol);
}
