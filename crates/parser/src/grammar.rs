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
        p.skip_trivia();

        if p.at_end() {
            break;
        }

        match p.current() {
            Some(TokenKind::KwProcedure) => items::procedure_def(p),
            Some(TokenKind::KwFunction) => items::function_def(p),
            Some(TokenKind::KwVar) => items::var_declaration(p),
            Some(TokenKind::PreRegion) => preprocessor_region(p),
            Some(TokenKind::PreIf) => preprocessor_if(p),
            Some(TokenKind::AnnAtClient)
            | Some(TokenKind::AnnAtServer)
            | Some(TokenKind::AnnAtServerNoContext)
            | Some(TokenKind::AnnAtClientAtServer)
            | Some(TokenKind::AnnAtClientAtServerNoContext) => {
                items::annotation(p);
                // After annotation, expect procedure or function
                p.skip_trivia();
                match p.current() {
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

fn preprocessor_region(p: &mut Parser) {
    let m = p.start();
    p.bump(); // #Область
    p.skip_trivia();

    if p.at(TokenKind::Ident) {
        p.bump();
    }

    // Parse content until #КонецОбласти
    while !p.at_end() && !p.at(TokenKind::PreEndRegion) {
        p.skip_trivia();
        if p.at_end() || p.at(TokenKind::PreEndRegion) {
            break;
        }

        match p.current() {
            Some(TokenKind::KwProcedure) => items::procedure_def(p),
            Some(TokenKind::KwFunction) => items::function_def(p),
            Some(TokenKind::KwVar) => items::var_declaration(p),
            Some(TokenKind::PreRegion) => preprocessor_region(p),
            Some(TokenKind::AnnAtClient)
            | Some(TokenKind::AnnAtServer)
            | Some(TokenKind::AnnAtServerNoContext)
            | Some(TokenKind::AnnAtClientAtServer)
            | Some(TokenKind::AnnAtClientAtServerNoContext) => {
                items::annotation(p);
                p.skip_trivia();
                match p.current() {
                    Some(TokenKind::KwProcedure) => items::procedure_def(p),
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

fn preprocessor_if(p: &mut Parser) {
    let m = p.start();
    p.bump(); // #Если

    // Skip condition for now
    while !p.at_end() && !p.at(TokenKind::PreThen) {
        p.bump();
    }

    p.eat(TokenKind::PreThen);

    // Parse content
    while !p.at_end()
        && !p.at(TokenKind::PreElsIf)
        && !p.at(TokenKind::PreElse)
        && !p.at(TokenKind::PreEndIf)
    {
        p.skip_trivia();
        if p.at_end() {
            break;
        }
        p.bump();
    }

    // Handle ElsIf and Else
    while p.at(TokenKind::PreElsIf) {
        p.bump();
        while !p.at_end() && !p.at(TokenKind::PreThen) {
            p.bump();
        }
        p.eat(TokenKind::PreThen);
        while !p.at_end()
            && !p.at(TokenKind::PreElsIf)
            && !p.at(TokenKind::PreElse)
            && !p.at(TokenKind::PreEndIf)
        {
            p.bump();
        }
    }

    if p.at(TokenKind::PreElse) {
        p.bump();
        while !p.at_end() && !p.at(TokenKind::PreEndIf) {
            p.bump();
        }
    }

    p.eat(TokenKind::PreEndIf);
    m.complete(p, NodeKind::PreIfDir);
}
