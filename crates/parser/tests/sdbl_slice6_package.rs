//! Clean-room acceptance tests for SDBL Slice 6 — parser root and package
//! skeleton.
//!
//! Sources:
//! - ITS query-language structure —
//!   <https://its.1c.ru/db/pubqlang/content/10/hdoc>
//!   (query package shape: one or more query items separated by `;`,
//!   trailing `;` allowed; `UNION` / `UNION ALL` skeleton; subquery).
//! - ITS lexical elements —
//!   <https://its.1c.ru/db/pubqlang/content/12/hdoc>
//!   (keyword vocabulary for `DROP` / `УНИЧТОЖИТЬ`, `UNION` /
//!   `ОБЪЕДИНИТЬ`, `ALL` / `ВСЕ`, `SELECT` / `ВЫБРАТЬ`).
//! - ITS temporary-table lifecycle —
//!   <https://its.1c.ru/db/pubqlang/content/51/hdoc/h47>
//!   (`DROP` terminates the lifetime of a temporary table).
//!
//! Per-test docstring shorthand: `ITS pubqlang/N` refers to the
//! documentation sub-tree rooted at
//! `https://its.1c.ru/db/pubqlang/content/N/hdoc` for the given `N`.
//!
//! These tests were authored against the specifications above, not
//! against the existing `parse_sdbl` output. They cover the Slice 6
//! surface — `query_package`, `queries`, `drop_table_query` (in
//! `grammar/sdbl.rs`) and the clean-room portion of `select_query`,
//! `subquery`, `union_clause` (in `grammar/sdbl/select.rs`). Several
//! boundary fixtures necessarily include downstream fragments (`FROM`,
//! `WHERE`) to exercise the subquery-vs-package boundary contract, but
//! every assertion targets a Slice 6 node kind (`SdblQueryPackage`,
//! `SdblSelectQuery`, `SdblDropQuery`, `SdblSubquery`,
//! `SdblUnionClause`) or a Slice 6 wrapper behaviour. The bodies of
//! those downstream clauses stay Tier B until their clean-room rewrite
//! in Slices 7–11.
//!
//! One documented pre-refactor behaviour is preserved: `UNION` and
//! `UNION ALL` share `SdblUnionClause` as a single node kind (the
//! optional `ALL` modifier is carried as an IDENT token inside the
//! node). See `docs/legal/sdbl-clean-room-slice6.md` § Preserved
//! pre-refactor behaviours; the split into a distinct
//! `SdblUnionAllClause` is deferred to Slice 13.

use parser::parse_sdbl;
use syntax::{
    ast::{AstNode, SdblQueryPackage, SdblSubquery},
    SyntaxKind,
};

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

fn package(input: &str) -> SdblQueryPackage {
    let parse = parse_sdbl(input);
    assert!(
        !parse.has_errors(),
        "unexpected parse errors for {input:?}: {errors:?}",
        errors = parse.errors()
    );
    SdblQueryPackage::cast(parse.syntax_node()).expect("top node is SdblQueryPackage")
}

fn package_allow_errors(input: &str) -> SdblQueryPackage {
    let parse = parse_sdbl(input);
    SdblQueryPackage::cast(parse.syntax_node()).expect("top node is SdblQueryPackage")
}

fn count_kind(pkg: &SdblQueryPackage, kind: SyntaxKind) -> usize {
    pkg.syntax().descendants().filter(|n| n.kind() == kind).count()
}

fn top_level_select_count(pkg: &SdblQueryPackage) -> usize {
    pkg.queries().count()
}

// ----------------------------------------------------------------------------
// Package shape — ITS pubqlang/10 query package
// ----------------------------------------------------------------------------

/// ITS pubqlang/10 — a single query is a valid package.
#[test]
fn test_single_select_is_a_package() {
    let pkg = package("SELECT Name FROM Products");
    assert_eq!(top_level_select_count(&pkg), 1);
}

/// ITS pubqlang/10 — two queries separated by `;` form a two-item package.
#[test]
fn test_two_selects_separated_by_semicolon() {
    let pkg = package("SELECT Name FROM Products; SELECT Code FROM Services");
    assert_eq!(top_level_select_count(&pkg), 2);
}

/// ITS pubqlang/10 — three queries separated by `;` form a three-item
/// package. Trivia between items is accepted.
#[test]
fn test_three_selects_separated_by_semicolons() {
    let pkg = package(
        "SELECT Name FROM Products;\n\
         SELECT Code FROM Services;\n\
         SELECT Price FROM Prices",
    );
    assert_eq!(top_level_select_count(&pkg), 3);
}

/// ITS pubqlang/10 — a trailing `;` after the last query is allowed
/// (the `SEMICOLON?` at the end of the package production).
#[test]
fn test_trailing_semicolon_is_accepted() {
    let pkg = package("SELECT Name FROM Products;");
    assert_eq!(top_level_select_count(&pkg), 1);
}

/// ITS pubqlang/10 — two queries with a trailing `;` also form a two-item
/// package (trailing `;` does not introduce a third item).
#[test]
fn test_two_selects_with_trailing_semicolon() {
    let pkg = package("SELECT Name FROM Products; SELECT Code FROM Services;");
    assert_eq!(top_level_select_count(&pkg), 2);
}

/// Parser tolerance (not formal grammar): empty input is accepted as an
/// `SdblQueryPackage` node with no query items. See the rustdoc on
/// `grammar::sdbl::query_package` for the rationale — the IDE must
/// reason about empty documents without parse aborts.
#[test]
fn test_empty_input_yields_empty_package() {
    let pkg = package("");
    assert_eq!(top_level_select_count(&pkg), 0);
    assert_eq!(count_kind(&pkg, SyntaxKind::SDBL_DROP_QUERY), 0);
}

/// Parser tolerance (not formal grammar): whitespace-only input is
/// accepted the same way as empty input.
#[test]
fn test_whitespace_only_input_yields_empty_package() {
    let pkg = package("   \n\t  \n");
    assert_eq!(top_level_select_count(&pkg), 0);
}

/// Parser tolerance (not formal grammar): comment-only input is accepted
/// the same way as empty input.
#[test]
fn test_comment_only_input_yields_empty_package() {
    let pkg = package("// just a comment\n");
    assert_eq!(top_level_select_count(&pkg), 0);
}

// ----------------------------------------------------------------------------
// DROP statement — ITS pubqlang/10 + /12 vocabulary, pubqlang/51 h47 lifecycle
// ----------------------------------------------------------------------------

/// ITS pubqlang/12 — the DROP keyword followed by a single identifier
/// produces an SdblDropQuery item inside the package.
#[test]
fn test_drop_english_keyword() {
    let parse = parse_sdbl("DROP T");
    assert!(!parse.has_errors(), "parse errors: {:?}", parse.errors());
    let root = parse.syntax_node();
    let pkg = SdblQueryPackage::cast(root.clone()).expect("package");
    assert_eq!(count_kind(&pkg, SyntaxKind::SDBL_DROP_QUERY), 1);
    assert_eq!(top_level_select_count(&pkg), 0);
    assert_eq!(
        root.children().filter(|n| n.kind() == SyntaxKind::SDBL_DROP_QUERY).count(),
        1,
        "SdblDropQuery sits directly under SdblQueryPackage"
    );
}

/// ITS pubqlang/12 — the Russian spelling `УНИЧТОЖИТЬ` is equivalent to
/// `DROP`. A bilingual invocation with a Cyrillic identifier is accepted.
#[test]
fn test_drop_russian_keyword() {
    let pkg = package("УНИЧТОЖИТЬ ВТ");
    assert_eq!(count_kind(&pkg, SyntaxKind::SDBL_DROP_QUERY), 1);
    assert_eq!(top_level_select_count(&pkg), 0);
}

/// ITS pubqlang/10 — a DROP query may be the last item in a package and
/// may be followed by a trailing `;`.
#[test]
fn test_drop_with_trailing_semicolon() {
    let pkg = package("DROP T;");
    assert_eq!(count_kind(&pkg, SyntaxKind::SDBL_DROP_QUERY), 1);
}

/// ITS pubqlang/10 — a package may mix SELECT and DROP items, in any
/// order. Here SELECT ... then DROP.
#[test]
fn test_select_then_drop_same_package() {
    let pkg = package("SELECT Name FROM Products; DROP T");
    assert_eq!(top_level_select_count(&pkg), 1);
    assert_eq!(count_kind(&pkg, SyntaxKind::SDBL_DROP_QUERY), 1);
}

/// ITS pubqlang/10 — a package may mix DROP and SELECT items, in any
/// order. Here DROP then SELECT.
#[test]
fn test_drop_then_select_same_package() {
    let pkg = package("DROP T; SELECT Name FROM Products");
    assert_eq!(top_level_select_count(&pkg), 1);
    assert_eq!(count_kind(&pkg, SyntaxKind::SDBL_DROP_QUERY), 1);
}

// ----------------------------------------------------------------------------
// UNION / UNION ALL — ITS pubqlang/10 subquery := query (union-clause)*
// ----------------------------------------------------------------------------

/// ITS pubqlang/10 — a bare `UNION` between two queries produces one
/// `SdblUnionClause` child of the containing subquery.
#[test]
fn test_union_english() {
    let pkg = package("SELECT Name FROM Products UNION SELECT Name FROM Services");
    assert_eq!(top_level_select_count(&pkg), 1);
    let subquery = pkg.queries().next().and_then(|q| q.subquery()).expect("subquery");
    assert_eq!(subquery.union_clauses().count(), 1);
}

/// ITS pubqlang/10 + /12 — `UNION ALL` between two queries is still a
/// single `SdblUnionClause` (the `ALL` modifier is carried as an IDENT
/// token inside the node per § Preserved pre-refactor behaviours).
#[test]
fn test_union_all_english() {
    let pkg = package("SELECT Name FROM Products UNION ALL SELECT Name FROM Services");
    assert_eq!(top_level_select_count(&pkg), 1);
    let subquery = pkg.queries().next().and_then(|q| q.subquery()).expect("subquery");
    assert_eq!(subquery.union_clauses().count(), 1);
}

/// ITS pubqlang/10 — chained UNION clauses `A UNION B UNION ALL C` yield
/// two `SdblUnionClause` children on the outer subquery.
#[test]
fn test_union_chain_english() {
    let pkg = package("SELECT A FROM T1 UNION SELECT A FROM T2 UNION ALL SELECT A FROM T3");
    let subquery = pkg.queries().next().and_then(|q| q.subquery()).expect("subquery");
    assert_eq!(subquery.union_clauses().count(), 2);
}

/// ITS pubqlang/12 — the Russian spellings `ОБЪЕДИНИТЬ` / `ОБЪЕДИНИТЬ
/// ВСЕ` are equivalent to `UNION` / `UNION ALL`.
#[test]
fn test_union_russian() {
    let pkg = package("ВЫБРАТЬ Наименование ИЗ Товары ОБЪЕДИНИТЬ ВЫБРАТЬ Наименование ИЗ Услуги");
    let subquery = pkg.queries().next().and_then(|q| q.subquery()).expect("subquery");
    assert_eq!(subquery.union_clauses().count(), 1);
}

/// ITS pubqlang/12 — `ОБЪЕДИНИТЬ ВСЕ` (Russian `UNION ALL`).
#[test]
fn test_union_all_russian() {
    let pkg = package("ВЫБРАТЬ Код ИЗ Товары ОБЪЕДИНИТЬ ВСЕ ВЫБРАТЬ Код ИЗ Услуги");
    let subquery = pkg.queries().next().and_then(|q| q.subquery()).expect("subquery");
    assert_eq!(subquery.union_clauses().count(), 1);
}

/// § Preserved pre-refactor behaviour — `UNION` alone and `UNION ALL`
/// both produce the same `SdblUnionClause` node kind (no
/// `SdblUnionAllClause` variant). `SdblUnionClause::has_all()`
/// distinguishes the two post-parse.
#[test]
fn test_union_and_union_all_share_one_node_kind() {
    let with_all = package("SELECT A FROM T1 UNION ALL SELECT A FROM T2");
    let without_all = package("SELECT A FROM T1 UNION SELECT A FROM T2");

    let one_clause = with_all
        .queries()
        .next()
        .and_then(|q| q.subquery())
        .expect("subquery with ALL")
        .union_clauses()
        .next()
        .expect("union clause with ALL");
    assert!(one_clause.has_all(), "UNION ALL → has_all() = true");

    let other_clause = without_all
        .queries()
        .next()
        .and_then(|q| q.subquery())
        .expect("subquery without ALL")
        .union_clauses()
        .next()
        .expect("union clause without ALL");
    assert!(!other_clause.has_all(), "UNION → has_all() = false");

    // Same SyntaxKind for both shapes.
    assert_eq!(
        one_clause.syntax().kind(),
        other_clause.syntax().kind(),
        "UNION and UNION ALL must share one node kind"
    );
    assert_eq!(one_clause.syntax().kind(), SyntaxKind::SDBL_UNION_CLAUSE);
}

// ----------------------------------------------------------------------------
// Subquery boundary — ITS pubqlang/10 subquery scope vs package scope
// ----------------------------------------------------------------------------

/// ITS pubqlang/10 — a subquery inside parentheses in `FROM` is closed
/// by `)`, not by the outer package. One inner `SdblSubquery` sits
/// inside the outer `SdblSubquery`.
#[test]
fn test_subquery_in_from_is_closed_by_paren() {
    let pkg = package("SELECT * FROM (SELECT 1)");
    let subqueries: Vec<_> =
        pkg.syntax().descendants().filter(|n| n.kind() == SyntaxKind::SDBL_SUBQUERY).collect();
    assert_eq!(subqueries.len(), 2, "outer + inner subquery");
    assert_eq!(top_level_select_count(&pkg), 1);
}

/// ITS pubqlang/10 — a UNION inside a parenthesised subquery stays
/// inside that subquery; it does not surface as a UNION clause on the
/// outer subquery.
#[test]
fn test_union_inside_parenthesised_subquery_stays_inside() {
    let pkg = package("SELECT * FROM (SELECT 1 UNION SELECT 2) AS s");
    let root = pkg.syntax();
    let mut subqueries: Vec<_> =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_SUBQUERY).collect();
    assert_eq!(subqueries.len(), 2);
    subqueries.sort_by_key(|n| usize::from(n.text_range().start()));
    let outer = SdblSubquery::cast(subqueries.remove(0)).expect("outer");
    let inner = SdblSubquery::cast(subqueries.remove(0)).expect("inner");
    assert_eq!(outer.union_clauses().count(), 0, "outer subquery has no UNION clause");
    assert_eq!(inner.union_clauses().count(), 1, "inner subquery owns the UNION clause");
}

/// ITS pubqlang/10 — package boundary (`;`) is a hard terminator for a
/// subquery's UNION loop; the next statement is a separate package item.
#[test]
fn test_package_boundary_after_parenthesised_subquery() {
    let pkg = package("SELECT * FROM (SELECT 1); SELECT 2");
    assert_eq!(top_level_select_count(&pkg), 2);
    let subqueries =
        pkg.syntax().descendants().filter(|n| n.kind() == SyntaxKind::SDBL_SUBQUERY).count();
    // One outer subquery per top-level SELECT (2) plus one inner parenthesised
    // subquery = 3 SdblSubquery descendants in total.
    assert_eq!(subqueries, 3);
}

/// ITS pubqlang/10 — a UNION inside a subquery-in-WHERE does not escape
/// into the outer subquery. The outer subquery carries its own UNION
/// clause attached to itself.
#[test]
fn test_subquery_in_where_does_not_steal_outer_union() {
    let pkg = package(
        "SELECT Name FROM Products WHERE Id IN (SELECT Id FROM Archive) \
         UNION ALL SELECT Name FROM Services",
    );
    let outer = pkg.queries().next().and_then(|q| q.subquery()).expect("outer subquery");
    assert_eq!(outer.union_clauses().count(), 1);

    // The parenthesised subquery-in-WHERE has no UNION clause of its own.
    let mut subqueries: Vec<_> =
        pkg.syntax().descendants().filter(|n| n.kind() == SyntaxKind::SDBL_SUBQUERY).collect();
    assert_eq!(subqueries.len(), 2);
    subqueries.sort_by_key(|n| usize::from(n.text_range().start()));
    let inner = SdblSubquery::cast(subqueries.remove(1)).expect("inner subquery");
    assert_eq!(inner.union_clauses().count(), 0, "inner subquery in WHERE has no UNION clause");
}

// ----------------------------------------------------------------------------
// Bilingual integration — ITS pubqlang/12 bilingual vocabulary
// ----------------------------------------------------------------------------

/// ITS pubqlang/10 + /12 — a three-item package mixing Russian SELECT,
/// Russian UNION ALL, and a DROP statement, terminated by an English
/// SELECT.
#[test]
fn test_bilingual_three_item_package() {
    let pkg = package(
        "ВЫБРАТЬ Поле ИЗ Таблица1 ОБЪЕДИНИТЬ ВСЕ ВЫБРАТЬ Поле ИЗ Таблица2; \
         УНИЧТОЖИТЬ ВТ; \
         SELECT Field FROM Table3",
    );
    assert_eq!(top_level_select_count(&pkg), 2);
    assert_eq!(count_kind(&pkg, SyntaxKind::SDBL_DROP_QUERY), 1);

    let first_subquery = pkg.queries().next().and_then(|q| q.subquery()).expect("first subquery");
    assert_eq!(first_subquery.union_clauses().count(), 1);
}

// ----------------------------------------------------------------------------
// Entry-point wrapper (`select_query`) — NodeKind marker preservation
// ----------------------------------------------------------------------------

/// ITS pubqlang/10 — each top-level SELECT produces exactly one
/// `SdblSelectQuery` node; each such node owns exactly one
/// `SdblSubquery` child.
#[test]
fn test_select_query_wraps_exactly_one_subquery() {
    let pkg = package("SELECT Name FROM Products");
    let q = pkg.queries().next().expect("select query");
    assert_eq!(q.syntax().kind(), SyntaxKind::SDBL_SELECT_QUERY);
    let direct_subqueries: Vec<_> =
        q.syntax().children().filter(|n| n.kind() == SyntaxKind::SDBL_SUBQUERY).collect();
    assert_eq!(
        direct_subqueries.len(),
        1,
        "SdblSelectQuery has exactly one SdblSubquery direct child"
    );
}

/// ITS pubqlang/10 — parser tolerance: a DROP with a missing identifier
/// still produces a parse tree with exactly one `SdblDropQuery` node so
/// the IDE can reason about partially-typed input without losing the
/// containing package. The identifier-recovery path is marked
/// local-preserved in `drop_table_query` (see
/// `sdbl-clean-room-slice6.md` § Preserved pre-refactor behaviours);
/// the `SdblDropQuery` node carries an `Error` sub-node where the
/// identifier would have gone.
#[test]
fn test_drop_missing_identifier_is_recoverable() {
    let pkg = package_allow_errors("DROP ");
    assert_eq!(
        count_kind(&pkg, SyntaxKind::SDBL_DROP_QUERY),
        1,
        "SdblDropQuery node is still produced for IDE recovery"
    );
    let drop_node = pkg
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_DROP_QUERY)
        .expect("SdblDropQuery");
    let has_error_marker = drop_node.descendants().any(|n| n.kind() == SyntaxKind::ERROR);
    assert!(
        has_error_marker,
        "missing identifier triggers an Error sub-node per \
         sdbl-clean-room-slice6.md § Preserved pre-refactor behaviours"
    );
}
