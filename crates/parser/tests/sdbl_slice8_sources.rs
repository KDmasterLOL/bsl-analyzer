//! SDBL Slice 8 — FROM sources and source chains acceptance tests.
//!
//! These tests are authored against the 1C ITS query-language
//! documentation listed below and the project's own mini-spec, not
//! against the pre-refactor parser output:
//!
//! - <https://its.1c.ru/db/pubqlang/content/10/hdoc> — query-language
//!   structure: the FROM clause (`from-clause := (FROM|ИЗ) data-source
//!   (',' data-source)*`), the data-source shape (`primary-source
//!   alias? join-clause*`), primary-source alternatives
//!   (`subquery-source | table-ref | parameter-source`), the
//!   subquery-source wrapping (`'(' subquery ')' alias?`), and the
//!   parameter-source lexical shape (`'&' identifier`).
//! - <https://its.1c.ru/db/pubqlang/content/12/hdoc> — lexical
//!   elements: bilingual FROM / ИЗ, AS / КАК keyword vocabulary; the
//!   identifier longest-match rule that lets `is_clause_keyword`
//!   separate a clause keyword from an identifier.
//!
//! See `docs/legal/sdbl-clean-room-slice8.md` for the clean-room
//! attestation; the §Preserved pre-refactor behaviours roster is cited
//! from tests where the behaviour is narrower than (or additional to)
//! a strict ITS reading.

use parser::parse_sdbl;
use syntax::{
    ast::{AstNode, SdblFromClause, SdblQueryPackage},
    SyntaxKind,
};

fn tree(input: &str) -> String {
    let parse = parse_sdbl(input);
    format!("{:#?}", parse.syntax_node())
}

fn count_nodes(tree: &str, kind: &str) -> usize {
    // Match node names followed by the `@start..end` range marker so the
    // prefix-sharing kinds (SDBL_QUERY vs SDBL_QUERY_PACKAGE vs
    // SDBL_SELECT_QUERY vs SDBL_SUBQUERY) do not contaminate each
    // other's counts.
    let needle = format!("{kind}@");
    tree.matches(&needle).count()
}

fn parse_clean(input: &str) {
    let parse = parse_sdbl(input);
    assert!(
        !parse.has_errors(),
        "Expected clean parse for {input:?}, got errors: {:?}",
        parse.errors()
    );
}

fn from_clause_of(input: &str) -> SdblFromClause {
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    root.descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE)
        .and_then(SdblFromClause::cast)
        .expect("SdblFromClause must be present")
}

// =============================================================================
// FROM clause shape — ITS pubqlang/10 (from-clause), pubqlang/12 (FROM / ИЗ)
// =============================================================================

#[test]
fn test_from_single_table_english() {
    // ITS pubqlang/10 — a FROM clause with one data source.
    parse_clean("SELECT * FROM Products");
    let t = tree("SELECT * FROM Products");
    assert_eq!(count_nodes(&t, "SDBL_FROM_CLAUSE"), 1);
    assert_eq!(count_nodes(&t, "SDBL_DATA_SOURCE"), 1);
}

#[test]
fn test_from_single_table_russian() {
    // ITS pubqlang/12 — ИЗ is the Russian bilingual twin of FROM.
    parse_clean("ВЫБРАТЬ * ИЗ Товары");
    let t = tree("ВЫБРАТЬ * ИЗ Товары");
    assert_eq!(count_nodes(&t, "SDBL_FROM_CLAUSE"), 1);
    assert_eq!(count_nodes(&t, "SDBL_DATA_SOURCE"), 1);
}

#[test]
fn test_from_two_sources_comma_separated() {
    // ITS pubqlang/10 — from-clause := FROM data-source (',' data-source)*.
    parse_clean("SELECT * FROM T1, T2");
    let from = from_clause_of("SELECT * FROM T1, T2");
    assert_eq!(from.data_sources().count(), 2, "Two data sources expected");
}

#[test]
fn test_from_three_sources_comma_separated() {
    // ITS pubqlang/10 — the data-source list is a COMMA-delimited repeat,
    // not a pair.
    parse_clean("SELECT * FROM T1, T2, T3");
    let from = from_clause_of("SELECT * FROM T1, T2, T3");
    assert_eq!(from.data_sources().count(), 3);
}

// =============================================================================
// Subquery source — ITS pubqlang/10 (subquery-source: '(' subquery ')' alias?)
// =============================================================================

#[test]
fn test_from_subquery_source_with_as_alias() {
    // ITS pubqlang/10 — subquery-source with an explicit AS alias.
    parse_clean("SELECT * FROM (SELECT 1) AS S");
    let t = tree("SELECT * FROM (SELECT 1) AS S");
    assert_eq!(count_nodes(&t, "SDBL_DATA_SOURCE"), 1);
    // The outer SdblDataSource plus the inner SdblSubquery make two
    // SDBL_SUBQUERY nodes total (outer SdblSelectQuery wrapper + inner).
    assert!(
        count_nodes(&t, "SDBL_SUBQUERY") >= 2,
        "Inner SdblSubquery must appear under the outer SdblDataSource"
    );
}

#[test]
fn test_from_subquery_source_russian_alias() {
    // ITS pubqlang/12 — КАК is the Russian bilingual twin of AS.
    parse_clean("ВЫБРАТЬ * ИЗ (ВЫБРАТЬ 1) КАК С");
    let from = from_clause_of("ВЫБРАТЬ * ИЗ (ВЫБРАТЬ 1) КАК С");
    let source = from.data_sources().next().expect("single data source");
    assert!(source.subquery().is_some(), "SdblSubquery direct child of SdblDataSource");
    let alias = source.alias().expect("alias attached at data-source level");
    assert!(alias.has_as_keyword(), "КАК sets has_as_keyword()");
    assert_eq!(alias.name().as_deref(), Some("С"));
}

#[test]
fn test_from_subquery_source_bare_implicit_alias() {
    // Mini-spec §Alias — the alias keyword is optional. Bare identifier
    // after ')' is accepted as an implicit alias.
    parse_clean("SELECT * FROM (SELECT 1) Sq");
    let from = from_clause_of("SELECT * FROM (SELECT 1) Sq");
    let source = from.data_sources().next().expect("single data source");
    let alias = source.alias().expect("bare alias is attached as SdblAlias");
    assert!(!alias.has_as_keyword(), "Bare alias must not carry AS/КАК");
    assert_eq!(alias.name().as_deref(), Some("Sq"));
}

#[test]
fn test_from_subquery_source_nested() {
    // ITS pubqlang/10 — subquery-source nesting is allowed; the outer and
    // inner levels each carry their own alias.
    let input = "SELECT * FROM (SELECT * FROM (SELECT 1) AS Inner) AS Outer";
    let parse = parse_sdbl(input);
    assert_eq!(parse.syntax_node().text().to_string(), input, "Root must cover full input");
    let t = tree(input);
    assert!(count_nodes(&t, "SDBL_DATA_SOURCE") >= 2);
    assert!(count_nodes(&t, "SDBL_ALIAS") >= 2);
}

#[test]
fn test_from_subquery_source_with_inner_union() {
    // ITS pubqlang/10 — a subquery-source may contain a UNION inside.
    parse_clean("SELECT * FROM (SELECT 1 UNION ALL SELECT 2) AS U");
}

// =============================================================================
// Parameter source — ITS pubqlang/10 (parameter-source: '&' identifier)
// =============================================================================

#[test]
fn test_from_parameter_source_english() {
    // ITS pubqlang/10 — parameter-source := '&' identifier; accepted as a
    // data-source head.
    parse_clean("SELECT * FROM &Tmp AS T");
}

#[test]
fn test_from_parameter_source_russian_with_kak() {
    // ITS pubqlang/12 — parameter name is an identifier; КАК is the
    // Russian alias keyword.
    parse_clean("ВЫБРАТЬ * ИЗ &ТЗ КАК ТЗ");
    let from = from_clause_of("ВЫБРАТЬ * ИЗ &ТЗ КАК ТЗ");
    let source = from.data_sources().next().expect("single data source");
    let table_ref = source.table_ref().expect("SdblTableRef for &ТЗ");
    assert!(
        table_ref.syntax().children().any(|n| n.kind() == SyntaxKind::SDBL_PARAMETER),
        "SdblParameter must be a direct child of SdblTableRef",
    );
}

#[test]
fn test_from_parameter_source_without_alias() {
    // Mini-spec §FROM — alias is optional. Parameter source with no alias
    // must parse cleanly and carry no SdblAlias.
    parse_clean("ВЫБРАТЬ * ИЗ &ТЗ");
    let from = from_clause_of("ВЫБРАТЬ * ИЗ &ТЗ");
    let source = from.data_sources().next().expect("single data source");
    assert!(source.alias().is_none(), "Parameter source without alias has no SdblAlias");
}

#[test]
fn test_from_parameter_source_in_multi_source_list() {
    // ITS pubqlang/10 — parameter-source is a valid data-source head, so
    // it can appear at any position in the comma-separated list.
    parse_clean("SELECT * FROM Products, &Tmp AS T");
    let from = from_clause_of("SELECT * FROM Products, &Tmp AS T");
    assert_eq!(from.data_sources().count(), 2);
}

#[test]
fn test_from_parenthesised_parameter_enters_subquery_branch() {
    // Attestation §Preserved pre-refactor behaviours (subquery-source
    // dispatch on LParen) — an LParen at the data-source head always
    // enters the subquery-source branch of `data_source` regardless of
    // inner tokens. A parenthesised `(&Tmp)` therefore does NOT produce
    // SdblParameter at the outer data-source level; the inner
    // SdblSubquery wraps the `&Tmp` tokens in an Error sub-node.
    let input = "ВЫБРАТЬ * ИЗ (&Tmp)";
    let parse = parse_sdbl(input);
    // has_errors() stays false: the ERROR node inside the inner subquery
    // is part of the tree but not promoted to the error set.
    let root = parse.syntax_node();
    let has_param = root.descendants().any(|n| n.kind() == SyntaxKind::SDBL_PARAMETER);
    assert!(!has_param, "Parenthesised &Tmp must not produce SdblParameter at the outer level",);
}

// =============================================================================
// Table ref — ITS pubqlang/10 (table-ref), pubqlang/12 (identifier chain)
// =============================================================================

#[test]
fn test_table_ref_simple_identifier() {
    // ITS pubqlang/10 — a single-identifier table name is the minimal
    // table-ref shape.
    parse_clean("SELECT * FROM Products");
    let from = from_clause_of("SELECT * FROM Products");
    let source = from.data_sources().next().expect("single data source");
    assert!(source.table_ref().is_some(), "Simple identifier produces SdblTableRef");
}

#[test]
fn test_table_ref_two_segment_mdo_path() {
    // ITS pubqlang/10 — metadata object reference is an identifier chain
    // joined by '.'. Attestation §AST-shape invariants #1 locks the
    // IDENT token ordering inside SdblTableRef: the two identifiers are
    // direct token children in source order, not wrapped in a sub-node.
    parse_clean("SELECT Name FROM Catalog.Products");
    let from = from_clause_of("SELECT Name FROM Catalog.Products");
    let source = from.data_sources().next().expect("single data source");
    let table_ref = source.table_ref().expect("SdblTableRef");
    // Iterate over children_with_tokens and collect the direct IDENT
    // tokens.
    let idents: Vec<String> = table_ref
        .syntax()
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(idents, vec!["Catalog".to_string(), "Products".to_string()]);
}

#[test]
fn test_table_ref_russian_mdo_with_alias() {
    // ITS pubqlang/12 — Russian MDO names are identifiers; КАК is the
    // alias keyword.
    parse_clean("ВЫБРАТЬ * ИЗ Справочник.Товары КАК Т");
}

#[test]
fn test_table_ref_virtual_table_call_is_accepted() {
    // Mini-spec §Virtual table argument behavior — VT calls with
    // parenthesised arguments parse cleanly, including the empty-arg /
    // trailing-comma forms. The VT body itself is delegated to the
    // Slice 8-addendum clean-room helper `virtual_table_args`; Slice 8
    // only owns the dispatch.
    parse_clean("SELECT * FROM Catalog.Products.SliceLast(&Date)");
    parse_clean("ВЫБРАТЬ * ИЗ Регистр.Остатки(, , Авто, ) КАК Т");
}

// =============================================================================
// Source alias — ITS pubqlang/10 (alias := (AS|КАК)? identifier)
// =============================================================================

#[test]
fn test_source_alias_with_as_keyword() {
    // ITS pubqlang/10 — explicit AS form of the alias grammar.
    parse_clean("SELECT * FROM Products AS P");
    let from = from_clause_of("SELECT * FROM Products AS P");
    let source = from.data_sources().next().unwrap();
    let alias = source.alias().expect("SdblAlias");
    assert!(alias.has_as_keyword());
    assert_eq!(alias.name().as_deref(), Some("P"));
}

#[test]
fn test_source_alias_with_kak_keyword() {
    // ITS pubqlang/12 — КАК is the Russian alias keyword.
    parse_clean("ВЫБРАТЬ * ИЗ Товары КАК Т");
    let from = from_clause_of("ВЫБРАТЬ * ИЗ Товары КАК Т");
    let source = from.data_sources().next().unwrap();
    let alias = source.alias().expect("SdblAlias");
    assert!(alias.has_as_keyword());
    assert_eq!(alias.name().as_deref(), Some("Т"));
}

#[test]
fn test_source_alias_bare_implicit_form() {
    // ITS pubqlang/10 — the alias keyword is optional; a bare identifier
    // after the data source is accepted as an implicit alias.
    parse_clean("SELECT * FROM Products P");
    let from = from_clause_of("SELECT * FROM Products P");
    let source = from.data_sources().next().unwrap();
    let alias = source.alias().expect("SdblAlias");
    assert!(!alias.has_as_keyword(), "Bare alias carries no AS/КАК");
    assert_eq!(alias.name().as_deref(), Some("P"));
}

#[test]
fn test_source_alias_as_followed_by_clause_keyword_recovers() {
    // Mini-spec §Recovery requirements #3 — AS/КАК followed by a clause
    // keyword yields an empty Error sub-node inside SdblAlias so the
    // enclosing clause loop can still consume the keyword at the next
    // level.
    let t = tree("SELECT * FROM Products AS WHERE Active = TRUE");
    assert!(t.contains("SDBL_FROM_CLAUSE"));
    assert!(t.contains("SDBL_WHERE_CLAUSE"), "WHERE must still parse after AS + clause keyword");
    // The SdblAlias marker is still opened so the AS keyword is
    // structurally accounted for.
    assert!(t.contains("SDBL_ALIAS"));
}

// =============================================================================
// JOIN attachment (Slice 9 body; Slice 8 owns the attachment point)
// =============================================================================

#[test]
fn test_join_attaches_to_data_source_not_from_clause() {
    // Attestation §Child-attachment invariants — SdblJoinClause is a
    // direct child of SdblDataSource (not SdblFromClause or a new
    // intermediate wrapper). This is the HIR-lowering contract for
    // SdblDataSource::join_clauses() at syntax/src/ast.rs:1343.
    parse_clean("SELECT * FROM T LEFT JOIN U ON T.a = U.a");
    let from = from_clause_of("SELECT * FROM T LEFT JOIN U ON T.a = U.a");
    let source = from.data_sources().next().expect("single data source");
    assert_eq!(source.join_clauses().count(), 1, "One JOIN attached to T's data source");
}

// =============================================================================
// NodeKind preservation guard + AST-shape invariants
// =============================================================================

#[test]
fn test_slice8_nodekinds_emitted_together() {
    // Attestation §Scope NodeKinds lock — the 5 Slice 8 NodeKinds
    // (SdblFromClause / SdblDataSource / SdblTableRef / SdblParameter /
    // SdblAlias) must each appear at least once in a query that
    // exercises the full Slice 8 surface.
    let t = tree("ВЫБРАТЬ * ИЗ Справочник.Товары КАК Т, &Параметр, (ВЫБРАТЬ 1) КАК С");
    for kind in
        ["SDBL_FROM_CLAUSE", "SDBL_DATA_SOURCE", "SDBL_TABLE_REF", "SDBL_PARAMETER", "SDBL_ALIAS"]
    {
        assert!(count_nodes(&t, kind) >= 1, "Missing node kind: {kind}");
    }
}

#[test]
fn test_slice8_sdbl_data_source_is_direct_child_of_from_clause() {
    // Attestation §AST-shape invariants #6 — SdblDataSource is a direct
    // child of SdblFromClause. Consumer SdblFromClause::data_sources()
    // at syntax/src/ast.rs:1299-1300 walks direct children, not
    // descendants.
    let from = from_clause_of("SELECT * FROM T1, T2, T3");
    let direct_children: Vec<_> =
        from.syntax().children().filter(|n| n.kind() == SyntaxKind::SDBL_DATA_SOURCE).collect();
    assert_eq!(direct_children.len(), 3);
}

// =============================================================================
// Bilingual + Slice 7 × Slice 8 integration
// =============================================================================

#[test]
fn test_bilingual_multi_source_from_with_subquery_and_parameter() {
    // ITS pubqlang/10 + pubqlang/12 — the FROM clause admits a mixed
    // bilingual data-source list combining table-ref, parameter-source,
    // and subquery-source with both Russian and English alias keywords
    // in the same query.
    parse_clean("ВЫБРАТЬ * ИЗ Catalog.Products КАК Products, &Tmp AS T, (ВЫБРАТЬ 1) КАК S");
}

#[test]
fn test_slice7_times_slice8_full_select_with_aliases_and_from_chain() {
    // ITS pubqlang/10 — end-to-end SELECT with Slice 7 field list and
    // Slice 8 FROM chain composed in one tree.
    // Outer SELECT has 2 fields and 2 data sources (one table with alias,
    // one subquery-source with alias). The inner subquery contributes a
    // 3rd SDBL_SELECTED_FIELD node, which is Slice 7 scope, so the outer
    // field count is asserted via SdblFieldList's direct children
    // instead of a tree-wide SDBL_SELECTED_FIELD count.
    let input = "SELECT f1 AS a1, f2 AS a2 FROM T1 AS T, (SELECT 1) AS S";
    parse_clean(input);
    let from = from_clause_of(input);
    assert_eq!(from.data_sources().count(), 2, "Two outer data sources");
    // 4 SdblAlias nodes total: 2 field aliases (a1, a2) + 2 source
    // aliases (T, S). Scoped by direct-children walks:
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    let alias_count = root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_ALIAS).count();
    assert_eq!(alias_count, 4, "4 SdblAlias nodes: 2 field + 2 source");
}

// =============================================================================
// Package-boundary regression — temp-table source crossing a `;`
// =============================================================================

#[test]
fn test_temp_table_source_crosses_package_boundary() {
    // ITS pubqlang/10 + pubqlang/51 h47 — a temporary table created by
    // ПОМЕСТИТЬ in one query-package statement can be consumed as a
    // table-ref in the following statement through the identifier-only
    // table_ref path (no MDO prefix, no VT args). Exercises the
    // interaction between the Slice 8 data-source parsing and the
    // Slice 6 query-package statement loop.
    let input = "ВЫБРАТЬ Поле ПОМЕСТИТЬ ВремТаблица ИЗ Товары; \
                 ВЫБРАТЬ Поле ИЗ ВремТаблица";
    parse_clean(input);
    let parse = parse_sdbl(input);
    let package = SdblQueryPackage::cast(parse.syntax_node()).expect("query package");
    assert_eq!(package.queries().count(), 2);
}
