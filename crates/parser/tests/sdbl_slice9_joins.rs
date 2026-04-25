//! SDBL Slice 9 — JOIN family clean-room acceptance tests.
//!
//! Spec-driven AST-shape acceptance tests authored against the
//! sources cited in `docs/legal/sdbl-clean-room-slice9.md`:
//!
//! - ITS pubqlang chapters 44/45/46/47/48 (via the local dump at
//!   `/home/itrous/src/tools_migration/its/dump/html/`):
//!   - chapter 44 — `ВНУТРЕННЕЕ СОЕДИНЕНИЕ` listing + standalone
//!     `СОЕДИНЕНИЕ` reference;
//!   - chapter 45 — `ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ` listing;
//!   - chapter 46 — `ПРАВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ` listing;
//!   - chapter 47 — `ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ` listing;
//!   - chapter 48 — chained / nested example listings.
//! - `docs/legal/sdbl-select-mini-spec.md` §JOIN clauses
//!   (lines 297–319) + §Recovery requirements item #6 (line 410).
//! - `docs/legal/sdbl-clean-room-slice2.md` for the bilingual
//!   EN/RU keyword vocabulary.
//! - The Slice 9 attestation cross-references three Slice-9
//!   parser-side AST-shape invariants (§Preserved invariants
//!   #1, #6, #7) that the §Invariant 7 group below pins.
//!
//! The 17-test floor is the v9-pinned minimum; per the v9 A2
//! overflow rule the file may grow if future C2-time evidence
//! adds genuinely-distinct A2 forms (none found at C2 author
//! time — chapters 44–47 contain only listings + chapter 44
//! standalone, so the bare LEFT/RIGHT/FULL/INNER no-OUTER forms
//! are pinned as Tier D local IDE-recovery allowances).

use parser::parse_sdbl;

/// Assert a clean parse: both `Parse::errors()` and the syntax
/// tree must be free of `SyntaxKind::ERROR` recovery nodes.
/// `Parser::error()` inserts ERROR into the tree without
/// populating `Parse::errors()`, so checking only `has_errors()`
/// would let recovered parses slip through (codex C0 review).
fn assert_clean_parse(parse: &syntax::Parse<syntax::SyntaxNode>, input: &str) {
    use syntax::SyntaxKind;
    assert!(
        !parse.has_errors(),
        "Expected clean parse for `{}`; got errors: {:#?}",
        input,
        parse.errors(),
    );
    let error_nodes: Vec<_> =
        parse.syntax_node().descendants().filter(|n| n.kind() == SyntaxKind::ERROR).collect();
    assert!(
        error_nodes.is_empty(),
        "Expected clean parse for `{}` — but tree contains {} ERROR recovery node(s): {:#?}",
        input,
        error_nodes.len(),
        error_nodes,
    );
}

fn first_join_clause(input: &str) -> syntax::SyntaxNode {
    use syntax::SyntaxKind;
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    parse
        .syntax_node()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_JOIN_CLAUSE)
        .expect("Tree must contain SdblJoinClause")
}

fn first_data_source(input: &str) -> syntax::ast::SdblDataSource {
    use syntax::ast::{AstNode, SdblQueryPackage};
    let parse = parse_sdbl(input);
    assert_clean_parse(&parse, input);
    let root = parse.syntax_node();
    let package = SdblQueryPackage::cast(root).expect("query package");
    let select_query = package.queries().next().expect("query");
    let main = select_query.subquery().and_then(|s| s.main_query()).expect("main query");
    let from = main.from_clause().expect("FROM clause");
    let ds = from.data_sources().next().expect("first data source");
    ds
}

// ----------------------------------------------------------------------------
// Tier A1 — ITS pubqlang chapter 44–47 RU canonical listings.
//
// Each chapter has exactly one listing for its join family:
//   ch.44 → ВНУТРЕННЕЕ СОЕДИНЕНИЕ
//   ch.45 → ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ
//   ch.46 → ПРАВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ
//   ch.47 → ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ
// ----------------------------------------------------------------------------

/// Tier A1 — ITS pubqlang chapter 44 listing.
#[test]
fn test_slice9_a1_inner_join_ru() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join = SdblJoinClause::cast(first_join_clause(
        "ВЫБРАТЬ * ИЗ Т1 ВНУТРЕННЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А",
    ))
    .expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Inner);
    assert!(join.data_source().is_some(), "JOIN must carry a joined SdblDataSource");
}

/// Tier A1 — ITS pubqlang chapter 45 listing.
#[test]
fn test_slice9_a1_left_outer_join_ru() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join = SdblJoinClause::cast(first_join_clause(
        "ВЫБРАТЬ * ИЗ Т1 ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А",
    ))
    .expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Left);
}

/// Tier A1 — ITS pubqlang chapter 46 listing.
#[test]
fn test_slice9_a1_right_outer_join_ru() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join = SdblJoinClause::cast(first_join_clause(
        "ВЫБРАТЬ * ИЗ Т1 ПРАВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А",
    ))
    .expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Right);
}

/// Tier A1 — ITS pubqlang chapter 47 listing.
#[test]
fn test_slice9_a1_full_outer_join_ru() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join = SdblJoinClause::cast(first_join_clause(
        "ВЫБРАТЬ * ИЗ Т1 ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А",
    ))
    .expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Full);
}

// ----------------------------------------------------------------------------
// Tier B — bilingual EN/RU keyword pairs per the lexer's Slice 2
// attestation. EN forms cover the same four join families.
// ----------------------------------------------------------------------------

/// Tier B — Slice 2 lexer EN INNER JOIN form.
#[test]
fn test_slice9_b_inner_join_en() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join =
        SdblJoinClause::cast(first_join_clause("SELECT * FROM T1 INNER JOIN T2 ON T1.A = T2.A"))
            .expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Inner);
}

/// Tier B — Slice 2 lexer EN LEFT OUTER JOIN form.
#[test]
fn test_slice9_b_left_outer_join_en() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join = SdblJoinClause::cast(first_join_clause(
        "SELECT * FROM T1 LEFT OUTER JOIN T2 ON T1.A = T2.A",
    ))
    .expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Left);
}

/// Tier B — Slice 2 lexer EN RIGHT OUTER JOIN form.
#[test]
fn test_slice9_b_right_outer_join_en() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join = SdblJoinClause::cast(first_join_clause(
        "SELECT * FROM T1 RIGHT OUTER JOIN T2 ON T1.A = T2.A",
    ))
    .expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Right);
}

/// Tier B — Slice 2 lexer EN FULL OUTER JOIN form.
#[test]
fn test_slice9_b_full_outer_join_en() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join = SdblJoinClause::cast(first_join_clause(
        "SELECT * FROM T1 FULL OUTER JOIN T2 ON T1.A = T2.A",
    ))
    .expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Full);
}

// ----------------------------------------------------------------------------
// Tier C — SELECT mini-spec §JOIN clauses behavioural note line 318:
// "bare JOIN without explicit type is accepted and treated structurally
// as a valid join form." Maps to implicit INNER per consumer-side
// invariant #1 default.
// ----------------------------------------------------------------------------

/// Tier C — mini-spec §JOIN clauses line 318 bare JOIN form (EN).
#[test]
fn test_slice9_c_bare_join_en() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join = SdblJoinClause::cast(first_join_clause("SELECT * FROM T1 JOIN T2 ON T1.A = T2.A"))
        .expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Inner);
}

// ----------------------------------------------------------------------------
// Tier D — local IDE-recovery allowances. Bare LEFT/RIGHT/FULL without
// OUTER/ВНЕШНЕЕ. No ITS prose-note attests OUTER optionality across
// chapters 45/46/47, so these are parser-accepted local allowances
// per the Slice 9 attestation §Preserved behaviours #2.
//
// (Bare СОЕДИНЕНИЕ standalone has chapter 44 prose attestation and is
// covered by the C0 Bucket-A regression-gate suite as Tier C/A1; it
// is not included here as a separate Tier D case to avoid double-
// counting.)
// ----------------------------------------------------------------------------

/// Tier D — bare ПОЛНОЕ without ВНЕШНЕЕ (no chapter 47 prose
/// attestation; preserved as local allowance).
#[test]
fn test_slice9_d_bare_full_ru_local_allowance() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join = SdblJoinClause::cast(first_join_clause(
        "ВЫБРАТЬ * ИЗ Т1 ПОЛНОЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А",
    ))
    .expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Full);
}

/// Tier D — bare ЛЕВОЕ without ВНЕШНЕЕ.
#[test]
fn test_slice9_d_bare_left_ru_local_allowance() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join = SdblJoinClause::cast(first_join_clause(
        "ВЫБРАТЬ * ИЗ Т1 ЛЕВОЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А",
    ))
    .expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Left);
}

/// Tier D — bare ПРАВОЕ without ВНЕШНЕЕ.
#[test]
fn test_slice9_d_bare_right_ru_local_allowance() {
    use syntax::ast::{AstNode, JoinType, SdblJoinClause};
    let join = SdblJoinClause::cast(first_join_clause(
        "ВЫБРАТЬ * ИЗ Т1 ПРАВОЕ СОЕДИНЕНИЕ Т2 ПО Т1.А = Т2.А",
    ))
    .expect("must cast to SdblJoinClause");
    assert_eq!(join.join_type(), JoinType::Right);
}

// ----------------------------------------------------------------------------
// Chapter 48 — chained / nested JOIN examples. The chapter pairs
// consecutive JOINs at the same source AND demonstrates a JOIN
// nested under another JOIN's data_source.
// ----------------------------------------------------------------------------

/// Chapter 48 — chained JOINs at the same data source. Both join
/// clauses attach as direct children of T1's SdblDataSource.
#[test]
fn test_slice9_chapter48_chained_joins_same_source() {
    let t1_source =
        first_data_source("SELECT * FROM T1 JOIN T2 ON T1.A = T2.A JOIN T3 ON T1.B = T3.B");
    let join_count = t1_source.join_clauses().count();
    assert_eq!(
        join_count, 2,
        "Both chained JOINs must attach as direct children of T1's SdblDataSource",
    );
}

/// Chapter 48 — nested JOIN inside JOIN'ed source. Outer LEFT JOIN
/// attaches to T1; the inner bare JOIN attaches to T2's data
/// source (NOT to T1's). Per consumer-side invariant #6, the inner
/// `join_type()` parent-tokens fallback walks up to T2's
/// SdblDataSource (which has no LEFT keyword), defaulting to
/// JoinType::Inner.
#[test]
fn test_slice9_chapter48_nested_join_inside_join() {
    use syntax::ast::JoinType;
    let t1_source =
        first_data_source("SELECT * FROM T1 LEFT JOIN T2 JOIN T3 ON T2.B = T3.B ON T1.A = T2.A");
    let outer_join = t1_source.join_clauses().next().expect("outer LEFT JOIN");
    assert_eq!(outer_join.join_type(), JoinType::Left);
    let t2_source = outer_join.data_source().expect("T2 source under outer JOIN");
    let inner_join = t2_source.join_clauses().next().expect("inner JOIN attached to T2");
    assert_eq!(
        inner_join.join_type(),
        JoinType::Inner,
        "Inner bare JOIN must default to Inner via parent-tokens fallback over T2's data source",
    );
}

// ----------------------------------------------------------------------------
// Invariant #7 — FROM-side `SdblDataSource::join_clauses()` reader.
// `crates/sdbl-hir/src/lower/from_clause.rs:36-72` reads the AST
// shape `subquery() Some && join_clauses().next() Some` to emit
// `JoinWithSubQuery`, and the analogous shape for virtual tables
// to emit `JoinWithVirtualTable`. The OR-in-ON pin matches the
// shape that `LogicalOrInJoin`
// (`crates/sdbl-hir/src/lower/join_clause.rs:188`) reads.
// ----------------------------------------------------------------------------

/// Invariant #7 — FROM-side subquery + JOIN AST-shape pin.
#[test]
fn test_slice9_inv7_subquery_join_shape() {
    let s_source =
        first_data_source("SELECT * FROM (SELECT * FROM T1) AS S LEFT JOIN T2 ON S.A = T2.A");
    assert!(
        s_source.subquery().is_some(),
        "Outer SdblDataSource must carry SdblSubquery as direct child",
    );
    assert!(
        s_source.join_clauses().next().is_some(),
        "Outer SdblDataSource must also carry the LEFT JOIN as direct child",
    );
}

/// Invariant #7 — FROM-side virtual-table + JOIN AST-shape pin.
#[test]
fn test_slice9_inv7_virtual_table_join_shape() {
    let r_source = first_data_source(
        "ВЫБРАТЬ * ИЗ РегистрНакопления.ТоварыНаСкладах.Остатки(&Дата) КАК Р \
         ЛЕВОЕ СОЕДИНЕНИЕ Т2 ПО Р.Х = Т2.Х",
    );
    assert!(
        r_source.table_ref().is_some(),
        "Outer SdblDataSource must carry SdblTableRef (virtual table) as direct child",
    );
    assert!(
        r_source.join_clauses().next().is_some(),
        "Outer SdblDataSource must also carry the JOIN as direct child",
    );
}

/// Invariant #7 — OR-in-ON parser-side AST-shape pin. The
/// SdblJoinClause direct ON-condition child is SdblLogicalOrExpr
/// (Slice 10a wrapper), which carries the OR.
#[test]
fn test_slice9_inv7_or_in_on_shape() {
    use syntax::SyntaxKind;
    let join = first_join_clause("SELECT * FROM T1 JOIN T2 ON T1.A = T2.A OR T1.B = T2.B");
    let direct_kinds: Vec<SyntaxKind> = join.children().map(|c| c.kind()).collect();
    assert!(
        direct_kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR),
        "ON-condition must wrap in SdblLogicalOrExpr direct child of SdblJoinClause. \
         Got: {:?}",
        direct_kinds,
    );
}
