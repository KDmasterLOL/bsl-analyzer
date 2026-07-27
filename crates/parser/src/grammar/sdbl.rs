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

    // The loop is driven by "there is input left", not by "the separator is
    // where I left it". Recovery inside a query may consume a `;` — several
    // paths do, by reporting through a token bump — and a package whose
    // members are parsed only while the separators survive is a package that
    // loses good queries behind bad ones.
    let mut a_query_is_due = true;

    p.skip_trivia();

    while !p.at_end() {
        p.check_iteration_limit();
        p.skip_trivia();

        if p.at(TokenKind::Semicolon) {
            if a_query_is_due {
                // Two separators in a row: a member of the sequence is
                // missing. Trailing separators are not this — there the loop
                // ends at the input rather than at another separator.
                p.error_custom_no_bump("ожидался запрос между разделителями");
            }
            p.bump();
            a_query_is_due = true;
            continue;
        }

        if p.at_end() {
            break;
        }

        if a_query_is_due {
            queries(p);
            a_query_is_due = false;
        } else if at_query_start(p) {
            // A query where a separator should be. Recognising it is what
            // keeps the rest of the package parseable, but the package is
            // still malformed and silence here would turn a bad package into
            // two good queries.
            p.error_custom_no_bump("ожидался разделитель ';' между запросами");
            queries(p);
        } else if !drain_to_boundary(p) {
            break;
        }
    }

    m.complete(p, NodeKind::SdblQueryPackage);
}

fn at_query_start(p: &Parser) -> bool {
    select::at_sdbl_keyword(p, "SELECT", "ВЫБРАТЬ")
        || select::at_sdbl_keyword(p, "DROP", "УНИЧТОЖИТЬ")
}

/// Takes whatever the query rules refused, up to the next boundary, so that
/// the tree's text is the source text. Returns whether anything was taken.
///
/// A query stops short whenever a clause is out of order, a modifier is one
/// this grammar does not know, or the text is simply wrong. Without this the
/// remainder is not merely unparsed — it is absent from the tree, and the
/// parse reports success, so a consumer cannot tell a clean query from half
/// of one.
///
/// The boundary is a separator *or* the start of the next query. Stopping at
/// the separator alone would be enough only if every recovery path left it
/// alone, and they do not; recognising a query start as well is what makes
/// "one bad member costs only itself" true rather than merely intended.
fn drain_to_boundary(p: &mut Parser) -> bool {
    p.skip_trivia();

    if p.at_end() || p.at(TokenKind::Semicolon) {
        return false;
    }

    let leftover = p.start();
    let mut took_anything = false;

    while !p.at_end() && !p.at(TokenKind::Semicolon) {
        p.check_iteration_limit();

        if took_anything && at_query_start(p) {
            break;
        }

        p.bump();
        took_anything = true;
    }

    if took_anything {
        p.emit_error_at_marker(
            leftover,
            ParseError::Custom {
                message: "не разобран остаток текста запроса",
                recovery: RecoveryKind::RecoverySpan,
            },
        );
    } else {
        leftover.abandon(p);
    }

    took_anything
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
