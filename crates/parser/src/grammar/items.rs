use lexer::TokenKind;

use crate::event::NodeKind;
use crate::parser::Parser;

use super::statements;

pub fn compiler_directive(p: &mut Parser) {
    let m = p.start();
    p.bump();
    m.complete(p, NodeKind::CompilerDirective);
}

pub fn annotation(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.skip_trivia();

    if p.at(TokenKind::LParen) {
        annotation_params(p);
    }

    m.complete(p, NodeKind::Annotation);
}

fn annotation_params(p: &mut Parser) {
    let m = p.start();
    p.bump();

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

fn annotation_param(p: &mut Parser) {
    let m = p.start();

    p.skip_trivia();

    if p.at(TokenKind::Ident) {
        p.bump();
        p.skip_trivia();
        if p.eat(TokenKind::Eq) {
            p.skip_trivia();
            annotation_param_value(p);
        }
    } else {
        annotation_param_value(p);
    }

    m.complete(p, NodeKind::AnnotationParam);
}

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
            annotation(p);
        }
        _ => {
            p.error_unexpected();
        }
    }
}

pub fn procedure_def(p: &mut Parser) {
    let m = p.start();
    procedure_def_content(p);
    m.complete(p, NodeKind::ProcedureDef);
}

// A definition states the word that closes it while it parses its header and
// its body, so that a rule tripping over that word leaves it for the
// definition instead of consuming it and reporting it missing at end of file.

fn at_end_procedure(p: &Parser) -> bool {
    p.at(TokenKind::KwEndProcedure)
}

fn at_end_function(p: &Parser) -> bool {
    p.at(TokenKind::KwEndFunction)
}

pub fn procedure_def_content(p: &mut Parser) {
    p.skip_trivia();

    p.eat(TokenKind::KwAsync);

    p.skip_trivia();
    p.expect(TokenKind::KwProcedure);

    p.within_boundary(at_end_procedure, |p| {
        p.skip_trivia();

        if p.at(TokenKind::Ident) || p.current().is_some_and(|k| k.is_keyword()) {
            p.bump();
        }

        p.skip_trivia();

        if p.at(TokenKind::LParen) {
            param_list(p);
        }

        p.skip_trivia();

        p.eat(TokenKind::KwExport);

        p.skip_trivia();

        statements::stmt_list(p, TokenKind::KwEndProcedure);
    });

    p.skip_trivia();
    p.expect(TokenKind::KwEndProcedure);
}

pub fn function_def(p: &mut Parser) {
    let m = p.start();
    function_def_content(p);
    m.complete(p, NodeKind::FunctionDef);
}

pub fn function_def_content(p: &mut Parser) {
    p.skip_trivia();

    p.eat(TokenKind::KwAsync);

    p.skip_trivia();
    p.expect(TokenKind::KwFunction);

    p.within_boundary(at_end_function, |p| {
        p.skip_trivia();

        if p.at(TokenKind::Ident) || p.current().is_some_and(|k| k.is_keyword()) {
            p.bump();
        }

        p.skip_trivia();

        if p.at(TokenKind::LParen) {
            param_list(p);
        }

        p.skip_trivia();

        p.eat(TokenKind::KwExport);

        p.skip_trivia();

        statements::stmt_list(p, TokenKind::KwEndFunction);
    });

    p.skip_trivia();
    p.expect(TokenKind::KwEndFunction);
}

fn param_list(p: &mut Parser) {
    let m = p.start();
    p.bump();

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

fn param(p: &mut Parser) {
    let m = p.start();

    p.skip_trivia();

    p.eat(TokenKind::KwVal);

    p.skip_trivia();

    if p.at(TokenKind::Ident) {
        p.bump();
    }

    p.skip_trivia();

    if p.eat(TokenKind::Eq) {
        p.skip_trivia();
        super::expressions::expression(p);
    }

    m.complete(p, NodeKind::Param);
}

pub fn var_declaration(p: &mut Parser) {
    let m = p.start();
    var_declaration_content(p);
    m.complete(p, NodeKind::VarDef);
}

pub fn var_declaration_content(p: &mut Parser) {
    p.bump();

    p.skip_trivia();

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

    p.eat(TokenKind::KwExport);

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);
}
