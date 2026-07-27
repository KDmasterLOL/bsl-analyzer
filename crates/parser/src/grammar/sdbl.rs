//! SDBL (1C:Enterprise query language) grammar.
//!
//! ## Provenance
//!
//! The grammar has been re-derived from official 1C sources one area at
//! a time. Areas re-derived after the repository-wide comment prune
//! carry a `CLEAN-ROOM` banner naming the attestation that records their
//! sources.
//!
//! - Slice 12 — recovery behaviour and editor allowances:
//!   `docs/legal/sdbl-clean-room-slice12.md`.
//!
//! The query package and `SELECT` skeleton, the field list, the source
//! chains, the `JOIN` family, the expression layer, the clauses after
//! `FROM`, the `SELECT` prefix qualifiers and the virtual-table argument
//! body are attested by
//! `docs/legal/sdbl-clean-room-slice{6,7,7-addendum,8,8-addendum,9,10a,10b,11}.md`.
//! Their in-file banners were removed in that prune and are not restored
//! here, so absence of a banner above a function does not imply absence
//! of an attestation — the attestation documents are authoritative on
//! scope.
//!
//! What remains unowned is the reattachment of this surface to semantic
//! lowering, tracked as Slice 13; `crates/sdbl-hir` is read-only for
//! every slice before it.

pub mod expressions;
pub mod select;

use crate::event::NodeKind;
use crate::parser::Parser;
use crate::token_set::TokenSet;
use lexer::TokenKind;
use parser_error::{ParseError, RecoveryKind};

pub(super) const LIST_RECOVERY: TokenSet =
    TokenSet::new(&[TokenKind::RParen, TokenKind::Semicolon]);

// =====================================================================
// CLEAN-ROOM Slice 12 — the entry point's treatment of leftover input
//
// A query package is a `;`-separated sequence of queries. What the
// official grammar does not describe, and what this function therefore
// decides on its own, is where the sequence ends when the current token
// is neither `;` nor the end of input.
//
// Provenance: `docs/legal/sdbl-clean-room-slice12.md`, entry D1.
// =====================================================================

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

    drain_unconsumed_input(p);

    m.complete(p, NodeKind::SdblQueryPackage);
}

/// Takes whatever the package loop refused, so that the tree's text is
/// the source text.
///
/// The loop above ends at the first token that is neither `;` nor the end
/// of input, which happens whenever a clause is out of order, a modifier
/// is one the grammar here does not know, or the query is simply wrong.
/// Without this the remainder is not merely unparsed — it is absent from
/// the tree, and the parse reports success, so a consumer cannot tell a
/// clean query from half of one.
fn drain_unconsumed_input(p: &mut Parser) {
    p.skip_trivia();

    if p.at_end() {
        return;
    }

    let leftover = p.start();

    while !p.at_end() {
        p.check_iteration_limit();
        p.bump();
    }

    p.emit_error_at_marker(
        leftover,
        ParseError::Custom {
            message: "не разобран остаток текста запроса",
            recovery: RecoveryKind::RecoverySpan,
        },
    );
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
