pub mod expressions;
pub mod select;

use crate::event::NodeKind;
use crate::parser::Parser;
use crate::token_set::TokenSet;
use lexer::TokenKind;

pub(super) const LIST_RECOVERY: TokenSet =
    TokenSet::new(&[TokenKind::RParen, TokenKind::Semicolon]);

pub fn query_package(p: &mut Parser) {
    let m = p.start();

    p.skip_trivia();
    if !p.at_end() {
        queries(p);
    }

    loop {
        p.check_iteration_limit();
        p.skip_trivia();

        if !p.at(TokenKind::Semicolon) {
            break;
        }

        p.bump();
        p.skip_trivia();

        if p.at_end() {
            break;
        }

        queries(p);
    }

    m.complete(p, NodeKind::SdblQueryPackage);
}

fn queries(p: &mut Parser) {
    if select::at_sdbl_keyword(p, "DROP", "УНИЧТОЖИТЬ") {
        drop_table_query(p);
    } else {
        select::select_query(p);
    }
}

fn drop_table_query(p: &mut Parser) {
    let m = p.start();
    select::eat_sdbl_keyword(p, "DROP", "УНИЧТОЖИТЬ");
    p.skip_trivia();
    if p.at(TokenKind::Ident) {
        p.bump();
    } else {
        p.error_custom("ожидалось имя таблицы после 'УНИЧТОЖИТЬ' / 'DROP'");
    }
    m.complete(p, NodeKind::SdblDropQuery);
}
