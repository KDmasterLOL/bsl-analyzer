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

    if p.at(TokenKind::LParen) {
        annotation_params(p);
    }

    m.complete(p, NodeKind::Annotation);
}

fn annotation_params(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.within_boundary(super::at_paren_list_punctuation, |p| {
        if !p.at(TokenKind::RParen) {
            annotation_param(p);
            while p.eat(TokenKind::Comma) {
                p.check_iteration_limit();
                annotation_param(p);
            }
        }
    });

    p.expect(TokenKind::RParen);

    m.complete(p, NodeKind::AnnotationParams);
}

fn annotation_param(p: &mut Parser) {
    let m = p.start();

    if p.at(TokenKind::Ident) {
        p.bump();
        if p.eat(TokenKind::Eq) {
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
    p.eat(TokenKind::KwAsync);

    p.expect(TokenKind::KwProcedure);

    p.within_boundary(at_end_procedure, |p| {
        if p.at(TokenKind::Ident) || p.current().is_some_and(|k| k.is_keyword()) {
            p.bump();
        }

        if p.at(TokenKind::LParen) {
            param_list(p);
        }

        p.eat(TokenKind::KwExport);

        statements::stmt_list(p, TokenKind::KwEndProcedure);
    });

    p.expect(TokenKind::KwEndProcedure);
}

pub fn function_def(p: &mut Parser) {
    let m = p.start();
    function_def_content(p);
    m.complete(p, NodeKind::FunctionDef);
}

pub fn function_def_content(p: &mut Parser) {
    p.eat(TokenKind::KwAsync);

    p.expect(TokenKind::KwFunction);

    p.within_boundary(at_end_function, |p| {
        if p.at(TokenKind::Ident) || p.current().is_some_and(|k| k.is_keyword()) {
            p.bump();
        }

        if p.at(TokenKind::LParen) {
            param_list(p);
        }

        p.eat(TokenKind::KwExport);

        statements::stmt_list(p, TokenKind::KwEndFunction);
    });

    p.expect(TokenKind::KwEndFunction);
}

fn param_list(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.within_boundary(super::at_paren_list_punctuation, |p| {
        if !p.at(TokenKind::RParen) {
            param(p);
            while p.eat(TokenKind::Comma) {
                p.check_iteration_limit();
                param(p);
            }
        }
    });

    p.expect(TokenKind::RParen);

    m.complete(p, NodeKind::ParamList);
}

fn param(p: &mut Parser) {
    let m = p.start();

    p.eat(TokenKind::KwVal);

    if p.at(TokenKind::Ident) {
        p.bump();
    }

    if p.eat(TokenKind::Eq) {
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

    if p.at(TokenKind::Ident) {
        p.bump();
    }

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        if p.at(TokenKind::Ident) {
            p.bump();
        }
    }

    p.eat(TokenKind::KwExport);

    p.eat(TokenKind::Semicolon);
}
