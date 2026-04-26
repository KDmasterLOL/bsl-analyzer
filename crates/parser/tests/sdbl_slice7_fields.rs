//! SDBL Slice 7 — SELECT prefix (field list, aliases, INTO) acceptance tests.
//!
//! These tests are authored against the 1C ITS query-language documentation
//! listed below and the project's own mini-spec, not against the pre-refactor
//! parser output:
//!
//! - <https://its.1c.ru/db/pubqlang/content/10/hdoc> — query-language
//!   structure: selected field list, asterisk field, alias grammar, INTO
//!   destination.
//! - <https://its.1c.ru/db/pubqlang/content/12/hdoc> — lexical elements:
//!   bilingual SELECT / ВЫБРАТЬ, INTO / ПОМЕСТИТЬ, AS / КАК, FROM / ИЗ
//!   vocabulary.
//! - <https://its.1c.ru/db/pubqlang/content/51/hdoc/h47> — temporary-table
//!   lifecycle: INTO names a temporary table by a single identifier.
//!
//! See `docs/legal/sdbl-clean-room-slice7.md` for the clean-room attestation
//! (§Preserved pre-refactor behaviours is cited from tests where the
//! behaviour is narrower than a strict ITS reading would produce).

use parser::parse_sdbl;
use syntax::SyntaxKind;

fn tree(input: &str) -> String {
    let parse = parse_sdbl(input);
    format!("{:#?}", parse.syntax_node())
}

fn count_nodes(tree: &str, kind: &str) -> usize {
    // Match node names followed by the `@start..end` range marker so the
    // prefix-sharing kinds (SDBL_QUERY vs SDBL_QUERY_PACKAGE vs
    // SDBL_SELECT_QUERY vs SDBL_SUBQUERY vs SDBL_DROP_QUERY) do not
    // contaminate each other's counts.
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

// =============================================================================
// Selected field list — ITS pubqlang/10 (selectedFields)
// =============================================================================

#[test]
fn test_single_field() {
    // ITS pubqlang/10 — a selected-field list of one field is the minimal
    // well-formed SELECT prefix.
    parse_clean("SELECT Name FROM T");
    let t = tree("SELECT Name FROM T");
    assert_eq!(count_nodes(&t, "SDBL_SELECTED_FIELD"), 1);
    assert_eq!(count_nodes(&t, "SDBL_FIELD_LIST"), 1);
}

#[test]
fn test_two_fields() {
    // ITS pubqlang/10 — selectedField COMMA selectedField.
    parse_clean("SELECT Name, Code FROM Products");
    let t = tree("SELECT Name, Code FROM Products");
    assert_eq!(count_nodes(&t, "SDBL_SELECTED_FIELD"), 2);
}

#[test]
fn test_four_fields() {
    // ITS pubqlang/10 — the selectedField list repeats via COMMA.
    parse_clean("SELECT A, B, C, D FROM T");
    let t = tree("SELECT A, B, C, D FROM T");
    assert_eq!(count_nodes(&t, "SDBL_SELECTED_FIELD"), 4);
}

#[test]
fn test_trailing_comma_recoverable() {
    // Parser tolerance (§Preserved pre-refactor behaviours item 4): a
    // trailing comma followed by a clause keyword must not abort the
    // parse — the FROM clause must still be reached.
    let t = tree("SELECT Name, FROM Products");
    assert!(
        t.contains("SDBL_FROM_CLAUSE"),
        "FROM clause must parse after trailing comma. Tree: {}",
        t
    );
}

// =============================================================================
// Asterisk field — ITS pubqlang/10 (asteriskField)
// =============================================================================

#[test]
fn test_bare_asterisk() {
    // ITS pubqlang/10 — `*` names all fields of the enclosing scope.
    parse_clean("SELECT * FROM T");
    let t = tree("SELECT * FROM T");
    assert_eq!(count_nodes(&t, "SDBL_ASTERISK_FIELD"), 1);
}

#[test]
fn test_qualified_asterisk_english() {
    // ITS pubqlang/10 — `Ident . *` names all fields of the named table.
    parse_clean("SELECT Products.* FROM Products");
    let t = tree("SELECT Products.* FROM Products");
    assert_eq!(count_nodes(&t, "SDBL_ASTERISK_FIELD"), 1);
}

#[test]
fn test_qualified_asterisk_russian() {
    // ITS pubqlang/12 — identifiers accept Cyrillic characters, and
    // Slice 2 keyword lookup is bilingual. A Russian-named table with
    // `.*` is the same production as its English counterpart.
    parse_clean("ВЫБРАТЬ Товары.* ИЗ Товары");
    let t = tree("ВЫБРАТЬ Товары.* ИЗ Товары");
    assert_eq!(count_nodes(&t, "SDBL_ASTERISK_FIELD"), 1);
}

#[test]
fn test_multi_segment_asterisk_not_detected_by_predicate() {
    // §Preserved pre-refactor behaviours item 3: the `is_asterisk_start`
    // predicate looks exactly one `Ident Dot Star` ahead; it does not see
    // a multi-segment `Ident Dot Ident Dot Star` prefix. A multi-segment
    // qualified asterisk therefore does not produce an
    // SDBL_ASTERISK_FIELD node via the predicate entry path.
    let t = tree("SELECT Catalog.Products.* FROM Products");
    assert_eq!(
        count_nodes(&t, "SDBL_ASTERISK_FIELD"),
        0,
        "Multi-segment Catalog.Products.* must not enter via is_asterisk_start. Tree: {}",
        t
    );
}

#[test]
fn test_temp_table_asterisk_not_parsed_as_asterisk_field() {
    // §Preserved pre-refactor behaviours item 3: `#Temp.*` is not detected
    // by `is_asterisk_start` because the lookahead matches only
    // `Ident Dot Star`, not `Hash Ident Dot Star`; `Hash` is also not in
    // `is_expression_start`, so the field list cannot start on this input.
    let t = tree("SELECT #Temp.* FROM #Temp");
    assert_eq!(
        count_nodes(&t, "SDBL_ASTERISK_FIELD"),
        0,
        "Temp-table-prefixed #Temp.* must not be detected as an asterisk field. Tree: {}",
        t
    );
}

#[test]
fn test_asterisk_with_regular_field() {
    // ITS pubqlang/10 — a selected-field list may mix asterisk fields and
    // ordinary expression fields.
    parse_clean("SELECT T.*, Name FROM T");
    let t = tree("SELECT T.*, Name FROM T");
    assert_eq!(count_nodes(&t, "SDBL_ASTERISK_FIELD"), 1);
    assert_eq!(count_nodes(&t, "SDBL_SELECTED_FIELD"), 2);
}

// =============================================================================
// Alias — ITS pubqlang/10 (alias)
// =============================================================================

#[test]
fn test_alias_with_as() {
    // ITS pubqlang/10 — alias: (AS | КАК)? identifier. Explicit English AS.
    parse_clean("SELECT Name AS ProductName FROM T");
    let t = tree("SELECT Name AS ProductName FROM T");
    assert_eq!(count_nodes(&t, "SDBL_ALIAS"), 1);
}

#[test]
fn test_alias_with_kak() {
    // ITS pubqlang/12 — Russian КАК is equivalent to English AS.
    parse_clean("ВЫБРАТЬ Имя КАК Имя2 ИЗ Товары");
    let t = tree("ВЫБРАТЬ Имя КАК Имя2 ИЗ Товары");
    assert_eq!(count_nodes(&t, "SDBL_ALIAS"), 1);
}

#[test]
fn test_alias_bare_identifier() {
    // ITS pubqlang/10 §Alias + mini-spec §Alias — the AS / КАК keyword is
    // structurally optional; a bare identifier after an expression is
    // accepted as an implicit alias. Strict-syntax semantics layer on top
    // of this via AssignAliasFieldsInQuery.
    parse_clean("SELECT Name ProductName FROM T");
    let t = tree("SELECT Name ProductName FROM T");
    assert_eq!(count_nodes(&t, "SDBL_ALIAS"), 1);
}

#[test]
fn test_alias_case_insensitive_as() {
    // ITS pubqlang/12 §Case-insensitive keywords — the AS keyword is
    // recognised regardless of case.
    parse_clean("SELECT Name as Alias1 FROM T");
    parse_clean("SELECT Name As Alias2 FROM T");
    parse_clean("SELECT Name AS Alias3 FROM T");
}

#[test]
fn test_alias_clause_keyword_guard() {
    // Parser tolerance: in `SELECT x FROM T`, the bare `FROM` keyword must
    // not be captured as the implicit alias of `x`. The
    // `is_clause_keyword` guard in the alias-dispatch path of
    // `selected_field` prevents this capture.
    let t = tree("SELECT x FROM T");
    assert!(t.contains("SDBL_FROM_CLAUSE"), "FROM clause must parse. Tree: {}", t);
    assert_eq!(
        count_nodes(&t, "SDBL_ALIAS"),
        0,
        "Clause keyword FROM must not be captured as alias. Tree: {}",
        t
    );
}

#[test]
fn test_alias_as_without_name_recoverable() {
    // ITS pubqlang/10 + mini-spec §Alias — if AS / КАК is present but the
    // alias name is missing (next token is a clause keyword), the parser
    // emits an empty ERROR sub-node inside SDBL_ALIAS and continues so
    // the rest of the query still parses.
    let t = tree("SELECT x AS FROM T");
    assert!(t.contains("SDBL_ALIAS"), "Alias node expected. Tree: {}", t);
    assert!(t.contains("ERROR"), "Empty alias name expected as ERROR. Tree: {}", t);
    assert!(t.contains("SDBL_FROM_CLAUSE"), "FROM must still parse. Tree: {}", t);
}

#[test]
fn test_multi_field_mixed_aliases() {
    // ITS pubqlang/10 — each selected field carries its own optional alias;
    // mixing AS and bare-identifier forms across fields must parse cleanly.
    parse_clean("SELECT Name AS N, Code C FROM Products");
    let t = tree("SELECT Name AS N, Code C FROM Products");
    assert_eq!(count_nodes(&t, "SDBL_SELECTED_FIELD"), 2);
    assert_eq!(count_nodes(&t, "SDBL_ALIAS"), 2);
}

// =============================================================================
// INTO clause — ITS pubqlang/10 + pubqlang/51 h47
// =============================================================================

#[test]
fn test_into_english_simple() {
    // ITS pubqlang/10 + /51 h47 — INTO identifier names a temporary-table
    // destination; the identifier is wrapped in SDBL_TEMP_TABLE_NAME.
    parse_clean("SELECT Name INTO TempNames FROM T");
    let t = tree("SELECT Name INTO TempNames FROM T");
    assert_eq!(count_nodes(&t, "SDBL_INTO_CLAUSE"), 1);
    assert_eq!(count_nodes(&t, "SDBL_TEMP_TABLE_NAME"), 1);
}

#[test]
fn test_into_russian_pomestit() {
    // ITS pubqlang/12 — ПОМЕСТИТЬ is the Russian INTO keyword. The
    // SDBL_TEMP_TABLE_NAME wrapper is identical to the English form.
    parse_clean("ВЫБРАТЬ Имя ПОМЕСТИТЬ ВремТаблица ИЗ Товары");
    let t = tree("ВЫБРАТЬ Имя ПОМЕСТИТЬ ВремТаблица ИЗ Товары");
    assert_eq!(count_nodes(&t, "SDBL_INTO_CLAUSE"), 1);
    assert_eq!(count_nodes(&t, "SDBL_TEMP_TABLE_NAME"), 1);
}

#[test]
fn test_into_before_from_ordering() {
    // ITS pubqlang/10 — INTO appears after the field list and before FROM.
    // Both clauses must parse and INTO must precede FROM in the tree.
    parse_clean("SELECT Name INTO TempNames FROM Products");
    let t = tree("SELECT Name INTO TempNames FROM Products");
    let into_pos = t.find("SDBL_INTO_CLAUSE").expect("INTO must parse");
    let from_pos = t.find("SDBL_FROM_CLAUSE").expect("FROM must parse");
    assert!(into_pos < from_pos, "INTO must appear before FROM in the tree. Tree: {}", t);
}

#[test]
fn test_into_semicolon_recoverable() {
    // §Preserved pre-refactor behaviours item 5: INTO followed by a
    // semicolon (no identifier) calls `p.error()`, which creates an
    // ERROR marker that consumes the next token (the semicolon). The
    // SDBL_INTO_CLAUSE node still exists but carries no
    // SDBL_TEMP_TABLE_NAME child — the load-bearing invariant for
    // sdbl-hir temp-table resolution. A tighter recovery that keeps
    // the semicolon as a package boundary is deferred to Slice 12.
    let t = tree("SELECT Name INTO ;");
    assert!(t.contains("SDBL_INTO_CLAUSE"), "INTO clause still emitted. Tree: {}", t);
    assert_eq!(
        count_nodes(&t, "SDBL_TEMP_TABLE_NAME"),
        0,
        "Missing-identifier path must not emit SDBL_TEMP_TABLE_NAME. Tree: {}",
        t
    );
    assert!(t.contains("ERROR@"), "Missing-identifier path must emit an ERROR marker. Tree: {}", t);
}

// =============================================================================
// Query wrapper — ITS pubqlang/10 (query)
// =============================================================================

#[test]
fn test_query_wrapper_minimal_shape() {
    // ITS pubqlang/10 — a single query is wrapped in SdblQuery with at
    // least one SdblSelectedField inside SdblFieldList. `SELECT 1` is the
    // minimal content (the expression layer decodes `1` to a numeric
    // literal — that decoding is Slice 10 Tier B and is not asserted here).
    parse_clean("SELECT 1");
    let t = tree("SELECT 1");
    assert_eq!(count_nodes(&t, "SDBL_QUERY"), 1);
    assert_eq!(count_nodes(&t, "SDBL_FIELD_LIST"), 1);
    assert_eq!(count_nodes(&t, "SDBL_SELECTED_FIELD"), 1);
}

#[test]
fn test_query_missing_select_keyword_recoverable() {
    // Parser tolerance — an input that does not start with SELECT / ВЫБРАТЬ
    // must still produce an SDBL_QUERY node with an ERROR marker so the
    // IDE can show the incomplete state. `p.error()` in `query()` creates
    // the ERROR marker in the tree (it does not populate
    // `parse.errors()` — that list is reserved for the lexer / list-level
    // path).
    let t = tree("FROM Products");
    assert_eq!(
        count_nodes(&t, "SDBL_QUERY"),
        1,
        "SDBL_QUERY must exist even without SELECT keyword. Tree: {}",
        t
    );
    assert!(t.contains("ERROR"), "Missing SELECT must produce an ERROR marker. Tree: {}", t);
}

// =============================================================================
// NodeKind preservation — AssignAliasFieldsInQuery HIR gate
// =============================================================================

#[test]
fn test_nodekind_identity_selected_field_with_alias() {
    // §Preserved pre-refactor behaviours item 6: the
    // AssignAliasFieldsInQuery diagnostic consumes the HIR diagnostic
    // AliasWithoutAsKeyword emitted from sdbl-hir/src/lower/diagnostics.rs,
    // which walks the AST via sdbl-hir/src/lower/select_fields.rs. NodeKind
    // identity for SdblSelectedField + SdblAlias is the load-bearing
    // invariant for that gate; Slice 7 must emit the same node kinds for
    // both explicit `AS` and bare-identifier alias forms.
    let with_as = tree("SELECT Name AS N FROM T");
    let without_as = tree("SELECT Name N FROM T");
    for t in [&with_as, &without_as] {
        assert_eq!(count_nodes(t, "SDBL_SELECTED_FIELD"), 1, "Tree: {}", t);
        assert_eq!(count_nodes(t, "SDBL_ALIAS"), 1, "Tree: {}", t);
    }
}

// =============================================================================
// Bilingual integration — ITS pubqlang/12
// =============================================================================

#[test]
fn test_bilingual_full_prefix() {
    // ITS pubqlang/12 — each Slice 7 keyword has a Russian and an English
    // spelling. A query using ВЫБРАТЬ, КАК, ПОМЕСТИТЬ, ИЗ exercises the
    // full bilingual keyword vocabulary of the Slice 7 surface.
    parse_clean("ВЫБРАТЬ Имя КАК Наименование ПОМЕСТИТЬ ВремТаблица ИЗ Товары");
    let t = tree("ВЫБРАТЬ Имя КАК Наименование ПОМЕСТИТЬ ВремТаблица ИЗ Товары");
    assert_eq!(count_nodes(&t, "SDBL_SELECTED_FIELD"), 1);
    assert_eq!(count_nodes(&t, "SDBL_ALIAS"), 1);
    assert_eq!(count_nodes(&t, "SDBL_INTO_CLAUSE"), 1);
    assert_eq!(count_nodes(&t, "SDBL_FROM_CLAUSE"), 1);
}

#[test]
fn test_into_drop_package_integration() {
    // ITS pubqlang/10 + /51 h47 — a query package can alternate SELECT ...
    // INTO (creating a temp table) with DROP / УНИЧТОЖИТЬ (terminating it).
    // Slice 7 INTO must coexist with the Slice 6 DROP statement without
    // package-boundary regressions.
    parse_clean("SELECT Name INTO TmpTable FROM T; DROP TmpTable");
    let t = tree("SELECT Name INTO TmpTable FROM T; DROP TmpTable");
    assert_eq!(count_nodes(&t, "SDBL_INTO_CLAUSE"), 1);
    assert_eq!(count_nodes(&t, "SDBL_DROP_QUERY"), 1);
}

// =============================================================================
// Slice 12 — recover_field_to_alias_or_delimiter clause-keyword,
//            Semicolon, and EOF stops at any nesting depth.
//
// Pre-Slice-12, the helper at `crates/parser/src/grammar/sdbl/select.rs`
// gated all six stop conditions (alias keyword, Comma, Semicolon,
// RParen, clause keyword, EOF) inside `if case_depth == 0 && paren_depth == 0`.
// An unterminated nested `(...)` inside a selected-field expression
// silently gobbled the outer query's clause keyword (FROM / WHERE /
// ...) and could spin until the iteration limit when EOF was reached
// at depth>0. Slice 12 lifted clause-keyword / Semicolon / EOF out of
// the depth gate so they fire at any nesting depth, mirroring the
// post-Slice-8-addendum `recover_to_delimiter_vt` contract and the
// Slice 12 `recover_to_delimiter` alignment.
//
// Trigger inputs use a literal `1` (NOT an Ident) as the field
// expression so that `expression(p)` returns cleanly at the bare
// `(`; an Ident would be consumed by `column_or_function` as a
// nested function-call start, routing recovery through the Slice
// 10a `recover_to_delimiter` instead of the field-tail helper.
// The literal forces the unexpected `(` to reach
// `selected_field`'s `at_expected_position` check
// (`select.rs:301-313`), which then invokes
// `recover_field_to_alias_or_delimiter` — the helper this slice
// fixes. Codex Round-2 caught the false-positive use of `A` in an
// earlier draft.
// =============================================================================

#[test]
fn test_slice7_field_recovery_stops_on_clause_keyword_at_any_depth_ru() {
    let input = "ВЫБРАТЬ 1 ( ИЗ T2 КАК Т";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    let from_clauses =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE).count();
    assert!(
        from_clauses >= 1,
        "Outer ВЫБРАТЬ must keep its ИЗ clause despite the unterminated nested `(`.\nTree: {:#?}",
        root
    );

    let bad_error = root.descendants().filter(|n| n.kind() == SyntaxKind::ERROR).any(|err| {
        err.descendants_with_tokens()
            .filter_map(|nt| nt.into_token())
            .any(|t| t.text().eq_ignore_ascii_case("ИЗ"))
    });
    assert!(
        !bad_error,
        "ИЗ clause keyword must not be consumed by recover_field_to_alias_or_delimiter at depth>0.\nTree: {:#?}",
        root
    );
}

#[test]
fn test_slice7_field_recovery_stops_on_clause_keyword_at_any_depth_en() {
    let input = "SELECT 1 ( FROM T2 AS T";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    let from_clauses =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE).count();
    assert!(
        from_clauses >= 1,
        "Outer SELECT must keep its FROM clause despite the unterminated nested `(`.\nTree: {:#?}",
        root
    );

    let bad_error = root.descendants().filter(|n| n.kind() == SyntaxKind::ERROR).any(|err| {
        err.descendants_with_tokens()
            .filter_map(|nt| nt.into_token())
            .any(|t| t.text().eq_ignore_ascii_case("FROM"))
    });
    assert!(
        !bad_error,
        "FROM clause keyword must not be consumed by recover_field_to_alias_or_delimiter at depth>0.\nTree: {:#?}",
        root
    );
}

#[test]
fn test_slice7_field_recovery_stops_on_clause_keyword_inside_case_and_paren() {
    // CASE-depth × paren-depth coverage. Pre-Slice-12, the
    // clause-keyword check fired only when BOTH case_depth == 0 AND
    // paren_depth == 0. Post-fix, the check fires at any depth
    // combination. This input drives the helper into both depths
    // simultaneously: the unterminated `(` increments paren_depth,
    // then `ВЫБОР` increments case_depth — `ИЗ` at depth (1, 1)
    // must still terminate recovery without consuming the keyword.
    let input = "ВЫБРАТЬ 1 ( ВЫБОР ИЗ T2 КАК Т";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    let from_clauses =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE).count();
    assert!(
        from_clauses >= 1,
        "Outer ВЫБРАТЬ must keep its ИЗ clause even when recovery is at case_depth>0 AND paren_depth>0.\nTree: {:#?}",
        root
    );

    let bad_error = root.descendants().filter(|n| n.kind() == SyntaxKind::ERROR).any(|err| {
        err.descendants_with_tokens()
            .filter_map(|nt| nt.into_token())
            .any(|t| t.text().eq_ignore_ascii_case("ИЗ"))
    });
    assert!(
        !bad_error,
        "ИЗ clause keyword must not be consumed by recover_field_to_alias_or_delimiter at case_depth>0 AND paren_depth>0.\nTree: {:#?}",
        root
    );
}

#[test]
fn test_slice7_field_recovery_breaks_at_eof_inside_unterminated_paren() {
    // Pre-Slice-12, the EOF check at depth>0 was inside the depth gate
    // and would not fire; `p.bump()` is a no-op at EOF, so the helper
    // would spin until `Parser::check_iteration_limit` panicked. The
    // post-fix helper exits the loop cleanly at EOF at any depth.
    //
    // Trigger uses a literal `1` (per the same rationale as the
    // clause-keyword tests above): an Ident would route recovery
    // through `recover_to_delimiter` instead of the field-tail
    // helper.
    //
    // The audit-gate property of this test is: the parser does NOT
    // panic with "iteration limit exceeded" on the unterminated input.
    // If the regression returns, this test fails by panic, not by
    // assertion.
    let input = "ВЫБРАТЬ 1 (";
    let _ = parse_sdbl(input);
}
