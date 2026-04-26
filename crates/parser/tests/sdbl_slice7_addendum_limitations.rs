//! Slice 7-addendum clean-room acceptance suite — SELECT prefix
//! qualifiers (DISTINCT / TOP / ALLOWED) plus the `is_identifier_token`
//! cross-slice predicate.
//!
//! These tests are the spec-driven acceptance gate for the Slice
//! 7-addendum clean-room rewrite of the SDBL SELECT-prefix qualifier
//! helpers (`is_identifier_token`, `is_limitation_keyword`,
//! `limitations`, `top_clause`). Each test cites either:
//! - the **primary** SDBL grammar specification: v8.3.27 Developer's
//!   Reference Глава 8 «Работа с запросами» at
//!   `its/dump/its_db_v8327doc_bookmark_dev_TI000000453/page.html`
//!   (line 1320 canonical EBNF + lines 1331-1356 prose semantics +
//!   bilingual word-list pairs at lines 1030-1034 / 1040-1044 /
//!   920-924);
//! - the **secondary** corroborating ITS pubqlang dump (textbook
//!   companion): chapter 19 (TOP demonstrative), chapter 20
//!   (DISTINCT demonstrative + DISTINCT × ORDER BY interaction),
//!   chapter 57:50 (ALLOWED query-designer UI prose);
//! - a §section of the SELECT mini-spec at
//!   `docs/legal/sdbl-select-mini-spec.md` §Limitations;
//! - a §invariant of the Slice 7-addendum attestation at
//!   `docs/legal/sdbl-clean-room-slice7-addendum.md`.
//!
//! Authored under the clean-room discipline documented in
//! `docs/legal/sdbl-clean-room-slices.md` — `../bsl-parser/*` was not
//! consulted; v8327doc Глава 8 + pubqlang chapter regions read
//! directly via the local dump paths cited above.

use parser::parse_sdbl;
use syntax::SyntaxKind;

fn assert_clean(input: &str) -> syntax::SyntaxNode {
    let parse = parse_sdbl(input);
    assert!(
        !parse.has_errors(),
        "Expected clean parse for `{}`; got errors: {:#?}",
        input,
        parse.errors(),
    );
    let root = parse.syntax_node();
    let error_descendants: Vec<_> =
        root.descendants().filter(|n| n.kind() == SyntaxKind::ERROR).collect();
    assert!(
        error_descendants.is_empty(),
        "Expected no ERROR recovery nodes for `{}`; got: {:#?}",
        input,
        error_descendants,
    );
    root
}

fn find_limitations(root: &syntax::SyntaxNode) -> syntax::SyntaxNode {
    root.descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_LIMITATIONS)
        .unwrap_or_else(|| panic!("Tree must contain SdblLimitations; got: {:#?}", root))
}

fn limitations_token_text(limitations: &syntax::SyntaxNode) -> String {
    limitations
        .children_with_tokens()
        .filter_map(|c| c.as_token().map(|t| t.text().to_string()))
        .collect::<Vec<_>>()
        .join("|")
        .to_uppercase()
}

// ============================================================
// §DISTINCT — v8327doc Глава 8 §<Описание запроса> at
// page.html:1320 canonical EBNF + :1346-1348 prose;
// pubqlang chapter 20 demonstrative.
// SELECT mini-spec §Limitations.
// ============================================================

/// Tier A1 canonical Russian form. Primary source: v8327doc
/// Глава 8 §<Описание запроса> at
/// `its/dump/its_db_v8327doc_bookmark_dev_TI000000453/page.html:1320`
/// (canonical EBNF places `[РАЗЛИЧНЫЕ]` in the second SELECT-prefix
/// slot) + `:1346-1348` (duplicate-elimination prose). Secondary
/// corroborating: pubqlang `chapter_020.html:18, 29` demonstrative
/// `ВЫБРАТЬ РАЗЛИЧНЫЕ`. Bilingual word-list pair РАЗЛИЧНЫЕ ↔
/// DISTINCT at `page.html:1030-1034`.
#[test]
fn test_slice7adn_distinct_canonical_ru() {
    let root = assert_clean("ВЫБРАТЬ РАЗЛИЧНЫЕ Клиент ИЗ ЗаказТовара");
    let limitations = find_limitations(&root);
    let kw_text = limitations_token_text(&limitations);
    assert!(
        kw_text.contains("РАЗЛИЧНЫЕ"),
        "SdblLimitations must contain РАЗЛИЧНЫЕ token. Got tokens: {}",
        kw_text,
    );
}

/// Tier A1 canonical English form. Bilingual DISTINCT ↔ РАЗЛИЧНЫЕ
/// attested at v8327doc page.html:1030-1034 word-list pair;
/// SELECT mini-spec §Limitations.
#[test]
fn test_slice7adn_distinct_canonical_en() {
    let root = assert_clean("SELECT DISTINCT Name FROM Products");
    let limitations = find_limitations(&root);
    let kw_text = limitations_token_text(&limitations);
    assert!(
        kw_text.contains("DISTINCT"),
        "SdblLimitations must contain DISTINCT token. Got tokens: {}",
        kw_text,
    );
}

// ============================================================
// §TOP — v8327doc Глава 8 §<Описание запроса> at
// page.html:1320 canonical EBNF [ПЕРВЫЕ <Количество>] slot +
// :1350-1356 prose; pubqlang chapter 19 demonstrative.
// SELECT mini-spec §Limitations §TOP.
// ============================================================

/// Tier A1 canonical Russian form. Primary source: v8327doc
/// Глава 8 at `page.html:1320` `[ПЕРВЫЕ <Количество>]` slot +
/// `:1350-1356` (limit / ordering / nested-query prose).
/// Secondary corroborating: pubqlang `chapter_019.html:19, 28`
/// demonstrative `ВЫБРАТЬ ПЕРВЫЕ 3`. Bilingual word-list pair
/// ПЕРВЫЕ ↔ TOP at `page.html:920-924`.
#[test]
fn test_slice7adn_top_canonical_ru() {
    let root = assert_clean("ВЫБРАТЬ ПЕРВЫЕ 5 СуммаЗаказа ИЗ ЗаказТовара");
    let limitations = find_limitations(&root);
    let top_clause = limitations
        .children()
        .find(|c| c.kind() == SyntaxKind::SDBL_TOP_CLAUSE)
        .expect("SdblLimitations must have an SdblTopClause direct child");
    let has_decimal_5 = top_clause
        .children_with_tokens()
        .filter_map(|c| c.as_token().map(|t| (t.kind(), t.text().to_string())))
        .any(|(k, t)| k == SyntaxKind::DECIMAL && t == "5");
    assert!(has_decimal_5, "SdblTopClause must contain Decimal `5`");
}

/// Tier A1 canonical English form. Bilingual TOP ↔ ПЕРВЫЕ
/// attested at v8327doc page.html:920-924 word-list pair;
/// SELECT mini-spec §Limitations §TOP.
#[test]
fn test_slice7adn_top_canonical_en() {
    let root = assert_clean("SELECT TOP 100 OrderTotal FROM Orders");
    let limitations = find_limitations(&root);
    let top_clause = limitations
        .children()
        .find(|c| c.kind() == SyntaxKind::SDBL_TOP_CLAUSE)
        .expect("SdblLimitations must have an SdblTopClause direct child");
    let has_decimal_100 = top_clause
        .children_with_tokens()
        .filter_map(|c| c.as_token().map(|t| (t.kind(), t.text().to_string())))
        .any(|(k, t)| k == SyntaxKind::DECIMAL && t == "100");
    assert!(has_decimal_100, "SdblTopClause must contain Decimal `100`");
}

// ============================================================
// §ALLOWED — v8327doc Глава 8 §<Описание запроса> at
// page.html:1320 canonical EBNF first SELECT-prefix slot +
// :1331-1344 prose (RLS scope, top-level constraint).
// SELECT mini-spec §Limitations §Tier classification (ALLOWED A1)
// + §Deferred semantic constraint.
// ============================================================

/// Tier A1 canonical Russian form. Primary source: v8327doc
/// Глава 8 at `page.html:1320` `[РАЗРЕШЕННЫЕ]` first
/// SELECT-prefix slot + `:1331-1344` (RLS scope, top-level
/// constraint, propagation into subqueries, ЧТЕНИЕ-rights
/// interaction). Secondary corroborating: pubqlang
/// `chapter_057.html:50` UI-checkbox prose. Bilingual
/// word-list pair РАЗРЕШЕННЫЕ ↔ ALLOWED at `page.html:1040-1044`.
#[test]
fn test_slice7adn_allowed_canonical_ru() {
    let root = assert_clean("ВЫБРАТЬ РАЗРЕШЕННЫЕ Наименование ИЗ Справочник.Товары");
    let limitations = find_limitations(&root);
    let kw_text = limitations_token_text(&limitations);
    assert!(
        kw_text.contains("РАЗРЕШЕННЫЕ"),
        "SdblLimitations must contain РАЗРЕШЕННЫЕ token. Got tokens: {}",
        kw_text,
    );
}

/// Tier A1 canonical English form. Bilingual ALLOWED ↔
/// РАЗРЕШЕННЫЕ attested at v8327doc page.html:1040-1044
/// word-list pair; SELECT mini-spec §Limitations.
#[test]
fn test_slice7adn_allowed_canonical_en() {
    let root = assert_clean("SELECT ALLOWED Name FROM Catalog.Products");
    let limitations = find_limitations(&root);
    let kw_text = limitations_token_text(&limitations);
    assert!(
        kw_text.contains("ALLOWED"),
        "SdblLimitations must contain ALLOWED token. Got tokens: {}",
        kw_text,
    );
}

// ============================================================
// §IDE-recovery allowance Q1 — any-order qualifier acceptance.
// SELECT mini-spec §Limitations §IDE-recovery allowances Q1;
// Slice 7-addendum attestation §Preserved pre-refactor
// behaviours Q1.
// The parser does not enforce a canonical permutation; v8327doc
// EBNF suggests ALLOWED → DISTINCT → TOP, but the four tests
// below pin the parser's tolerance of arbitrary orderings.
// ============================================================

/// Q1: DISTINCT before TOP (the
/// `assign_alias_fields_in_query.rs:514-519` HIR-consumer gate
/// labels this valid).
#[test]
fn test_slice7adn_q1_distinct_before_top() {
    let root = assert_clean("ВЫБРАТЬ РАЗЛИЧНЫЕ ПЕРВЫЕ 10 Код ИЗ Товары");
    let limitations = find_limitations(&root);
    let kinds: Vec<_> = limitations.children().map(|c| c.kind()).collect();
    assert!(
        kinds.contains(&SyntaxKind::SDBL_TOP_CLAUSE),
        "Limitations must include SdblTopClause for `ПЕРВЫЕ 10`. Got: {:?}",
        kinds,
    );
    let kw_text = limitations_token_text(&limitations);
    assert!(
        kw_text.contains("РАЗЛИЧНЫЕ"),
        "Limitations must include РАЗЛИЧНЫЕ. Got tokens: {}",
        kw_text,
    );
}

/// Q1: TOP before ALLOWED (uncommon ordering — the parser must
/// tolerate it as IDE-recovery; the
/// `assign_alias_fields_in_query.rs:520-528` HIR-consumer gate
/// confirms tolerance of arbitrary orderings such as
/// `SELECT TOP 50 DISTINCT …`).
#[test]
fn test_slice7adn_q1_top_before_allowed() {
    let root = assert_clean("ВЫБРАТЬ ПЕРВЫЕ 3 РАЗРЕШЕННЫЕ Наименование ИЗ Справочник.Товары");
    let limitations = find_limitations(&root);
    let kinds: Vec<_> = limitations.children().map(|c| c.kind()).collect();
    assert!(
        kinds.contains(&SyntaxKind::SDBL_TOP_CLAUSE),
        "Limitations must include SdblTopClause for `ПЕРВЫЕ 3`. Got: {:?}",
        kinds,
    );
    let kw_text = limitations_token_text(&limitations);
    assert!(
        kw_text.contains("РАЗРЕШЕННЫЕ"),
        "Limitations must include РАЗРЕШЕННЫЕ. Got tokens: {}",
        kw_text,
    );
}

/// Q1: ALLOWED before DISTINCT (matches v8327doc EBNF canonical
/// permutation up to the missing TOP).
#[test]
fn test_slice7adn_q1_allowed_before_distinct() {
    let root = assert_clean("ВЫБРАТЬ РАЗРЕШЕННЫЕ РАЗЛИЧНЫЕ Клиент ИЗ ЗаказТовара");
    let limitations = find_limitations(&root);
    let kw_text = limitations_token_text(&limitations);
    assert!(
        kw_text.contains("РАЗРЕШЕННЫЕ") && kw_text.contains("РАЗЛИЧНЫЕ"),
        "Limitations must include both РАЗРЕШЕННЫЕ and РАЗЛИЧНЫЕ. Got tokens: {}",
        kw_text,
    );
}

/// Q1: all three qualifiers together (canonical v8327doc
/// permutation ALLOWED → DISTINCT → TOP, fully exercising the
/// `while is_limitation_keyword(p)` loop).
#[test]
fn test_slice7adn_q1_all_three_canonical_order() {
    let root = assert_clean("ВЫБРАТЬ РАЗРЕШЕННЫЕ РАЗЛИЧНЫЕ ПЕРВЫЕ 10 Клиент ИЗ ЗаказТовара");
    let limitations = find_limitations(&root);
    let kinds: Vec<_> = limitations.children().map(|c| c.kind()).collect();
    let top_count = kinds.iter().filter(|&&k| k == SyntaxKind::SDBL_TOP_CLAUSE).count();
    assert_eq!(
        top_count, 1,
        "Limitations must include exactly one SdblTopClause. Got: {:?}",
        kinds,
    );
    let kw_text = limitations_token_text(&limitations);
    assert!(
        kw_text.contains("РАЗРЕШЕННЫЕ") && kw_text.contains("РАЗЛИЧНЫЕ"),
        "Limitations must include both РАЗРЕШЕННЫЕ and РАЗЛИЧНЫЕ. Got tokens: {}",
        kw_text,
    );
}

// ============================================================
// §IDE-recovery allowance Q2 — duplicate-qualifier loop
// tolerance.
// SELECT mini-spec §Limitations §IDE-recovery allowances Q2;
// Slice 7-addendum attestation §Preserved pre-refactor
// behaviours Q2.
// Per codex Round-5 finding 2: a dedicated Q2 test (the C0
// banner says Q2 is documented but not directly tested in C0;
// the dedicated acceptance test lands here in C3).
// ============================================================

/// Q2: input `ВЫБРАТЬ РАЗЛИЧНЫЕ РАЗЛИЧНЫЕ A` is accepted
/// without error (the loop body re-enters on every
/// `is_limitation_keyword` hit without deduplication;
/// semantic uniqueness is not enforced at parser level).
/// HIR consumer at
/// `crates/sdbl-hir/src/lower/select_fields.rs:45-90` extracts
/// DISTINCT and TOP without ordering or duplicate-qualifier
/// legality checks.
#[test]
fn test_slice7adn_distinct_distinct_duplicate_tolerance() {
    let root = assert_clean("ВЫБРАТЬ РАЗЛИЧНЫЕ РАЗЛИЧНЫЕ Клиент ИЗ ЗаказТовара");
    let limitations = find_limitations(&root);
    let kw_text = limitations_token_text(&limitations);
    let occurrences = kw_text.matches("РАЗЛИЧНЫЕ").count();
    assert_eq!(
        occurrences, 2,
        "Limitations must contain two РАЗЛИЧНЫЕ tokens (Q2 \
         duplicate-qualifier loop tolerance). Got tokens: {}",
        kw_text,
    );
}

// ============================================================
// §IDE-recovery allowance Q3 — missing-TOP-count recovery.
// SELECT mini-spec §Limitations §IDE-recovery allowances Q3;
// Slice 7-addendum attestation §Preserved pre-refactor
// behaviours Q3.
// ============================================================

/// Q3: `top_clause` calls `p.expect(TokenKind::Decimal)`;
/// when the next non-trivia token is not a Decimal,
/// `Parser::expect` invokes `Parser::error` at
/// `parser.rs:160-166`, which BUMPS the next token into an
/// `ERROR` sub-node attached as a direct child of
/// `SdblTopClause`. For input `ВЫБРАТЬ ПЕРВЫЕ A ИЗ Т`, the
/// `A` Ident is consumed into the ERROR child; the
/// limitations loop then exits because the following `ИЗ` is
/// not a limitation keyword; the outer `selected_fields`
/// parser then consumes `ИЗ Т` as a bare `SdblColumnRef` +
/// `SdblAlias` (no `SdblFromClause` is emitted). A tighter
/// recovery is deferred to Slice 12.
#[test]
fn test_slice7adn_q3_top_missing_decimal_recovery() {
    let input = "ВЫБРАТЬ ПЕРВЫЕ A ИЗ Т";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    let limitations = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_LIMITATIONS)
        .expect("SdblLimitations marker must still be completed when the Decimal is missing");
    let top_clause = limitations
        .children()
        .find(|c| c.kind() == SyntaxKind::SDBL_TOP_CLAUSE)
        .expect("SdblLimitations must still have an SdblTopClause direct child");
    let decimal_count = top_clause
        .children_with_tokens()
        .filter_map(|c| c.as_token().filter(|t| t.kind() == SyntaxKind::DECIMAL).cloned())
        .count();
    assert_eq!(
        decimal_count, 0,
        "SdblTopClause must have NO Decimal token when count is missing (Q3)",
    );
    let error_children: Vec<_> =
        top_clause.children().filter(|c| c.kind() == SyntaxKind::ERROR).collect();
    assert_eq!(
        error_children.len(),
        1,
        "SdblTopClause must have exactly one ERROR sub-node child (the bumped `A` Ident)",
    );
    assert!(
        error_children[0].text().to_string().contains('A'),
        "ERROR sub-node must contain the bumped `A` Ident. Got text: {:?}",
        error_children[0].text().to_string(),
    );
    let from_clauses_count =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE).count();
    assert_eq!(
        from_clauses_count, 0,
        "Pre-rewrite recovery shape: NO SdblFromClause emitted; \
         `ИЗ` falls through to selected_fields as bare SdblColumnRef \
         (Slice 12 owns the recovery-quality fix)",
    );
}

// ============================================================
// Cross-slice integration — Slice 7-addendum × Slice 7
// selected-field + alias.
// Verifies that the Slice 7-addendum surface composes cleanly
// with the Slice 7 selected_field + alias_with_kak path.
// ============================================================

/// Cross-slice integration: DISTINCT qualifier composes with
/// the Slice 7 multi-field + bilingual alias-with-КАК path.
/// Pins the contract that `is_identifier_token` (predicate
/// shared with Slice 7 alias-scan and Slice 8 source-alias
/// guard) does not interfere with the limitations dispatch.
#[test]
fn test_slice7adn_x_slice7_distinct_with_alias_kak() {
    let root =
        assert_clean("ВЫБРАТЬ РАЗЛИЧНЫЕ Клиент КАК Покупатель, ДатаЗаказа КАК Дата ИЗ ЗаказТовара");
    let limitations = find_limitations(&root);
    let kw_text = limitations_token_text(&limitations);
    assert!(
        kw_text.contains("РАЗЛИЧНЫЕ"),
        "Limitations must contain РАЗЛИЧНЫЕ. Got tokens: {}",
        kw_text,
    );
    let alias_count = root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_ALIAS).count();
    assert_eq!(
        alias_count, 2,
        "Two SdblAlias nodes expected (Покупатель + Дата); got {}",
        alias_count,
    );
}
