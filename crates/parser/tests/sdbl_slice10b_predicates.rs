//! SDBL Slice 10b clean-room acceptance tests — predicates,
//! comparison, function calls, CAST, CASE.
//!
//! This file is the spec-driven acceptance suite for the Slice 10b
//! clean-room rewrite under the `CLEAN-ROOM Slice 10b` banner in
//! `crates/parser/src/grammar/sdbl/expressions.rs`. It exercises the
//! 8 functions `comparison_expr`, `predicate_expr`,
//! `column_or_function`, `inline_table_fields`, `is_cast_function`,
//! `parse_cast_type`, `case_expr`, `when_clause` and the 13 NodeKinds
//! they emit (`SdblComparisonExpr`, `SdblInExpr`,
//! `SdblInHierarchyExpr`, `SdblIsNullExpr`, `SdblBetweenExpr`,
//! `SdblLikeExpr`, `SdblRefsExpr`, `SdblColumnRef`,
//! `SdblFunctionCall`, `SdblType`, `SdblInlineTableFields`,
//! `SdblCaseExpr`, `SdblWhenClause`).
//!
//! Tests authored from:
//!   - `docs/legal/sdbl-expressions-mini-spec.md` (the C0a-extended
//!     clean-room reference, sections §Predicates, §Comparison,
//!     §Column references and function calls, §CAST type
//!     specification, §CASE expressions);
//!   - 1C ITS pubqlang documentation, accessed via the local dump
//!     at `/home/itrous/src/tools_migration/its/dump/`:
//!     - chapter 21 (`/db/pubqlang/content/21/hdoc`) — DISTINCT /
//!       РАЗЛИЧНЫЕ aggregate prefix canonical example
//!       `КОЛИЧЕСТВО(РАЗЛИЧНЫЕ ЗаказТовара.Клиент)`;
//!     - chapter 22 (`/db/pubqlang/content/22/hdoc`) — WHERE
//!       conditions, BETWEEN canonical
//!       `Дата МЕЖДУ ДАТАВРЕМЯ(...) И ДАТАВРЕМЯ(...)`;
//!     - chapter 23 (`/db/pubqlang/content/23/hdoc`) — LIKE /
//!       ПОДОБНО pattern primitive (`Наименование ПОДОБНО "%Иван%"`);
//!     - chapter 27 (`/db/pubqlang/content/27/hdoc`) — IS NULL /
//!       ЕСТЬ NULL canonical
//!       `КОГДА (Товары.Производитель) ЕСТЬ NULL ТОГДА "NULL"`;
//!     - chapter 32 (`/db/pubqlang/content/32/hdoc`) — IN HIERARCHY
//!       / В ИЕРАРХИИ canonical
//!       `Товары.Ссылка В ИЕРАРХИИ (&ГруппаТоваров)` (NOT chapter 28
//!       — codex Round-1 finding 1);
//!     - chapter 40 (`/db/pubqlang/content/40/hdoc`) — CASE / ВЫБОР,
//!       CAST / ВЫРАЗИТЬ, REFS / ССЫЛКА canonical examples.
//!
//! The oracle is the mini-spec §Predicates / §Comparison / §Column
//! references and function calls / §CAST type specification / §CASE
//! expressions and the §AST-shape contracts they specify, NOT any
//! pre-rewrite parser implementation. Each per-test comment cites
//! the relevant mini-spec section / ITS chapter so the post-Slice-
//! 10b-C2 parser is validated against the mini-spec contract.
//!
//! `../bsl-parser/*` was not consulted during authoring of this file
//! per the Slice 10b attestation
//! (`docs/legal/sdbl-clean-room-slice10b.md`) §Non-consultation
//! statement.

use parser::parse_sdbl;
use syntax::SyntaxKind;

fn parse_no_errors(input: &str) -> syntax::SyntaxNode {
    let parse = parse_sdbl(input);
    assert!(
        !parse.has_errors(),
        "Expected clean parse for {input:?}, got errors: {:?}",
        parse.errors()
    );
    let root = parse.syntax_node();
    assert_eq!(root.text().to_string(), input, "Root must cover full input");
    root
}

fn count_kind(root: &syntax::SyntaxNode, kind: SyntaxKind) -> usize {
    root.descendants().filter(|n| n.kind() == kind).count()
}

fn first_of_kind(root: &syntax::SyntaxNode, kind: SyntaxKind) -> syntax::SyntaxNode {
    root.descendants()
        .find(|n| n.kind() == kind)
        .unwrap_or_else(|| panic!("Expected at least one {kind:?} in tree:\n{root:#?}"))
}

fn first_selected_field_direct_child_kinds(input: &str) -> Vec<SyntaxKind> {
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();
    let field = first_of_kind(&root, SyntaxKind::SDBL_SELECTED_FIELD);
    field.children().map(|n| n.kind()).collect()
}

// ============================================================================
// 1. Comparison operators (mini-spec §Comparison)
// ============================================================================
//
// Six binary comparison operators sharing a single SdblComparisonExpr
// wrapper. Single-shot tail per mini-spec §Comparison.

#[test]
fn test_slice10b_comparison_eq() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ А = 1");
    assert!(
        count_kind(&root, SyntaxKind::SDBL_COMPARISON_EXPR) >= 1,
        "= must emit SdblComparisonExpr"
    );
}

#[test]
fn test_slice10b_comparison_neq() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ А <> 1");
    assert!(count_kind(&root, SyntaxKind::SDBL_COMPARISON_EXPR) >= 1);
}

#[test]
fn test_slice10b_comparison_lt() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ А < 1");
    assert!(count_kind(&root, SyntaxKind::SDBL_COMPARISON_EXPR) >= 1);
}

#[test]
fn test_slice10b_comparison_le() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ А <= 1");
    assert!(count_kind(&root, SyntaxKind::SDBL_COMPARISON_EXPR) >= 1);
}

#[test]
fn test_slice10b_comparison_gt() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ А > 1");
    assert!(count_kind(&root, SyntaxKind::SDBL_COMPARISON_EXPR) >= 1);
}

#[test]
fn test_slice10b_comparison_ge() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ А >= 1");
    assert!(count_kind(&root, SyntaxKind::SDBL_COMPARISON_EXPR) >= 1);
}

// ============================================================================
// 2. IN predicate (mini-spec §SdblInExpr; ITS pubqlang/22 by analogy)
// ============================================================================

// IN with two-element value list — recoverable parse of the
// canonical inline-list form.
#[test]
fn test_slice10b_in_value_list_two_elements() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Поле В (1, 2)");
    assert!(count_kind(&root, SyntaxKind::SDBL_IN_EXPR) >= 1);
}

// Empty IN list `IN ()` — local IDE-recovery allowance per
// mini-spec §IDE-recovery allowances #10.
#[test]
fn test_slice10b_in_empty_list_recoverable() {
    let parse = parse_sdbl("ВЫБРАТЬ * ИЗ Т ГДЕ Поле В ()");
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_IN_EXPR"),
        "Empty IN () must still emit SdblInExpr (recoverable). Tree: {}",
        tree
    );
}

// NOT IN with subquery — KwNot before KwIn, SdblSubquery inside parens.
#[test]
fn test_slice10b_not_in_with_subquery() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Поле НЕ В (ВЫБРАТЬ Х ИЗ С)");
    let in_expr = first_of_kind(&root, SyntaxKind::SDBL_IN_EXPR);
    let in_text = in_expr.text().to_string().to_uppercase();
    assert!(in_text.contains("НЕ"));
    assert!(in_text.contains("ВЫБРАТЬ"));
    assert!(
        count_kind(&root, SyntaxKind::SDBL_SUBQUERY) >= 1,
        "IN with subquery must produce SdblSubquery"
    );
}

// ============================================================================
// 3. IN HIERARCHY predicate (mini-spec §SdblInHierarchyExpr;
//    ITS pubqlang/32 canonical example)
// ============================================================================

// pubqlang/32 (chapter_032.html, листинг 1.51):
// «Товары.Ссылка В ИЕРАРХИИ (&ГруппаТоваров)»
#[test]
fn test_slice10b_in_hierarchy_canonical_russian() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Товары.Ссылка В ИЕРАРХИИ (&Корень)");
    assert!(
        count_kind(&root, SyntaxKind::SDBL_IN_HIERARCHY_EXPR) >= 1,
        "В ИЕРАРХИИ must emit SdblInHierarchyExpr"
    );
}

// EN variant `IN HIERARCHY (...)` — bilingual.
#[test]
fn test_slice10b_in_hierarchy_english() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Field IN HIERARCHY (&Root)");
    assert!(count_kind(&root, SyntaxKind::SDBL_IN_HIERARCHY_EXPR) >= 1);
}

// ============================================================================
// 4. IS NULL predicate (mini-spec §SdblIsNullExpr;
//    ITS pubqlang/27 canonical example)
// ============================================================================

// pubqlang/27 (chapter_027.html):
// «КОГДА (Товары.Производитель) ЕСТЬ NULL ТОГДА "NULL"»
#[test]
fn test_slice10b_is_null_russian_canonical() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Поле ЕСТЬ NULL");
    assert!(count_kind(&root, SyntaxKind::SDBL_IS_NULL_EXPR) >= 1);
}

// IS NULL EN variant.
#[test]
fn test_slice10b_is_null_english() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Поле IS NULL");
    assert!(count_kind(&root, SyntaxKind::SDBL_IS_NULL_EXPR) >= 1);
}

// IS NOT NULL — KwNot direct child of SdblIsNullExpr between IS and NULL.
#[test]
fn test_slice10b_is_not_null_english() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Поле IS NOT NULL");
    let is_null = first_of_kind(&root, SyntaxKind::SDBL_IS_NULL_EXPR);
    assert!(
        is_null.text().to_string().to_uppercase().contains("NOT"),
        "IS NOT NULL must carry NOT inside the SdblIsNullExpr text"
    );
}

// ============================================================================
// 5. BETWEEN predicate (mini-spec §SdblBetweenExpr;
//    ITS pubqlang/22 канonical example)
// ============================================================================

// pubqlang/22 (chapter_022.html, листинг 1.33):
// «Дата МЕЖДУ ДАТАВРЕМЯ(2012, 10, 01) И ДАТАВРЕМЯ(2012, 10, 31)»
#[test]
fn test_slice10b_between_canonical_russian_dates() {
    let root = parse_no_errors(
        "ВЫБРАТЬ * ИЗ Т ГДЕ Дата МЕЖДУ ДАТАВРЕМЯ(2012, 10, 01) И ДАТАВРЕМЯ(2012, 10, 31)",
    );
    assert!(count_kind(&root, SyntaxKind::SDBL_BETWEEN_EXPR) >= 1);
}

// BETWEEN with simple integer bounds.
#[test]
fn test_slice10b_between_integer_bounds() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Поле МЕЖДУ 1 И 5");
    assert!(count_kind(&root, SyntaxKind::SDBL_BETWEEN_EXPR) >= 1);
}

// BETWEEN missing-AND recovery — local IDE-recovery allowance per
// mini-spec §IDE-recovery allowances #12.
#[test]
fn test_slice10b_between_missing_and_recovery() {
    let parse = parse_sdbl("ВЫБРАТЬ * ИЗ Т ГДЕ Поле МЕЖДУ 1");
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_BETWEEN_EXPR"),
        "BETWEEN without AND must still emit SdblBetweenExpr (recovery). Tree: {}",
        tree
    );
}

// ============================================================================
// 6. LIKE predicate (mini-spec §SdblLikeExpr;
//    ITS pubqlang/23 canonical example)
// ============================================================================

// pubqlang/23 (chapter_023.html, листинг 1.34):
// «Наименование ПОДОБНО "%Иван%"»
#[test]
fn test_slice10b_like_canonical_russian() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Наименование ПОДОБНО \"%Иван%\"");
    assert!(count_kind(&root, SyntaxKind::SDBL_LIKE_EXPR) >= 1);
}

// LIKE with ESCAPE clause — local IDE-recovery allowance per
// mini-spec §IDE-recovery allowances #13 (ESCAPE / СПЕЦСИМВОЛ is
// NOT documented in dumped ITS chapters).
#[test]
fn test_slice10b_like_with_escape_local_allowance() {
    let parse = parse_sdbl("ВЫБРАТЬ * ИЗ Т ГДЕ Поле ПОДОБНО \"abc%\" СПЕЦСИМВОЛ \"!\"");
    let tree = format!("{:#?}", parse.syntax_node());
    assert!(
        tree.contains("SDBL_LIKE_EXPR"),
        "ПОДОБНО ... СПЕЦСИМВОЛ must emit SdblLikeExpr.\nTree: {}",
        tree
    );
}

// ============================================================================
// 7. REFS predicate (mini-spec §SdblRefsExpr;
//    ITS pubqlang/40 canonical example)
// ============================================================================

// pubqlang/40 (chapter_040.html):
// «(ОстаткиТоваров.Регистратор ССЫЛКА Документ.ПриходнаяНакладная)»
#[test]
fn test_slice10b_refs_canonical_russian() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Регистратор ССЫЛКА Документ.ПриходнаяНакладная");
    assert!(count_kind(&root, SyntaxKind::SDBL_REFS_EXPR) >= 1);
}

// Deep MDO chain — REFS is greedy (mini-spec §SdblRefsExpr).
// English segment names avoid collision with Russian keywords
// (e.g. `В` lexes as `KwIn` rather than `Ident`, so a chain like
// `А.Б.В` would terminate after the second segment).
#[test]
fn test_slice10b_refs_deep_mdo_chain() {
    let root = parse_no_errors("SELECT * FROM T WHERE Field REFS Catalog.Products.Item");
    let refs = first_of_kind(&root, SyntaxKind::SDBL_REFS_EXPR);
    let text = refs.text().to_string();
    assert!(text.contains("Catalog"));
    assert!(text.contains("Products"));
    assert!(text.contains("Item"));
}

// ============================================================================
// 8. CAST primitive types (mini-spec §CAST type specification;
//    ITS pubqlang/40 canonical example)
// ============================================================================

// pubqlang/40 (chapter_040.html): «ВЫРАЗИТЬ(... КАК ЧИСЛО(8,2))».
#[test]
fn test_slice10b_cast_number_two_args() {
    let root = parse_no_errors("ВЫБРАТЬ ВЫРАЗИТЬ(Поле КАК ЧИСЛО(8, 2)) ИЗ Т");
    assert!(count_kind(&root, SyntaxKind::SDBL_TYPE) >= 1);
    assert!(count_kind(&root, SyntaxKind::SDBL_FUNCTION_CALL) >= 1);
}

// CAST with primitive STRING(length).
#[test]
fn test_slice10b_cast_string_with_length() {
    let root = parse_no_errors("ВЫБРАТЬ ВЫРАЗИТЬ(Поле КАК СТРОКА(200)) ИЗ Т");
    assert!(count_kind(&root, SyntaxKind::SDBL_TYPE) >= 1);
}

// CAST with primitive DATE (no parameters).
#[test]
fn test_slice10b_cast_date_no_params() {
    let root = parse_no_errors("ВЫБРАТЬ ВЫРАЗИТЬ(Поле КАК ДАТА) ИЗ Т");
    assert!(count_kind(&root, SyntaxKind::SDBL_TYPE) >= 1);
}

// ============================================================================
// 9. CAST MDO type and member access (mini-spec §CAST type
//    specification; ITS pubqlang/40 canonical example)
// ============================================================================

// pubqlang/40 (chapter_040.html):
// «ВЫРАЗИТЬ (ОстаткиТоваров.Регистратор КАК Документ.ПриходнаяНакладная).Поставщик»
#[test]
fn test_slice10b_cast_mdo_with_member_access_canonical() {
    let root = parse_no_errors(
        "ВЫБРАТЬ ВЫРАЗИТЬ(Регистратор КАК Документ.ПриходнаяНакладная).Поставщик ИЗ Т",
    );
    let func_call = first_of_kind(&root, SyntaxKind::SDBL_FUNCTION_CALL);
    let func_text = func_call.text().to_string();
    assert!(
        func_text.contains("Поставщик"),
        "Member access on CAST result must be preserved as a child of SdblFunctionCall. Got: {}",
        func_text
    );
    assert!(count_kind(&root, SyntaxKind::SDBL_TYPE) >= 1);
}

// CAST with simple MDO type (no member access).
#[test]
fn test_slice10b_cast_mdo_simple() {
    let root = parse_no_errors("ВЫБРАТЬ ВЫРАЗИТЬ(Поле КАК Справочник.Товары) ИЗ Т");
    assert!(count_kind(&root, SyntaxKind::SDBL_TYPE) >= 1);
}

// ============================================================================
// 10. CASE expressions (mini-spec §CASE expressions;
//     ITS pubqlang/40 canonical example)
// ============================================================================

// pubqlang/40 (chapter_040.html):
// «ВЫБОР КОГДА Товары.ЭтоГруппа = ИСТИНА ТОГДА "Это группа"
//        ИНАЧЕ "Это элемент" КОНЕЦ КАК ПризнакГруппы»
#[test]
fn test_slice10b_case_searched_canonical_russian() {
    let root = parse_no_errors(
        "ВЫБРАТЬ ВЫБОР КОГДА Товары.ЭтоГруппа = ИСТИНА ТОГДА \"Это группа\" ИНАЧЕ \"Это элемент\" КОНЕЦ КАК ПризнакГруппы ИЗ Т",
    );
    let case = first_of_kind(&root, SyntaxKind::SDBL_CASE_EXPR);
    let first_child_kind = case.children().next().map(|n| n.kind());
    assert_eq!(
        first_child_kind,
        Some(SyntaxKind::SDBL_WHEN_CLAUSE),
        "Searched CASE first child node must be SdblWhenClause (no operand). Got: {:?}",
        first_child_kind
    );
}

// Simple CASE — operand expression as first child node BEFORE
// any SdblWhenClause (HIR contract at case_expr.rs:40-45).
#[test]
fn test_slice10b_case_simple_form_operand_first() {
    let root = parse_no_errors("ВЫБРАТЬ ВЫБОР Поле КОГДА 1 ТОГДА \"А\" ИНАЧЕ \"Б\" КОНЕЦ ИЗ Т");
    let case = first_of_kind(&root, SyntaxKind::SDBL_CASE_EXPR);
    let first_child_kind = case.children().next().map(|n| n.kind());
    assert_ne!(
        first_child_kind,
        Some(SyntaxKind::SDBL_WHEN_CLAUSE),
        "Simple CASE first child node must be the operand, not SdblWhenClause. Got: {:?}",
        first_child_kind
    );
}

// CASE with multiple WHEN clauses.
#[test]
fn test_slice10b_case_multiple_when_clauses() {
    let root =
        parse_no_errors("ВЫБРАТЬ ВЫБОР КОГДА А = 1 ТОГДА \"X\" КОГДА А = 2 ТОГДА \"Y\" КОНЕЦ ИЗ Т");
    assert!(
        count_kind(&root, SyntaxKind::SDBL_WHEN_CLAUSE) >= 2,
        "CASE with two WHEN clauses must emit two SdblWhenClause nodes"
    );
}

// ============================================================================
// 11. column_or_function dispatch (mini-spec §Column references and
//     function calls)
// ============================================================================

// COUNT(*) — function call with bare `*` argument. Mini-spec §Atoms
// + §SdblFunctionCall.
#[test]
fn test_slice10b_count_asterisk() {
    let root = parse_no_errors("ВЫБРАТЬ КОЛИЧЕСТВО(*) ИЗ Т");
    assert!(count_kind(&root, SyntaxKind::SDBL_FUNCTION_CALL) >= 1);
}

// Dot-chain column reference — `Т.Х.Y` flat token chain.
#[test]
fn test_slice10b_dot_chain_column_ref() {
    let root = parse_no_errors("ВЫБРАТЬ Т.Х.Y ИЗ Т");
    let column_ref = first_of_kind(&root, SyntaxKind::SDBL_COLUMN_REF);
    let text = column_ref.text().to_string();
    assert!(text.contains("Т") && text.contains("Х") && text.contains("Y"));
}

// Inline tabular field syntax — Т.ТабЧасть.(Поле1, Поле2).
// Mini-spec §Inline tabular field syntax.
//
// SdblInlineTableFields wraps the result of `selected_fields()`
// (Slice 7), which emits an SdblFieldList containing
// SdblSelectedField children — so SdblSelectedField appears as a
// descendant (not a direct child) of SdblInlineTableFields.
#[test]
fn test_slice10b_inline_tabular_fields() {
    let root = parse_no_errors("ВЫБРАТЬ Т.ТабЧасть.(Поле1, Поле2) ИЗ Т");
    assert!(count_kind(&root, SyntaxKind::SDBL_INLINE_TABLE_FIELDS) >= 1);
    let inline = first_of_kind(&root, SyntaxKind::SDBL_INLINE_TABLE_FIELDS);
    let inner_field_count =
        inline.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_SELECTED_FIELD).count();
    assert!(
        inner_field_count >= 2,
        "SdblInlineTableFields must contain at least two SdblSelectedField descendants (one per field)"
    );
}

// DISTINCT / РАЗЛИЧНЫЕ aggregate prefix —
// pubqlang/21 (chapter_021.html, листинг 1.29):
// «КОЛИЧЕСТВО(РАЗЛИЧНЫЕ ЗаказТовара.Клиент)»
#[test]
fn test_slice10b_distinct_aggregate_prefix_canonical() {
    let root = parse_no_errors("ВЫБРАТЬ КОЛИЧЕСТВО(РАЗЛИЧНЫЕ ЗаказТовара.Клиент) ИЗ Т");
    assert!(count_kind(&root, SyntaxKind::SDBL_FUNCTION_CALL) >= 1);
    let func = first_of_kind(&root, SyntaxKind::SDBL_FUNCTION_CALL);
    assert!(
        func.text().to_string().to_uppercase().contains("РАЗЛИЧНЫЕ"),
        "Aggregate function with DISTINCT prefix must keep РАЗЛИЧНЫЕ in its text"
    );
}

// ============================================================================
// 12. Function-call clause-keyword recovery (codex Round-1 finding 2
//     → C2 FIX). Mini-spec §SdblFunctionCall + §IDE-recovery
//     allowances #15. MANDATORY per Slice 10b plan v7 §C3.
// ============================================================================

// EN regression gate for the C2 fix. Pre-C2 parser hijacked FROM as
// an Ident-shaped argument; post-C2 parser leaves FROM for the
// outer SELECT body.
#[test]
fn test_slice10b_func_call_clause_keyword_recovery_en() {
    let input = "SELECT func(x, FROM T)";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    let from_clauses =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE).count();
    assert!(
        from_clauses >= 1,
        "Outer SELECT must keep its FROM clause despite the unbalanced func call.\nTree: {:#?}",
        root
    );

    let func_call = first_of_kind(&root, SyntaxKind::SDBL_FUNCTION_CALL);
    let func_text = func_call.text().to_string();
    assert!(
        !func_text.to_uppercase().contains("FROM"),
        "Function call must NOT consume FROM as an argument: got `{}`",
        func_text
    );
}

// RU regression gate for the C2 fix.
#[test]
fn test_slice10b_func_call_clause_keyword_recovery_ru() {
    let input = "ВЫБРАТЬ функ(х, ИЗ Т)";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    let from_clauses =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE).count();
    assert!(
        from_clauses >= 1,
        "Outer ВЫБРАТЬ must keep its ИЗ clause despite the unbalanced func call.\nTree: {:#?}",
        root
    );

    let func_call = first_of_kind(&root, SyntaxKind::SDBL_FUNCTION_CALL);
    let func_text = func_call.text().to_string();
    assert!(
        !func_text.to_uppercase().contains("ИЗ"),
        "Function call must NOT consume ИЗ as an argument: got `{}`",
        func_text
    );
}

// ============================================================================
// 13. SELECT-field predicate descendant guards (codex Round-1
//     finding 3 → 3-of-13 consumer correction; Round-3 expansion to
//     5 named tests). MANDATORY per Slice 10b plan v7 §C3.
//
// Producer-side invariant: `expression(p)` always wraps in
// `logical_or_expr` (Slice 10a) so consumer-side
// `SdblSelectedField::expression()` (which directly matches only 3
// of the 13 Slice-10b kinds) reaches predicate / CASE descendants
// via `SdblLogicalOrExpr` traversal.
//
// Each guard test asserts:
//  1. SdblSelectedField direct child IS SdblLogicalOrExpr;
//  2. SdblSelectedField direct child IS NOT a bare predicate /
//     CASE / comparison node.
// ============================================================================

#[test]
fn test_slice10b_select_field_comparison_descendant_guard() {
    let kinds = first_selected_field_direct_child_kinds("ВЫБРАТЬ Поле = 1 ИЗ Т");
    assert!(
        kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR),
        "SelectedField must have SdblLogicalOrExpr as direct child. Got: {:?}",
        kinds
    );
    assert!(
        !kinds.contains(&SyntaxKind::SDBL_COMPARISON_EXPR),
        "SelectedField must NOT have bare SdblComparisonExpr. Got: {:?}",
        kinds
    );
}

#[test]
fn test_slice10b_select_field_in_descendant_guard() {
    let kinds = first_selected_field_direct_child_kinds("ВЫБРАТЬ Поле В (1, 2) ИЗ Т");
    assert!(kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR));
    assert!(!kinds.contains(&SyntaxKind::SDBL_IN_EXPR));
}

#[test]
fn test_slice10b_select_field_between_descendant_guard() {
    let kinds = first_selected_field_direct_child_kinds("ВЫБРАТЬ Поле МЕЖДУ 1 И 5 ИЗ Т");
    assert!(kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR));
    assert!(!kinds.contains(&SyntaxKind::SDBL_BETWEEN_EXPR));
}

#[test]
fn test_slice10b_select_field_is_null_descendant_guard() {
    let kinds = first_selected_field_direct_child_kinds("ВЫБРАТЬ Поле ЕСТЬ NULL ИЗ Т");
    assert!(kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR));
    assert!(!kinds.contains(&SyntaxKind::SDBL_IS_NULL_EXPR));
}

#[test]
fn test_slice10b_select_field_case_descendant_guard() {
    let kinds =
        first_selected_field_direct_child_kinds("ВЫБРАТЬ ВЫБОР КОГДА 1 = 1 ТОГДА \"А\" КОНЕЦ ИЗ Т");
    assert!(kinds.contains(&SyntaxKind::SDBL_LOGICAL_OR_EXPR));
    assert!(!kinds.contains(&SyntaxKind::SDBL_CASE_EXPR));
}

// ============================================================================
// 14. Preserved behaviour #2 — leading NOT capture and orphan-NOT
//     boundary (mini-spec §IDE-recovery allowances #14;
//     attestation §Preserved pre-refactor behaviours #2)
//
// `predicate_expr` consumes a leading `KwNot` BEFORE probing for
// IN/IS/BETWEEN/LIKE/REFS/comparison. The consumed NOT becomes a
// direct token child of the eventual predicate wrapper when a
// branch matches; if no branch matches, the marker is abandoned
// and the consumed NOT remains as a stray token in the syntax
// tree (the orphan-NOT boundary).
// ============================================================================

// NOT BETWEEN — KwNot direct child appears between operand and
// МЕЖДУ keyword inside SdblBetweenExpr.
#[test]
fn test_slice10b_not_between_captures_kwnot() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Поле НЕ МЕЖДУ 1 И 5");
    let between = first_of_kind(&root, SyntaxKind::SDBL_BETWEEN_EXPR);
    let between_text = between.text().to_string().to_uppercase();
    assert!(
        between_text.contains("НЕ"),
        "NOT BETWEEN must keep НЕ inside SdblBetweenExpr; got `{}`",
        between_text
    );
    assert!(between_text.contains("МЕЖДУ"));
}

// NOT LIKE — KwNot direct child appears between operand and
// ПОДОБНО keyword inside SdblLikeExpr.
#[test]
fn test_slice10b_not_like_captures_kwnot() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Поле НЕ ПОДОБНО \"X%\"");
    let like = first_of_kind(&root, SyntaxKind::SDBL_LIKE_EXPR);
    let like_text = like.text().to_string().to_uppercase();
    assert!(
        like_text.contains("НЕ"),
        "NOT LIKE must keep НЕ inside SdblLikeExpr; got `{}`",
        like_text
    );
    assert!(like_text.contains("ПОДОБНО"));
}

// Orphan-NOT no-branch boundary — input `1 НЕ 2` consumes `1` as
// additive operand, consumes `НЕ` as the leading NOT prefix, then
// finds neither a predicate keyword nor a comparison operator at
// `2`. The marker is abandoned and `НЕ` remains as a stray token
// in the syntax tree. Mini-spec §IDE-recovery allowances #14.
//
// The contract pinned here: NO predicate / comparison wrapper is
// emitted for this input, so a future rewrite that "fixes" the
// orphan-NOT boundary by emitting a partial wrapper would break
// this test and trigger an explicit decision in the next slice
// instead of silently changing the AST shape.
#[test]
fn test_slice10b_orphan_not_no_predicate_wrapper() {
    let parse = parse_sdbl("ВЫБРАТЬ * ИЗ Т ГДЕ 1 НЕ 2");
    let root = parse.syntax_node();
    let predicate_wrapper_count = root
        .descendants()
        .filter(|n| {
            matches!(
                n.kind(),
                SyntaxKind::SDBL_IN_EXPR
                    | SyntaxKind::SDBL_IN_HIERARCHY_EXPR
                    | SyntaxKind::SDBL_IS_NULL_EXPR
                    | SyntaxKind::SDBL_BETWEEN_EXPR
                    | SyntaxKind::SDBL_LIKE_EXPR
                    | SyntaxKind::SDBL_REFS_EXPR
                    | SyntaxKind::SDBL_COMPARISON_EXPR
            )
        })
        .count();
    assert_eq!(
        predicate_wrapper_count, 0,
        "Orphan-NOT input `1 НЕ 2` must NOT emit any predicate/comparison wrapper (mini-spec §IDE-recovery allowances #14). Tree:\n{:#?}",
        root
    );

    // The НЕ token must still appear in the tree as a stray
    // (consumed but not wrapped).
    let has_ne_token = root
        .descendants_with_tokens()
        .filter_map(|c| c.into_token())
        .any(|t| t.text().eq_ignore_ascii_case("НЕ"));
    assert!(
        has_ne_token,
        "Orphan НЕ must remain as a stray token in the syntax tree.\nTree:\n{:#?}",
        root
    );
}
