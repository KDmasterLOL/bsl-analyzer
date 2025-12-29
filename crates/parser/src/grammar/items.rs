//! Item parsing (procedures, functions, variables).

use lexer::TokenKind;

use crate::event::NodeKind;
use crate::parser::Parser;

use super::statements;

/// Parses an annotation.
pub fn annotation(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Annotation token
    m.complete(p, NodeKind::Annotation);
}

/// Parses a procedure definition.
pub fn procedure_def(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Процедура

    p.skip_trivia();

    // Name
    if p.at(TokenKind::Ident) {
        p.bump();
    }

    p.skip_trivia();

    // Parameters
    if p.at(TokenKind::LParen) {
        param_list(p);
    }

    p.skip_trivia();

    // Export
    p.eat(TokenKind::KwExport);

    p.skip_trivia();

    // Body
    statements::stmt_list(p, TokenKind::KwEndProcedure);

    p.skip_trivia();
    p.expect(TokenKind::KwEndProcedure);

    m.complete(p, NodeKind::ProcedureDef);
}

/// Parses a function definition.
pub fn function_def(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Функция

    p.skip_trivia();

    // Name
    if p.at(TokenKind::Ident) {
        p.bump();
    }

    p.skip_trivia();

    // Parameters
    if p.at(TokenKind::LParen) {
        param_list(p);
    }

    p.skip_trivia();

    // Export
    p.eat(TokenKind::KwExport);

    p.skip_trivia();

    // Body
    statements::stmt_list(p, TokenKind::KwEndFunction);

    p.skip_trivia();
    p.expect(TokenKind::KwEndFunction);

    m.complete(p, NodeKind::FunctionDef);
}

/// Parses a parameter list.
fn param_list(p: &mut Parser) {
    let m = p.start();
    p.bump(); // (

    p.skip_trivia();

    if !p.at(TokenKind::RParen) {
        param(p);
        while p.eat(TokenKind::Comma) {
            p.skip_trivia();
            param(p);
        }
    }

    p.skip_trivia();
    p.expect(TokenKind::RParen);

    m.complete(p, NodeKind::ParamList);
}

/// Parses a single parameter.
fn param(p: &mut Parser) {
    let m = p.start();

    p.skip_trivia();

    // Val keyword
    p.eat(TokenKind::KwVal);

    p.skip_trivia();

    // Name
    if p.at(TokenKind::Ident) {
        p.bump();
    }

    p.skip_trivia();

    // Default value
    if p.eat(TokenKind::Eq) {
        p.skip_trivia();
        super::expressions::expression(p);
    }

    m.complete(p, NodeKind::Param);
}

/// Parses a variable declaration.
pub fn var_declaration(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Перем

    p.skip_trivia();

    // Variable name(s)
    if p.at(TokenKind::Ident) {
        p.bump();
    }

    while p.eat(TokenKind::Comma) {
        p.skip_trivia();
        if p.at(TokenKind::Ident) {
            p.bump();
        }
    }

    p.skip_trivia();

    // Export
    p.eat(TokenKind::KwExport);

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);

    m.complete(p, NodeKind::VarDef);
}
