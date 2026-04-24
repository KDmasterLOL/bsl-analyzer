//! SDBL (Structured Data Base Language) grammar module
//!
//! Implements parsing for 1C:Enterprise query language embedded in BSL code.
//!
//! Architecture: Event-based parser
//!
//! ## Token Handling
//!
//! SDBL tokens are converted to BSL TokenKind via sdbl_token_converter.
//! Most SDBL keywords are mapped to TokenKind::Ident, so we check token text
//! to identify keywords.
//!
//! ## Provenance
//!
//! Slice 6 — clean-room: `query_package`, `queries`, `drop_table_query`
//! (this file) and `select_query` wrapper, `subquery`, `union_clause`
//! (in submodule `select`) are authored from ITS pubqlang/10 and /12
//! grammar-shape rules and from the project's own event-parser conventions
//! established in Slices 1 and 2. `../bsl-parser/*` grammar text was not
//! consulted during authoring; per-function provenance comments appear on
//! each function body. See `docs/legal/sdbl-clean-room-slice6.md` for the
//! attestation (landed with C3).
//!
//! Slice 7 — clean-room (complete, landed with C3): `query` wrapper,
//! `selected_fields`, `selected_field`, `is_field_start`, `is_asterisk_start`,
//! `asterisk_field`, `selected_field_alias`, `into_clause` (all in submodule
//! `select`) cover the SELECT prefix: field list, aliases, and INTO /
//! ПОМЕСТИТЬ. Authored from ITS pubqlang/10, /12, /51 h47 (for INTO), the
//! local mini-spec at `docs/legal/sdbl-select-mini-spec.md`, and the
//! project's own event-parser conventions from Slices 1, 2, and 6. The
//! former `alias` helper was call-site-split in C1: `selected_field_alias`
//! (Slice 7 target) and `source_alias_legacy` (LEGACY, Slice 8 target) —
//! see `docs/legal/sdbl-clean-room-slice7.md` for the attestation.
//!
//! Slices 8–11 pending: FROM-source chains (`from_clause`, `data_source`,
//! `table_ref`, `source_alias_legacy`), JOIN family, WHERE / GROUP / HAVING
//! / ORDER / TOTALS / FOR UPDATE / INDEX BY clause bodies, and the
//! `query_body_clauses` dispatch helper remain Tier B until their respective
//! clean-room slices.

pub mod expressions;
pub mod select;

use crate::event::NodeKind;
use crate::parser::Parser;
use crate::token_set::TokenSet;
use lexer::TokenKind;

// ============================================================================
// Recovery Sets
// ============================================================================
//
// TokenSets for error recovery during parsing.
// These define "safe" stopping points where the parser can recover from errors.

/// Recovery set for expressions and list elements.
/// Recovery set for list parsing (SELECT fields, FROM sources, IN value lists, etc.)
///
/// Does NOT include Comma because comma is the delimiter in parse_delimited_list,
/// not a recovery point. Including Comma would cause the list to break before
/// consuming the comma separator (e.g., `IN (VALUE(...), VALUE(...))` would fail).
///
/// Used in:
/// - SELECT field list
/// - FROM data source list
/// - IN predicate value list
pub(super) const LIST_RECOVERY: TokenSet = TokenSet::new(&[
    TokenKind::RParen,    // End of parenthesized expression/list
    TokenKind::Semicolon, // End of query/statement
]);

// ============================================================================
// CLEAN-ROOM Slice 6 — query package, DROP, select entry
// ============================================================================
//
// See `docs/legal/sdbl-clean-room-slice6.md` for authorship and source
// citations. Per-function provenance comments are attached at C2.

/// Entry point for SDBL parsing.
///
/// Grammar: `query-package := query-item (';' query-item)* ';'?`
///
/// Consumes an optional leading trivia run, then one or more query items
/// separated by `;`. A trailing `;` is allowed; the package ends at EOF.
///
/// Parser tolerance (not formal grammar): empty or trivia-only input is
/// accepted as an `SdblQueryPackage` node with no children, so the IDE can
/// reason about incomplete documents without parse aborts.
///
/// # Examples
///
/// ```sdbl
/// SELECT Name FROM Catalog.Products;
/// SELECT Code FROM Catalog.Products
/// ```
pub fn query_package(p: &mut Parser) {
    // ITS pubqlang/10 — query package shape: queries (SEMICOLON queries)* SEMICOLON? EOF.
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

/// Dispatch a single query item to SELECT or DROP.
///
/// Grammar: `query-item := select-query | drop-query`
fn queries(p: &mut Parser) {
    // local: dispatcher between SELECT entry and DROP statement; DROP-first
    // check because the DROP/УНИЧТОЖИТЬ keyword is the single-token prefix
    // per ITS pubqlang/10 + /12 temporary-table statement vocabulary.
    if select::at_sdbl_keyword(p, "DROP", "УНИЧТОЖИТЬ") {
        drop_table_query(p);
    } else {
        select::select_query(p);
    }
}

/// Parse a DROP statement for a temporary table.
///
/// Grammar: `drop-query := (DROP|УНИЧТОЖИТЬ) identifier`
fn drop_table_query(p: &mut Parser) {
    // ITS pubqlang/10 + /12 (statement vocabulary) and pubqlang/51 h47
    // (temporary-table lifecycle) — DROP / УНИЧТОЖИТЬ terminates the
    // lifetime of a temporary table named by a single identifier. The
    // minimal `DROP <ident>` shape below is spec-derivable from these pages;
    // the attestation §Preserved pre-refactor behaviours notes that the
    // identifier-recovery path is local-preserved and that a tightened
    // rewrite is expected when Slice 3 promotes the KwDrop lexer variant.
    let m = p.start();
    select::eat_sdbl_keyword(p, "DROP", "УНИЧТОЖИТЬ");
    p.skip_trivia();
    if p.at(TokenKind::Ident) {
        p.bump();
    } else {
        p.error();
    }
    m.complete(p, NodeKind::SdblDropQuery);
}
