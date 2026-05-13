//! BSL grammar rules.

pub mod expressions;
pub mod items;
pub mod sdbl;
pub mod statements;

use lexer::TokenKind;

use crate::event::NodeKind;
use crate::parser::Parser;

/// Parses an annotated item (procedure, function, or variable with compiler directives).
fn annotated_item(p: &mut Parser) {
    // Start the outer node for the procedure/function/variable
    let outer = p.start();

    // Parse all types of annotations as children
    while matches!(
        p.current(),
        Some(TokenKind::AnnAtClient)
            | Some(TokenKind::AnnAtServer)
            | Some(TokenKind::AnnAtServerNoContext)
            | Some(TokenKind::AnnAtClientAtServer)
            | Some(TokenKind::AnnAtClientAtServerNoContext)
            | Some(TokenKind::AnnBefore)
            | Some(TokenKind::AnnAfter)
            | Some(TokenKind::AnnAround)
            | Some(TokenKind::AnnChangeAndValidate)
            | Some(TokenKind::AnnCustom)
    ) {
        p.check_iteration_limit();
        // For built-in annotations, use compiler_directive
        // For custom annotations, use annotation
        match p.current() {
            Some(TokenKind::AnnAtClient)
            | Some(TokenKind::AnnAtServer)
            | Some(TokenKind::AnnAtServerNoContext)
            | Some(TokenKind::AnnAtClientAtServer)
            | Some(TokenKind::AnnAtClientAtServerNoContext) => {
                items::compiler_directive(p);
            }
            _ => {
                items::annotation(p);
            }
        }
        p.skip_trivia();
    }

    // Now parse the actual procedure/function/variable
    match p.current() {
        Some(TokenKind::KwAsync) => match p.nth_non_trivia(0) {
            Some(TokenKind::KwProcedure) => {
                items::procedure_def_content(p);
                outer.complete(p, NodeKind::ProcedureDef);
            }
            Some(TokenKind::KwFunction) => {
                items::function_def_content(p);
                outer.complete(p, NodeKind::FunctionDef);
            }
            _ => {
                outer.abandon(p);
                p.error_unexpected();
            }
        },
        Some(TokenKind::KwProcedure) => {
            items::procedure_def_content(p);
            outer.complete(p, NodeKind::ProcedureDef);
        }
        Some(TokenKind::KwFunction) => {
            items::function_def_content(p);
            outer.complete(p, NodeKind::FunctionDef);
        }
        Some(TokenKind::KwVar) => {
            items::var_declaration_content(p);
            outer.complete(p, NodeKind::VarDef);
        }
        _ => {
            outer.abandon(p);
            p.error_unexpected();
        }
    }
}

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
                match p.nth_non_trivia(0) {
                    Some(TokenKind::KwProcedure) => items::procedure_def(p),
                    Some(TokenKind::KwFunction) => items::function_def(p),
                    _ => p.error_unexpected(),
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
                // Parse annotated procedure/function as a single node
                annotated_item(p);
            }
            // Custom annotations (&До, &После, &Вместо и т.д.)
            Some(TokenKind::AnnBefore)
            | Some(TokenKind::AnnAfter)
            | Some(TokenKind::AnnAround)
            | Some(TokenKind::AnnChangeAndValidate)
            | Some(TokenKind::AnnCustom) => {
                annotated_item(p);
            }
            // Module-level statements (assignments, expressions, etc.)
            // This is common in BSL for module initialization code
            _ => {
                statements::statement(p);
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
                match p.nth_non_trivia(0) {
                    Some(TokenKind::KwProcedure) => items::procedure_def(p),
                    Some(TokenKind::KwFunction) => items::function_def(p),
                    _ => p.error_unexpected(),
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
                // Use annotated_item to handle procedure, function, or variable
                annotated_item(p);
            }
            Some(TokenKind::AnnBefore)
            | Some(TokenKind::AnnAfter)
            | Some(TokenKind::AnnAround)
            | Some(TokenKind::AnnChangeAndValidate)
            | Some(TokenKind::AnnCustom) => {
                // Use annotated_item for consistency with other annotations
                annotated_item(p);
            }
            // Module-level statements in regions
            _ => {
                statements::statement(p);
            }
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
            Some(TokenKind::KwAsync) => match p.nth_non_trivia(0) {
                Some(TokenKind::KwProcedure) => items::procedure_def(p),
                Some(TokenKind::KwFunction) => items::function_def(p),
                _ => p.error_unexpected(),
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
                // Use annotated_item to handle procedure, function, or variable
                annotated_item(p);
            }
            Some(TokenKind::AnnBefore)
            | Some(TokenKind::AnnAfter)
            | Some(TokenKind::AnnAround)
            | Some(TokenKind::AnnChangeAndValidate)
            | Some(TokenKind::AnnCustom) => {
                // Use annotated_item for consistency with other annotations
                annotated_item(p);
            }
            // Module-level statements in preprocessor content
            _ => {
                statements::statement(p);
            }
        }
    }
}

/// Parses a preprocessor expression: operand (AND/OR operand)*
fn preproc_expression(p: &mut Parser) {
    let m = p.start();
    preproc_logical_expression(p);
    m.complete(p, NodeKind::PreExpr);
}

/// Parses a preprocessor logical expression: operand (AND/OR operand)*
fn preproc_logical_expression(p: &mut Parser) {
    let m = p.start();

    preproc_logical_operand(p);
    p.skip_trivia();

    while matches!(p.current(), Some(TokenKind::KwAnd) | Some(TokenKind::KwOr)) {
        p.check_iteration_limit();
        let op_m = p.start();
        p.bump(); // AND/OR
        op_m.complete(p, NodeKind::PreBoolOp);
        p.skip_trivia();
        preproc_logical_operand(p);
        p.skip_trivia();
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
        preproc_logical_operand(p);
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
            p.error_unexpected();
        }
    }

    m.complete(p, NodeKind::PreSymbol);
}
