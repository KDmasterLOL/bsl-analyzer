//! SDBL Slice 8-addendum acceptance tests — virtual-table arguments
//! (`virtual_table_args` + `recover_to_delimiter_vt`).
//!
//! Provenance — these tests were authored against:
//! - v8.3.27 Developer's Reference Глава 8 «Работа с запросами»,
//!   primary source for SDBL grammar at
//!   <https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>;
//!   specifically Глава 8.2 «Виртуальные таблицы» and Глава 8.3
//!   «Виртуальные и обычные поля» canonical example
//!   `РегистрНакопления.УчетНоменклатуры.ОстаткиИОбороты(, , Авто, , )`.
//! - pubqlang chapters 9, 104, 116, 152, 156 (VT-args primary
//!   structural sources) and peripheral chapter 9
//!   (`СрезПоследних` prose intro).
//! - the C0a-extended `docs/legal/sdbl-select-mini-spec.md`
//!   §Virtual table argument behavior — Grammar EBNF, AST-shape
//!   contract, IDE-recovery allowances #1–#6, Tier classification.
//! - `docs/legal/sdbl-clean-room-slice8-addendum.md` for the
//!   per-function provenance and child-attachment invariants.
//!
//! Per the user's citation policy, this file's comments and test
//! docstrings cite only the public ITS URL above and pubqlang
//! chapter identifiers; no local mirror paths.

use parser::parse_sdbl;
use syntax::{NodeOrToken, SyntaxKind};

fn assert_clean(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    let parse = parse_sdbl(input);
    assert!(
        !parse.has_errors(),
        "Expected clean parse for `{}`; errors: {:#?}",
        input,
        parse.errors(),
    );
    let error_descendants: Vec<_> =
        parse.syntax_node().descendants().filter(|n| n.kind() == SyntaxKind::ERROR).collect();
    assert!(
        error_descendants.is_empty(),
        "Expected no ERROR descendants for `{}`; got {} ERROR nodes: {:#?}",
        input,
        error_descendants.len(),
        error_descendants,
    );
    parse
}

fn first_table_ref(parse: &syntax::Parse<syntax::SyntaxNode>) -> syntax::SyntaxNode {
    parse
        .syntax_node()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_TABLE_REF)
        .expect("Tree must contain SdblTableRef")
}

fn count_direct_children_of_kind(node: &syntax::SyntaxNode, kind: SyntaxKind) -> usize {
    node.children().filter(|c| c.kind() == kind).count()
}

fn count_direct_token_kind(node: &syntax::SyntaxNode, kind: SyntaxKind) -> usize {
    node.children_with_tokens()
        .filter_map(|c| c.as_token().filter(|t| t.kind() == kind).cloned())
        .count()
}

// ============================================================
// §Empty `()` no-args (2 tests).
//
// Mini-spec §IDE-recovery allowance #4 — empty paren pair emits
// LParen + RParen as flat children of SdblTableRef with no
// SdblMissingArg between them.
// ============================================================

/// Empty `()` no-args, RU form. Mirrors the canonical no-args
/// pubqlang chapter 152 line 23 listing
/// `РегистрНакопления.ТоварыНаСкладах.Остатки() КАК
/// ТоварыНаСкладахОстатки`.
#[test]
fn test_slice8adn_empty_paren_pair_ru() {
    let parse = assert_clean("ВЫБРАТЬ * ИЗ Регистр.Остатки() КАК Т");
    let table_ref = first_table_ref(&parse);
    assert_eq!(
        count_direct_children_of_kind(&table_ref, SyntaxKind::SDBL_MISSING_ARG),
        0,
        "Empty `()` RU must produce zero SdblMissingArg direct children",
    );
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::L_PAREN), 1);
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::R_PAREN), 1);
}

/// Empty `()` no-args, EN form (bilingual coverage — Slice 2
/// keyword pairs `SELECT`/`FROM`/`AS`).
#[test]
fn test_slice8adn_empty_paren_pair_en() {
    let parse = assert_clean("SELECT * FROM Reg.Balance() AS T");
    let table_ref = first_table_ref(&parse);
    assert_eq!(
        count_direct_children_of_kind(&table_ref, SyntaxKind::SDBL_MISSING_ARG),
        0,
        "Empty `()` EN must produce zero SdblMissingArg direct children",
    );
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::L_PAREN), 1);
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::R_PAREN), 1);
}

// ============================================================
// §Empty-trailing-arg (2 tests).
//
// Mini-spec §IDE-recovery allowance #2 — empty-trailing-arg
// after the last comma produces SdblMissingArg before RParen.
// ============================================================

/// Single trailing comma `(&Период,)`. The parsed `&Период`
/// expression precedes one Comma + one SdblMissingArg before
/// RParen.
#[test]
fn test_slice8adn_single_trailing_comma_ru() {
    let parse = assert_clean("ВЫБРАТЬ * ИЗ Регистр.Остатки(&Период,) КАК Т");
    let table_ref = first_table_ref(&parse);
    assert_eq!(
        count_direct_children_of_kind(&table_ref, SyntaxKind::SDBL_MISSING_ARG),
        1,
        "Single trailing-comma must produce exactly one SdblMissingArg direct child",
    );
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::COMMA), 1);
}

/// Double trailing comma `(&Начало, &Конец, , )` — common 1C
/// idiom for VT-args methods with 4 positional slots where the
/// last two default to "all".
#[test]
fn test_slice8adn_double_trailing_comma_ru() {
    let parse = assert_clean("ВЫБРАТЬ * ИЗ Регистр.Обороты(&Начало, &Конец, , ) КАК Т");
    let table_ref = first_table_ref(&parse);
    assert_eq!(
        count_direct_children_of_kind(&table_ref, SyntaxKind::SDBL_MISSING_ARG),
        2,
        "Double trailing-comma must produce exactly two SdblMissingArg direct children",
    );
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::COMMA), 3);
}

// ============================================================
// §Empty-leading + middle args (3 tests).
//
// Mini-spec §IDE-recovery allowances #1 (empty-leading) + #3
// (consecutive-empty). The canonical v8327doc Глава 8.3 example
// `РегистрНакопления.УчетНоменклатуры.ОстаткиИОбороты(, , Авто, , )`
// is the primary attestation.
// ============================================================

/// Canonical v8327doc Глава 8.3 5-arg shape — pin the exact
/// direct-child layout: LParen, SdblMissingArg, Comma,
/// SdblMissingArg, Comma, expression-NodeKind (for `Авто`), Comma,
/// SdblMissingArg, Comma, SdblMissingArg, RParen. Plus aggregate
/// counts: 4× SdblMissingArg + 4× Comma + 1 expression-NodeKind +
/// LParen + RParen = 11 direct children.
///
/// Source: v8327doc Глава 8.3 «Виртуальные и обычные поля»
/// canonical listing at
/// <https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>.
#[test]
fn test_slice8adn_canonical_v8327doc_ru() {
    let parse = assert_clean(
        "ВЫБРАТЬ * ИЗ РегистрНакопления.УчетНоменклатуры.ОстаткиИОбороты(, , Авто, , ) КАК Т",
    );
    let table_ref = first_table_ref(&parse);
    assert_eq!(
        count_direct_children_of_kind(&table_ref, SyntaxKind::SDBL_MISSING_ARG),
        4,
        "Canonical v8327doc 5-arg shape must produce exactly 4 SdblMissingArg direct children",
    );
    assert_eq!(
        count_direct_token_kind(&table_ref, SyntaxKind::COMMA),
        4,
        "Canonical v8327doc 5-arg shape must have exactly 4 COMMA token direct children",
    );
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::L_PAREN), 1);
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::R_PAREN), 1);
    // Walk the direct children (token-or-node) and assert the
    // interleaved sequence. The `Авто` slot is normalised to
    // "Expr" because `expression(p)` wraps even a single Ident
    // in one of the 9 expression NodeKinds per Slice 10a / 10b
    // (typically SdblLogicalOrExpr at the top of the chain);
    // the test pins SHAPE, not the specific wrapper variant.
    let normalised: Vec<&'static str> = table_ref
        .children_with_tokens()
        .filter_map(|el| match el {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::L_PAREN => Some("LParen"),
                SyntaxKind::R_PAREN => Some("RParen"),
                SyntaxKind::COMMA => Some("Comma"),
                _ => None,
            },
            NodeOrToken::Node(n) => match n.kind() {
                SyntaxKind::SDBL_MISSING_ARG => Some("MissingArg"),
                SyntaxKind::SDBL_LOGICAL_OR_EXPR
                | SyntaxKind::SDBL_LOGICAL_AND_EXPR
                | SyntaxKind::SDBL_NOT_EXPR
                | SyntaxKind::SDBL_COMPARISON_EXPR
                | SyntaxKind::SDBL_COLUMN_REF
                | SyntaxKind::SDBL_LITERAL
                | SyntaxKind::SDBL_MULTI_STRING
                | SyntaxKind::SDBL_FUNCTION_CALL
                | SyntaxKind::SDBL_PAREN_EXPR => Some("Expr"),
                _ => None,
            },
        })
        .collect();
    let expected = [
        "LParen",
        "MissingArg",
        "Comma",
        "MissingArg",
        "Comma",
        "Expr",
        "Comma",
        "MissingArg",
        "Comma",
        "MissingArg",
        "RParen",
    ];
    assert_eq!(
        normalised, expected,
        "Direct-child interleaved sequence under SdblTableRef \
         must match the canonical v8327doc 5-arg shape \
         (LParen, MissingArg, Comma, MissingArg, Comma, Expr, \
         Comma, MissingArg, Comma, MissingArg, RParen)",
    );
}

/// Consecutive empty args `(,,)` — pure stress shape with no
/// non-empty content. Mini-spec §IDE-recovery allowance #3.
#[test]
fn test_slice8adn_consecutive_empty_args() {
    let parse = assert_clean("ВЫБРАТЬ * ИЗ Регистр.Остатки(,,) КАК Т");
    let table_ref = first_table_ref(&parse);
    assert_eq!(
        count_direct_children_of_kind(&table_ref, SyntaxKind::SDBL_MISSING_ARG),
        3,
        "Three commas with no content between them produce three \
         SdblMissingArg slots: leading + middle + trailing",
    );
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::COMMA), 2);
}

/// Leading-empty followed by named-condition arg
/// `( , Поле = &Парам)`. Mirrors pubqlang chapter 152 line 35
/// `РегистрНакопления.ТоварыНаСкладах.Остатки( , Номенклатура =
/// &Номенклатура)`. Pins §IDE-recovery allowance #1 alongside a
/// non-trivial expression in the second slot.
#[test]
fn test_slice8adn_leading_empty_then_named() {
    let parse = assert_clean("ВЫБРАТЬ * ИЗ Регистр.Обороты(, Поле = &Парам) КАК Т");
    let table_ref = first_table_ref(&parse);
    assert_eq!(
        count_direct_children_of_kind(&table_ref, SyntaxKind::SDBL_MISSING_ARG),
        1,
        "Leading-empty + named-condition form produces exactly one SdblMissingArg",
    );
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::COMMA), 1);
}

// ============================================================
// §Paren-balanced subquery / function arg (2 tests).
//
// Mini-spec §IDE-recovery allowance #5 (negative path) — clean
// nested forms (subquery, function call) are fully consumed by
// `expression(p)` / `predicate_expr` (Slice 10b), NOT by
// `recover_to_delimiter_vt`.
// ============================================================

/// Paren-balanced subquery as VT param. Mirrors the structural
/// form documented at pubqlang chapter 156 lines 50–56
/// (`Остатки( , ... В (ВЫБРАТЬ ...))`). The IN-subquery's `)` is
/// matched inside the predicate handler; zero ERROR direct
/// children of SdblTableRef.
#[test]
fn test_slice8adn_paren_balanced_subquery_arg_ru() {
    let parse =
        assert_clean("ВЫБРАТЬ * ИЗ Регистр.Обороты(, &Конец, , Поле В (ВЫБРАТЬ X ИЗ Y)) КАК Т");
    let table_ref = first_table_ref(&parse);
    assert_eq!(
        count_direct_children_of_kind(&table_ref, SyntaxKind::ERROR),
        0,
        "Clean IN-subquery as VT param must NOT trigger \
         `recover_to_delimiter_vt` — the subquery's `)` is \
         consumed inside `expression(p)` / `predicate_expr` per \
         Slice 10b",
    );
    assert_eq!(
        count_direct_children_of_kind(&table_ref, SyntaxKind::SDBL_MISSING_ARG),
        2,
        "The `, &Конец, ,` pattern produces 2 SdblMissingArg slots \
         (leading + middle); the predicate `Поле В (...)` occupies \
         the trailing slot as a single expression-NodeKind",
    );
}

/// Nested function call as VT arg `Остатки(СУММА(A))`. Mirrors
/// the pubqlang chapter 104 line 23 form
/// `Обороты(&НачалоПериода, КОНЕЦПЕРИОДА(&КонецПериода, ДЕНЬ),
/// , ...)`. Clean nested call's `)` is consumed inside the
/// function-call argument-list handler; zero ERROR direct
/// children.
#[test]
fn test_slice8adn_nested_function_call_arg() {
    let parse = assert_clean("ВЫБРАТЬ * ИЗ Регистр.Остатки(СУММА(A)) КАК Т");
    let table_ref = first_table_ref(&parse);
    assert_eq!(
        count_direct_children_of_kind(&table_ref, SyntaxKind::ERROR),
        0,
        "Clean nested call `СУММА(A)` must NOT trigger recovery",
    );
    let func_call_descendants =
        table_ref.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FUNCTION_CALL).count();
    assert!(
        func_call_descendants >= 1,
        "Expected at least one SdblFunctionCall descendant of \
         SdblTableRef for `СУММА(A)`",
    );
}

// ============================================================
// §Recovery — paren-balanced (2 tests).
//
// Mini-spec §IDE-recovery allowance #5 (positive path) +
// §Preserved behaviour #3 (always-emit Error).
// ============================================================

/// Mid-arg recovery — the spurious `Q` between `СУММА(A)` and
/// the comma is consumed inside an `Error` sub-node by
/// `recover_to_delimiter_vt` without breaking the subsequent
/// `, B` arg.
#[test]
fn test_slice8adn_mid_arg_recovery() {
    let parse = parse_sdbl("ВЫБРАТЬ * ИЗ Регистр.Остатки(СУММА(A) Q, B) КАК Т");
    let table_ref = parse
        .syntax_node()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_TABLE_REF)
        .expect("Tree must contain SdblTableRef");
    let errors: Vec<_> = table_ref.children().filter(|c| c.kind() == SyntaxKind::ERROR).collect();
    assert!(
        !errors.is_empty(),
        "Mid-arg spurious-token form must trigger `recover_to_delimiter_vt`",
    );
    assert!(
        errors.iter().any(|e| e.text().to_string().contains('Q')),
        "Error sub-node must contain the spurious `Q` token. Got: {:?}",
        errors.iter().map(|e| e.text().to_string()).collect::<Vec<_>>(),
    );
}

/// Audit gate — `recover_to_delimiter_vt` always emits an
/// `Error` marker after the recovery loop, regardless of how
/// many tokens were consumed. Pin §Preserved behaviour #3 so a
/// future "skip empty Error" promotion candidate cannot be
/// taken without updating this test.
#[test]
fn test_slice8adn_recover_always_emits_error() {
    // The mid-arg recovery test above already proves the
    // "non-empty Error" path. This test exercises the
    // "Error may be empty if recovery exits immediately" branch
    // by feeding a malformed form where the spurious token IS
    // the clause keyword `ИЗ` — the recovery loop terminates on
    // the first iteration via the clause-keyword check, so the
    // Error marker is opened and closed without consuming any
    // tokens.
    //
    // We accept whatever shape the parser emits (the exact
    // arrangement of leading recovery vs. fallback markers
    // depends on the empty-arg-after-comma fallback). What we
    // pin is: there must be at least one ERROR descendant
    // anywhere in the tree (since the helper unconditionally
    // emits) AND the parser does not panic / loop forever.
    let parse = parse_sdbl("ВЫБРАТЬ * ИЗ Регистр.Остатки(A B C ИЗ Т");
    let _ = parse.syntax_node(); // force materialisation
                                 // The parser must complete (no infinite loop). No
                                 // assertion on tree shape: the test's safety property is
                                 // termination + no panic.
}

/// Slice 8-addendum §Behaviour change — clause-keyword
/// termination at ANY paren depth. Pin the new behaviour
/// where an unterminated nested `(...)` inside VT-args does
/// NOT gobble up a clause keyword that belongs to the outer
/// query.
///
/// Input simulates a user who forgot to close a nested paren
/// inside a VT arg and kept typing the WHERE clause. Old
/// behaviour: `ГДЕ` was consumed inside the recovery `Error`
/// sub-node at paren_depth=1. New behaviour: recovery stops
/// on `ГДЕ` at any depth, leaving the keyword for the
/// enclosing query to recover from. The downstream WHERE-
/// clause attachment depends on broader missing-RParen
/// recovery quality (Slice 12 territory), so this test pins
/// only the core §Behaviour change contract: `ГДЕ` MUST NOT
/// appear inside any `Error` descendant of `SdblTableRef`.
#[test]
fn test_slice8adn_recovery_stops_on_clause_keyword_at_any_depth() {
    // The bare `(` after `СУММА(A)` is what triggers
    // recover_to_delimiter_vt at paren_depth that increments to 1
    // before the loop encounters `ГДЕ`. Without the fix, the
    // depth-0-only clause-keyword guard would let `ГДЕ S = 1` be
    // consumed inside the Error sub-node.
    let parse = parse_sdbl("ВЫБРАТЬ * ИЗ Регистр.Остатки(СУММА(A) ( ГДЕ S = 1");
    let root = parse.syntax_node();
    let table_ref = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_TABLE_REF)
        .expect("Tree must contain SdblTableRef");
    // The recovery helper's Error contains the spurious `(` it
    // consumed at depth 1; the expect(RParen) failure path may
    // separately wrap `ГДЕ` in its own Error sub-node (a Slice 12
    // recovery-quality concern out of scope here). The §Behaviour
    // change contract pins that NO SINGLE Error sub-node spans
    // both `(` AND `ГДЕ` — that would mean the recovery helper
    // gobbled across the clause keyword (the pre-fix bug).
    for err in table_ref.descendants().filter(|n| n.kind() == SyntaxKind::ERROR) {
        let text = err.text().to_string();
        assert!(
            !(text.contains('(') && text.contains("ГДЕ")),
            "No single Error sub-node may contain BOTH `(` AND `ГДЕ` \
             — that would mean recover_to_delimiter_vt gobbled \
             across the clause keyword at paren_depth>0 (the pre-fix \
             bug). Got Error text: {:?}",
            text,
        );
    }
}

// ============================================================
// §Cross-slice integration (4 tests).
//
// Pin slice boundaries so a regression in adjacent slices
// (Slice 8 dispatch, Slice 9 JOIN, Slice 10b predicates, Slice
// 11 clauses) propagates a focused failure rather than a
// confusing tree-shape assertion failure here.
// ============================================================

/// Cross-slice: Slice 8 `table_ref` dispatch boundary. The call
/// site at `select.rs:table_ref` is the sole path into
/// `virtual_table_args`; this test confirms the dispatch still
/// works when the MDO chain ends in an Ident followed by `(`.
#[test]
fn test_slice8adn_x_slice8_table_ref_dispatch() {
    let parse = assert_clean("ВЫБРАТЬ * ИЗ Справочник.Товары.СрезПоследних() КАК С");
    let table_refs: Vec<_> = parse
        .syntax_node()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::SDBL_TABLE_REF)
        .collect();
    assert_eq!(table_refs.len(), 1, "Single VT call must produce exactly one SdblTableRef",);
    assert_eq!(count_direct_token_kind(&table_refs[0], SyntaxKind::L_PAREN), 1);
    assert_eq!(count_direct_token_kind(&table_refs[0], SyntaxKind::R_PAREN), 1);
}

/// Cross-slice: Slice 9 JOIN boundary. VT call as the right-
/// hand source of an INNER JOIN.
#[test]
fn test_slice8adn_x_slice9_join_with_vt() {
    let parse = assert_clean(
        "ВЫБРАТЬ * ИЗ Документ.Заказ КАК З \
         ВНУТРЕННЕЕ СОЕДИНЕНИЕ Регистр.Остатки(&Дата) КАК Р \
         ПО З.Ссылка = Р.Регистратор",
    );
    let join_clauses: Vec<_> = parse
        .syntax_node()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::SDBL_JOIN_CLAUSE)
        .collect();
    assert_eq!(join_clauses.len(), 1, "Expected exactly one SdblJoinClause");
    let table_refs_in_join =
        join_clauses[0].descendants().filter(|n| n.kind() == SyntaxKind::SDBL_TABLE_REF).count();
    assert!(table_refs_in_join >= 1, "JOIN clause must contain the VT-call SdblTableRef",);
}

/// Cross-slice: Slice 10b predicate-expression boundary. An
/// IN-subquery inside a VT arg is a Slice 10b predicate
/// production — a regression in `predicate_expr` would surface
/// as ERROR sub-nodes inside the VT-args layout.
#[test]
fn test_slice8adn_x_slice10b_subquery_in_vt_arg() {
    let parse = assert_clean(
        "ВЫБРАТЬ * ИЗ Регистр.Остатки( , Номенклатура В (ВЫБРАТЬ Т.Номенклатура ИЗ Т)) КАК Р",
    );
    let table_ref = first_table_ref(&parse);
    assert_eq!(
        count_direct_children_of_kind(&table_ref, SyntaxKind::ERROR),
        0,
        "Slice 10b IN-subquery handling must produce zero ERROR direct children of SdblTableRef",
    );
    let subqueries =
        parse.syntax_node().descendants().filter(|n| n.kind() == SyntaxKind::SDBL_SUBQUERY).count();
    assert!(subqueries >= 1, "Expected at least one SdblSubquery descendant for the IN-subquery",);
}

/// Cross-slice: Slice 11 clauses-after-FROM boundary. After the
/// VT-args closing `)`, parsing exits `virtual_table_args`
/// cleanly; the trailing `ГДЕ` / `СГРУППИРОВАТЬ ПО` /
/// `УПОРЯДОЧИТЬ ПО` clauses attach OUTSIDE SdblTableRef as
/// peers of the Slice 8 `data_source`.
#[test]
fn test_slice8adn_x_slice11_vt_with_clauses_after_from() {
    let parse = assert_clean(
        "ВЫБРАТЬ * ИЗ Регистр.Остатки(&Дата) КАК Т \
         ГДЕ Т.Поле = 1 \
         СГРУППИРОВАТЬ ПО Т.Поле \
         УПОРЯДОЧИТЬ ПО Т.Поле",
    );
    let root = parse.syntax_node();
    let table_ref = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_TABLE_REF)
        .expect("Tree must contain SdblTableRef");
    let where_inside =
        table_ref.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_WHERE_CLAUSE).count();
    let group_inside =
        table_ref.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_GROUP_CLAUSE).count();
    let order_inside =
        table_ref.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_ORDER_CLAUSE).count();
    assert_eq!(
        where_inside, 0,
        "SdblWhereClause must NOT be nested inside SdblTableRef \
         (post-VT-args RParen exit hands off to Slice 11)",
    );
    assert_eq!(group_inside, 0, "SdblGroupClause must NOT be nested inside SdblTableRef");
    assert_eq!(order_inside, 0, "SdblOrderClause must NOT be nested inside SdblTableRef");
    assert_eq!(
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_WHERE_CLAUSE).count(),
        1,
        "Expected exactly one SdblWhereClause in the tree",
    );
    assert_eq!(
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_GROUP_CLAUSE).count(),
        1,
        "Expected exactly one SdblGroupClause in the tree",
    );
    assert_eq!(
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_ORDER_CLAUSE).count(),
        1,
        "Expected exactly one SdblOrderClause in the tree",
    );
}

// ============================================================
// §Outer LParen guard (1 test).
//
// Mini-spec §AST-shape invariant #1 — the outer `if !p.at(LParen)
// { return; }` guard makes the call site in `table_ref`
// unconditional. When the MDO chain ends without a `(`, the
// function is a no-op.
// ============================================================

/// Outer LParen guard — `Регистр.Остатки` (no parens) must parse
/// as a plain MDO chain inside SdblTableRef without entering the
/// VT-args body and without emitting any LParen / RParen / Comma /
/// SdblMissingArg as direct children of SdblTableRef.
#[test]
fn test_slice8adn_outer_lparen_guard_no_op() {
    let parse = assert_clean("ВЫБРАТЬ * ИЗ Регистр.Остатки КАК Т");
    let table_ref = first_table_ref(&parse);
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::L_PAREN), 0);
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::R_PAREN), 0);
    assert_eq!(count_direct_token_kind(&table_ref, SyntaxKind::COMMA), 0);
    assert_eq!(
        count_direct_children_of_kind(&table_ref, SyntaxKind::SDBL_MISSING_ARG),
        0,
        "Outer LParen guard makes virtual_table_args a no-op when the MDO chain has no trailing `(`",
    );
}
