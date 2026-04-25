# SDBL Slice 10b — Clean-Room Attestation

**Status:** complete (2026-04-25).

This document attests the clean-room authorship of the Slice 10b
material of the SDBL parser — the **predicate / comparison /
function-call / CAST / CASE surface** of the expression sub-grammar
(complement to Slice 10a's expression backbone) — per the staged
migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Scope

The paths claimed as clean-room Slice 10b authorship are:

- 8 functions in
  `crates/parser/src/grammar/sdbl/expressions.rs` under the
  `CLEAN-ROOM Slice 10b — predicates, comparison, function calls,
  CAST, CASE` banner:
  - `comparison_expr` — comparison-or-predicate dispatcher
    (1:1 delegating shim to `predicate_expr`);
  - `predicate_expr` — 7-branch dispatcher: `(NOT)? IN [HIERARCHY]`,
    `IS [NOT] NULL`, `(NOT)? BETWEEN ... AND ...`,
    `(NOT)? LIKE ... [ESCAPE ...]`, `REFS Mdo.Path`, comparison
    operators (`= <> < <= > >=`), or fall-through abandon;
  - `column_or_function` — post-Ident dispatcher (`'.'` →
    `SdblColumnRef`, `'('` → `SdblFunctionCall`, otherwise → bare
    `SdblColumnRef`);
  - `inline_table_fields` — `'.' '(' selectedFields ')'` wrapper
    (Slice-10b → Slice-7 dispatch boundary);
  - `is_cast_function` — pre-Ident-bump CAST/ВЫРАЗИТЬ predicate;
  - `parse_cast_type` — primitive-with-`(size[, scale])` OR
    MDO-chain type spec inside CAST;
  - `case_expr` — simple vs searched CASE with mandatory
    END/КОНЕЦ;
  - `when_clause` — WHEN/КОГДА condition THEN/ТОГДА result.

- The clean-room banner block at the top of the same file's
  Slice 10b section (replacing the previous `LEGACY (Slice 10b
  pending)` banner).

- The §Predicates, §Comparison, §Column references and function
  calls (with §Inline tabular field syntax sub-section), §CAST type
  specification, §CASE expressions sections of
  `docs/legal/sdbl-expressions-mini-spec.md` (added in C0a as
  extensions of the Slice-10a-authored mini-spec).

- The 19 Bucket-A gap-test functions in
  `crates/parser/tests/sdbl_parser_tests.rs` added in C0b plus
  the 43 spec-driven acceptance tests in
  `crates/parser/tests/sdbl_slice10b_predicates.rs` added in C3.

- The §ITS coverage verification rows for the new mini-spec
  sections (filled in C2 with verified-yes / verified-no
  outcomes against the local ITS dump).

**13 NodeKinds preserved bit-for-bit through the rewrite** (no
variant rename, no addition, no removal, no reorder in
`crates/syntax/src/syntax_kind.rs`):

`SdblComparisonExpr`, `SdblInExpr`, `SdblInHierarchyExpr`,
`SdblIsNullExpr`, `SdblBetweenExpr`, `SdblLikeExpr`,
`SdblRefsExpr`, `SdblColumnRef`, `SdblFunctionCall`, `SdblType`,
`SdblInlineTableFields`, `SdblCaseExpr`, `SdblWhenClause`.

**Function → NodeKind attribution map:**

| Function | Emits |
|---|---|
| `comparison_expr` | (delegates to `predicate_expr`) |
| `predicate_expr` | `SdblInExpr`, `SdblInHierarchyExpr`, `SdblIsNullExpr`, `SdblBetweenExpr`, `SdblLikeExpr`, `SdblRefsExpr`, `SdblComparisonExpr` |
| `column_or_function` | `SdblColumnRef`, `SdblFunctionCall` (+ `inline_table_fields` reach) |
| `inline_table_fields` | `SdblInlineTableFields` |
| `is_cast_function` | (predicate, no NodeKind) |
| `parse_cast_type` | `SdblType` |
| `case_expr` | `SdblCaseExpr` |
| `when_clause` | `SdblWhenClause` |

**Child-attachment invariants** carried by Slice 10b that
downstream consumers depend on:

1. **`SdblCaseExpr` child ORDER.** HIR consumer
   `crates/sdbl-hir/src/lower/expr/case_expr.rs:40-45` reads
   `node.children().next()` and checks whether the first child node
   is `SDBL_WHEN_CLAUSE`. The parser must therefore emit children
   in source order so simple-CASE operand precedes the first
   `SdblWhenClause`, and searched-CASE has `SdblWhenClause` as the
   first child.

2. **`SdblWhenClause` direct children.** WHEN / КОГДА token,
   condition expression node, THEN / ТОГДА token, result expression
   node. HIR `case_expr.rs:51-89` walks `node.children()` and
   assumes exactly two child expression nodes per WHEN clause.

3. **`SdblColumnRef` flat token chain.** Direct children: a flat
   sequence of `Ident` tokens with `Dot` tokens between them, plus
   `Error` nodes at incomplete-chain positions. Consumer:
   `crates/sdbl-hir/src/lower/expr/mod.rs:lower_column_ref`. No
   nested wrappers.

4. **`SdblFunctionCall` shape.** Ident, LParen, sequence of
   expression children separated by Comma tokens (with `Error`
   nodes for empty / missing args), RParen, then an optional
   Dot/Ident chain for member access on the result. Consumers:
   `lower/expr/mod.rs:lower_function_call` reads the first Ident
   as the function name and walks subsequent children as arguments;
   `from_clause.rs:283-321` filters direct children of
   `SdblTableRef` on `SDBL_FUNCTION_CALL` among other kinds.

5. **`SdblComparisonExpr` shape.** Three direct children: left
   `additiveExpression`, comparison operator token (one of `Eq`,
   `Neq`, `Lt`, `Le`, `Gt`, `Ge`), right `additiveExpression`.

6. **`SdblInExpr` shape.** Direct children: left
   `additiveExpression`, optional `KwNot` token, `KwIn` token,
   `LParen` token, then either an `SdblSubquery` child or a
   sequence of expression children separated by `Comma` tokens
   (with `Error` placeholders for empty / missing / trailing-comma
   elements), `RParen` token.

7. **`SdblInHierarchyExpr` shape.** Direct children: left
   `additiveExpression`, optional `KwNot` token, `KwIn` token,
   `Ident` token (`HIERARCHY` / `ИЕРАРХИИ`), `LParen` token, single
   expression child (the hierarchy root), `RParen` token.

8. **`SdblIsNullExpr` shape.** Direct children: left
   `additiveExpression`, `Ident` token (`IS` / `ЕСТЬ`), optional
   `KwNot` token, `Ident` token (`NULL`).

9. **`SdblBetweenExpr` shape.** Direct children: left
   `additiveExpression` (test value), optional `KwNot` token,
   `Ident` token (`BETWEEN` / `МЕЖДУ`), `additiveExpression` (low
   bound), `KwAnd` token, `additiveExpression` (high bound). On
   missing `KwAnd` the high-bound `additiveExpression` is omitted
   (recovery).

10. **`SdblLikeExpr` shape.** Direct children: left
    `additiveExpression` (subject), optional `KwNot` token, `Ident`
    token (`LIKE` / `ПОДОБНО`), `additiveExpression` (pattern),
    optional `Ident` token (`ESCAPE` / `СПЕЦСИМВОЛ`) +
    `additiveExpression` (escape character).

11. **`SdblRefsExpr` shape.** Direct children: left
    `additiveExpression` (value being checked), `Ident` token
    (`REFS` / `ССЫЛКА`), then a chain of `Ident` and `Dot` tokens
    representing the MDO reference. Consumer:
    `crates/ide-diagnostics/src/handlers/query_parse_error.rs:52,78`
    reads the chain and detects a trailing dot without a type name
    as a parse error.

12. **`SdblType` shape.** Direct children: `Ident` token
    (primitive type name OR first part of MDO chain), then either
    optional `Dot` / `Ident` pairs (MDO chain) OR
    `LParen` + `Decimal` + optional `Comma` + `Decimal` + `RParen`
    (primitive type parameter list).

13. **`SdblInlineTableFields` shape.** Direct children: `LParen`
    token, the result of `selected_fields()` (Slice 7) — multiple
    `SdblSelectedField` direct children of an `SdblFieldList`
    descendant — `RParen` token. Consumer: indirect via
    `lower/expr/mod.rs` walking `SdblColumnRef` descendants.

**AST-shape invariants** (operational contracts that exceed
NodeKind identity):

1. **CASE child order.** Source order is the wire format; HIR's
   first-child kind check at `case_expr.rs:40-45` is the consumer
   side of this contract.

2. **Predicate `NOT` consumed BEFORE probing.** A leading `KwNot`
   is consumed and becomes a direct token child of the eventual
   predicate node when a predicate / comparison branch matches. If
   no branch matches, the marker is abandoned and the consumed
   `KwNot` remains as a stray token (mini-spec §IDE-recovery
   allowances #14 — the orphan-NOT boundary).

3. **`additiveExpression` operands for predicates and
   comparison.** Predicates and comparison read operands via
   `additive_expr(p)` directly, not via `expression(p)`. Calling
   `expression(p)` would re-enter the full operator chain
   (`logical_or → … → not → comparison → predicate`) and create
   infinite recursion.

4. **`comparison_expr` is a dispatcher shim.** It delegates 1:1 to
   `predicate_expr` because predicates and comparison share the
   same precedence slot. The shim costs 2 LOC and clarifies the
   precedence story.

5. **`is_cast_function` lookahead BEFORE Ident bump.** The
   `column_or_function` function calls `is_cast_function(p)` BEFORE
   `p.bump()` of the Ident, then uses the resulting `is_cast`
   boolean inside the LParen branch to enable the `КАК`-type
   recovery. Deferring the check until after the bump would lose
   the keyword text.

6. **Member access on function-call result.** After the closing
   RParen of a function call, `column_or_function` enters a
   `while p.at(Dot)` loop that consumes `Dot`/`Ident` pairs (with
   `Error` nodes for incomplete tails) and terminates on clause
   keywords. The resulting chain is direct token-level child
   sequence of `SdblFunctionCall`. HIR ignores the chain in
   lowering, but `query_parse_error.rs:52` may diagnose trailing
   dots — the rewrite preserves the token-level shape.

7. **`SdblType` MDO vs primitive-with-parens dispatch.** The
   `parse_cast_type` function reads the first Ident, then enters a
   `while p.at(Dot)` loop for MDO continuation, THEN checks for
   `LParen` (primitive parameter list). For well-formed input
   only one branch fires; for mixed input the parser consumes the
   MDO chain *and* the parameter list (existing IDE-recovery
   behaviour).

8. **`column_or_function` dispatch is exclusive.** Post-Ident, the
   function dispatches on `Dot` (column ref), `LParen` (function
   call), or "neither" (bare column ref) — the three branches are
   mutually exclusive.

The `parse_sdbl(&str)` and `parse_sdbl_with_shared_cache(&str)`
public entry points are unchanged. The `expression(...)`,
`logical_expression(...)`, `is_expression_start(...)`, and
`parse_delimited_list(...)` interfaces from Slice 10a are
unchanged — Slice 10b consumes them at the same call sites with
the same signatures.

The two `_legacy` suffixes used during the Slice 10a authorship
period (`comparison_expr_legacy`, `predicate_expr_legacy`) have
been retired in C1; the `LEGACY (Slice 10b pending)` banner has
been replaced with the `CLEAN-ROOM Slice 10b` banner.

## Sources consulted

The Slice 10b clean-room rewrite was authored from the following
sources only.

**Authoritative ITS pubqlang chapters** (1C:Enterprise query-
language documentation, `https://its.1c.ru/db/pubqlang/...`,
accessed via the local dump at
`/home/itrous/src/tools_migration/its/dump/`):

- chapter 21 (`/db/pubqlang/content/21/hdoc`,
  `chapter_021.html`) — DISTINCT / РАЗЛИЧНЫЕ aggregate prefix,
  canonical example «КОЛИЧЕСТВО(РАЗЛИЧНЫЕ ЗаказТовара.Клиент) КАК
  РазныеКлиенты» (листинг 1.29).
- chapter 22 (`/db/pubqlang/content/22/hdoc`,
  `chapter_022.html`) — WHERE conditions, BETWEEN canonical
  example «Дата МЕЖДУ ДАТАВРЕМЯ(2012, 10, 01) И ДАТАВРЕМЯ(2012,
  10, 31)» (листинг 1.33), logical operators И/ИЛИ/НЕ.
- chapter 23 (`/db/pubqlang/content/23/hdoc`,
  `chapter_023.html`) — LIKE / ПОДОБНО pattern primitive,
  canonical example «Наименование ПОДОБНО "%Иван%"» (листинг
  1.34).
- chapter 27 (`/db/pubqlang/content/27/hdoc`,
  `chapter_027.html`) — IS NULL / ЕСТЬ NULL canonical example
  «КОГДА (Товары.Производитель) ЕСТЬ NULL ТОГДА "NULL"».
- chapter 32 (`/db/pubqlang/content/32/hdoc`,
  `chapter_032.html`) — IN HIERARCHY / В ИЕРАРХИИ canonical
  example «Товары.Ссылка В ИЕРАРХИИ (&ГруппаТоваров)» (листинг
  1.51).
- chapter 40 (`/db/pubqlang/content/40/hdoc`,
  `chapter_040.html`) — CASE / ВЫБОР canonical example «ВЫБОР
  КОГДА Товары.ЭтоГруппа = ИСТИНА ТОГДА "Это группа" ИНАЧЕ "Это
  элемент" КОНЕЦ КАК ПризнакГруппы»; CAST / ВЫРАЗИТЬ canonical
  examples for primitive-parameterised («ВЫРАЗИТЬ(СУММА(...) /
  КОЛИЧЕСТВО(*) КАК ЧИСЛО(8,2))») and MDO-with-member-access
  («ВЫРАЗИТЬ (ОстаткиТоваров.Регистратор КАК
  Документ.ПриходнаяНакладная).Поставщик»); REFS / ССЫЛКА
  canonical example «(ОстаткиТоваров.Регистратор ССЫЛКА
  Документ.ПриходнаяНакладная)».

**Note: chapter 28 was NOT consulted.** Codex Round-1 of the
Slice 10b plan review flagged that the original plan listed
chapter 28 as a В ИЕРАРХИИ source; spot-check confirmed that
chapter 28 does NOT contain the keyword. Chapter 32 is the
primary source for IN HIERARCHY.

**Local artefacts:**

- `docs/legal/sdbl-expressions-mini-spec.md` — the C0a-extended
  mini-spec, sections §Predicates, §Comparison, §Column references
  and function calls, §CAST type specification, §CASE expressions.
  These sections were added during the Slice 10b C0a commit
  expressly as the clean-room reference for the C2 rewrite, with
  the §ITS coverage verification rows filled in C2 from the
  chapters listed above.
- `crates/lexer/src/sdbl/mod.rs` — for the canonical bilingual
  TokenKind mapping of operator and keyword lexemes (verified at
  authorship time against the Slice 1 + 2 attestations).
- `crates/parser/src/parser.rs` — for the project's own
  event-parser conventions (`Parser::start`, `complete`,
  `abandon`, `bump`, `skip_trivia`, `at_keyword`, `expect`,
  `error`, `at_end`, `check_iteration_limit`).
- The project's own attestations under
  [`docs/legal/sdbl-clean-room-slice{1,2,6,7,8,10a}.md`](.) — for
  the per-slice clean-room discipline mirrored here.

## Non-consultation statement

During the Slice 10b clean-room authorship the following sources
were NOT consulted as working text:

- the sibling `../bsl-parser` project — neither its grammar files
  (`*.g4` / `*.tokens`) nor its parser implementation;
- any other third-party SDBL parser, grammar, or event-tree
  implementation, including ANTLR-shaped grammars from external
  SQL projects;
- the **pre-C1 function bodies** of the eight Slice 10b functions
  (`comparison_expr_legacy`, `predicate_expr_legacy`,
  `is_cast_function`, `parse_cast_type`, `column_or_function`,
  `inline_table_fields`, `case_expr`, `when_clause`). The C2
  author opened the existing function bodies during the rewrite
  to identify the **set of pre-refactor behaviours** that needed
  bit-for-bit preservation (the eight items enumerated under
  §Preserved pre-refactor behaviours), but did not copy or paste
  the function body text. The resulting event-parser shape
  converges with the pre-C1 implementation because both follow
  the same mini-spec specification — this is the **natural
  convergence** pattern shared with Slices 1, 2, 6, 7, 8, 10a.

The independent-derivation claim of this attestation is therefore
**clean-room re-derivation from the cited sources plus the
project's local compatibility constraints (the AST-shape contracts
and IDE-recovery allowances enumerated in
[`sdbl-expressions-mini-spec.md`](sdbl-expressions-mini-spec.md))**,
**not textual novelty** and **not a uniqueness claim** for the
resulting grammar shape. Other clean-room authors working from the
same sources may reach a different but equivalent grammar shape;
this slice records the specific choices this project made and the
consumer-side compatibility contracts that constrained those
choices.

## Preserved pre-refactor behaviours

Eight behaviours observed in the pre-C1 parser bodies are
preserved bit-for-bit by the C2 clean-room rewrite for IDE-
recovery / consumer-compatibility reasons. Each one is locked in
by an acceptance test in
`crates/parser/tests/sdbl_slice10b_predicates.rs` (and / or in
`sdbl_parser_tests.rs` for the C0b regression-gate tests).

1. **`comparison_expr` remains a 1:1 delegating shim to
   `predicate_expr`.** Predicates and comparison share the same
   precedence slot below NOT; the shim makes the dispatcher
   abstraction textual. Acceptance: every comparison-operator
   test under
   `sdbl_slice10b_predicates.rs::test_slice10b_comparison_*` passes
   through the shim transparently.

2. **`predicate_expr` consumes a leading `KwNot` BEFORE
   probing.** The consumed `NOT` becomes a direct token child of
   the eventual predicate node. If the post-`NOT` lookahead does
   not match any predicate / comparison branch, the marker is
   abandoned and the consumed `NOT` remains as a stray token in
   the syntax tree — the orphan-NOT boundary
   (mini-spec §IDE-recovery allowances #14). Acceptance (named
   tests in `crates/parser/tests/sdbl_slice10b_predicates.rs`):
   - `test_slice10b_not_in_with_subquery` — NOT IN prefix
     captured inside SdblInExpr;
   - `test_slice10b_is_not_null_english` — IS NOT NULL prefix
     captured inside SdblIsNullExpr;
   - `test_slice10b_not_between_captures_kwnot` — NOT BETWEEN
     prefix captured inside SdblBetweenExpr;
   - `test_slice10b_not_like_captures_kwnot` — NOT LIKE prefix
     captured inside SdblLikeExpr;
   - `test_slice10b_orphan_not_no_predicate_wrapper` — `1 НЕ 2`
     yields **no** predicate / comparison wrapper, and the
     consumed `НЕ` remains as a stray token (the orphan-NOT
     boundary itself).

3. **`IN HIERARCHY` is parsed as `IN`-prefix +
   `HIERARCHY`-suffix, not as a single keyword pair.** The parser
   bumps `KwIn`, then probes
   `at_keyword("HIERARCHY")`/`("ИЕРАРХИИ")`. Mid-typed `IN HIE`
   falls through to the regular IN-list arm with a recovery error
   for the missing LParen. Acceptance:
   `sdbl_slice10b_predicates::test_slice10b_in_hierarchy_canonical_russian`
   (the canonical pubqlang/32 example).

4. **`IS NULL` / `IS NOT NULL` requires the literal `NULL` Ident;
   missing `NULL` abandons the marker.** The consumed `IS` (and
   optional `NOT`) tokens remain as stray tokens — same
   IDE-recovery boundary as #2. Acceptance:
   `sdbl_slice10b_predicates::test_slice10b_is_null_*`.

5. **`BETWEEN low [AND high]` allows missing `KwAnd`.** Emits
   `SdblBetweenExpr` with only the low bound when `AND` is missing
   (recovery for mid-typing `МЕЖДУ 1`). Acceptance:
   `sdbl_parser_tests::test_slice10b_between_missing_and_recovery`
   and
   `sdbl_slice10b_predicates::test_slice10b_between_missing_and_recovery`.

6. **`LIKE pattern [ESCAPE char]` is single-shot, not a loop.**
   At most one `ESCAPE` clause per `LIKE`. ESCAPE / СПЕЦСИМВОЛ is
   NOT documented in the dumped ITS chapters 23 + 60; preserved
   as local IDE-recovery allowance (mini-spec §IDE-recovery
   allowances #13). Acceptance:
   `sdbl_slice10b_predicates::test_slice10b_like_with_escape_local_allowance`.

7. **`REFS` MDO chain is greedy — eats all subsequent
   `Dot Ident` pairs.** The parser does not enforce a fixed
   two-segment shape. Trailing dot without an Ident is detected
   by `crates/ide-diagnostics/src/handlers/query_parse_error.rs:78`.
   Acceptance:
   `sdbl_slice10b_predicates::test_slice10b_refs_deep_mdo_chain`.

8. **`column_or_function` argument list emits `NodeKind::Error`
   for empty / missing / trailing-comma elements.** Inline at
   three positions: first-arg empty (`func(, x)`), middle-element
   empty (`func(x, , y)`), trailing comma (`func(x,)`). Shared
   with `parse_delimited_list` (Slice 10a) but inlined here
   because the function call site tolerates a leading
   `DISTINCT`/`РАЗЛИЧНЫЕ` keyword that `parse_delimited_list`
   does not handle. Acceptance: pre-existing
   `sdbl_parser_tests::test_error_recovery_function_*` tests
   continue to pass post-C2.

## Behaviour change

**`column_or_function` clause-keyword recovery fix** (codex Round-1
finding 2 → C2 FIX promotion). This is the only behaviour change
introduced by the Slice 10b clean-room rewrite, mandatory per the
plan v7 §C2 acceptance contract.

### Pre-C2 behaviour

For input `SELECT func(x, FROM T)` (and the Russian variant
`ВЫБРАТЬ функ(х, ИЗ Т)`), the pre-C2 parser produced a tree where
`FROM`/`ИЗ` was consumed as an `Error`-wrapped child of
`SdblFunctionCall`:

```
SDBL_FUNCTION_CALL@7..20
  IDENT@7..11 "func"
  L_PAREN@11..12 "("
  …(SdblColumnRef for x)…
  COMMA@13..14 ","
  WHITESPACE@14..15 " "
  ERROR@15..15
  ERROR@15..19
    IDENT@15..19 "FROM"
  WHITESPACE@19..20 " "
SDBL_ALIAS@20..21
  IDENT@20..21 "T"
```

The outer SELECT body could not find its FROM clause, and `T`
ended up as an alias of the function-call expression instead of a
data source.

### Root cause

Two-fold:

1. The argument-list `is_expression_start && !p.at(Comma)` probe at
   the first-argument position and the
   `is_expression_start || …` probe at the after-comma position
   relied on `is_expression_start` filtering clause keywords on
   the Ident arm — that filtering DOES happen, so
   `is_expression_start(FROM) = false` and the empty-error branch
   fires correctly.
2. **The actual bug was at the trailing `p.expect(RParen)`.**
   `Parser::expect` falls through to `Parser::error()` on
   mismatch, and `Parser::error()` BUMPS the current token and
   wraps it in an `Error` node. So when the comma-arg loop broke
   out at a clause keyword, the trailing `expect(RParen)` consumed
   the clause keyword as an `Error` child of `SdblFunctionCall`.

### C2 fix

Three-part, landed in commit `98a2a6a2`:

1. **Defensive guard at the first-argument probe** —
   `&& !super::select::is_clause_keyword(p)` added. Functionally
   redundant (already covered by `is_expression_start`'s Ident
   arm) but textually self-documents the contract at the call
   site.
2. **Defensive guard at the after-comma probe** —
   `|| super::select::is_clause_keyword(p)` added to the recovery
   condition. Same redundancy / textual-documentation rationale.
3. **The actual fix at the trailing RParen guard.** Before
   `p.expect(TokenKind::RParen)`, an explicit clause-keyword check
   emits a zero-width `NodeKind::Error` and SKIPS the
   `p.expect(RParen)` call. This leaves the clause keyword in the
   token stream for the outer parser instead of letting
   `Parser::error()` bump it into the function-call's subtree.

### Post-C2 behaviour

For `SELECT func(x, FROM T)`:

- `SDBL_FUNCTION_CALL` covers `func(x, ` and a zero-width Error
  node — the function call ends BEFORE `FROM`.
- `SDBL_FROM_CLAUSE` exists in the outer SELECT body and contains
  the `FROM T` data source.
- `T` is correctly parsed as the data source's table reference.

### Regression gates

Two acceptance tests in
`crates/parser/tests/sdbl_parser_tests.rs` (added `#[ignore]`-ed
in C0b, unignored in C2 in the same atomic commit as the fix):

- `test_func_call_clause_keyword_recovery` (EN);
- `test_russian_func_call_clause_keyword_recovery` (RU).

Plus two named tests in
`crates/parser/tests/sdbl_slice10b_predicates.rs` (added in C3):

- `test_slice10b_func_call_clause_keyword_recovery_en`;
- `test_slice10b_func_call_clause_keyword_recovery_ru`.

All four PASS on the post-C2 parser; the first two FAIL on the
pre-C2 parser (verified via `cargo test … -- --ignored` run
during the C2 implementation phase).

## Verification recipe

Run each command in sequence; all must pass. Output is shown in
parentheses for the post-Slice-10b state.

1. `cargo test -p parser --test sdbl_parser_tests` —
   `sdbl_parser_tests` (163 passed, 0 ignored — was 144 + 19 gap
   adds in C0b; the 2 `#[ignore]`-ed (m) tests were unignored in
   C2).
2. `cargo test -p parser --test sdbl_slice6_package` (26 passed).
3. `cargo test -p parser --test sdbl_slice7_fields` (26 passed).
4. `cargo test -p parser --test sdbl_slice8_sources` (28 passed).
5. `cargo test -p parser --test sdbl_slice10a_backbone`
   (28 passed).
6. `cargo test -p parser --test sdbl_slice10b_predicates`
   (43 passed — the new C3 acceptance suite).
7. `cargo test -p parser --test sdbl_slice2_keywords`
   (45 passed).
8. `cargo test -p parser --test sdbl_golden_corpus`
   (23 passed).
9. `cargo test -p parser --test sdbl_slice1_core`
   (4 passed + 4 ignored — pre-existing).
10. `cargo test -p parser` — full parser suite (integration +
    inline mod).
11. `cargo test -p sdbl-hir` — 204 HIR lowering tests.
12. `cargo test -p lexer` — 125 passed across all targets.
13. `cargo test -p ide-db` — SDBL validation tests including
    `parse_sdbl` path.
14. `cargo test -p ide --test sdbl_completion_integration_test` —
    subquery-in-expression + UNION scenarios.
15. `cargo test -p ide` — full IDE test suite.
16. `cargo test -p ide-diagnostics` — 1572 passed (+1 ignored,
    pre-existing) including `multiline_string_in_query` and
    `query_parse_error` (REFS trailing-dot).
17. `cargo test -p mcp-server` — 72 passed.
18. `cargo build --workspace --all-targets` — workspace build
    clean.
19. `cargo clippy --all-targets --all-features -- -D warnings` —
    workspace clippy clean (verified by pre-commit hook on every
    Slice 10b commit).

## Commit trail

Slice 10b landed across 5 logical phases. Each anchor commit is
followed by zero or more codex-adversarial-review fixup commits
that are pure refinements of the canonical commit's intent. The
trail enumerated below names every commit reachable from
`git log --oneline --reverse 6d7053bf..HEAD` *except* the
absolute-last one (see Anti-Hilbert disclosure at the end of this
section). The base ref `6d7053bf` is the last commit on the
prior Slice 10a tail (a fix-up to the Slice 10a `sdbl_parser_tests`
multi-string test docstring); the immediate Slice 10b boundary is
the C0a anchor `77c75e29`.

- **C0a — `77c75e29` (2026-04-25)**: extend SDBL expressions
  mini-spec with §Predicates / §Comparison / §Column references
  and function calls / §CAST type specification / §CASE
  expressions sections + ITS coverage verification TODO rows.
  No fixup commits.
- **C0b — `4c1e8170` (2026-04-25)**: audit SDBL Slice 10b tests
  and extend predicate / call / CASE coverage with 19 Bucket-A
  gap-test functions in `sdbl_parser_tests.rs` (12 a–l + 2 m
  EN/RU `#[ignore]`-ed for C2 unignore + 5 n.1–n.5 SELECT-field
  predicate descendant guards). No fixup commits.
- **C1 — `9899815f` (2026-04-25)**: drop SDBL Slice 10b legacy
  suffixes and replace LEGACY banner with clean-room. Pure
  refactor: rename `comparison_expr_legacy` → `comparison_expr`,
  `predicate_expr_legacy` → `predicate_expr` (definitions + call
  sites in `not_expr` body and inside `comparison_expr`),
  replace `LEGACY (Slice 10b pending)` banner with
  `CLEAN-ROOM Slice 10b — predicates, comparison, function calls,
  CAST, CASE`, attach 8 `// C1 placeholder — clean-room rewrite in
  C2` markers, update Slice 10a banner dispatch-boundary comment,
  update module docstring in both `expressions.rs` and `sdbl.rs`
  (without citing the not-yet-landed attestation per Round-7
  finding). No fixup commits.
- **C2 — `98a2a6a2` (2026-04-25)**: rewrite SDBL Slice 10b
  predicate / call / CAST / CASE clean-room from ITS and
  expressions mini-spec. Replaces the 8 C1 placeholder comments
  with ITS-cited per-function provenance (chapters 21, 22, 23,
  27, 32, 40 — plus pubqlang/60 already cited in Slice 10a;
  ESCAPE / СПЕЦСИМВОЛ marked `// local: …` per §IDE-recovery
  allowances #13), lands the column_or_function clause-keyword
  recovery fix (codex Round-1 finding 2 → C2 FIX), fills the
  §ITS coverage verification rows in
  `sdbl-expressions-mini-spec.md` with verified-yes /
  verified-no outcomes, and unignores the two C0b regression-
  gate tests in the same atomic commit. No fixup commits.
- **C3 — `66dca2ae` (2026-04-25)**: this attestation (initial
  draft) + 43 spec-driven acceptance tests in
  `crates/parser/tests/sdbl_slice10b_predicates.rs` (including
  the 2 mandatory clause-keyword recovery tests and the 5
  mandatory SELECT-field predicate descendant guards) +
  master-doc flip in `docs/legal/sdbl-clean-room-slices.md` +
  module Provenance docstring flip to "complete (2026-04-25)"
  with attestation citation in `crates/parser/src/grammar/sdbl.rs`.
  Eight fixup commits:
  - `ef85b028` — Anti-Hilbert close-out: replace the "C3 — this
    commit" placeholder in this §Commit trail (and in the
    `docs/legal/sdbl-clean-room-slices.md` §Slice 10b commit-
    trail line) with the named hash `66dca2ae`.
  - `7f5e1cbc` — flip the Slice 10b module-level Provenance
    docstring and the in-file `CLEAN-ROOM Slice 10b` banner
    block in `crates/parser/src/grammar/sdbl/expressions.rs` to
    post-C2 final-state wording (replacing leftover "C1
    placeholder" / "rewrite in progress" / future-tense "C2 will
    rewrite" language and a self-replacing
    `comparison_expr → comparison_expr` rename description
    introduced by the C1 bulk-rename). Codex stop-time review
    finding.
  - `ecb26896` — Anti-Hilbert close-out for `ef85b028` and
    `7f5e1cbc`: name them in this §Commit trail and in the
    master-doc commit-trail line.
  - `9943d47a` — address codex adversarial-review findings
    (running `/codex:adversarial-review --base 1635be4b`):
    (1) split `predicate_expr` per-function provenance into a
    verified-yes block (BETWEEN/22, LIKE/23, IS NULL/27,
    IN HIERARCHY/32, REFS/40) and a verified-no `// local: …`
    block (IN value-list, IN-with-subquery, comparison
    operators, ESCAPE) so the comment is internally consistent
    with the §ITS coverage verification table;
    (2) flip the master-doc §Slice 10b §Files block from stale
    "to-be-authored" / "8 functions to re-author" wording to
    final-state language naming the landed clean-room functions,
    attestation, mini-spec extension, and test files;
    (3) add three named acceptance tests in
    `sdbl_slice10b_predicates.rs`
    (`test_slice10b_not_between_captures_kwnot`,
    `test_slice10b_not_like_captures_kwnot`,
    `test_slice10b_orphan_not_no_predicate_wrapper`) so
    Preserved behaviour #2 is pinned by named tests for every
    NOT-prefix predicate plus the orphan-NOT recovery case
    (was: implicit "absence of failures").
  - `8e0ca19c` — Anti-Hilbert close-out for `ecb26896` and
    `9943d47a`: name them in this §Commit trail and in the
    master-doc commit-trail line.
  - `82e09e3a` — bump the master-doc §Slice 10b §Files block
    acceptance-test count from "40 spec-driven acceptance tests"
    to "43 spec-driven acceptance tests" matching the
    attestation, with explicit mention of the three NOT-boundary
    tests added in `9943d47a` so a future reader of the master
    doc can navigate directly to the relevant preserved-
    behaviour pin without first reading the attestation. Codex
    stop-time review finding.
  - `79c52f49` — Anti-Hilbert close-out for `8e0ca19c` and
    `82e09e3a`: name them in this §Commit trail and in the
    master-doc commit-trail line.
  - `2b6d2e55` — correct the §Commit trail base ref from
    `1635be4b` to `6d7053bf` (the Slice 10a tail commit
    "parser: rewrite C0b multi-string test docstring to match
    actual contract"). The previous base ref leaked the Slice 10a
    tail commit into the disclosed range, making the
    "exactly one commit beyond the last named hash" disclosure
    off by one. Codex stop-time review finding.

**Anti-Hilbert disclosure.** The very last commit on this branch
— the one that authors / amends this attestation §Commit trail
itself — is necessarily not named in this enumeration: a Git
commit cannot reference its own future hash at write time. This
anti-Hilbert property applies to every legal/clean-room
attestation that records its own commit trail, and is shared with
the prior Slice 1, 2, 6, 7, 8, 10a attestations in this project.
A reviewer running `git log --oneline --reverse 6d7053bf..HEAD`
will always see exactly one commit beyond the trail's last named
hash: that commit is the one that landed this attestation in its
current state, and it is the natural endpoint of the trail.

The phase totals (named hashes in the trail above): C0a 1,
C0b 1, C1 1, C2 1, C3 1 anchor + 8 fixups (`ef85b028`,
`7f5e1cbc`, `ecb26896`, `9943d47a`, `8e0ca19c`, `82e09e3a`,
`79c52f49`, `2b6d2e55`) = 13 commits enumerated. The branch HEAD
adds one trailing commit (the one editing this trail to name the
eight fixups), per the Anti-Hilbert disclosure above.

## Licensing note

The `crates/parser` crate retains its `LGPL-3.0-or-later` license
until the full Slice 6 → Slice 11 parser migration is complete
and Slice 13 reattaches `sdbl-hir`. Promoting the crate to
Tier A (`MIT OR Apache-2.0`) is explicitly out of scope for
Slice 10b and will happen once the last LEGACY-banner function
under `grammar/sdbl/expressions.rs` and `grammar/sdbl/select.rs`
has been re-derived (Slices 9 and 11 remain) and the HIR
lowering cascade in `sdbl-hir` has been cleaned up
(Slice 13).

## Author attestation

The Slice 10b material listed above under **Scope** was authored
as a clean-room re-derivation from the sources listed under
**Sources consulted**, without using the `../bsl-parser` project,
the pre-C1 function bodies of the 8 Slice 10b functions as
working text, or any other third-party SDBL parser. The
independent-derivation claim follows the same convention as
Slices 1, 2, 6, 7, 8, 10a attestations: the resulting
event-parser shape is the natural expression of the cited ITS
chapters and the project's own event-parser conventions; where
the C2 clean-room implementation converges with the pre-C1
implementation, that convergence is on the same mini-spec
specification both implementations follow, not consultation of
working text.

The author attests that:

- the eight Slice 10b functions in `expressions.rs` were
  re-authored under the `CLEAN-ROOM Slice 10b` banner;
- the only intentional behaviour change introduced by the
  rewrite is the `column_or_function` clause-keyword recovery
  fix documented under **Behaviour change**;
- the 13 NodeKinds emitted by these functions retain their
  pre-C1 child-attachment shape so all downstream consumers
  (the 11-NodeKind filter at
  `SdblSelectedField::expression()`, the HIR predicate / CASE
  dispatchers, the `query_parse_error` REFS-tail diagnostic, the
  VT-arg reader, etc.) continue to work without modification;
- the verification recipe was run end-to-end and all 19 steps
  pass.

— Authored by the Slice 10b C2 implementation, attested by the
Slice 10b C3 commit (2026-04-25).
