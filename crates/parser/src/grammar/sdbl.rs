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
//! (Slice 7 target) and `source_alias_legacy` (LEGACY twin, Slice 8 target;
//! renamed to `source_alias` in Slice 8 C1) — see
//! `docs/legal/sdbl-clean-room-slice7.md` for the attestation.
//!
//! Slice 8 — clean-room (complete, landed with C3): `is_data_source_start`,
//! `from_clause`, `data_source`, `table_ref`, `source_alias` (all in
//! submodule `select`) cover FROM sources and source chains: table
//! references, subqueries in FROM, parameter sources, and source aliases.
//! Authored from ITS pubqlang/10 and /12 grammar-shape rules, the local
//! mini-spec at `docs/legal/sdbl-select-mini-spec.md` §FROM clause, and
//! the project's own event-parser conventions from Slices 1, 2, 6, and 7.
//! `../bsl-parser/*` grammar text was not consulted during authoring;
//! per-function provenance comments appear on each function body. The
//! virtual-table method-call argument parser was extracted during Slice 8
//! C1 into LEGACY helper `virtual_table_args_legacy`; its clean-room
//! rewrite is deferred to Slice 5 (virtual table and external-source
//! handling). See `docs/legal/sdbl-clean-room-slice8.md` for the
//! attestation.
//!
//! Slice 10a — clean-room (complete, landed with C3 2026-04-25): the
//! expression backbone in submodule `expressions` covering atoms
//! (literals, parameters, parens/tuples/subqueries, Star) plus the
//! operator precedence chain (logical OR / AND / NOT / additive /
//! multiplicative / unary). The 17 functions live under the
//! `CLEAN-ROOM Slice 10a` banner: `is_expression_start`,
//! `is_recovery_point`, `recover_to_delimiter`, `parse_delimited_list`,
//! `logical_expression`, `expression`, `logical_or_expr`,
//! `logical_and_expr`, `not_expr`, `additive_expr`,
//! `multiplicative_expr`, `unary_expr`, `primary_expr`, `literal_expr`,
//! `string_literal_or_multi`, `parameter_expr`,
//! `paren_or_subquery_expr`. Authored from ITS pubqlang/10, /12, /22,
//! /40, /60 (via the local dump at
//! `/home/itrous/src/tools_migration/its/dump/`) and the local
//! mini-spec at `docs/legal/sdbl-expressions-mini-spec.md`. The C1
//! commit performed pure-refactor renames `comparison_expr` →
//! `comparison_expr_legacy` and `predicate_expr` →
//! `predicate_expr_legacy` and moved the deferred bodies under the
//! LEGACY banner; per-function provenance comments were attached at
//! C2; the C2 commit also fixed a pre-existing bug routing bare
//! `NULL` through `column_or_function`. See
//! `docs/legal/sdbl-clean-room-slice10a.md` for the attestation.
//!
//! Slice 10b — clean-room (complete, landed with C3 2026-04-25):
//! the 8 functions `comparison_expr`, `predicate_expr`,
//! `column_or_function`, `inline_table_fields`, `is_cast_function`,
//! `parse_cast_type`, `case_expr`, `when_clause` (all in submodule
//! `expressions` under the `CLEAN-ROOM Slice 10b` banner) cover
//! predicates / comparison / column-or-function dispatch / CAST
//! type spec / CASE expressions. Authored from ITS pubqlang
//! chapters 21, 22, 23, 27, 32, 40 (via the local dump at
//! `/home/itrous/src/tools_migration/its/dump/`) and the
//! C0a-extended `docs/legal/sdbl-expressions-mini-spec.md`. The C1
//! commit performed pure-refactor renames
//! `comparison_expr_legacy` → `comparison_expr` and
//! `predicate_expr_legacy` → `predicate_expr`, replaced the
//! previous LEGACY banner with the clean-room banner, and attached
//! per-function placeholder provenance comments. The C2 commit
//! re-authored each function body from the cited sources, replaced
//! the placeholders with ITS / mini-spec provenance comments, and
//! landed the `column_or_function` clause-keyword recovery fix
//! (codex Round-1 finding 2 → C2 FIX). See
//! `docs/legal/sdbl-clean-room-slice10b.md` for the attestation.
//!
//! Slice 9 — clean-room (complete, landed with C3 2026-04-25):
//! the JOIN family surface — `is_join_keyword` and `join_clause`
//! (in submodule `select` under the `CLEAN-ROOM Slice 9 — JOIN
//! family` banner) — was re-authored in C2 from ITS pubqlang
//! chapters 44 (`ВНУТРЕННЕЕ СОЕДИНЕНИЕ` listing + standalone
//! `СОЕДИНЕНИЕ` reference), 45 (`ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ`),
//! 46 (`ПРАВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ`), 47 (`ПОЛНОЕ ВНЕШНЕЕ
//! СОЕДИНЕНИЕ`), 48 (chained / nested examples) via the local
//! dump at `/home/itrous/src/tools_migration/its/dump/`,
//! `docs/legal/sdbl-select-mini-spec.md` §JOIN clauses (lines
//! 297–319) + §Recovery requirements item #6 (line 410), and
//! the lexer's Slice 2 attestation for bilingual EN/RU keyword
//! pairs (LEFT/ЛЕВОЕ, RIGHT/ПРАВОЕ, FULL/ПОЛНОЕ,
//! INNER/ВНУТРЕННЕЕ, JOIN/СОЕДИНЕНИЕ, OUTER/ВНЕШНЕЕ, ON/ПО).
//! The C1 commit physically split the two functions out of the
//! previous `LEGACY (Slices 9–11 pending)` block (banner header
//! shrunk to `LEGACY (Slices 5, 11 pending)`); C2 attached
//! tiered (A1/B/C/D) per-function provenance comments. The
//! audit-gate decision was **Option B PRESERVE** for both
//! `Parser::error()`-bumps in `join_clause` (recovery hardening
//! deferred to Slice 12). See
//! `docs/legal/sdbl-clean-room-slice9.md` for the attestation.
//!
//! Slice 11 — clean-room (complete, landed with C3 2026-04-26).
//! The 12 functions `select_tail_clauses` / `query_body_clauses`
//! / `where_clause` / `is_clause_keyword` / `group_by_clause` /
//! `order_by_clause` / `order_by_item` / `having_clause` /
//! `for_update_clause` / `index_by_clause` / `autoorder_clause`
//! / `totals_by_clause` were re-authored under the
//! `CLEAN-ROOM Slice 11 — clauses after FROM` banner in
//! `select.rs`. Authored from ITS pubqlang chapters 16, 17, 22,
//! 23, 24, 27, 34, 35, 39 via the local dump at
//! `/home/itrous/src/tools_migration/its/dump/`,
//! `docs/legal/sdbl-select-mini-spec.md` §WHERE / §GROUP BY /
//! §HAVING / §ORDER BY / §AUTOORDER / §TOTALS BY / §FOR UPDATE
//! / §INDEX BY full-body sections (extended in C0a) + §IDE-
//! recovery allowances block (4 entries) + §ITS coverage
//! verification table (filled in C2), and the lexer's Slice 2
//! attestation for bilingual EN/RU keyword pairs. The C2
//! commit landed one MANDATORY behaviour-change fix:
//! `order_by_item` now consumes the optional HIERARCHY/ИЕРАРХИЯ
//! modifier as a flat sibling IDENT token of `SdblOrderClause`
//! (per ITS chapter 27 — `chapter_027.html:39, 51` —
//! `УПОРЯДОЧИТЬ ПО Наименование ИЕРАРХИЯ`), atomic with
//! unignoring the C0b regression-gate test
//! `test_slice11_order_by_hierarchy_consumed`. Parser-only
//! acceptance: HIR semantic interpretation is deferred to
//! Slice 13 (`crates/sdbl-hir/**` was read-only for this
//! slice). See
//! `docs/legal/sdbl-clean-room-slice11.md` for the
//! attestation.
//!
//! Slice 5 pending: virtual-table method-call arguments
//! (`virtual_table_args_legacy`) remain Tier B under the Slice 5
//! banner.
//!
//! Slice 7-addendum — clean-room (rewrite in progress): the
//! SELECT-prefix qualifier helpers `is_identifier_token`,
//! `is_limitation_keyword`, `limitations`, `top_clause` (DISTINCT
//! / РАЗЛИЧНЫЕ, TOP / ПЕРВЫЕ, ALLOWED / РАЗРЕШЕННЫЕ) are
//! relocated under the
//! `CLEAN-ROOM Slice 7-addendum — SELECT prefix qualifiers`
//! banner in `select.rs` at C1; per-function provenance comments
//! and clean-room body re-derivation land at C2; attestation +
//! acceptance tests + this docstring flip to "complete" land at
//! C3.

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
