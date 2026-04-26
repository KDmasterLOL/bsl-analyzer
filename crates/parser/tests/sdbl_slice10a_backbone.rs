//! SDBL Slice 10a clean-room acceptance tests — expression backbone.
//!
//! This file is the spec-driven acceptance suite for the Slice 10a
//! clean-room rewrite in `crates/parser/src/grammar/sdbl/expressions.rs`
//! covering atoms (literals, parameters, parens / tuples / subqueries,
//! the bare `*` for `COUNT(*)`) plus the operator precedence chain
//! (logical OR / AND / NOT / additive / multiplicative / unary).
//!
//! Tests authored from:
//!   - `docs/legal/sdbl-expressions-mini-spec.md` (the C0a clean-room
//!     reference);
//!   - 1C ITS pubqlang documentation, accessed via the local dump at
//!     `/home/itrous/src/tools_migration/its/dump/`:
//!     - chapter 22 (`/db/pubqlang/content/22/hdoc`,
//!       "Как получить записи из таблицы, отобранные по некоторому
//!       условию") — WHERE clause, **logical-operator precedence
//!       ladder verbatim**, `И` / `ИЛИ` / `НЕ` operator inventory,
//!       МЕЖДУ, parens-override-precedence rule;
//!     - chapter 40 (`/db/pubqlang/content/40/hdoc`,
//!       "Примеры использования выражений в списке полей выборки
//!       запроса") — literal types verbatim, arithmetic operators
//!       (+, −, /, *) with explicit exclusion of `%`, string
//!       concatenation `+`, ВЫБОР, ВЫРАЗИТЬ, ССЫЛКА;
//!     - chapter 60 (`/db/pubqlang/content/60/hdoc`,
//!       "Передача параметров в запрос") — `&Identifier` parameter
//!       prefix, ПОДОБНО.
//!
//! The oracle is the mini-spec §AST-shape invariants and §Operator-
//! binding pin list, NOT any pre-rewrite parser implementation. Each
//! per-test comment cites the relevant mini-spec section / ITS
//! chapter so the post-Slice-10a-C2 parser is validated against the
//! mini-spec contract.
//!
//! `../bsl-parser/*` was not consulted during authoring of this file
//! per the Slice 10a attestation (`sdbl-clean-room-slice10a.md`)
//! §Non-consultation statement.

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

fn first_with_token(
    root: &syntax::SyntaxNode,
    wrapper_kind: SyntaxKind,
    direct_token: SyntaxKind,
) -> syntax::SyntaxNode {
    root.descendants()
        .filter(|n| n.kind() == wrapper_kind)
        .find(|n| {
            n.children_with_tokens()
                .filter_map(|c| c.into_token())
                .any(|t| t.kind() == direct_token)
        })
        .unwrap_or_else(|| {
            panic!(
                "Expected {wrapper_kind:?} with a direct {direct_token:?} token child in tree:\n{root:#?}"
            )
        })
}

// ============================================================================
// 1. Entry-point dispatch (mini-spec §Expression entry points)
// ============================================================================

// `expression` (called from SELECT field position) and
// `logical_expression` (called from WHERE) both delegate to
// logical_or_expr; the simplest input `A OR B` produces an
// SdblLogicalOrExpr at the top of the dispatch chain in both
// contexts. Mini-spec §Expression entry points.
#[test]
fn test_slice10a_expression_entry_in_select_field() {
    let root = parse_no_errors("ВЫБРАТЬ А ИЛИ Б ИЗ Т");
    let or_count = count_kind(&root, SyntaxKind::SDBL_LOGICAL_OR_EXPR);
    assert!(
        or_count >= 1,
        "SELECT-field `expression` entry must produce at least one SdblLogicalOrExpr",
    );
}

#[test]
fn test_slice10a_logical_expression_entry_in_where() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ А ИЛИ Б");
    let or_count = count_kind(&root, SyntaxKind::SDBL_LOGICAL_OR_EXPR);
    assert!(
        or_count >= 1,
        "WHERE `logical_expression` entry must produce at least one SdblLogicalOrExpr",
    );
}

// ============================================================================
// 2. Operator precedence ladder (ITS pubqlang/22 verbatim quote)
// ============================================================================

// ITS pubqlang/22: «сначала вычисляются простые логические выражения,
// затем операции НЕ, затем операции И, в последнюю очередь – операции
// ИЛИ». NOT > AND > OR. `НЕ А И Б` parses as `(НЕ А) И Б` —
// SdblLogicalAndExpr contains an SdblNotExpr that wraps А alone.
#[test]
fn test_slice10a_precedence_not_binds_tighter_than_and() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ НЕ А И Б");
    let and_with_kw =
        first_with_token(&root, SyntaxKind::SDBL_LOGICAL_AND_EXPR, SyntaxKind::KW_AND);
    // The SdblNotExpr must be a direct child of the AND wrapper that
    // owns the operator; its text must contain А but NOT Б — meaning
    // НЕ wrapped А alone, not the full А И Б.
    let not_under_and = and_with_kw
        .children()
        .find(|n| n.kind() == SyntaxKind::SDBL_NOT_EXPR)
        .expect("SdblNotExpr must be a direct child of the SdblLogicalAndExpr");
    let not_text = not_under_and.text().to_string();
    assert!(
        not_text.contains('А') && !not_text.contains('Б'),
        "НЕ must wrap А alone, not the А И Б pair; got {not_text:?}",
    );
}

// ITS pubqlang/22: «затем операции И, в последнюю очередь – операции
// ИЛИ». AND binds tighter than OR. `А ИЛИ Б И В` parses as
// `А ИЛИ (Б И В)` — SdblLogicalOrExpr contains an SdblLogicalAndExpr
// at the right operand position.
#[test]
fn test_slice10a_precedence_and_binds_tighter_than_or() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ А ИЛИ Б И В");
    let or_with_kw = first_with_token(&root, SyntaxKind::SDBL_LOGICAL_OR_EXPR, SyntaxKind::KW_OR);
    let and_under_or =
        or_with_kw.children().find(|n| n.kind() == SyntaxKind::SDBL_LOGICAL_AND_EXPR);
    assert!(
        and_under_or.is_some(),
        "SdblLogicalAndExpr must sit under SdblLogicalOrExpr (AND binds tighter than OR)",
    );
}

// ITS pubqlang/40 §Арифметические операции — multiplicative tighter
// than additive (standard SQL convention adopted by mini-spec).
// `А + Б * В` parses as `А + (Б * В)` — SdblAdditiveExpr contains an
// SdblMultiplicativeExpr at the right operand position.
#[test]
fn test_slice10a_precedence_mul_binds_tighter_than_add() {
    let root = parse_no_errors("ВЫБРАТЬ А + Б * В ИЗ Т");
    let add_with_plus = first_with_token(&root, SyntaxKind::SDBL_ADDITIVE_EXPR, SyntaxKind::PLUS);
    let mul_under_add =
        add_with_plus.children().find(|n| n.kind() == SyntaxKind::SDBL_MULTIPLICATIVE_EXPR);
    assert!(
        mul_under_add.is_some(),
        "SdblMultiplicativeExpr must sit under SdblAdditiveExpr (* tighter than +)",
    );
}

// Mini-spec §Operator-binding pin list item 1 + ITS pubqlang/22:
// multi-NOT is right-recursive; `НЕ НЕ А` parses as nested
// `SdblNotExpr( SdblNotExpr( А ) )`.
#[test]
fn test_slice10a_multi_not_right_recursive() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ НЕ НЕ А");
    let outer_not = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_NOT_EXPR)
        .expect("outer SdblNotExpr");
    let inner_not = outer_not
        .children()
        .find(|n| n.kind() == SyntaxKind::SDBL_NOT_EXPR)
        .expect("inner SdblNotExpr nested directly inside outer (right-recursive)");
    assert!(
        inner_not.text().to_string().contains("НЕ"),
        "Inner SdblNotExpr text must include the second НЕ token",
    );
}

// Mini-spec §Operator-binding pin list item 2: multi-unary is
// right-recursive; `- - А` parses as nested SdblUnaryExpr.
#[test]
fn test_slice10a_multi_unary_right_recursive() {
    let root = parse_no_errors("ВЫБРАТЬ - - А ИЗ Т");
    let outer_unary = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_UNARY_EXPR)
        .expect("outer SdblUnaryExpr");
    let inner_unary = outer_unary
        .children()
        .find(|n| n.kind() == SyntaxKind::SDBL_UNARY_EXPR)
        .expect("inner SdblUnaryExpr nested directly inside outer (right-recursive)");
    assert!(
        inner_unary.text().to_string().contains('-'),
        "Inner SdblUnaryExpr text must include the second `-` token",
    );
}

// ============================================================================
// 3. FLAT operator wrapper invariant (mini-spec §AST-shape #1)
// ============================================================================

// `А + Б + В` produces a SINGLE SdblAdditiveExpr with 3 expression
// children + 2 PLUS direct token children — FLAT, not nested.
// Consumer at sdbl-hir/src/lower/expr/ops.rs:42 collects all direct
// children into a Vec.
#[test]
fn test_slice10a_flat_additive_three_operands() {
    let root = parse_no_errors("ВЫБРАТЬ А + Б + В ИЗ Т");
    let additives: Vec<_> =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_ADDITIVE_EXPR).collect();
    assert_eq!(additives.len(), 1, "Exactly one SdblAdditiveExpr — wrapper is FLAT not nested");
    let plus_count = additives[0]
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .filter(|t| t.kind() == SyntaxKind::PLUS)
        .count();
    assert_eq!(
        plus_count, 2,
        "FLAT SdblAdditiveExpr for `А + Б + В` must have exactly 2 PLUS direct token children",
    );
}

// `А ИЛИ Б ИЛИ В` produces a SINGLE SdblLogicalOrExpr with 3 expression
// children + 2 KW_OR tokens — FLAT.
#[test]
fn test_slice10a_flat_logical_or_three_operands() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ А ИЛИ Б ИЛИ В");
    // Find the OR wrapper that owns the operator (the one with direct
    // KW_OR tokens — empty single-child wrappers exist throughout the
    // chain).
    let or_with_kw = first_with_token(&root, SyntaxKind::SDBL_LOGICAL_OR_EXPR, SyntaxKind::KW_OR);
    let or_count = or_with_kw
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .filter(|t| t.kind() == SyntaxKind::KW_OR)
        .count();
    assert_eq!(
        or_count, 2,
        "FLAT SdblLogicalOrExpr for `А ИЛИ Б ИЛИ В` must have exactly 2 KW_OR direct token children",
    );
}

// ============================================================================
// 4. Trivia-before-operator invariant (mini-spec §Trivia handling)
// ============================================================================

// `1\n+\n2` parses correctly with newlines around the operator —
// p.skip_trivia() runs BEFORE the operator probe per the load-bearing
// CRITICAL invariant. Asserts FLAT SdblAdditiveExpr with direct PLUS
// + 2 operand children.
#[test]
fn test_slice10a_trivia_newlines_around_operator() {
    let root = parse_no_errors("ВЫБРАТЬ 1\n+\n2 ИЗ Т");
    let add_with_plus = first_with_token(&root, SyntaxKind::SDBL_ADDITIVE_EXPR, SyntaxKind::PLUS);
    let add_text = add_with_plus.text().to_string();
    assert!(
        add_text.contains("\n") && add_text.contains('+'),
        "SdblAdditiveExpr text must preserve trivia (newlines around `+`); got {add_text:?}",
    );
}

// Multi-line whitespace + tabs around operator: `1\n\t+\t\n2` — the
// p.skip_trivia() before-probe pattern handles arbitrary whitespace
// trivia, not only newlines.
#[test]
fn test_slice10a_trivia_mixed_whitespace_around_operator() {
    let root = parse_no_errors("ВЫБРАТЬ 1\n\t+\t\n2 ИЗ Т");
    let add_with_plus = first_with_token(&root, SyntaxKind::SDBL_ADDITIVE_EXPR, SyntaxKind::PLUS);
    let add_text = add_with_plus.text().to_string();
    assert!(
        add_text.contains('+') && add_text.contains('\n') && add_text.contains('\t'),
        "SdblAdditiveExpr text must preserve mixed whitespace/newline/tab trivia around `+`; got {add_text:?}",
    );
}

// ============================================================================
// 5. Atom coverage (mini-spec §Atoms + ITS pubqlang/40 §Литералы)
// ============================================================================

// ITS pubqlang/40 §Литералы — numeric / string / boolean (Истина,
// Ложь) / Null / Неопределено; all parse as SdblLiteral atoms.
#[test]
fn test_slice10a_atom_numeric_literal() {
    let root = parse_no_errors("ВЫБРАТЬ 222.77 ИЗ Т");
    assert!(
        count_kind(&root, SyntaxKind::SDBL_LITERAL) >= 1,
        "Numeric literal must emit SdblLiteral",
    );
}

#[test]
fn test_slice10a_atom_russian_boolean_true_false() {
    let root_true = parse_no_errors("ВЫБРАТЬ ИСТИНА ИЗ Т");
    assert!(count_kind(&root_true, SyntaxKind::SDBL_LITERAL) >= 1, "ИСТИНА must emit SdblLiteral",);
    let root_false = parse_no_errors("ВЫБРАТЬ ЛОЖЬ ИЗ Т");
    assert!(count_kind(&root_false, SyntaxKind::SDBL_LITERAL) >= 1, "ЛОЖЬ must emit SdblLiteral",);
}

#[test]
fn test_slice10a_atom_undefined_literal() {
    let root = parse_no_errors("ВЫБРАТЬ НЕОПРЕДЕЛЕНО ИЗ Т");
    assert!(count_kind(&root, SyntaxKind::SDBL_LITERAL) >= 1, "НЕОПРЕДЕЛЕНО must emit SdblLiteral",);
}

// ITS pubqlang/60 §Передача параметров в запрос — `&Identifier`
// parameter prefix syntax. Slice 10a `parameter_expr` emits
// SdblParameter wrapping Ampersand + Ident.
#[test]
fn test_slice10a_atom_parameter_with_identifier() {
    let root = parse_no_errors("ВЫБРАТЬ &ДатаНачала ИЗ Т");
    let param = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_PARAMETER)
        .expect("SdblParameter must exist");
    // The lexer may fuse `&Identifier` into a single token (one
    // SDBL_PARAMETER token kind) or split it into two (AMPERSAND +
    // IDENT). The clean-room contract is: parser-side, no
    // `p.skip_trivia()` call between bumps and the resulting node
    // covers the full `&ДатаНачала` text. We accept either
    // tokenisation as correct.
    assert!(
        param.text().to_string().contains("&ДатаНачала"),
        "SdblParameter text must cover the full `&ДатаНачала` source",
    );
}

// ============================================================================
// 6. Multi-string concatenation (mini-spec §Atoms + IDE-recovery)
// ============================================================================

// Mini-spec §Atoms string literal — 2+ consecutive String tokens
// emit SdblMultiString. Single String emits SdblLiteral. Local IDE
// allowance for multi-line BSL query strings.
#[test]
fn test_slice10a_string_literal_wrapper_covers_user_string() {
    // BSL/SDBL string-lexer note: the lexer splits a user-visible
    // string `"X"` into multiple internal STRING tokens (typically
    // an opening `"`, content tokens, and a closing `"`). The
    // Slice 10a parser's `string_literal_or_multi` collects every
    // **consecutive** run of STRING tokens (with NO trivia between
    // them) into one wrapper. count == 1 → SdblLiteral; count >= 2
    // → SdblMultiString. Because the lexer-internal split typically
    // produces 3 STRING tokens per user-visible string, even a
    // single user-typed string `"Мария"` lands in SdblMultiString.
    //
    // This test asserts the wrapper exists and covers the full
    // user-visible string text. Two user-visible strings separated
    // by whitespace (e.g. `"a" "b"`) is NOT valid BSL string-
    // concatenation syntax (BSL uses `+` for that — see ITS
    // pubqlang/40 §"Операцию конкатенации строк (+)"); each
    // user-visible string lands in its own wrapper.
    let root = parse_no_errors(r#"ВЫБРАТЬ "Мария" ИЗ Т"#);
    let wrapper = root
        .descendants()
        .find(|n| {
            (n.kind() == SyntaxKind::SDBL_MULTI_STRING || n.kind() == SyntaxKind::SDBL_LITERAL)
                && n.text().to_string().contains("\"Мария\"")
        })
        .expect("`\"Мария\"` must land in some SdblLiteral / SdblMultiString wrapper");
    assert!(
        wrapper.text().to_string().starts_with('"') && wrapper.text().to_string().ends_with('"'),
        "Wrapper text must cover the full user-visible string including delimiters",
    );
}

#[test]
fn test_slice10a_string_concat_via_plus() {
    // ITS pubqlang/40: «Операцию конкатенации строк (+)». Two
    // user-visible strings concatenated via `+` parse as
    // SdblAdditiveExpr containing two string-literal-bearing
    // wrappers (the parser doesn't distinguish string-`+` from
    // numeric-`+` at parse time — that's a HIR semantic concern).
    let root = parse_no_errors(r#"ВЫБРАТЬ "a" + "b" ИЗ Т"#);
    let add_with_plus = first_with_token(&root, SyntaxKind::SDBL_ADDITIVE_EXPR, SyntaxKind::PLUS);
    let add_text = add_with_plus.text().to_string();
    assert!(
        add_text.contains("\"a\"") && add_text.contains("\"b\""),
        "SdblAdditiveExpr text must cover both string operands; got {add_text:?}",
    );
}

// ============================================================================
// 7. Tuple vs paren dispatch (mini-spec §Atoms paren dispatch)
// ============================================================================

// Single expression in parens emits SdblParenExpr (NOT SdblTupleExpr).
#[test]
fn test_slice10a_paren_single_expression() {
    let root = parse_no_errors("ВЫБРАТЬ (1) ИЗ Т");
    assert!(count_kind(&root, SyntaxKind::SDBL_PAREN_EXPR) >= 1, "(1) must emit SdblParenExpr",);
    assert_eq!(
        count_kind(&root, SyntaxKind::SDBL_TUPLE_EXPR),
        0,
        "(1) must NOT emit SdblTupleExpr",
    );
}

// 2+ comma-separated expressions in parens emit SdblTupleExpr.
#[test]
fn test_slice10a_tuple_two_expressions() {
    let root = parse_no_errors("ВЫБРАТЬ (1, 2) ИЗ Т");
    let tuple = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_TUPLE_EXPR)
        .expect("(1, 2) must emit SdblTupleExpr");
    let comma_count = tuple
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .filter(|t| t.kind() == SyntaxKind::COMMA)
        .count();
    assert_eq!(comma_count, 1, "SdblTupleExpr for (1, 2) must have 1 COMMA direct token child",);
}

#[test]
fn test_slice10a_tuple_three_expressions() {
    let root = parse_no_errors("ВЫБРАТЬ (1, 2, 3) ИЗ Т");
    let tuple = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SDBL_TUPLE_EXPR)
        .expect("(1, 2, 3) must emit SdblTupleExpr");
    let comma_count = tuple
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .filter(|t| t.kind() == SyntaxKind::COMMA)
        .count();
    assert_eq!(comma_count, 2, "SdblTupleExpr for (1, 2, 3) must have 2 COMMA direct tokens");
}

// ============================================================================
// 8. SELECT-keyword-only lookahead (mini-spec §AST-shape #6)
// ============================================================================

// Mini-spec §Atoms paren dispatch — `(SELECT ...)` and
// `(ВЫБРАТЬ ...)` route to subquery branch. `(&T)` and `(1)` route
// to expression branch.
#[test]
fn test_slice10a_paren_select_routes_to_subquery() {
    let root = parse_no_errors("ВЫБРАТЬ Поле ИЗ Т ГДЕ Поле = (ВЫБРАТЬ 1)");
    assert!(
        count_kind(&root, SyntaxKind::SDBL_SUBQUERY_EXPR) >= 1,
        "(ВЫБРАТЬ ...) must route to subquery branch and emit SdblSubqueryExpr",
    );
}

// `(&T)` parsed in expression context routes to the expression
// branch (no SELECT keyword), emits SdblParenExpr wrapping a
// parameter_expr emission. Note: opposite routing decision from the
// FROM-context `data_source` (Slice 8) where any `(` routes to
// subquery-source.
#[test]
fn test_slice10a_paren_parameter_routes_to_expression_branch() {
    let root = parse_no_errors("ВЫБРАТЬ (&ДатаНачала) ИЗ Т");
    assert!(
        count_kind(&root, SyntaxKind::SDBL_PAREN_EXPR) >= 1,
        "(&ДатаНачала) in expression context must emit SdblParenExpr",
    );
    assert_eq!(
        count_kind(&root, SyntaxKind::SDBL_SUBQUERY_EXPR),
        0,
        "(&ДатаНачала) in expression context must NOT route to subquery branch",
    );
    assert!(
        count_kind(&root, SyntaxKind::SDBL_PARAMETER) >= 1,
        "Inner SdblParameter must be present",
    );
}

// ============================================================================
// 9. Bilingual operators (ITS pubqlang/12 + /22)
// ============================================================================

// ITS pubqlang/12: «все ключевые слова имеют два варианта написания:
// на русском и английском языках». Test mixed RU/EN logical operators.
#[test]
fn test_slice10a_bilingual_or_and_not() {
    // English form
    let root_en = parse_no_errors("SELECT * FROM Т WHERE NOT А AND Б OR В");
    assert!(
        count_kind(&root_en, SyntaxKind::SDBL_LOGICAL_OR_EXPR) >= 1
            && count_kind(&root_en, SyntaxKind::SDBL_LOGICAL_AND_EXPR) >= 1
            && count_kind(&root_en, SyntaxKind::SDBL_NOT_EXPR) >= 1,
        "English NOT/AND/OR forms must produce all three operator wrapper kinds",
    );
    // Russian form
    let root_ru = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ НЕ А И Б ИЛИ В");
    assert!(
        count_kind(&root_ru, SyntaxKind::SDBL_LOGICAL_OR_EXPR) >= 1
            && count_kind(&root_ru, SyntaxKind::SDBL_LOGICAL_AND_EXPR) >= 1
            && count_kind(&root_ru, SyntaxKind::SDBL_NOT_EXPR) >= 1,
        "Russian НЕ/И/ИЛИ forms must produce all three operator wrapper kinds",
    );
}

// ============================================================================
// 10. Modulo `%` local IDE allowance (mini-spec §IDE-recovery #9)
// ============================================================================

// ITS pubqlang/40: «Операция получения остатка % в языке запросов
// не поддерживается». The Slice 10a parser preserves `%` acceptance
// as a local IDE-recovery allowance — the input parses without
// errors and produces SdblMultiplicativeExpr containing the `%`
// token between two operands. The IDE reports the misuse via
// diagnostics rather than aborting the whole query.
#[test]
fn test_slice10a_modulo_local_ide_allowance() {
    let root = parse_no_errors("ВЫБРАТЬ А % Б ИЗ Т");
    let mul_with_percent =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_MULTIPLICATIVE_EXPR).find(|n| {
            n.children_with_tokens()
                .filter_map(|c| c.into_token())
                .any(|t| t.kind() == SyntaxKind::PERCENT)
        });
    assert!(
        mul_with_percent.is_some(),
        "Slice 10a preserves `%` acceptance as a local IDE allowance — recoverable parse tree expected",
    );
}

// ============================================================================
// 11. Slice 10a × Slice 6 / 7 / 8 cross-slice integration
// ============================================================================

// Slice 10a × Slice 6: subquery-in-expression position with UNION.
// Exercises paren_or_subquery_expr → select::subquery → union_clause.
#[test]
fn test_slice10a_x_slice6_subquery_with_union_in_expression() {
    let root = parse_no_errors("ВЫБРАТЬ Поле ИЗ Т ГДЕ Поле = (ВЫБРАТЬ 1 ОБЪЕДИНИТЬ ВСЕ ВЫБРАТЬ 2)");
    assert!(
        count_kind(&root, SyntaxKind::SDBL_SUBQUERY_EXPR) >= 1
            && count_kind(&root, SyntaxKind::SDBL_UNION_CLAUSE) >= 1,
        "Subquery in expression position with UNION must emit both SdblSubqueryExpr and SdblUnionClause",
    );
}

// Slice 10a × Slice 7: SELECT field with operator-chain expression
// + alias. Exercises selected_field → expression → additive_expr.
#[test]
fn test_slice10a_x_slice7_field_with_arithmetic_alias() {
    let root = parse_no_errors("ВЫБРАТЬ А + Б КАК Сумма ИЗ Т");
    assert!(
        count_kind(&root, SyntaxKind::SDBL_ADDITIVE_EXPR) >= 1
            && count_kind(&root, SyntaxKind::SDBL_ALIAS) >= 1,
        "SELECT field with arithmetic + alias must emit SdblAdditiveExpr and SdblAlias",
    );
}

// Slice 10a × Slice 8: pure-Slice-10a WHERE content (no comparison
// operators which would route through Slice 10b predicate_expr_legacy).
// Exercises WHERE → logical_expression → logical_or_expr → … →
// primary_expr atoms.
#[test]
fn test_slice10a_x_slice8_pure_logical_where_with_from_chain() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ ИСТИНА И ЛОЖЬ");
    assert!(
        count_kind(&root, SyntaxKind::SDBL_LOGICAL_AND_EXPR) >= 1,
        "Pure-Slice-10a WHERE content with bilingual booleans must emit SdblLogicalAndExpr",
    );
    assert!(
        count_kind(&root, SyntaxKind::SDBL_FROM_CLAUSE) >= 1,
        "Slice 8 SdblFromClause must still be present",
    );
}

// ============================================================================
// 12. NULL-bug-fix structural gate (mini-spec §Atoms primary dispatch)
// ============================================================================

// Slice 10a C2 fixed the dispatch order so bare NULL emits
// SdblLiteral, not SdblColumnRef. Tests in `sdbl_parser_tests.rs`
// (`test_slice10a_bare_null_emits_literal_not_column_ref` and
// `test_slice10a_select_field_null_emits_literal`) gate the
// structural assertion. This file's gate exercises the SELECT-field
// position from a different angle: NULL inside an OR expression
// must still emit SdblLiteral wrapping the IDENT token.
#[test]
fn test_slice10a_null_inside_or_emits_literal() {
    let root = parse_no_errors("ВЫБРАТЬ * ИЗ Т ГДЕ Поле = ИСТИНА ИЛИ NULL");
    let null_token = root
        .descendants_with_tokens()
        .filter_map(|c| c.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT && t.text().eq_ignore_ascii_case("NULL"))
        .expect("NULL Ident token must be present");
    assert_eq!(
        null_token.parent().map(|p| p.kind()),
        Some(SyntaxKind::SDBL_LITERAL),
        "Bare NULL inside OR expression must still emit SdblLiteral (not SdblColumnRef)",
    );
}

// ============================================================================
// 13. Slice 12 recover_to_delimiter clause-keyword-at-any-depth gates.
//
// Regression for the Slice 12 fix (commit landing on
// `legal/sdbl-slice8-addendum-clean-room`). Pre-fix,
// `recover_to_delimiter` checked `is_clause_keyword` only when
// `paren_depth == 0`, so an unterminated nested `(...)` inside a
// function-call argument silently gobbled the outer query's clause
// keyword. Post-fix, the clause-keyword check fires at any depth,
// mirroring `recover_to_delimiter_vt` (Slice 8-addendum post-C3
// fix `7e4f6a9e`).
//
// The trigger input uses a literal `1` (NOT an Ident) as the first
// argument so that `column_or_function` does not consume the
// following `(` as a nested function-call start; the literal forces
// `expression(p)` to return, leaving the outer recovery to enter
// `recover_to_delimiter` at depth 0 and reach depth 1 via the bare
// `(` token.
// ============================================================================

#[test]
fn test_slice10a_recover_to_delimiter_stops_on_clause_keyword_at_any_depth_ru() {
    let input = "ВЫБРАТЬ СУММА(1 ( ИЗ T2";
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
        "ИЗ clause keyword must not be consumed by recover_to_delimiter at depth>0.\nTree: {:#?}",
        root
    );
}

#[test]
fn test_slice10a_recover_to_delimiter_stops_on_clause_keyword_at_any_depth_en() {
    let input = "SELECT SUM(1 ( FROM T2";
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
        "FROM clause keyword must not be consumed by recover_to_delimiter at depth>0.\nTree: {:#?}",
        root
    );
}

// ============================================================================
// 14. Slice 12 codex Round-5 stop-hook fix —
//     `recover_to_delimiter` does NOT stop at nested SELECT/UNION
//     at paren_depth > 0, so the outer query's clause-tail (e.g.
//     `ИЗ T`) survives an unterminated nested subquery `(`.
//
// The original Slice 12 promotion of `is_clause_keyword` to ANY
// paren depth was overly broad: at depth>0 a `SELECT`/`ВЫБРАТЬ`
// most likely starts a nested subquery whose body recovery should
// absorb. The post-stop-hook fix keeps hard intra-clause keywords
// (FROM/WHERE/GROUP/...) at any-depth stops but reverts statement-
// starters / combiners (SELECT/UNION) to depth-0-only stops via
// `is_query_starter_or_combiner_keyword` (Codex Round-5).
// ============================================================================

#[test]
fn test_slice10a_recover_to_delimiter_does_not_stop_on_nested_select_at_depth_ru() {
    let input = "ВЫБРАТЬ СУММА(1 ( ВЫБРАТЬ X )) ИЗ T";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    // The outer ИЗ T must be parsed as a FROM clause; pre-fix the
    // depth-1 ВЫБРАТЬ stopped recovery prematurely and the outer
    // FROM was lost.
    let from_clauses =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE).count();
    assert!(
        from_clauses >= 1,
        "Outer ИЗ T must survive an unterminated nested ВЫБРАТЬ subquery body.\nTree: {:#?}",
        root
    );
}

#[test]
fn test_slice10a_recover_to_delimiter_does_not_stop_on_nested_select_at_depth_en() {
    let input = "SELECT SUM(1 ( SELECT X )) FROM T";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    let from_clauses =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE).count();
    assert!(
        from_clauses >= 1,
        "Outer FROM T must survive an unterminated nested SELECT subquery body.\nTree: {:#?}",
        root
    );
}

// ============================================================================
// 15. Slice 12 codex Round-5b stop-hook —
//     inner-FROM misattribution gate.
//
// Round-5 made nested SELECT-at-depth recoverable (the outer query's
// trailing tail survived an unterminated nested ВЫБРАТЬ subquery
// HEAD), but the residual gap was: when the nested subquery itself
// contains a hard intra-clause keyword (e.g. ИЗ Y), recovery still
// stopped at that inner ИЗ, exposing it to the outer parser, which
// misattributed it as the OUTER FROM clause and lost the real outer
// `ИЗ T`. Round-5b adds a `nested_query_starts: Vec<i32>` tracker:
// while inside an active nested subquery body, hard intra-clause
// keywords belong to the nested query and are absorbed.
// ============================================================================

#[test]
fn test_slice10a_recover_to_delimiter_inner_from_misattribution_gate() {
    let input = "ВЫБРАТЬ СУММА(1 ( ВЫБРАТЬ X ИЗ Y )) ИЗ T";
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    // The outer query must reference table T as its FROM clause,
    // not the inner Y.
    let from_clauses: Vec<_> =
        root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_FROM_CLAUSE).collect();
    let outer_from_text = from_clauses.first().map(|fc| fc.text().to_string()).unwrap_or_default();
    assert!(
        outer_from_text.contains('T') && !outer_from_text.contains('Y'),
        "Outer FROM clause must reference T, not the inner Y; got {outer_from_text:?}.\nTree: {:#?}",
        root
    );
}
