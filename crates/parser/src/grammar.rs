pub mod expressions;
pub mod items;
pub mod sdbl;
pub mod statements;

use lexer::TokenKind;

use crate::event::NodeKind;
use crate::parser::Parser;

fn annotated_item(p: &mut Parser) {
    let outer = p.start();

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

    // Region directives are flat folding markers and may sit between an
    // annotation and the declaration it applies to (e.g. `&НаКлиенте #Область X
    // <newline> Перем Y;`). Consume them here so the annotation still binds to
    // the following Procedure/Function/Var instead of derailing the parse.
    while matches!(p.current(), Some(TokenKind::PreRegion) | Some(TokenKind::PreEndRegion)) {
        p.check_iteration_limit();
        match p.current() {
            Some(TokenKind::PreRegion) => preprocessor_region(p),
            _ => preprocessor_end_region(p),
        }
        p.skip_trivia();
    }

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

pub fn source_file(p: &mut Parser) {
    let m = p.start();

    while !p.at_end() {
        p.check_iteration_limit();
        p.skip_trivia();

        if p.at_end() {
            break;
        }

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
            Some(TokenKind::PreEndRegion) => preprocessor_end_region(p),
            Some(TokenKind::PreIf) => preprocessor_if(p),
            Some(TokenKind::PreDelete) => preprocessor_delete(p),
            Some(TokenKind::PreInsert) => preprocessor_insert(p),
            Some(TokenKind::AnnAtClient)
            | Some(TokenKind::AnnAtServer)
            | Some(TokenKind::AnnAtServerNoContext)
            | Some(TokenKind::AnnAtClientAtServer)
            | Some(TokenKind::AnnAtClientAtServerNoContext) => {
                annotated_item(p);
            }
            Some(TokenKind::AnnBefore)
            | Some(TokenKind::AnnAfter)
            | Some(TokenKind::AnnAround)
            | Some(TokenKind::AnnChangeAndValidate)
            | Some(TokenKind::AnnCustom) => {
                annotated_item(p);
            }
            _ => {
                statements::statement(p);
            }
        }
    }

    m.complete(p, NodeKind::SourceFile);
}

/// `#Область Имя` is a flat folding marker, not a container.
///
/// 1C region directives are preprocessor markers stripped before compilation;
/// they may interleave with control flow without nesting (e.g. `#КонецОбласти`
/// inside an `Если` body before `КонецЕсли`). A container node cannot represent
/// such overlapping ranges in a full-fidelity tree, so each `#Область` and each
/// `#КонецОбласти` is its own leaf node. Region nesting is reconstructed
/// post-hoc by pairing start/end markers (see `hir-def::region_tree`).
pub(super) fn preprocessor_region(p: &mut Parser) {
    let m = p.start();
    p.bump();
    p.skip_trivia();

    if p.at(TokenKind::Ident) {
        p.bump();
    }

    m.complete(p, NodeKind::PreRegionDir);
}

/// `#КонецОбласти` is a flat folding marker. See [`preprocessor_region`].
pub(super) fn preprocessor_end_region(p: &mut Parser) {
    let m = p.start();
    p.bump();
    m.complete(p, NodeKind::PreRegionDir);
}

pub(super) fn preprocessor_if(p: &mut Parser) {
    let m = p.start();
    p.bump();
    p.skip_trivia();

    p.within_boundary(at_preproc_closer, |p| {
        preproc_expression(p);
        p.skip_trivia();

        p.expect(TokenKind::KwThen);

        preproc_content(p);

        while p.at(TokenKind::PreElsIf) {
            p.check_iteration_limit();
            let elsif_m = p.start();
            p.bump();
            p.skip_trivia();

            preproc_expression(p);
            p.skip_trivia();

            p.expect(TokenKind::KwThen);

            preproc_content(p);

            elsif_m.complete(p, NodeKind::PreElsIfClause);
        }

        if p.at(TokenKind::PreElse) {
            let else_m = p.start();
            p.bump();

            preproc_content(p);

            else_m.complete(p, NodeKind::PreElseClause);
        }
    });

    p.expect(TokenKind::PreEndIf);
    m.complete(p, NodeKind::PreIfDir);
}

fn at_preproc_closer(p: &Parser) -> bool {
    matches!(p.current(), Some(TokenKind::PreElsIf | TokenKind::PreElse | TokenKind::PreEndIf))
}

/// A conditional region and a statement block can cross each other: a
/// `#Если` may open inside `Если` and the `ИначеЕсли` closing that `Если` may
/// stand inside the region. The region's content therefore ends at the
/// closers of whatever encloses it as well as at its own — rules inside it
/// will not consume an enclosing closer, so a region waiting only for
/// `#КонецЕсли` would wait for a token nothing reaches.
fn preproc_content(p: &mut Parser) {
    while !p.at_end() && !at_preproc_closer(p) && !p.at_enclosing_boundary() {
        p.check_iteration_limit();
        p.skip_trivia();
        if p.at_end() || at_preproc_closer(p) || p.at_enclosing_boundary() {
            break;
        }

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
            Some(TokenKind::PreEndRegion) => preprocessor_end_region(p),
            Some(TokenKind::PreIf) => preprocessor_if(p),
            Some(TokenKind::PreDelete) => preprocessor_delete(p),
            Some(TokenKind::PreInsert) => preprocessor_insert(p),
            Some(TokenKind::AnnAtClient)
            | Some(TokenKind::AnnAtServer)
            | Some(TokenKind::AnnAtServerNoContext)
            | Some(TokenKind::AnnAtClientAtServer)
            | Some(TokenKind::AnnAtClientAtServerNoContext) => {
                annotated_item(p);
            }
            Some(TokenKind::AnnBefore)
            | Some(TokenKind::AnnAfter)
            | Some(TokenKind::AnnAround)
            | Some(TokenKind::AnnChangeAndValidate)
            | Some(TokenKind::AnnCustom) => {
                annotated_item(p);
            }
            _ => {
                statements::statement(p);
            }
        }
    }
}

fn preproc_expression(p: &mut Parser) {
    let m = p.start();
    preproc_logical_expression(p);
    m.complete(p, NodeKind::PreExpr);
}

fn preproc_logical_expression(p: &mut Parser) {
    let m = p.start();

    preproc_logical_operand(p);
    p.skip_trivia();

    while matches!(p.current(), Some(TokenKind::KwAnd) | Some(TokenKind::KwOr)) {
        p.check_iteration_limit();
        let op_m = p.start();
        p.bump();
        op_m.complete(p, NodeKind::PreBoolOp);
        p.skip_trivia();
        preproc_logical_operand(p);
        p.skip_trivia();
    }

    m.complete(p, NodeKind::PreLogicalExpr);
}

fn preproc_logical_operand(p: &mut Parser) {
    let m = p.start();

    if p.at(TokenKind::LParen) {
        p.bump();
        p.skip_trivia();

        if p.at(TokenKind::KwNot) {
            p.bump();
            p.skip_trivia();
            preproc_logical_operand(p);
        } else {
            preproc_logical_expression(p);
        }

        p.skip_trivia();
        p.expect(TokenKind::RParen);
    } else if p.at(TokenKind::KwNot) {
        p.bump();
        p.skip_trivia();
        preproc_logical_operand(p);
    } else {
        preproc_symbol(p);
    }

    m.complete(p, NodeKind::PreLogicalOperand);
}

pub(super) fn preprocessor_delete(p: &mut Parser) {
    let m = p.start();
    p.bump();
    p.skip_trivia();

    while !p.at_end() && !p.at(TokenKind::PreEndDelete) {
        p.check_iteration_limit();
        p.bump();
    }

    p.eat(TokenKind::PreEndDelete);
    m.complete(p, NodeKind::PreDeleteDir);
}

pub(super) fn preprocessor_insert(p: &mut Parser) {
    let m = p.start();
    p.bump();
    p.skip_trivia();

    while !p.at_end() && !p.at(TokenKind::PreEndInsert) {
        p.check_iteration_limit();
        p.bump();
    }

    p.eat(TokenKind::PreEndInsert);
    m.complete(p, NodeKind::PreInsertDir);
}

fn preproc_symbol(p: &mut Parser) {
    let m = p.start();

    match p.current() {
        Some(TokenKind::Ident) => {
            p.bump();
        }
        _ => {
            p.error_unexpected();
        }
    }

    m.complete(p, NodeKind::PreSymbol);
}
