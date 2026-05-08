# SDBL Clean-Room Rewrite Slices

## Purpose

This document breaks the future SDBL lexer/parser cleanup into concrete slices.

The goal is not to rewrite the whole subsystem at once, but to isolate the work
into bounded chunks that can be implemented, audited, and validated
independently.

## Legal framing

### Working ownership note

For this project, use the following practical distinction:

- the **SDBL language itself** is part of the 1C platform;
- only **1C** can realistically claim rights in the official language
  specification and official language documentation;
- third-party projects can hold rights only in their own concrete expression:
  grammar texts, token inventories, examples, tests, implementation code, and
  prose documentation.

This is the project’s working legal position, not a court determination. It is
based on:

- official 1C query-language materials:
  - `https://its.1c.ru/db/pubqlang/content/12/hdoc`
  - `https://its.1c.ru/db/pubqlang/content/10/hdoc`
- the general copyright principle that ideas, systems, and methods of operation
  are distinct from a concrete text or implementation.

### Clean-room rule

For SDBL cleanup, the implementation source of truth should be:

1. official 1C documentation;
2. independently written local specs and tests;
3. observed local parser behavior only where the project explicitly chooses to
   preserve it for IDE/recovery reasons.

Do **not** use `bsl-parser` grammar files as the working text while implementing
the replacement slices.

## Slice map

The slices below are ordered by dependency and rewrite value.

## Slice 0: test and fixture baseline

### Goal

Stabilize the acceptance surface before rewriting lexer/parser internals.

### Scope

- inventory and classify `crates/parser/tests/sdbl_parser_tests.rs`
- inventory inline SDBL fixtures in `crates/sdbl-hir/src/lower/tests.rs`
- identify which tests are:
  - essential language coverage
  - IDE recovery coverage
  - likely upstream-shaped examples

### Deliverable

- a reduced, explicitly owned local acceptance suite for SDBL
- clear marking of fixtures to rewrite, keep, or replace

## Slice 1: lexer core, without vocabulary-heavy domains

**Status: complete (2026-04-24).** See
[`sdbl-clean-room-slice1.md`](sdbl-clean-room-slice1.md) for the full
attestation. Commit trail: C0 `f4a3c9ce`, C1 `49aa192c`, C2
`ac4cbad2`, C3 landed with the attestation.

### Goal

Replace the most generic lexer mechanics first.

### Scope

- whitespace and newline handling
- separators and operators
- numbers
- date literals
- string tokenization
- identifiers
- parameter references
- temporary-table marker handling

### Files

- `crates/lexer/src/sdbl/mod.rs` (the CLEAN-ROOM section of the
  `SdblTokenKind` enum)
- `crates/lexer/src/sdbl/strings_mode.rs`
- `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt`
- `crates/lexer/tests/sdbl_golden_corpus.rs`
- `crates/lexer/tests/sdbl_slice1_core.rs`

### Notes

This slice intentionally excludes the large vocabulary-heavy sets —
they remain under the `LEGACY (Slices 2–5 pending)` banner inside the
same `SdblTokenKind` enum and stay Tier B material until their own
slice PRs:

- metadata object kinds
- virtual tables
- specialized function names
- period types

## Slice 2: structural keyword vocabulary

**Status: complete (2026-04-24).** See
[`sdbl-clean-room-slice2.md`](sdbl-clean-room-slice2.md) for the full
attestation. Commit trail: C0 `3da0f41d`, C1 `bc8fd550`, C2
`ea0e34d2`, C3 landed with the attestation.

### Goal

Rebuild only the core clause keywords from official SDBL syntax.

### Scope

- `SELECT`
- `FROM`
- `WHERE`
- `INTO`
- `GROUP`
- `ORDER`
- `HAVING`
- `TOTALS`
- `UNION`
- `ALL`
- `DISTINCT`
- `TOP`
- join family
- CASE family
- basic predicate keywords
- logical operators (`AND` / `OR` / `NOT`)
- boolean and `NULL` literals

### Files

- `crates/lexer/src/sdbl/mod.rs` (the `CLEAN-ROOM Slice 2` section
  of the `SdblTokenKind` enum)
- `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt`
- `crates/lexer/tests/sdbl_golden_corpus.rs`
- `crates/lexer/tests/sdbl_slice2_keywords.rs`
- `docs/legal/sdbl-clean-room-slice2.md`

### Notes

The Slice 2 block is organised into five labeled sub-sections
(clause starters, join family, aliasing & predicates, CASE family,
logical operators & literals) with a top-of-block convenience index
mapping every variant to its ITS section. The `#[regex]` attributes
remain the single source of truth; the index is an authorship
scanning aid, not a separate vocabulary table (that would create a
drift hazard since logos requires regex at the variant declaration
site). A true tabular vocabulary map lives in
[`sdbl-clean-room-slice2.md`](sdbl-clean-room-slice2.md) §Scope.

Sibling-module extraction (a dedicated `keywords.rs`) is reserved
for Slices 3–4 where the vocabularies (metadata objects, function
names) are genuinely catalog-shaped and may outgrow `mod.rs`.

`KwOnOrBy` bundles the `ON` / `BY` / `ПО` keywords into a single
token kind — preserved pre-refactor behaviour. The split will
happen naturally in Slice 9 (joins) and/or Slice 11 (clauses after
FROM) where converter edits are in scope.

## Slice 3: metadata object and type vocabulary

The original Slice 3 scope (metadata object names + type literals +
period names — 22 lexer variants total) was split into two
sub-slices by the codex pair-mode plan-review (2026-05-07):

- **Slice 3a** owns the seven variants whose canonical SDBL
  grammar attestation in v8327doc Глава 8 «Работа с запросами» is
  unambiguous and direct (4 primitive type literals,
  `LitUndefined`, 2 narrow `Period*` keywords). Status: complete
  (2026-05-07) — see §Slice 3a below.
- **Slice 3b** owns the 14 metadata-object variants (`Mdo*`
  excluding `MdoExternalDataSource`) which require a per-variant
  discrepancy audit to determine the right tier classification per
  variant. Status: pending.
- The platform-late `MdoExternalDataSource` variant is deferred to
  master-doc Slice 5, which already owns external-source handling.

### Goal

Separate the most provenance-sensitive catalogs into their own owned tables.

### Scope (original)

- metadata object names
- type literals
- period names

### Files

- likely extracted from `crates/lexer/src/sdbl.rs` into dedicated local tables or
  modules

## Slice 4: function vocabulary

### Goal

Rebuild SDBL function names from official 1C query-language behavior rather than
from upstream grammar inventory.

### Scope

- aggregate functions
- date/time functions
- string functions
- type/presentation helpers

### Files

- lexer token inventory
- parser expression entry points
- tests

## Slice 5: virtual table and external-source handling

### Goal

Isolate the trickiest vocabulary/context subsystem.

### Scope

- virtual table suffixes
- `DOT`-sensitive table resolution
- external data source mode
- any special field names that currently require dedicated lexer states

### Files

- `crates/lexer/src/sdbl.rs`
- `crates/parser/src/sdbl_token_converter.rs`
- SDBL parser tests

## Slice 6: parser root and package skeleton

**Status: complete (2026-04-24).** See
[`sdbl-clean-room-slice6.md`](sdbl-clean-room-slice6.md) for the full
attestation. Commit trail: C0 `cd709cac`, C1 `1acb9875`, C2
`66a210a1`, C3 landed with the attestation.

### Goal

Rebuild the top-level SDBL parse shape with minimum grammar content.

### Scope

- query package
- query item separation by semicolon
- `DROP` query if retained
- `SELECT` query entry point
- subquery vs package boundaries
- `UNION` / `UNION ALL` skeleton

### Files

- `crates/parser/src/grammar/sdbl.rs` (the `CLEAN-ROOM Slice 6`
  section — `query_package`, `queries`, `drop_table_query`, plus the
  module-level `## Provenance` docstring)
- `crates/parser/src/grammar/sdbl/select.rs` (the `CLEAN-ROOM Slice 6`
  section — `select_query` wrapper, `subquery`, `union_clause`)
- `crates/parser/tests/sdbl_parser_tests.rs` (C0 Bucket audit: two
  Slice 6 Bucket-C tests rewritten and promoted to Bucket B; three
  Slice 6 gap tests added)
- `crates/parser/tests/sdbl_slice6_package.rs`
- `docs/legal/sdbl-clean-room-slice6.md`

## Slice 7: SELECT field list, aliases, and INTO

**Status: complete (2026-04-25).** See
[`sdbl-clean-room-slice7.md`](sdbl-clean-room-slice7.md) for the full
attestation. Commit trail: C0 `062d0a72`, C1 `2e091d85`, C2
`a22d98a7`, C3 landed with the attestation.

### Goal

Rebuild the smallest useful `SELECT` body.

### Scope

- selected fields
- asterisk fields
- aliases
- `INTO` / `ПОМЕСТИТЬ`

### Files

- `crates/parser/src/grammar/sdbl.rs` (module-level `## Provenance`
  docstring Slice 7 addition)
- `crates/parser/src/grammar/sdbl/select.rs` (the `CLEAN-ROOM Slice 7`
  section — `query` wrapper, `selected_fields`, `selected_field`,
  `is_field_start`, `is_asterisk_start`, `asterisk_field`,
  `selected_field_alias`, `into_clause`; plus the C1-born LEGACY
  helpers `query_body_clauses` and `source_alias_legacy`)
- `crates/parser/tests/sdbl_parser_tests.rs` (C0 Bucket-A additions:
  `test_russian_table_asterisk`, `test_russian_into_simple`)
- `crates/parser/tests/sdbl_slice7_fields.rs`
- `docs/legal/sdbl-clean-room-slice7.md`

## Slice 8: FROM sources and source chains

**Status: complete (2026-04-25).** See
[`sdbl-clean-room-slice8.md`](sdbl-clean-room-slice8.md) for the full
attestation. Commit trail: C0 `078dd808`, C1 `1be6dd69`, C2
`85b4005e`, C3 landed with the attestation.

### Goal

Rebuild source parsing independently from full expression complexity.

### Scope

- table references
- subqueries in `FROM`
- parameter sources
- source aliases

### Files

- `crates/parser/src/grammar/sdbl.rs` (module-level `## Provenance`
  docstring Slice 8 addition)
- `crates/parser/src/grammar/sdbl/select.rs` (the `CLEAN-ROOM Slice 8`
  section — `is_data_source_start`, `from_clause`, `data_source`,
  `table_ref`, `source_alias`; plus the C1-born helper
  `virtual_table_args` extracted from pre-C1 `table_ref` —
  renamed and clean-room rewritten in Slice 8-addendum landed
  2026-04-26; see `sdbl-clean-room-slice8-addendum.md`)
- `crates/parser/tests/sdbl_parser_tests.rs` (C0 Bucket-A additions:
  `test_slice8_from_multi_source_with_bare_alias`,
  `test_slice8_russian_subquery_source_with_alias`,
  `test_slice8_temp_table_source_across_package_boundary`,
  `test_slice8_parameter_source_without_alias`)
- `crates/parser/tests/sdbl_slice8_sources.rs`
- `docs/legal/sdbl-clean-room-slice8.md`

## Slice 9: JOIN family

**Status: complete (2026-04-25).** See
[`sdbl-clean-room-slice9.md`](sdbl-clean-room-slice9.md) for the
attestation. The 2 functions `is_join_keyword` and `join_clause`
were re-authored under the `CLEAN-ROOM Slice 9 — JOIN family`
banner in `crates/parser/src/grammar/sdbl/select.rs`. The single
NodeKind emitted by these functions (`SdblJoinClause`) retains
its pre-C1 child-attachment shape; the seven parser-side AST-shape
invariants in the attestation §Preserved invariants section pin
the contract that downstream consumers
(`SdblJoinClause::join_type()` substring matcher in
`crates/syntax/src/ast.rs:1403-1437`, HIR ON-condition reader at
`crates/sdbl-hir/src/lower/join_clause.rs:142-153`, FROM-side
`JoinWithSubQuery`/`JoinWithVirtualTable` shape readers at
`crates/sdbl-hir/src/lower/from_clause.rs:36-72`,
`LogicalOrInJoin` shape reader at
`crates/sdbl-hir/src/lower/join_clause.rs:188`, recursive
`lower_join_clause_recursive` at
`crates/sdbl-hir/src/lower/join_clause.rs:35-51`) read.

Authored from ITS pubqlang chapters 44 (`ВНУТРЕННЕЕ
СОЕДИНЕНИЕ` listing + standalone `СОЕДИНЕНИЕ` reference),
45 (`ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ` listing), 46 (`ПРАВОЕ ВНЕШНЕЕ
СОЕДИНЕНИЕ` listing), 47 (`ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ` listing),
48 (chained / nested examples) via the local dump at
`<ITS pubqlang dump>/`; SELECT mini-spec
§JOIN clauses (lines 297–319) and §Recovery requirements item
#6 (line 410); the lexer's Slice 2 attestation for bilingual
EN/RU keyword pairs (LEFT/ЛЕВОЕ, RIGHT/ПРАВОЕ, FULL/ПОЛНОЕ,
INNER/ВНУТРЕННЕЕ, JOIN/СОЕДИНЕНИЕ, OUTER/ВНЕШНЕЕ, ON/ПО).

Commit trail (4 phases, each a single anchor commit):

- C0 `de6820f8` (audit + 15 Bucket-A gap tests in
  `sdbl_parser_tests.rs` — 4 Tier A1 RU canonical listings
  (`test_slice9_canonical_{inner,left_outer,right_outer,
  full_outer}_join_ru`) + 2 Tier A2/D candidates (bare
  ПОЛНОЕ / ЛЕВОЕ — resolved at C2 as Tier D local
  allowances: `test_slice9_bare_{full,left}_join_ru`) + 2
  Tier C / chapter-44-standalone bare JOIN forms (EN + RU:
  `test_slice9_bare_join_{en,ru}`) + 2 chapter 48
  chained/nested (`test_slice9_chained_joins_same_source`,
  `test_slice9_nested_join_inside_join`) + 3 invariant-7
  parser-side AST-shape pins for the
  `JoinWithSubQuery`/`JoinWithVirtualTable`/`LogicalOrInJoin`
  HIR diagnostics
  (`test_slice9_from_{subquery,virtual_table}_with_join_ast_shape`,
  `test_slice9_or_in_on_ast_shape`) + 2 audit-gate
  `Parser::error()`-bump tests
  (`test_slice9_missing_{join_keyword,on}_current_behavior`).
  EN bilingual canonical-form tests live in the C3
  acceptance suite, not C0. Codex pair-review found one P2
  (the helper `assert_clean_parse` must reject
  `SyntaxKind::ERROR` recovery descendants because
  `Parser::error()` does not populate `Parse::errors()`);
  fix landed in the same C0 commit);
- C1 `dc10cd6c` (split out of LEGACY into clean-room
  banner; pure refactor — function bodies move byte-for-
  byte, only banner header / placeholder provenance comments
  / Slice 8 `data_source` cross-reference / `sdbl.rs`
  Provenance docstring change);
- C2 `5b8168a6` (clean-room rewrite + tiered A1/B/C/D
  per-function provenance; audit-gate decision **Option B
  PRESERVE** for both `Parser::error()`-bumps with
  recovery hardening deferred to Slice 12);
- C3 `9af02c0b` (2026-04-25): attestation +
  `crates/parser/tests/sdbl_slice9_joins.rs` (17 spec-
  driven AST-shape acceptance tests organised into
  Tier A1 RU + Tier B EN + Tier C bare-JOIN + Tier D
  local-allowance + chapter 48 + invariant-7 sub-suites)
  + master-doc flip + `sdbl.rs` Provenance docstring flip
  to "complete (2026-04-25)" with attestation citation.

The attestation's §Commit trail records an "anti-Hilbert
disclosure" noting that the absolute-last commit on the
branch — the one editing the trail itself — is necessarily
not named in the enumeration; this is shared with the prior
Slice 1, 2, 6, 7, 8, 10a, 10b attestations.

### Notes

The C2 author chose **Option B PRESERVE** for the two
`Parser::error()`-bumps in `join_clause` (missing JOIN
keyword after type Ident; missing ON/ПО after data source).
Rationale: both options leave bad recovery trees in the
error case (the recovery-quality gap is marginal, not a
production-correctness bug like Slice 10b's `column_or_function`
clause-keyword hijack); Slice 9's clean-room scope is the
happy-path JOIN grammar; recovery hardening lives naturally
under Slice 12's IDE-recovery rewrite. The audit-gate tests
`test_slice9_missing_join_keyword_current_behavior` and
`test_slice9_missing_on_current_behavior` (added in C0) flip
roles from "pre-rewrite regression gate" to "post-rewrite
preservation gate" without any test edits.

### Files

- `crates/parser/src/grammar/sdbl/select.rs` (the
  `CLEAN-ROOM Slice 9 — JOIN family` section — 2 functions);
- `crates/parser/src/grammar/sdbl.rs` (module-level
  `## Provenance` docstring Slice 9 entry, flipped to
  complete-state final-form in C3);
- `crates/parser/tests/sdbl_parser_tests.rs` (15 Bucket-A
  gap tests added in C0);
- `crates/parser/tests/sdbl_slice9_joins.rs` — the new
  spec-driven acceptance suite (17 tests);
- `docs/legal/sdbl-clean-room-slice9.md` — the C3
  attestation.

## Slice 10: expression minimum

The expression surface (1108 LOC, 26 functions, 26 NodeKinds) is split
into two sub-slices for review-surface manageability. See the planning
doc at `<engineering scratch plans>/serialized-moseying-orbit.md` (Slice 10a) and
the §Slice 10a + 10b sub-slices below.

### Slice 10a: expression backbone

**Status: complete (2026-04-25).** See
[`sdbl-clean-room-slice10a.md`](sdbl-clean-room-slice10a.md) for the
full attestation. Authored from
[`sdbl-expressions-mini-spec.md`](sdbl-expressions-mini-spec.md) (the
C0a clean-room reference) and ITS pubqlang chapters 10, 12, 22, 40,
60. Commit trail (5 phases, 20 named commits + 1 absolute-last
trailing commit per the attestation's anti-Hilbert disclosure;
per-phase totals named in trail: C0a 5, C0b 2, C1 2, C2 8, C3 3):
- C0a `820f5984` (mini-spec) + 4 fixup commits (`6d398d4a`,
  `8c50977d`, `90b1e061`, `a184935f`);
- C0b `3eaddae2` (10 Bucket-A gap tests) + 1 fixup (`53111d0b`);
- C1 `422851fd` (renames + reorder under clean-room banner) + 1
  fixup (`0c8a8de7`);
- C2 `dd4777db` (clean-room rewrite of 17 functions + NULL bug
  fix) + 7 fixup commits (`9038e9eb`, `ca75ffb6`, `56583a32`,
  `b199eb90`, `84840228`, `e7aed40a`, `8e14d843`);
- C3 `9fc55462` (attestation + 28 spec-driven acceptance tests +
  master-doc flip) + 2 fixups: `7718ae6d` (final-state
  provenance + commit-trail correction); `ba88c05f` (named the
  `7718ae6d` fixup hash explicitly in the attestation). The
  attestation's §Commit trail records an "anti-Hilbert
  disclosure" noting that the absolute-last commit on the
  branch — the one editing the trail itself — is necessarily
  not named in the enumeration; this is shared with the prior
  Slice 1, 2, 6, 7, 8 attestations.

#### Goal

Rebuild the expression backbone — atoms (literals, parameters,
parens / tuples / subqueries, the bare `*` for `COUNT(*)`) plus the
operator precedence chain (logical OR / AND / NOT / additive /
multiplicative / unary).

#### Scope

- literals (numeric, string, boolean Истина/Ложь, NULL, Неопределено);
- string literal multi-part IDE-recovery (multi-line BSL queries);
- parameters (`&Identifier`);
- parens / tuples / subqueries dispatch (SELECT-keyword lookahead
  routes to subquery; otherwise expression(s) → `SdblParenExpr` or
  `SdblTupleExpr`);
- the bare `*` for `COUNT(*)`;
- operator precedence ladder NOT > AND > OR (ITS pubqlang/22) +
  arithmetic +/-/*/(local-allowance %) (ITS pubqlang/40);
- error-recovery helpers (`is_expression_start`, `is_recovery_point`,
  `recover_to_delimiter`, `parse_delimited_list`).

#### Files

- `crates/parser/src/grammar/sdbl/expressions.rs` (the
  `CLEAN-ROOM Slice 10a` section — 17 functions);
- `crates/parser/src/grammar/sdbl.rs` (module-level `## Provenance`
  docstring Slice 10a addition);
- `crates/parser/tests/sdbl_parser_tests.rs` (12 Bucket-A tests:
  10 C0b gap tests + 2 NULL-bug-fix regression gates);
- `crates/parser/tests/sdbl_slice10a_backbone.rs` — the new
  spec-driven acceptance suite;
- `docs/legal/sdbl-expressions-mini-spec.md` — the C0a clean-room
  reference;
- `docs/legal/sdbl-clean-room-slice10a.md` — the C3 attestation.

#### Notes

The Slice 10a precedence ladder NOT > AND > OR is **ITS-derived**
from pubqlang/22 §Условие отбора (verbatim quote in the
attestation). The arithmetic operator inventory and string-`+`
concatenation are ITS-derived from pubqlang/40. The relative
binding strength between the comparison/predicate slot and the
arithmetic chain (multiplicative tighter than additive tighter
than comparison) is the standard SQL convention adopted by the
mini-spec without consulting third-party SQL grammar text.

The Slice 10a C2 commit fixed a pre-existing parser bug: bare
`NULL` at expression-head positions was routed through
`column_or_function` because the converter at
`sdbl_token_converter.rs:57` maps `LitNull → TokenKind::Ident` and
the historical `Some(TokenKind::KwNull)` arm was unreachable dead
code. Slice 10a C2 added an `at_keyword("NULL")` probe in
`primary_expr` before the generic `Ident → column_or_function`
match arm so bare `NULL` now correctly emits `SdblLiteral`.

Modulo `%` is preserved as a local IDE-recovery allowance —
ITS pubqlang/40 explicitly states «Операция получения остатка %
в языке запросов не поддерживается» but the parser accepts it
to produce a recoverable parse tree (one `SdblMultiplicativeExpr`
containing the `%` token between two operands) so the IDE can
report the misuse via diagnostics.

### Slice 10b: predicates, comparison, function calls, CAST, CASE

**Status: complete (2026-04-25).** See
[`sdbl-clean-room-slice10b.md`](sdbl-clean-room-slice10b.md) for
the attestation. The 8 functions `comparison_expr`,
`predicate_expr`, `column_or_function`, `inline_table_fields`,
`is_cast_function`, `parse_cast_type`, `case_expr`,
`when_clause` were re-authored under the `CLEAN-ROOM Slice 10b`
banner in `crates/parser/src/grammar/sdbl/expressions.rs`. The
13 NodeKinds emitted by these functions
(`SdblComparisonExpr`, `SdblInExpr`, `SdblInHierarchyExpr`,
`SdblIsNullExpr`, `SdblBetweenExpr`, `SdblLikeExpr`,
`SdblRefsExpr`, `SdblColumnRef`, `SdblFunctionCall`, `SdblType`,
`SdblInlineTableFields`, `SdblCaseExpr`, `SdblWhenClause`) retain
their pre-C1 child-attachment shapes; the only intentional
behaviour change is the `column_or_function` clause-keyword
recovery fix (codex Round-1 finding 2 → C2 FIX) documented in the
attestation §Behaviour change. The Slice 10a mini-spec
(`sdbl-expressions-mini-spec.md`) was extended in C0a with new
sections §Predicates, §Comparison, §Column references and
function calls, §CAST type specification, §CASE expressions; the
§ITS coverage verification table was filled in C2 with verified-
yes / verified-no outcomes against the local ITS dump (chapters
21, 22, 23, 27, 32, 40 — chapter 28 deliberately NOT consulted
per codex Round-1 finding 1). The two `_legacy`-suffixed shims
born during Slice 10a C1 (`comparison_expr_legacy`,
`predicate_expr_legacy`) were retired in C1; the
`LEGACY (Slice 10b pending)` banner is empty.

Commit trail: C0a `77c75e29`, C0b `4c1e8170`, C1 `9899815f`,
C2 `98a2a6a2`, C3 `66dca2ae` + 8 C3 fixups (`ef85b028`
Anti-Hilbert close-out, `7f5e1cbc` post-C2 docstring/banner
final-state wording, `ecb26896` Anti-Hilbert close-out for the
two fixups, `9943d47a` codex adversarial-review findings:
provenance split + master-doc Files block flip + 3 NOT-boundary
acceptance tests, `8e0ca19c` Anti-Hilbert close-out for the next
two fixups, `82e09e3a` master-doc test-count bump 40 → 43,
`79c52f49` Anti-Hilbert close-out for the next two fixups,
`2b6d2e55` §Commit trail base-ref correction from `1635be4b` to
`6d7053bf`). See the attestation for the full per-phase breakdown
and the Anti-Hilbert disclosure for the trailing fixup that
names these hashes here.

#### Goal

Rebuild the remaining expression sub-grammar — predicates,
comparison, column / function call dispatch, CAST type spec, CASE.

#### Scope

- predicate bodies: IN, IN HIERARCHY, IS NULL, BETWEEN, LIKE, REFS;
- comparison operator tail (`=`, `<>`, `<`, `<=`, `>`, `>=`);
- column references and function call argument shape;
- CAST type specification (`ВЫРАЗИТЬ(... КАК type)`);
- CASE expression body (WHEN / THEN / ELSE / END);
- inline tabular field syntax (`.(Field1, Field2, …)`).

#### Files

- `crates/parser/src/grammar/sdbl/expressions.rs` — the 8 Slice
  10b functions (`comparison_expr`, `predicate_expr`,
  `column_or_function`, `inline_table_fields`, `is_cast_function`,
  `parse_cast_type`, `case_expr`, `when_clause`) live under the
  `CLEAN-ROOM Slice 10b — predicates, comparison, function calls,
  CAST, CASE` banner. C2 re-authored each body and attached
  per-function ITS / mini-spec / `// local: …` provenance
  comments; the previous LEGACY banner is empty.
- `docs/legal/sdbl-clean-room-slice10b.md` — Slice 10b clean-room
  attestation (landed with C3 `66dca2ae`).
- `docs/legal/sdbl-expressions-mini-spec.md` — extended in C0a
  (`77c75e29`) with §Predicates, §Comparison, §Column references
  and function calls (with §Inline tabular field syntax sub-
  section), §CAST type specification, §CASE expressions; the §ITS
  coverage verification table was filled in C2 with verified-yes
  / verified-no outcomes against the local pubqlang dump.
- `crates/parser/tests/sdbl_parser_tests.rs` — extended in C0b
  with 19 Bucket-A gap-test functions (12 a-l + 2 m EN/RU
  unignored in C2 + 5 n.1-n.5 SELECT-field predicate descendant
  guards).
- `crates/parser/tests/sdbl_slice10b_predicates.rs` — new in C3,
  43 spec-driven acceptance tests including the 2 mandatory
  clause-keyword recovery regression-gates, the 5 mandatory
  SELECT-field descendant guards, and the 3 NOT-boundary tests
  added in fixup `9943d47a` (NOT BETWEEN, NOT LIKE, orphan-NOT
  no-predicate-wrapper) per codex adversarial-review finding 3.

#### Notes

Slice 10b retires the two `_legacy`-suffixed shims born during
Slice 10a C1 (`comparison_expr_legacy`, `predicate_expr_legacy`)
and empties the `LEGACY (Slice 10b pending)` banner in
`expressions.rs`.

## Slice 11: clauses after FROM

**Status: complete (2026-04-26).** See
[`sdbl-clean-room-slice11.md`](sdbl-clean-room-slice11.md) for the
attestation. The 12 functions `select_tail_clauses`,
`query_body_clauses`, `where_clause`, `is_clause_keyword`,
`group_by_clause`, `order_by_clause`, `order_by_item`,
`having_clause`, `for_update_clause`, `index_by_clause`,
`autoorder_clause`, `totals_by_clause` were re-authored under the
`CLEAN-ROOM Slice 11 — clauses after FROM` banner in
`crates/parser/src/grammar/sdbl/select.rs`. The 8 NodeKinds
emitted by these functions (`SdblWhereClause`, `SdblGroupClause`,
`SdblOrderClause`, `SdblHavingClause`, `SdblForUpdate`,
`SdblIndexBy`, `SdblAutoorder`, `SdblTotalsBy`) retain their
pre-C1 child-attachment shapes; the ten parser-side AST-shape
invariants and ten child-attachment invariants in the attestation
pin the contracts that downstream consumers
(`crates/sdbl-hir/src/lower/clauses.rs:28-156` for WHERE / GROUP /
ORDER readers, `LogicalOrInWhere` recursive-walk reachability at
`clauses.rs:170-192`) read.

Authored from ITS pubqlang chapters 12 (§Структура запроса), 16
(§Сортировка результата запроса), 17 (§АВТОУПОРЯДОЧИВАНИЕ plus
sort-by-ссылочное-поле), 22 (§Условие отбора), 23 (§LIKE+WHERE),
24 (§WHERE+parameters), 27 (§Иерархическая упорядоченная выборка),
34 (§Группировка результата запроса), 35 (§Расчет агрегатов +
§Условие на агрегаты), 39 (§Расчет общих итогов); the C0a-extended
SELECT mini-spec §WHERE / §GROUP BY / §HAVING / §ORDER BY /
§AUTOORDER / §TOTALS BY / §FOR UPDATE / §INDEX BY clause-body
sections, §IDE-recovery allowances block (4 entries), §ITS
coverage verification table, and §Non-consultation statement
(Slice 11 reaffirmation); the lexer's Slice 2 attestation for
bilingual EN/RU keyword pairs (WHERE/ГДЕ, GROUP/СГРУППИРОВАТЬ,
HAVING/ИМЕЮЩИЕ, ORDER/УПОРЯДОЧИТЬ, BY/ПО, FOR/ДЛЯ,
UPDATE/ИЗМЕНЕНИЯ, INDEX/ИНДЕКСИРОВАТЬ,
AUTOORDER/АВТОУПОРЯДОЧИВАНИЕ, TOTALS/ИТОГИ, OVERALL/ОБЩИЕ,
ASC/ВОЗР, DESC/УБЫВ, HIERARCHY/ИЕРАРХИЯ).

The C2 commit landed one MANDATORY behaviour-change fix:
`order_by_item` now consumes the optional HIERARCHY/ИЕРАРХИЯ
modifier as a flat sibling IDENT token of `SdblOrderClause` (per
ITS chapter 27 attestation — `УПОРЯДОЧИТЬ ПО Наименование ИЕРАРХИЯ`),
atomic with unignoring
the C0b regression-gate test
`test_slice11_order_by_hierarchy_consumed`. Parser-only
acceptance: HIR semantic interpretation (adding hierarchy field
to `OrderByItem` + HIR regression test) is owned by Slice 13 since
`crates/sdbl-hir/**` is read-only per Slice 11's parser-only
scope. The remaining audit-gate decisions defaulted to
**Option B PRESERVE** per Slice 9 pattern (recovery hardening
deferred to Slice 12).

After Slice 11 landed, the residual `LEGACY` banner in
`select.rs` shrunk from `LEGACY (Slices 5, 11 pending)` to
`LEGACY (Slice 5 + SELECT limitation helpers pending)` and
contained 4 functions: `virtual_table_args_legacy` (Slice 5
target) plus `is_limitation_keyword` / `limitations` /
`top_clause` (SELECT prefix qualifiers, pending Slice-7-addendum)
plus the small `is_identifier_token` helper consumed by Slice 7
/ Slice 8 (same future addendum scope). The Slice-7-addendum
landed 2026-04-26 (see §Slice 7-addendum below); after that
addendum, the residual block shrinks further to
`LEGACY (Slice 5 pending)` and contains only
`virtual_table_args_legacy`.

### Scope

- `WHERE`
- `GROUP BY`
- `HAVING`
- `ORDER BY` (with HIERARCHY modifier per ITS chapter 27 —
  C2 MANDATORY FIX)
- `AUTOORDER`
- `TOTALS ... BY` (narrowed flat-list shape; structured
  ONLY/HIERARCHY-in-TOTALS/PERIODS modifier promotion deferred
  to Slice 12)
- `FOR UPDATE`
- `INDEX BY`

### Files

- `crates/parser/src/grammar/sdbl/select.rs` — the new
  `CLEAN-ROOM Slice 11 — clauses after FROM` section — 12
  functions.
- `crates/parser/src/grammar/sdbl.rs` — module-level
  `## Provenance` docstring Slice 11 entry, flipped to
  "complete (landed with C3 2026-04-26)" with attestation
  citation.
- `docs/legal/sdbl-select-mini-spec.md` — extended C0a +
  ITS verification rows filled C2 + post-C2 wording.
- `docs/legal/sdbl-clean-room-slice11.md` — Slice 11
  clean-room attestation (this slice's anchor document).
- `crates/parser/tests/sdbl_parser_tests.rs` — 14 Bucket-A
  gap-test functions added in C0b; test (g) flipped from
  `#[ignore]` to active in C2.
- `crates/parser/tests/sdbl_slice11_clauses.rs` — 35
  spec-driven acceptance tests added in C3.

## Slice 7-addendum: SELECT prefix qualifiers (DISTINCT / TOP / ALLOWED + helpers)

**Status: complete (2026-04-26).** See
[`sdbl-clean-room-slice7-addendum.md`](sdbl-clean-room-slice7-addendum.md)
for the attestation.

The Slice 7-addendum is a deferred follow-up to the Slice 7
clean-room (which landed 2026-04-25 and explicitly excluded the
SELECT-prefix qualifier helpers from its scope). The addendum
re-authors the four limitation-helper functions
(`is_identifier_token`, `is_limitation_keyword`, `limitations`,
`top_clause`) under the new
`CLEAN-ROOM Slice 7-addendum — SELECT prefix qualifiers` banner
in `crates/parser/src/grammar/sdbl/select.rs`, attaches
per-function provenance comments, and shrinks the residual
LEGACY banner in `select.rs` from
`LEGACY (Slice 5 + SELECT limitation helpers pending)` to
`LEGACY (Slice 5 pending)` — leaving only
`virtual_table_args_legacy` in the residual block.

A primary SDBL grammar source — v8.3.27 Developer's Reference
Глава 8 «Работа с запросами» —
<https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>
landed during the C0 review window. The §<Описание запроса>
EBNF skeleton places all three SELECT-prefix qualifiers
(РАЗРЕШЕННЫЕ / РАЗЛИЧНЫЕ / ПЕРВЫЕ) in their canonical first
three slots; the accompanying prose gives full semantics for
each qualifier. This source is **primary**; the pubqlang dump
(chapters 19/20/57) remains a **secondary** corroborating
textbook companion. After the addendum lands, ALL three
SELECT-prefix qualifiers are Tier A1 with v8327doc Глава 8 as
the cited primary source.

The addendum carries a §Deferred semantic constraint note
(codex Round-4 finding 4): the v8327doc-attested top-level-only
constraint on РАЗРЕШЕННЫЕ is NOT enforced at parser level;
HIR-level / IDE-diagnostic-level enforcement is deferred to Slice 13
or a dedicated diagnostic.

### Scope

- `is_identifier_token` — Tier C/B local parser contract
  (Ident predicate; cross-slice consumer of Slice 7 alias-scan
  + Slice 8 source-alias guard).
- `is_limitation_keyword` — Tier A1 predicate matching the
  bilingual SELECT-prefix qualifier vocabulary.
- `limitations` — Tier A1 main entry; emits `SdblLimitations`.
- `top_clause` — Tier A1 helper; emits `SdblTopClause`.

### Files

- `crates/parser/src/grammar/sdbl/select.rs` — the new
  `CLEAN-ROOM Slice 7-addendum — SELECT prefix qualifiers`
  banner — 4 functions; LEGACY banner shrunk to
  `LEGACY (Slice 5 pending)`.
- `crates/parser/src/grammar/sdbl.rs` — module-level
  `## Provenance` docstring Slice 7-addendum entry, flipped to
  "complete (landed with C3 2026-04-26)" with attestation
  citation.
- `docs/legal/sdbl-select-mini-spec.md` — §Limitations
  extended at C0 with full AST-shape contract + IDE-recovery
  allowances Q1/Q2/Q3 + Tier classification + ITS coverage +
  §Deferred semantic constraint + §Non-consultation statement
  (Slice 7-addendum reaffirmation); §ITS coverage verification
  table extended with three new rows for DISTINCT, TOP,
  ALLOWED.
- `docs/legal/sdbl-clean-room-slice7-addendum.md` — Slice
  7-addendum clean-room attestation (this addendum's anchor
  document).
- `crates/parser/tests/sdbl_parser_tests.rs` — 5 Bucket-A
  gap-test functions added in C0 (192 → 197 tests).
- `crates/parser/tests/sdbl_slice7_addendum_limitations.rs` —
  13 spec-driven acceptance tests added in C3.

## Slice 8-addendum: virtual-table arguments parser body

**Status: complete (2026-04-26).** See
[`sdbl-clean-room-slice8-addendum.md`](sdbl-clean-room-slice8-addendum.md)
for the attestation. Commit trail: C0a `a8e262f4`, C0b
`dd7b4b02`, C1 `228db0b2`, C2 `9267b29e`, C3 landed with the
attestation.

The Slice 8-addendum is a deferred follow-up to Slice 8 (which
landed 2026-04-25 and carved the virtual-table argument-list
parsing out into a Tier B `virtual_table_args_legacy` helper to
keep its own clean-room scope tight). The addendum re-authors
the two virtual-table argument helpers
(`virtual_table_args` — renamed from `virtual_table_args_legacy`
in C1 — and the paren-depth-tracking recovery utility
`recover_to_delimiter_vt`) under a new
`CLEAN-ROOM Slice 8-addendum — virtual-table arguments` banner
in `crates/parser/src/grammar/sdbl/select.rs`, attaches
per-function provenance comments citing the public ITS URL +
pubqlang chapter identifiers, and removes the residual LEGACY
banner block from `select.rs` entirely. After this slice lands,
`select.rs` carries zero LEGACY content; the next remaining
parser-side LEGACY surface is the lexer-side
`crates/lexer/src/sdbl/mod.rs` Slice-2 LEGACY block, which
master-doc Slice 5 still owns.

A primary SDBL grammar source — v8.3.27 Developer's Reference
Глава 8 «Работа с запросами» —
<https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>
provides the canonical example
`РегистрНакопления.УчетНоменклатуры.ОстаткиИОбороты(, , Авто, , )`
in Глава 8.3 «Виртуальные и обычные поля» plus VT-introduction
prose in Глава 8.2 «Виртуальные таблицы». Pubqlang chapters
9 (`СрезПоследних` peripheral intro), 104 (`Обороты` with
nested function-call arg + named-condition trailing arg),
116 (`Обороты()` parameter-order prose), 152 (no-args
`Остатки()` + leading-empty `Остатки( , cond)`), and 156
(IN-subquery as VT param structural form) provide the
remaining structural attestations. Per the user's
citation-policy directive, the Slice 8-addendum attestation,
mini-spec extension, source-code provenance comments, and
commit messages cite ONLY the public ITS URL and pubqlang
chapter identifiers — no local mirror paths. This is the
first SDBL clean-room slice authored under that prospective
policy; prior slices retain their pre-policy citation form.

### Scope

- `virtual_table_args` — Tier A1. Parses
  `'(' [vt-arg-list] ')'` per the SELECT mini-spec
  §Virtual table argument behavior Grammar EBNF; emits
  `SdblMissingArg` markers for empty slots and (via
  `recover_to_delimiter_vt`) `Error` sub-nodes for
  spurious-token recovery.
- `recover_to_delimiter_vt` — Tier D. Parser-internal
  paren-depth-tracking recovery utility for VT-args context;
  functionally equivalent to `recover_to_delimiter` in
  `expressions.rs` (both share paren-depth tracking,
  comma/semicolon stop, `is_clause_keyword` stop, and
  unconditional `Error` emit).

### Files

- `crates/parser/src/grammar/sdbl/select.rs` — the new
  `CLEAN-ROOM Slice 8-addendum — virtual-table arguments`
  banner — 2 functions; the residual LEGACY banner block
  (`LEGACY (Slice 5 pending)`) is removed entirely.
- `crates/parser/src/grammar/sdbl.rs` — module-level
  `## Provenance` docstring Slice 8-addendum entry, flipped
  to "complete (landed with C3 2026-04-26)" with attestation
  citation.
- `docs/legal/sdbl-select-mini-spec.md` — §Virtual table
  argument behavior extended at C0a with Grammar EBNF +
  AST-shape contract + IDE-recovery allowances #1–#6 + Tier
  classification + §ITS coverage verification rows for
  v8327doc Глава 8.2 / 8.3 + pubqlang chapters 9 / 104 /
  116 / 152 / 156 (filled in at C2); §Non-consultation
  statement (Slice 8-addendum reaffirmation).
- `docs/legal/sdbl-clean-room-slice8-addendum.md` — Slice
  8-addendum clean-room attestation (this addendum's anchor
  document).
- `crates/parser/tests/sdbl_parser_tests.rs` — 7 Bucket-A
  gap-test functions added in C0b (197 → 204 tests). C1
  also touches the C0b header comment to drop the
  pre-rename function name.
- `crates/parser/tests/sdbl_slice8_sources.rs` — comment-
  only update at C1 (test-side rename).
- `crates/parser/tests/sdbl_slice8_addendum_virtual_table_args.rs`
  — 16 spec-driven acceptance tests added in C3.

## Slice 2-addendum: clause keyword leftovers (lexer)

**Status: complete (2026-05-07).** See
[`sdbl-clean-room-slice2-addendum.md`](sdbl-clean-room-slice2-addendum.md)
for the attestation. Commit trail: C0a `7a6baf09`, C0b `768704a6`,
C1 `9f535e0d`, C2 `4e615e95`, C3 landed with the attestation.

The Slice 2-addendum is a deferred follow-up to the Slice 2 lexer
clean-room (which landed 2026-04-24 and explicitly excluded the
long-tail clause keywords from its `CLEAN-ROOM Slice 2 — structural
keyword vocabulary` banner). The addendum re-authors 17 clause-level
keyword variants under a new `CLEAN-ROOM Slice 2-addendum — clause
keyword leftovers` banner in `crates/lexer/src/sdbl/mod.rs`,
attaches per-variant ITS provenance comments citing v8327doc Глава 8
plus pubqlang corroborating chapters (16, 17, 27, 31, 39, 40, 51,
73, 96), and shrinks the residual `LEGACY` banner header from
`LEGACY (Slices 3–5 pending)` to
`LEGACY (Slices 3, 4, 5 pending — metadata / function /
virtual-table vocabularies)`. The remaining LEGACY surface after
this addendum is the `Mdo*` (Slice 3) + `Type*` (Slice 3) +
`LitUndefined` (Slice 3) + `Period*` (Slice 3) + `Fn*` (Slice 4) +
`Vt*` (Slice 5) + `Error` fallback families — no clause-shaped
keywords remain.

The addendum landed one MANDATORY behaviour-change fix: KwPeriods'
Russian regex alternation was corrected from the pre-addendum
nominative-case `ПЕРИОДЫ` to the canonical instrumental-case
`ПЕРИОДАМИ` per v8327doc Глава 8 bilingual word-list +
canonical EBNF + canonical example. Parser-tree-invariant: the
token converter at
`crates/parser/src/sdbl_token_converter.rs` already maps
`KwPeriods → TokenKind::Ident` and Slice 11 explicitly defers
structured PERIODS handling to Slice 12, so no observable
parse-tree shape changes today. See attestation § Behaviour
change for the full Option A decision rationale.

The addendum does NOT touch parser-side rustdoc Tier-D
classifications for FOR UPDATE / INDEX BY at
`crates/parser/src/grammar/sdbl/select.rs:1292-1297, 1349-1352`
or in `docs/legal/sdbl-select-mini-spec.md:759-789`; those
classifications are stale (they predate v8327doc landing in
Slice 7-addendum 2026-04-26, which now Tier-A1-attests both
clauses) and should be flipped in a separate parser-only
follow-up commit. See attestation § Pre-existing parser-side
stale-classification follow-up.

### Scope

- 17 lexer token variants in `crates/lexer/src/sdbl/mod.rs`:
  `KwDrop`, `KwAutoOrder`, `KwAsc`, `KwDesc`, `KwHierarchy`,
  `KwAllowed`, `KwFor`, `KwUpdate`, `KwIndex`, `KwOnly`,
  `KwOverall`, `KwPeriods`, `KwEscape`, `KwRefs`, `KwCast`,
  `KwType`, `KwValue`. All 17 classified Tier A1 with v8327doc
  Глава 8 as the primary canonical SDBL grammar source per the
  attestation § Per-variant tier source map.

### Files

- `crates/lexer/src/sdbl/mod.rs` — the new
  `CLEAN-ROOM Slice 2-addendum — clause keyword leftovers`
  banner (17 `#[regex]` declarations with per-variant ITS
  provenance docstrings + thematic convenience index). The
  file-level `## Provenance` docstring carries a fourth bullet
  for the Slice 2-addendum scope. The residual `LEGACY` banner
  header text is shrunk to enumerate the per-slice ownership of
  remaining tokens.
- `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt` — 13
  thematic Slice 2-addendum corpus entries (058–070) covering
  the 13 Russian spelling blind spots + 3 English gap-fillers;
  entry 040 byte-string updated from `ПЕРИОДЫ` to canonical
  `ПЕРИОДАМИ` at C2.
- `crates/lexer/tests/sdbl_golden_corpus.rs` — snapshot
  regenerated at C0b (16-variant gap-fill) and C2 (KwPeriods
  byte-string flip).
- `crates/lexer/tests/sdbl_slice2_keywords.rs` — 13 RU
  Bucket-A regression-gate tests added at C0b (count 30 → 43).
- `crates/lexer/tests/sdbl_slice2_addendum_clause_keywords.rs`
  — new acceptance test file born at C2 with 3 KwPeriods
  regression gates (canonical / English / legacy-misspelling-
  now-Ident); expanded at C3 to 30 spec-driven acceptance
  tests covering bilingual EN+RU pairing for 16 variants
  (KwPeriods covered by the regression gates), a case-
  insensitivity sweep, 9 structural integration tests
  exercising addendum keywords in realistic SDBL clause
  fragments, and 1 keyword-prefix Ident longest-match guard.
- `docs/legal/sdbl-clean-room-slice2-addendum.md` — Slice
  2-addendum clean-room attestation (this addendum's anchor
  document).
- `docs/legal/sdbl-clean-room-slice2.md` — Slice 2 attestation
  § Scope flipped at C3 to acknowledge the addendum claim of
  the 17 clause-keyword variants.

### Notes

The addendum is a **lexer-only** clean-room. Parser-side files
(`crates/parser/src/grammar/sdbl/**`, `crates/sdbl-hir/**`) and
their existing rustdoc Tier classifications are not modified;
the converter mapping `KwPeriods → TokenKind::Ident` keeps the
KwPeriods regex flip parser-tree-invariant.

Codex pair-mode review pass: 2 plan-review rounds + 1 C0a
document review + 1 post-edit consistency verification + 1 C2
review; 0 BLOCKER, 4 STRONG (all addressed inline), 6+ VERIFIED.

## Slice 3a: primitive types, undefined literal, narrow period vocabulary (lexer)

**Status: complete (2026-05-07).** See
[`sdbl-clean-room-slice3a.md`](sdbl-clean-room-slice3a.md) for the
attestation. Commit trail: C0a `51f17fff`, C0b `f6fcdc2e`, C1
`297b529f`, C2 `077ae770`, C3 landed with the attestation.

Slice 3a is the first sub-slice carved out of master-doc Slice 3
per the codex pair-mode plan-review (2026-05-07). It claims the
seven lexer variants whose canonical SDBL grammar attestation in
v8327doc Глава 8 «Работа с запросами» is unambiguous and direct:
the four primitive type literals (`Булево / Boolean`,
`Число / Number`, `Строка / String`, `Дата / Date`), the
`Неопределено / UNDEFINED` literal, and the two narrow period-type
keywords carried as dedicated lexer tokens (`Декада / TENDAYS`,
`Полугодие / HALFYEAR`). The remaining 14 `Mdo*` variants are
claimed by Slice 3b (separate clean-room arc, pending); the
platform-late `MdoExternalDataSource` is deferred to master-doc
Slice 5 (which already owns external-source handling).

The slice carries **no behaviour change**: the C0a discrepancy
audit (named explicitly per codex pair-mode plan-review STRONG
finding to forestall a KwPeriods-style regex defect from
recurring) found zero defects in the seven `#[regex]` bodies. The
seven attribute texts are byte-identical to pre-Slice-3a; only
banner placement, per-variant provenance docstrings, the thematic
convenience index, and the cross-references to Slice 2-addendum's
`KwType` and `KwPeriods` are new. The byte-identity golden corpus
(`crates/lexer/tests/sdbl_golden_corpus.rs`) gates the PRESERVE-
shape conclusion.

### Scope

- 7 lexer token variants in `crates/lexer/src/sdbl/mod.rs`:
  `TypeBoolean`, `TypeNumber`, `TypeString`, `TypeDate`,
  `LitUndefined`, `PeriodTenDays`, `PeriodHalfYear`. All 7
  classified Tier A1 with v8327doc Глава 8 as the primary
  canonical SDBL grammar source per the attestation § Per-variant
  tier source map.

### Files

- `crates/lexer/src/sdbl/mod.rs` — the new
  `CLEAN-ROOM Slice 3a — primitive types, undefined literal,
  narrow period vocabulary` banner (7 `#[regex]` declarations
  with per-variant v8327doc Глава 8 provenance docstrings + the
  thematic convenience index + cross-reference paragraph for
  `KwType` / `KwPeriods`). The file-level `## Provenance`
  docstring carries a fourth bullet for the Slice 3a scope. The
  residual `LEGACY` banner header text shrinks from
  `LEGACY (Slices 3, 4, 5 pending — metadata / function /
  virtual-table vocabularies)` to
  `LEGACY (Slices 3b, 4, 5 pending — Mdo*/function/virtual-table
  vocabularies + ExternalDataSource)`.
- `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt` — three
  thematic Slice 3a corpus entries (071–073) closing the six
  bilingual blind spots surfaced by the attestation's Pre-C0b
  corpus coverage audit (RU spellings of `БУЛЕВО`, `СТРОКА`,
  `ДАТА`; EN spellings of `UNDEFINED`, `TENDAYS`, `HALFYEAR`).
- `crates/lexer/tests/sdbl_golden_corpus.rs` — snapshot
  regenerated at C0b.
- `crates/lexer/tests/sdbl_slice3a_types.rs` — new acceptance
  test file born at C3 with 25 spec-driven tests: 14 bilingual
  EN+RU canonical-form pins (7 variants × 2 spellings), 1 case-
  insensitivity sweep, 9 structural integration tests (4 CAST
  type-slot, 2 TYPE() expression, 1 LitUndefined / LitNull
  predicate-position asymmetry, 2 TOTALS BY PERIODS period-type
  slot), 1 keyword-prefix Ident longest-match guard.
- `docs/legal/sdbl-clean-room-slice3a.md` — Slice 3a clean-room
  attestation (this slice's anchor document).

### Notes

The slice is a **lexer-only** clean-room — parser-side files
(`crates/parser/src/grammar/sdbl/**`, `crates/sdbl-hir/**`) are
not modified. The converter at
`crates/parser/src/sdbl_token_converter.rs` maps the seven
Slice 3a variants asymmetrically and Slice 3a does not modify
any of those mappings:

- `TypeBoolean | TypeNumber | TypeString | TypeDate → Ident`
  (shared arm at line 125) — parser disambiguates by text in
  the CAST `<Тип значения>` and `<Значение>` slots.
- `PeriodTenDays | PeriodHalfYear → Ident` (shared arm at
  line 168) — parser disambiguates by text in the TOTALS BY
  ПЕРИОДАМИ list.
- `LitUndefined → KwUndefined` (single arm at line 196) — the
  only Slice 3a variant with a dedicated downstream
  `TokenKind`. The companion NULL literal owned by Slice 2's
  `LitNull` maps the other way (`LitNull → Ident` at line 57
  with `at_keyword("NULL")` text probe). The LitUndefined /
  LitNull converter asymmetry is recorded in the C2
  `LitUndefined` rustdoc as contextual prose; the C3 acceptance
  suite test `undefined_predicate_position_english` pins only
  the lexer-side contract (`UNDEFINED` emits `LitUndefined`,
  `NULL` emits `LitNull`, distinct kinds emitted side-by-side),
  not the parser-side converter mapping itself.

The Pre-existing parser-side stale-classification follow-up
documented in the Slice 2-addendum attestation
([`sdbl-clean-room-slice2-addendum.md`](sdbl-clean-room-slice2-addendum.md)
§ Pre-existing parser-side stale-classification follow-up) is
explicitly out of scope for Slice 3a — the same out-of-scope
boundary applies as the Slice 2-addendum precedent. The follow-up
remains a separate parser-only commit candidate.

Codex pair-mode review pass: 2 plan-review rounds + 1 C0a
document review + 1 C2 source review; 1 BLOCKER (LitUndefined
converter mapping claim — addressed inline before C2 commit), 4
STRONG (all addressed inline), multiple VERIFIED.

## Slice 12: recovery and IDE allowances

### Goal

Reintroduce non-normative parser behavior deliberately, instead of inheriting it
accidentally from upstream grammar or old tests.

### Scope

- incomplete queries while typing
- flexible clause ordering retained for IDE usefulness
- conservative error nodes
- multiline query string artifacts
- convenience handling such as line comments if the project still wants them

### Principle

Every recovery rule in this slice should be explicitly documented as:

- required by official syntax
or
- intentionally kept for editor/IDE behavior

## Slice 13: `sdbl-hir` reattachment

### Goal

Reconnect the cleaned parser surface to semantic lowering without dragging along
old parser assumptions blindly.

### Scope

- `crates/parser/src/sdbl_token_converter.rs`
- lowerers/source maps relying on old syntax shapes
- semantic tests that assumed old parser quirks

## Recommended implementation order

1. Slice 0
2. Slice 1
3. Slice 2
4. Slice 6
5. Slice 7
6. Slice 8
7. Slice 10
8. Slice 9
9. Slice 11
10. Slice 3
11. Slice 4
12. Slice 5
13. Slice 12
14. Slice 13

This ordering deliberately pulls forward the smallest end-to-end parser path:

- lexer core
- core keywords
- query package
- basic `SELECT`
- basic `FROM`
- basic expressions

and delays the heaviest vocabulary rebuild until the core parser shape is owned.

## File ownership map

### Mostly lexer slices

- Slice 1
- Slice 2
- Slice 3
- Slice 4
- Slice 5

### Mostly parser slices

- Slice 6
- Slice 7
- Slice 8
- Slice 9
- Slice 10
- Slice 11
- Slice 12

### Cross-layer slices

- Slice 0
- Slice 13

## Bottom line

The clean-room path should not start from “rewrite the whole parser”.

It should start from a narrower claim:

- the **language** belongs to 1C as part of the platform;
- the **third-party grammar texts and token inventories** are what create the
  current copyleft risk;
- therefore the safest migration path is a **slice-by-slice replacement** of
  the SDBL lexer/parser expression layer while preserving as much local parser
  architecture as possible.
