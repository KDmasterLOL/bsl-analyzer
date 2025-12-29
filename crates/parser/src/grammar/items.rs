//! Item parsing (procedures, functions, variables).

use lexer::TokenKind;

use crate::event::NodeKind;
use crate::parser::Parser;

use super::statements;

/// Parses a compiler directive (&НаКлиенте, &НаСервере, etc).
pub fn compiler_directive(p: &mut Parser) {
    let m = p.start();
    p.bump(); // CompilerDirective token
    m.complete(p, NodeKind::CompilerDirective);
}

/// Parses an annotation with optional parameters.
pub fn annotation(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Annotation token

    p.skip_trivia();

    // Optional parameters
    if p.at(TokenKind::LParen) {
        annotation_params(p);
    }

    m.complete(p, NodeKind::Annotation);
}

/// Parses annotation parameters: (param1, param2=value, ...)
fn annotation_params(p: &mut Parser) {
    let m = p.start();
    p.bump(); // (

    p.skip_trivia();

    if !p.at(TokenKind::RParen) {
        annotation_param(p);
        while p.eat(TokenKind::Comma) {
            p.check_iteration_limit();
            p.skip_trivia();
            annotation_param(p);
        }
    }

    p.skip_trivia();
    p.expect(TokenKind::RParen);

    m.complete(p, NodeKind::AnnotationParams);
}

/// Parses a single annotation parameter: name or name=value
fn annotation_param(p: &mut Parser) {
    let m = p.start();

    p.skip_trivia();

    // Could be identifier (param name) or value
    if p.at(TokenKind::Ident) {
        p.bump();
        p.skip_trivia();
        // If followed by =, this is named parameter
        if p.eat(TokenKind::Eq) {
            p.skip_trivia();
            annotation_param_value(p);
        }
    } else {
        // Just a value
        annotation_param_value(p);
    }

    m.complete(p, NodeKind::AnnotationParam);
}

/// Parses annotation parameter value (const value or nested annotation)
fn annotation_param_value(p: &mut Parser) {
    match p.current() {
        Some(TokenKind::Decimal)
        | Some(TokenKind::Float)
        | Some(TokenKind::String)
        | Some(TokenKind::Date)
        | Some(TokenKind::KwTrue)
        | Some(TokenKind::KwFalse)
        | Some(TokenKind::KwUndefined)
        | Some(TokenKind::KwNull) => {
            p.bump();
        }
        Some(TokenKind::Minus) | Some(TokenKind::Plus) => {
            p.bump();
            p.skip_trivia();
            if p.at(TokenKind::Decimal) || p.at(TokenKind::Float) {
                p.bump();
            }
        }
        Some(
            TokenKind::AnnAtClient
            | TokenKind::AnnAtServer
            | TokenKind::AnnAtServerNoContext
            | TokenKind::AnnAtClientAtServer
            | TokenKind::AnnAtClientAtServerNoContext
            | TokenKind::AnnBefore
            | TokenKind::AnnAfter
            | TokenKind::AnnAround
            | TokenKind::AnnChangeAndValidate
            | TokenKind::AnnCustom,
        ) => {
            // Nested annotation
            annotation(p);
        }
        _ => {
            p.error();
        }
    }
}

/// Parses a procedure definition with optional Async.
pub fn procedure_def(p: &mut Parser) {
    let m = p.start();

    p.skip_trivia();

    // Optional Async keyword
    p.eat(TokenKind::KwAsync);

    p.skip_trivia();
    p.expect(TokenKind::KwProcedure);

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

/// Parses a function definition with optional Async.
pub fn function_def(p: &mut Parser) {
    let m = p.start();

    p.skip_trivia();

    // Optional Async keyword
    p.eat(TokenKind::KwAsync);

    p.skip_trivia();
    p.expect(TokenKind::KwFunction);

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
            p.check_iteration_limit();
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
        p.check_iteration_limit();
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
