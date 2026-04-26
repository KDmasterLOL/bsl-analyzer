# SDBL Slice 10a — Clean-Room Attestation

**Status:** complete (2026-04-25).

This document attests the clean-room authorship of the Slice 10a
material of the SDBL parser — the **expression backbone**: atoms
(literals, parameters, parens / tuples / subqueries, the bare `*`
for `COUNT(*)`) plus the operator precedence chain (logical OR /
AND / NOT / additive / multiplicative / unary) — per the staged
migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Scope

The paths claimed as clean-room Slice 10a authorship are:

- `crates/parser/src/grammar/sdbl.rs` — specifically the Slice 10a
  bullet of the module-level `## Provenance` docstring and the
  renamed `Slices 9, 10b, 11 pending` bullet enumerating the
  remaining LEGACY-banner functions.
- `crates/parser/src/grammar/sdbl/expressions.rs` — specifically
  the 17 functions declared under the
  `CLEAN-ROOM Slice 10a — expression backbone` banner with their
  per-function provenance comments:
    - `is_expression_start` — predicate accepting literals,
      non-clause-keyword `Ident`, unary-op starters, `LParen`,
      `Ampersand`, `Star`; the `_ => at_keyword("CASE" | "ВЫБОР" |
      "NULL")` fallback is unreachable under the current
      `Parser::at_keyword` API and is kept for textual symmetry
      with `primary_expr`'s keyword-probe pattern.
    - `is_recovery_point` — predicate returning true at any token
      in the caller's `recovery_set`, any clause keyword, or
      end-of-input.
    - `recover_to_delimiter` — paren-depth-tracking error recovery
      so `ВЫРАЗИТЬ(поле КАК СТРОКА(200))` recovery walks to the
      outer `)`, not the inner one; emits one `Error` marker
      around the consumed run.
    - `parse_delimited_list` — generic delimited-list helper with
      empty-element + trailing-delimiter recovery emitting `Error`
      placeholders.
    - `logical_expression` — `pub fn` entry point for WHERE /
      HAVING / JOIN ON clause bodies; delegates to the operator
      chain at the bottom of the precedence ladder.
    - `expression` — `pub fn` entry point for SELECT fields,
      ORDER BY items, function-call args, etc.; currently
      identical to `logical_expression` (Slice 12 may merge).
    - `logical_or_expr` — left-associative FLAT wrapper for
      chained OR; emits `SdblLogicalOrExpr`.
    - `logical_and_expr` — left-associative FLAT wrapper for
      chained AND; emits `SdblLogicalAndExpr`.
    - `not_expr` — right-recursive multi-NOT; emits
      `SdblNotExpr`. Delegates to Slice 10b
      `comparison_expr_legacy` in the non-NOT branch — the only
      Slice-10a → Slice-10b dispatch boundary in this file.
    - `additive_expr` — left-associative FLAT wrapper for
      `+`/`-`; emits `SdblAdditiveExpr`. `skip_trivia` BEFORE
      operator probe per the load-bearing `CRITICAL` invariant.
    - `multiplicative_expr` — left-associative FLAT wrapper for
      `*`/`/`/`%`; emits `SdblMultiplicativeExpr`. The `%`
      acceptance is a preserved local IDE-recovery allowance, NOT
      ITS-supported (see §Preserved pre-refactor behaviours #7).
    - `unary_expr` — right-recursive `+`/`-`/`NOT` prefix; emits
      `SdblUnaryExpr`.
    - `primary_expr` — keyword-probe-FIRST dispatcher (CASE /
      ВЫБОР, NULL) before the generic `Some(TokenKind::Ident) =>
      column_or_function(p)` arm; the NULL probe is the **decisive
      gate** that fixes the pre-Slice-10a-C2 bug routing bare
      `NULL` through `column_or_function`.
    - `literal_expr` — dispatcher between the multi-string
      collector and a single-token `SdblLiteral`.
    - `string_literal_or_multi` — collects 2+ consecutive
      `String` tokens into `SdblMultiString`; single `String`
      emits `SdblLiteral` (preserved as IDE-recovery allowance
      for multi-line BSL query strings).
    - `parameter_expr` — `&Identifier` parameter prefix with NO
      `p.skip_trivia()` between the `Ampersand` bump and the
      `Ident` bump; the bare-`&` recovery shape (single
      `Ampersand` token, no `Ident`) is preserved per Slice 8
      attestation §Preserved-behaviour #7.
    - `paren_or_subquery_expr` — SELECT-keyword-only lookahead
      routes to `select::subquery` for `SdblSubqueryExpr`;
      otherwise `expression(s)` → `SdblParenExpr` (single child)
      or `SdblTupleExpr` (2+ comma-separated children).
- `crates/parser/tests/sdbl_slice10a_backbone.rs` — the new
  spec-driven acceptance test file authored against ITS pubqlang
  /22 + /40 + /60 and the C0a mini-spec, NOT against the
  pre-Slice-10a-C2 `parse_sdbl` output.
- `docs/legal/sdbl-expressions-mini-spec.md` — the C0a clean-room
  reference document (authored as a separate C0a commit before
  C2; the §ITS coverage verification section was filled at C2).

The following 12 `SyntaxKind` node kinds are locked in place by
Slice 10a (no rename, no addition, no removal, no enum reorder):

- `SdblLogicalOrExpr`
- `SdblLogicalAndExpr`
- `SdblNotExpr`
- `SdblAdditiveExpr`
- `SdblMultiplicativeExpr`
- `SdblUnaryExpr`
- `SdblLiteral`
- `SdblMultiString`
- `SdblParameter` (shared with Slice 8 — `SdblTableRef` subtree
  parameter-source position; Slice 10a owns the expression-context
  emission in `parameter_expr`)
- `SdblParenExpr`
- `SdblTupleExpr`
- `SdblSubqueryExpr`

`SdblParameter` is shared with Slice 8 across two production
sites (Slice 8 `table_ref` for FROM-context; Slice 10a
`parameter_expr` for expression-context). Both sites emit the
same node shape: `Ampersand` token + optional `Ident` token, no
trivia between them; bare-`&` recovery preserved at both sites.

**Child-attachment invariants locked by Slice 10a** (shape contract
of the 12 NodeKinds):

- `SdblSubqueryExpr` inside `paren_or_subquery_expr` places
  `SdblSubquery` (Slice 6-attested NodeKind) as a direct child;
  HIR consumers walk this direct child.
- `SdblParameter` from `parameter_expr` has the `Ampersand` token
  as the first direct child and (optionally, in the complete
  form) the `Ident` token as the second direct child, with no
  trivia node between them. Bare-`&` recovery emits `SdblParameter`
  with only the `Ampersand` direct child.
- Operator wrappers (`SdblLogicalOrExpr`, `SdblLogicalAndExpr`,
  `SdblAdditiveExpr`, `SdblMultiplicativeExpr`) place operands as
  *direct expression children* in source order with operator
  tokens (`KwOr`, `KwAnd`, `Plus`, `Minus`, `Star`, `Slash`,
  `Percent`) as direct sibling tokens between them. The wrappers
  are FLAT (one wrapper covers all operands and operators of the
  same precedence chain), not nested.
- `SdblNotExpr` has the `KwNot` token as the first direct child
  and exactly one expression operand as the second direct child;
  multi-NOT inputs produce nested `SdblNotExpr( KwNot,
  SdblNotExpr( KwNot, … ) )` wrappers via right-recursion.
- `SdblUnaryExpr` has the unary operator token (`Plus`, `Minus`,
  or `KwNot`) as the first direct child and exactly one
  expression operand as the second direct child; multi-unary
  inputs produce nested wrappers via right-recursion.
- `SdblTupleExpr` has 2+ direct expression children separated by
  `Comma` direct sibling tokens.
- `SdblParenExpr` has exactly one direct expression child between
  `LParen` and `RParen` direct sibling tokens.
- `SdblLiteral` for a single-`String` literal keeps `String` as a
  direct token child (the multi-line-string diagnostic at
  `crates/ide-diagnostics/src/handlers/multiline_string_in_query.rs`
  scans for `String` direct tokens).
- `SdblMultiString` keeps every `String` token as a direct token
  child of the wrapper, in source order.

**AST-shape invariants locked by Slice 10a** (ordering / direct-child
contracts that HIR lowering reads beyond NodeKind identity):

1. **FLAT operator wrappers.** For chained operators at one
   precedence level (e.g. `a + b + c`, `a OR b OR c`,
   `a AND b AND c`, `a * b * c`), the parser opens **one**
   wrapper marker before the loop and emits **one**
   `SdblXxxExpr` containing all operands + all operator tokens.
   Consumer at `crates/sdbl-hir/src/lower/expr/ops.rs:42-43`
   collects `node.children()` as a flat `Vec` and detects the
   operator from `node.text().contains(...)`. The clean-room
   rewrite preserves this contract bit-for-bit; nested
   left-associative trees would break HIR's flat-children
   collection.
2. **Empty-wrapper unwrapping.** Walking a single non-operator
   atom through the chain (e.g. `Таблица.Поле`) emits a tower of
   single-child wrapper nodes. HIR's `lower_binary_expr` at
   `ops.rs:45-55` unwraps each single-child wrapper. The
   clean-room rewrite preserves the unconditional wrapper
   opening so HIR's unwrapping path is exercised.
3. **`skip_trivia` BEFORE operator probe.** Every operator-level
   loop calls `p.skip_trivia()` as the first statement of the
   loop body, BEFORE the `p.at(operator)` probe. This is
   load-bearing for `a\n+\nb` and `a /* comment */ + b`
   recognition — without the pre-probe trivia skip the operator
   would be invisible behind whitespace / newline / comment.
4. **`parameter_expr` NO `skip_trivia` between `&` and `Ident`.**
   The clean-room rewrite explicitly does NOT call
   `p.skip_trivia()` between the `Ampersand` bump and the `Ident`
   bump. Whether the lexer fuses `&\nT` into one or two tokens is
   a lexer-level decision; the parser-side guarantee is "no
   `p.skip_trivia()` call between the two bumps". HIR
   `lower_parameter` reads `node.text()` to derive the parameter
   name and tolerates the gap.
5. **`SdblMultiString` only for 2+ consecutive `String` tokens.**
   Single `String` emits `SdblLiteral` even with embedded
   newlines. Consumer at
   `crates/ide-diagnostics/src/handlers/multiline_string_in_query.rs`
   distinguishes the two cases.
6. **`paren_or_subquery_expr` SELECT-only lookahead.** Subquery
   branch entered only when the post-`(` keyword is `SELECT` /
   `ВЫБРАТЬ`. Every other input (numbers, identifiers, `&T`,
   `(`, `*`, etc.) routes to the expression branch. This is the
   *opposite* routing decision from Slice 8's FROM-context
   `data_source` (where any `(` routes to subquery-source); both
   directions are intentional and tested.
7. **`primary_expr` keyword-probes BEFORE generic `Ident` arm.**
   `at_keyword("CASE")`, `at_keyword("ВЫБОР")`, and
   `at_keyword("NULL")` run before the
   `Some(TokenKind::Ident) => column_or_function(p)` match arm.
   This is the **decisive gate** that fixes the
   pre-Slice-10a-C2 bug routing bare `NULL` through
   `column_or_function`. Regression gates:
   `test_slice10a_bare_null_emits_literal_not_column_ref` and
   `test_slice10a_select_field_null_emits_literal` in
   `crates/parser/tests/sdbl_parser_tests.rs`.
8. **`primary_expr` `Star` atom emits `SdblLiteral`.** The bare
   `*` token at the head of a primary position is wrapped in a
   single-token `SdblLiteral`. This is the `COUNT(*)` syntax —
   the Slice 7 `select::asterisk_field` handles `*` only at
   selected-field position; inside function-call args `*` is an
   expression atom.

**Also in Slice 10a deliverable (not Slice-10a-attested):**

The two `_legacy`-suffixed shims born during Slice 10a C1 are
listed here as deliverables but are NOT clean-room-attested:

- `comparison_expr_legacy` — pure-rename extraction of the
  pre-rename `comparison_expr` body; it remains a 2-line
  delegating shim that calls `predicate_expr_legacy`. Slice 10b
  re-authors this function and drops the `_legacy` suffix.
- `predicate_expr_legacy` — pure-rename extraction of the
  pre-rename `predicate_expr` body. The body is unchanged from
  the pre-Slice-10a state; Slice 10b re-authors it (predicates IN
  / IN HIERARCHY / IS NULL / BETWEEN / LIKE / REFS, plus the
  comparison operator tail). The `comparison_expr_legacy →
  predicate_expr_legacy` chain preserves the Slice 10a `not_expr`
  → Slice 10b dispatch boundary.

The `LEGACY (Slice 10b pending)` portion of `expressions.rs`
(`comparison_expr_legacy`, `predicate_expr_legacy`,
`column_or_function`, `inline_table_fields`, `is_cast_function`,
`parse_cast_type`, `case_expr`, `when_clause`) remains explicitly
**not** covered by this attestation.

Downstream consumers of the 12 Slice 10a node kinds
(`crates/parser/src/sdbl_token_converter.rs`,
`crates/parser/src/lib.rs`, `crates/parser/src/event.rs`,
`crates/syntax/src/syntax_kind.rs`, `crates/syntax/src/ast.rs`,
`crates/sdbl-hir/src/lower/**`,
`crates/ide-diagnostics/src/handlers/**`,
`crates/ide-db/src/database_impl_tests.rs`,
`crates/ide/tests/sdbl_completion_integration_test.rs`,
`crates/mcp-server/src/tools/query.rs`) were NOT modified in Slice
10a; they continue to see the public surfaces
`parser::parse_sdbl(&str) -> syntax::Parse<SyntaxNode>` and
`parser::parse_sdbl_with_shared_cache(&str)` unchanged, with the
12 locked `SyntaxKind` variants and their child-attachment
invariants preserved.

## Sources consulted

The Slice 10a material was authored from:

1. **1C ITS query-language documentation** — accessed via the
   local dump at `/home/itrous/src/tools_migration/its/dump/`
   (`index.json` maps each `https://its.1c.ru/db/pubqlang/...`
   URL to a `chapter_NNN.html` snapshot of the published page;
   the public ITS URLs are paywalled and serve JS-rendered
   navigation only). The chapters materially consulted:
   - `https://its.1c.ru/db/pubqlang/content/10/hdoc` (chapter
     "Язык запросов «1С:Предприятия»") — short overview chapter.
   - `https://its.1c.ru/db/pubqlang/content/12/hdoc` (chapter
     "Синтаксис текста запросов") — bilingual-keywords
     principle, list of query sections.
   - `https://its.1c.ru/db/pubqlang/content/22/hdoc` (chapter
     "Как получить записи из таблицы, отобранные по некоторому
     условию") — WHERE clause, **logical-operator precedence
     ladder verbatim**, `И` / `ИЛИ` / `НЕ` operator inventory,
     `МЕЖДУ` (BETWEEN), parens-override-precedence rule.
   - `https://its.1c.ru/db/pubqlang/content/40/hdoc` (chapter
     "Примеры использования выражений в списке полей выборки
     запроса") — literal types verbatim («число, строка (в
     кавычках), булево (значения Истина и Ложь), Null,
     Неопределено»), arithmetic operators (+, −, /, *) with
     explicit exclusion of `%` («Операция получения остатка % в
     языке запросов не поддерживается»), string concatenation
     `+`, ВЫБОР, ВЫРАЗИТЬ, ССЫЛКА.
   - `https://its.1c.ru/db/pubqlang/content/60/hdoc` (chapter
     "Передача параметров в запрос") — concrete `&Identifier`
     parameter prefix examples (`&ЧастьНаименования`,
     `&ДатаНачала`, `&ДатаОкончания`), ПОДОБНО (LIKE).
2. **The local SDBL expressions mini-spec** at
   [`sdbl-expressions-mini-spec.md`](sdbl-expressions-mini-spec.md)
   — the C0a clean-room reference document authored before C2.
   The mini-spec carries its own §Non-consultation statement and
   §ITS coverage verification table; the verification entries
   match the chapter citations above.
3. **The local SDBL SELECT mini-spec** at
   [`sdbl-select-mini-spec.md`](sdbl-select-mini-spec.md) —
   cross-referenced from the expressions mini-spec for the
   `paren_or_subquery_expr` → `select::subquery` dispatch and
   for the SELECT-keyword-only routing (the FROM-context
   `data_source` makes the opposite routing decision per Slice
   8's attestation).
4. **The Slice 1, Slice 2, Slice 6, Slice 7, and Slice 8
   clean-room material** already present in
   `crates/lexer/src/sdbl/mod.rs` and
   `crates/parser/src/grammar/sdbl.rs` /
   `crates/parser/src/grammar/sdbl/select.rs` — consulted only
   for the shape of per-function provenance comments, the
   CLEAN-ROOM / LEGACY banner layout, and the project's
   event-parser conventions (marker `p.start()` /
   `m.complete(...)`, `p.bump()`, `p.skip_trivia()`,
   `p.at_keyword(...)`, `p.at(TokenKind::...)`,
   `p.eat(TokenKind::...)`, `p.expect(...)`,
   `p.check_iteration_limit()`).

The resulting event-parser shape for the 17 Slice 10a functions
is the natural expression of the ITS grammar-shape rules and the
project's own event-parser conventions, and would converge
regardless of author. The claim made here is **independent
derivation from the sources above plus the project's local
compatibility constraints (the AST-shape invariants and
IDE-recovery allowances enumerated in the C0a mini-spec) — not
textual novelty, and not a uniqueness claim** for the resulting
grammar shape.

## Non-consultation statement

During the authorship of the Slice 10a material the following
sources were not used as working text:

- the sibling `../bsl-parser` project — neither its grammar files
  nor its parser implementation were consulted;
- the pre-C1 function bodies of the 17 Slice 10a functions — the
  C1 commit performed pure-refactor renames (`comparison_expr` →
  `comparison_expr_legacy`, `predicate_expr` →
  `predicate_expr_legacy`) and a banner reorder; the C2 body text
  of each of the 17 functions was re-derived against the C0a
  mini-spec and the cited ITS chapters with knowledge of the
  project's event-parser conventions established in Slices 1, 2,
  6, 7, 8;
- any other third-party SDBL parser, grammar, or event-tree
  implementation;
- ANTLR-style precedence rules from third-party SQL grammars
  (the precedence ladder is ITS-derived from pubqlang/22 for
  logical operators and adopted from standard SQL convention for
  the relative binding strength between the comparison /
  predicate slot and the arithmetic chain — see the C0a mini-spec
  §Operator precedence §Source attribution for the full
  breakdown).

The 144 SDBL parser integration tests in
`crates/parser/tests/sdbl_parser_tests.rs` (132 pre-existing + 10
Slice 10a Bucket-A gap tests added in C0b + 2 NULL-bug-fix
regression gates added in C2-fixup), the 26 Slice 6 acceptance
tests in `sdbl_slice6_package.rs`, the 26 Slice 7 acceptance
tests in `sdbl_slice7_fields.rs`, the 28 Slice 8 acceptance tests
in `sdbl_slice8_sources.rs`, the new Slice 10a acceptance tests
in `sdbl_slice10a_backbone.rs` (added in C3, this slice), the
204 HIR lowering tests in `sdbl-hir`, and the SDBL-touching tests
in `ide-diagnostics`, `ide-db`, `ide`, and `mcp-server` form the
regression gate for Slice 10a. They cover the locked
compatibility surfaces (public API, NodeKind identity,
child-attachment invariants, AST-shape invariants, bilingual
keyword acceptance, recovery shapes for incomplete input, and the
NULL dispatch fix) and a sampled regression corpus of accepted
inputs; they do not constitute a byte-identity golden corpus
across the full SDBL input space.

## Preserved pre-refactor behaviours

Eight behaviours observed in the pre-clean-room parser are not
directly derivable from a strict reading of the ITS spec alone
and are preserved bit-for-bit in Slice 10a:

1. **`expression` and `logical_expression` remain two distinct
   `pub fn` entries with equivalent bodies.** Both delegate to
   `logical_or_expr`. The duplicated entry was a Phase 1
   placeholder for an intended Phase 2 CASE-at-top-level split
   that did not land. Re-merge would touch 14+ call sites in
   Slice 7 / 8 / 11 territory and is deferred to Slice 12
   (recovery and IDE allowances).

2. **`parse_delimited_list` empty-element / trailing-delimiter
   recovery.** `a, , b` parses as two valid items + one `Error`
   placeholder; `a, b,` parses as two valid items + one
   trailing `Error`. IDE-recovery glue used by every
   comma-delimited list (SELECT fields, FROM sources, IN value
   lists, INDEX BY items).

3. **`recover_to_delimiter` paren-depth tracking.** The helper
   maintains an integer `paren_depth` so that
   `ВЫРАЗИТЬ(поле КАК СТРОКА(200))` recovery consumes to the
   outer `)`, not the inner one. Load-bearing for
   `test_real_query_with_type_cast` and related Bucket-C tests.

4. **Every operator loop calls `p.skip_trivia()` BEFORE the
   operator probe.** Load-bearing `CRITICAL` invariant. A single
   operator loop with trivia-skip AFTER the probe would fail on
   `a\n+\nb` and `a /* comment */ + b`.

5. **`is_expression_start` accepts `Star` as a legitimate
   start.** Load-bearing for `COUNT(*)` — the `*` is passed
   through to `primary_expr`, which emits an `SdblLiteral`. The
   `*` inside a SELECT-field position is handled separately by
   `select::asterisk_field` (Slice 7 territory).

6. **`SdblMultiString` for 2+ consecutive `String` tokens.**
   Multi-line BSL query strings are split across consecutive
   `String` tokens by the lexer; the parser collects them into a
   single `SdblMultiString` wrapper as an IDE-recovery
   allowance. Single `String` (even with embedded newlines)
   emits `SdblLiteral`.

7. **Modulo `%` operator accepted in `multiplicative_expr`.**
   ITS pubqlang/40 explicitly states «Операция получения остатка
   % в языке запросов не поддерживается» — `%` is **not** an
   ITS-supported SDBL operator. The pre-clean-room parser
   nonetheless accepted `TokenKind::Percent` in the
   multiplicative chain, and the Slice 10a C2 rewrite preserves
   that acceptance as a *local IDE-recovery allowance*: a query
   containing `a % b` produces a recoverable parse tree (one
   `SdblMultiplicativeExpr` containing the `%` token between two
   operands) rather than an immediate parse error, so the IDE
   reports the misuse via diagnostics rather than aborting the
   whole query. This is the **only** ITS-mandated negative claim
   that the parser deliberately violates; all other accepted
   operators / atoms / forms are ITS-supported.

8. **`parameter_expr` admits a bare `&` without identifier as an
   IDE-recovery allowance.** The identifier bump is guarded by
   `if p.at(TokenKind::Ident)`, not required by `p.expect`, so
   an incomplete `ВЫБРАТЬ &` completes the `SdblParameter`
   marker without aborting the enclosing query — the user can
   keep typing the parameter name with the marker already open.
   Mirror of Slice 8 attestation §Preserved-behaviour #7 for the
   FROM-context production site.

## Behaviour change (NOT preserved)

Two pre-refactor behaviours are **not** preserved bit-for-bit by
Slice 10a — both are deliberate bug fixes:

- **Bare `NULL` at expression-head positions emits `SdblLiteral`,
  NOT `SdblColumnRef`.** Pre-Slice-10a-C2 the parser routed bare
  `NULL` through `column_or_function` because the converter at
  `crates/parser/src/sdbl_token_converter.rs` maps
  `LitNull → TokenKind::Ident` (with the comment "FIXED (treated
  as keyword in SDBL)") and the historical
  `Some(TokenKind::KwNull)` arm in `is_expression_start` and
  `primary_expr` was unreachable dead code. Slice 10a C2:
    - dropped the dead `KwNull` arm from `is_expression_start`;
    - added an `at_keyword("NULL")` probe in `primary_expr`
      **before** the generic `Some(TokenKind::Ident) =>
      column_or_function(p)` match arm so a bare `NULL` literal
      emits `SdblLiteral` wrapping the `Ident` token.
  Regression gates: `test_slice10a_bare_null_emits_literal_not_column_ref`
  and `test_slice10a_select_field_null_emits_literal` in
  `crates/parser/tests/sdbl_parser_tests.rs` assert structurally
  that the NULL token's direct parent kind is `SDBL_LITERAL` and
  that no `SdblColumnRef` in the tree contains the NULL text.

  The pre-existing `test_null_literal` at line 290 (which uses
  `check_no_errors`) continues to pass because the new shape
  preserves the surrounding query structure — only the inner NULL
  node kind changes.

- **`recover_to_delimiter` stops at clause keywords at any paren
  depth (Slice 12 post-landing fix).** Pre-Slice-12 the helper at
  `crates/parser/src/grammar/sdbl/expressions.rs` (currently
  starting at line 187) gated `is_clause_keyword(p)` inside
  `if paren_depth == 0`, so an unterminated nested `(...)` inside
  a function-call argument silently gobbled the outer query's
  clause keyword (FROM / WHERE / GROUP BY / ...). Slice 12
  (commit `9d418084`, 2026-04-26) lifted the clause-keyword
  check **out of** the depth gate so it fires at any paren
  depth. The comma check remains depth-0-only because a comma
  inside a nested function-call argument list is a valid
  continuation token. The Semicolon check stays depth-0-only
  for parallel reasons in well-formed input (`;` cannot
  syntactically appear at depth>0 in an expression-argument
  stream), but on **malformed** input — e.g. `СУММА(1 (;` — a
  Semicolon at depth>0 IS reachable and is bumped into the
  Error node, deliberately diverging from
  `recover_field_to_alias_or_delimiter` (Slice 7), which stops
  on `;` at any depth because it runs at the top-level
  query-package boundary. See the Slice 7 attestation
  §Behaviour change for the full two-tier rationale (Codex
  Round-4 WEAK 1). The fix mirrors the
  post-Slice-8-addendum `recover_to_delimiter_vt` contract
  (commit `7e4f6a9e`).

  **Codex Round-5 stop-hook follow-up (commit `88439afa`).**
  After landing, codex Round-5 caught the any-depth clause-
  keyword promotion as overly broad: `is_clause_keyword`
  (`crates/parser/src/grammar/sdbl/select.rs:1025-1040`)
  includes `SELECT`/`ВЫБРАТЬ` and `UNION`/`ОБЪЕДИНИТЬ`, which
  are statement-starters / combiners rather than intra-clause
  boundaries. On `ВЫБРАТЬ СУММА(1 ( ВЫБРАТЬ X )) ИЗ T` the
  depth-1 `ВЫБРАТЬ` stopped recovery prematurely; the
  function-call empty-Error guard left it for the outer
  parser; `query_body_clauses` does not accept `SELECT` as a
  clause continuation, so the outer `ИЗ T` was lost. The
  Round-5 fix introduces
  `is_query_starter_or_combiner_keyword`
  (`crates/parser/src/grammar/sdbl/select.rs`) and applies the
  depth-conditional stop:

      if is_clause_keyword(p)
          && (paren_depth == 0
              || !is_query_starter_or_combiner_keyword(p)) {
          break;
      }

  Hard intra-clause keywords (FROM/WHERE/GROUP/...) still stop
  at any depth; SELECT/UNION revert to depth-0-only stops.
  Empirically verified: pre-Round-5 the outer FROM count was
  0; post-Round-5 the outer FROM clause survives. Regression
  gates:
  `test_slice10a_recover_to_delimiter_does_not_stop_on_nested_select_at_depth_ru`
  and `_en` in `crates/parser/tests/sdbl_slice10a_backbone.rs`.

  All three SDBL recovery helpers
  (`recover_to_delimiter`, `recover_to_delimiter_vt`,
  `recover_field_to_alias_or_delimiter`) now share the
  refined two-class contract; the Slice 8-addendum and
  Slice 7 attestations record the matching post-Round-5
  entries.

  **Codex Round-5b stop-hook follow-up (commit `94eb3a6f`).**
  Round-5 made bare nested `SELECT`/`UNION` text-preserving,
  but a residual gap remained: when the nested subquery
  itself contains a hard intra-clause keyword (e.g.
  `ВЫБРАТЬ X ИЗ Y`), the inner `ИЗ` still stopped recovery
  at depth 1 because hard intra-clause keywords are
  any-depth stops by default. The outer parser then
  misattributed the inner `ИЗ Y` as the OUTER FROM clause,
  losing the real outer `ИЗ T`. Empirical pre-fix output:
  outer FROM clause text was `ИЗ Y` instead of `ИЗ T`.

  The Round-5b fix introduces `nested_query_starts: Vec<i32>`
  scope tracking in all three helpers. Each entry holds the
  `paren_depth` at which a `(` followed by `SELECT`/
  `ВЫБРАТЬ`/`UNION`/`ОБЪЕДИНИТЬ` was bumped. While any
  marker is active, the helper treats hard intra-clause
  keywords as belonging to the nested query body and
  absorbs them rather than stopping. The matching `)`
  pops the marker (when its `paren_depth` matches the
  marker's depth at push time).

  The fix structure (applied uniformly to all three helpers):

      let mut nested_query_starts: Vec<i32> = Vec::new();
      // ... in loop:
      if p.at(LParen) {
          paren_depth += 1; p.bump();
          p.skip_trivia();
          if is_query_starter_or_combiner_keyword(p) {
              nested_query_starts.push(paren_depth);
          }
          continue;
      }
      if p.at(RParen) && paren_depth > 0 {
          if let Some(&d) = nested_query_starts.last() {
              if d == paren_depth { nested_query_starts.pop(); }
          }
          paren_depth -= 1; p.bump(); continue;
      }
      let inside_nested_query = !nested_query_starts.is_empty();
      if is_clause_keyword(p) {
          let stop = if at_top_level {
              true
          } else if inside_nested_query {
              false
          } else {
              !is_query_starter_or_combiner_keyword(p)
          };
          if stop { break; }
      }

  Regression gate:
  `test_slice10a_recover_to_delimiter_inner_from_misattribution_gate`
  asserts the outer FROM clause references `T`, not the
  inner `Y`, on the Round-5b trigger
  `ВЫБРАТЬ СУММА(1 ( ВЫБРАТЬ X ИЗ Y )) ИЗ T`. Slice 7 and
  Slice 8-addendum carry analog gate tests for their helpers.
  Regression gates:
  `test_slice10a_recover_to_delimiter_stops_on_clause_keyword_at_any_depth_ru`
  and `_en` in `crates/parser/tests/sdbl_slice10a_backbone.rs`
  pin the corrected behaviour using the trigger input
  `ВЫБРАТЬ СУММА(1 ( ИЗ T2` (and EN equivalent) — a literal `1`
  is intentional so `column_or_function` does not consume the
  bare `(` as a nested function-call start. Empirically verified
  at landing time: both tests fail on the pre-F1 helper (revert
  via `git apply -R` of commit `9d418084`) and pass after the
  fix. The companion fix for `recover_field_to_alias_or_delimiter`
  lands under the Slice 7 attestation (commit `80a3129c`); test
  trigger corrections per Codex Round-2 land under commit
  `4a335a82`.

## Verification recipe

All of the following must be green before this attestation is
considered live:

1. `cargo test -p parser --test sdbl_parser_tests` — 144 SDBL
   parser tests (132 pre-existing + 10 Slice 10a Bucket-A gap
   tests + 2 NULL-bug-fix regression gates).
2. `cargo test -p parser --test sdbl_slice6_package` — 26 Slice
   6 acceptance tests.
3. `cargo test -p parser --test sdbl_slice7_fields` — 26 Slice 7
   acceptance tests.
4. `cargo test -p parser --test sdbl_slice8_sources` — 28 Slice
   8 acceptance tests.
5. `cargo test -p parser --test sdbl_slice10a_backbone` — Slice
   10a spec-driven acceptance tests (this attestation's primary
   gate).
6. `cargo test -p parser` — full parser suite (integration tests
   + inline `mod tests` in `select.rs`).
7. `cargo test -p sdbl-hir --lib` — 204 HIR lowering tests
   including the operator wrapper consumer at
   `lower/expr/ops.rs` and the WHERE / GROUP / ORDER readers at
   `lower/clauses.rs`.
8. `cargo test -p lexer` — Slices 1 + 2 regression gate.
9. `cargo test -p ide-db` — SDBL validation tests via
   `parse_sdbl`.
10. `cargo test -p ide` — full IDE test suite including
    `sdbl_completion_integration_test`.
11. `cargo test -p ide-diagnostics` — diagnostic tests including
    `multiline_string_in_query` (regression gate for
    `string_literal_or_multi`) and `query_parse_error`.
12. `cargo test -p mcp-server` — MCP server regression gate;
    `parse_sdbl` consumer at `crates/mcp-server/src/tools/query.rs`.
13. `cargo build --workspace --all-targets` — workspace build.
14. `cargo clippy -p parser --all-targets --all-features --
    -D warnings` — parser clippy clean.

## Commit trail

Slice 10a landed across 5 logical phases. Each anchor commit is
followed by zero or more codex-adversarial-review fixup commits
that are pure refinements of the canonical commit's intent. The
trail enumerated below names every commit reachable from
`git log --oneline --reverse develop..HEAD` *except* the
absolute-last one (see Anti-Hilbert disclosure at the end of this
section).

- **C0a — `820f5984` (2026-04-25)**: publish SDBL expressions
  mini-spec (`docs/legal/sdbl-expressions-mini-spec.md`). Four
  fixup commits: `6d398d4a` (VT-arg filter + parameter shape +
  non-consultation tightening), `8c50977d` (lexical-assumptions
  table correction), `90b1e061` (residual KwNull scrub),
  `a184935f` (NULL-before-column dispatch order locked in
  mini-spec).
- **C0b — `3eaddae2` (2026-04-25)**: audit SDBL Slice 10a tests
  and extend operator chain / atom coverage with 10 Bucket-A gap
  tests (a–j) in `sdbl_parser_tests.rs`. One fixup: `53111d0b`
  (strengthen weak structural assertions in the precedence and
  newline-AND tests).
- **C1 — `422851fd` (2026-04-25)**: rename Slice 10b legacy
  helpers (`comparison_expr` → `comparison_expr_legacy`,
  `predicate_expr` → `predicate_expr_legacy`) and move 17 Slice
  10a functions under the `CLEAN-ROOM Slice 10a` banner. One
  fixup: `0c8a8de7` (drop forward C3-attestation references).
- **C2 — `dd4777db` (2026-04-25)**: rewrite SDBL Slice 10a
  expression backbone clean-room from ITS pubqlang/22, /40, /60
  + the C0a mini-spec, with per-function provenance comments and
  the NULL bug fix. Seven fixup commits:
  - `9038e9eb` — clean-room independence note + NULL regression
    gates;
  - `ca75ffb6` — credit pubqlang/22 for operator precedence;
  - `56583a32` — extend Primary sources + add `%` IDE-recovery
    allowance;
  - `b199eb90` — align test banner provenance with the verified
    ITS chapters (`/22`, `/40`, `/60`);
  - `84840228` — rewrite remaining mini-spec NULL/Ident notes in
    final-state form;
  - `e7aed40a` — clarify two-site NULL recognition split
    (`is_expression_start` Ident-arm + `primary_expr` decisive
    keyword probe);
  - `8e14d843` — mark NULL fallback unreachable under current
    `Parser::at_keyword` API.
- **C3 — `9fc55462` (2026-04-25)**: this attestation (initial
  draft) + 28 spec-driven acceptance tests in
  `crates/parser/tests/sdbl_slice10a_backbone.rs` + master-doc
  flip in `docs/legal/sdbl-clean-room-slices.md`. Fixups:
  - `7718ae6d` — flip Slice 10a provenance docstrings in
    `sdbl.rs` and `expressions.rs` to "complete (2026-04-25)"
    final-state wording, and correct the commit-trail count
    here and in the master doc to match the reproducible
    `git log --oneline --reverse develop..HEAD` output.
  - `ba88c05f` — name the `7718ae6d` fixup hash explicitly in
    the attestation §Commit trail (was "this commit").

**Anti-Hilbert disclosure.** The very last commit on this branch
— the one that authors / amends the attestation §Commit trail
itself — is necessarily not named in this enumeration: a Git
commit cannot reference its own future hash at write time. This
anti-Hilbert property applies to every legal/clean-room
attestation that records its own commit trail and is shared with
the prior Slice 1, 2, 6, 7, 8 attestations in this project. A
reviewer running `git log --oneline --reverse develop..HEAD` will
always see exactly one commit beyond the trail's last named hash:
that commit is the one that landed this attestation in its
current state, and it is the natural endpoint of the trail.

The phase totals (named hashes in the trail above): C0a 5, C0b 2,
C1 2, C2 8, C3 3 — 20 commits in total enumerated. The branch
HEAD adds one trailing commit (the one editing this trail), per
the Anti-Hilbert disclosure above. The original attestation
(commit `9fc55462`) miscounted: it claimed 19 commits but the
figure was reached by listing `ca75ffb6` under both C0a and C2;
each subsequent fixup commit (`7718ae6d`, `ba88c05f`, and the
absolute-last edit not named here) progressively closed the
trail until each commit is counted in exactly one phase.

## Licensing note

The `crates/parser` crate retains its `LGPL-3.0-or-later` license
until the full Slice 6 → Slice 11 parser migration is complete
and Slice 13 reattaches `sdbl-hir`. Promoting the crate to
Tier A (`MIT OR Apache-2.0`) is explicitly out of scope for
Slice 10a and will happen once the last LEGACY-banner function
under `grammar/sdbl/expressions.rs` and `grammar/sdbl/select.rs`
has been re-derived and the HIR lowering cascade in `sdbl-hir`
has been cleaned up.

## Author attestation

The Slice 10a material listed above under **Scope** was authored
as a clean-room re-derivation from the sources listed under
**Sources consulted**, without using the `../bsl-parser` project,
the pre-C1 function bodies of the 17 Slice 10a functions, or any
other third-party SDBL parser as working text. The independent
derivation claim follows the same convention as Slices 1, 2, 6,
7, 8 attestations: the resulting event-parser shape is the
natural expression of the cited ITS chapters and the project's
own event-parser conventions, and would converge regardless of
author. The claim is **independent derivation, not textual
novelty**; this attestation does not assert the grammar shape is
unique or unavoidable.

This attestation applies at the date recorded at the top of the
document.
