# SDBL Test Corpus Slice 0 Audit

## Purpose

This note is the Slice 0 baseline for SDBL clean-room work.

It classifies the current SDBL-heavy test corpus into categories that matter for
provenance and rewrite planning.

## Scope

Reviewed test sources:

- `crates/parser/tests/sdbl_parser_tests.rs` (`2160` lines)
- `crates/sdbl-hir/src/lower/tests.rs` (`2765` lines)
- `crates/parser/tests/fixtures/user_query_with_highlighting_issue.sdbl`

## High-level conclusion

The current SDBL test corpus is **too mixed** to use as a clean-room baseline
without triage.

Right now it contains at least four different kinds of tests:

1. generic language-acceptance coverage
2. AST/API contract tests for local syntax wrappers
3. local semantic/HIR behavior tests
4. regression fixtures and reference-shaped examples, including some explicitly
   labeled `java` / `reference` style scenarios

That means Slice 0 should not try to preserve this corpus as-is. It should split
it into:

- clearly owned tests to keep
- tests to rewrite in fresh wording
- tests to postpone until parser/lexer replacement slices are stable

## File-by-file assessment

## `crates/parser/tests/sdbl_parser_tests.rs`

Current assessment: **high audit value, mixed provenance**

### What is favorable

A large part of this file is generic language acceptance coverage and looks
replaceable from first principles:

- simple `SELECT`
- aliases
- `UNION`
- subqueries
- arithmetic/logical/comparison expressions
- basic functions
- literals and parameters
- `GROUP BY`, `ORDER BY`, `BETWEEN`, `LIKE`, `IS NULL`
- `CASE`
- `INTO`
- `REFS`
- parameterized table sources

These tests mostly encode obvious language behavior, and many of them can be
rewritten from official 1C docs without difficulty.

### What needs caution

This file also contains tests that are much more provenance-sensitive:

- explicit AST shape assertions via `expect!` snapshots
- tests labeled around `java` / `reference`
- tests built from exact large multiline query texts
- regression tests for very specific parser quirks
- fixture-backed test:
  - `test_complete_user_query_from_fixture`
  - `include_str!("fixtures/user_query_with_highlighting_issue.sdbl")`

Examples of provenance-sensitive anchors found during this pass:

- `test_exact_java_query_structure`
- comments like `reference test`
- `test_complete_user_query_from_fixture`

### Working split for this file

#### Keep as concept, but rewrite text/examples

- basic language-acceptance tests
- most alias / union / function / literal / clause tests
- high-level parser recovery tests that can be restated briefly

#### Keep structurally, but re-own carefully

- AST/API contract tests:
  - alias `has_as_keyword`
  - asterisk field helpers
  - union-query traversal helpers

These tests are valuable because they protect local syntax API behavior, but the
query texts used in them should be treated as replaceable.

#### Highest rewrite priority

- large reference-shaped multiline queries
- anything explicitly tied to `java` or `reference`
- fixture-backed tests
- tests whose main value is historical regression rather than language surface

## `crates/sdbl-hir/src/lower/tests.rs`

Current assessment: **medium risk, more local than parser tests**

### What is favorable

This file is dominated by local semantic behavior:

- source-map collection
- aggregate/special keyword capture
- join type lowering
- temp table propagation
- metadata-backed resolution
- tabular section resolution
- bilingual name matching
- type mapping into local `SdblType`
- incomplete-query resilience for IDE scenarios

A lot of that value is clearly specific to this codebase and to `sdbl-hir` as a
semantic layer.

### What still needs caution

Even here, some tests still sit on top of parser quirks or use detailed query
examples that may have grown out of upstream-shaped acceptance cases.

The most caution-worthy areas are:

- large multiline join/recovery scenarios
- temp-table / union regression cases copied from prior parser behavior
- keyword-collection tests that mirror parser token categories very closely

### Working split for this file

#### Good candidates to keep with minimal rewriting

- metadata resolution tests
- tabular section tests
- source-map collection tests tied to local HIR categories
- type inference / resolved-table behavior tests

#### Rewrite opportunistically

- large parser-shaped query texts
- tests that are really parser acceptance tests disguised as HIR tests

## `user_query_with_highlighting_issue.sdbl`

Current assessment: **probably local, still not ideal as a foundational clean-room asset**

Favorable signal:

- earlier audit did not find an obvious match in the sibling `../bsl-parser`
  tree.

Caution:

- this is still a historical regression fixture, not a principled language-spec
  example;
- it is useful as a compatibility regression, but not as the main design input
  for a clean-room parser rewrite.

Working conclusion:

- keep it as a regression artifact if still useful;
- do not let it define the new parser shape.

## Recommended Slice 0 buckets

## Bucket A: owned and safe to preserve conceptually

Definition:

- generic language behavior
- local syntax API contracts
- local semantic/HIR behavior

Examples:

- simple `SELECT` / `FROM` / `WHERE`
- alias API tests
- basic `UNION`
- metadata resolution
- tabular section lowering
- `TOP` / `DISTINCT` lowering checks

Action:

- keep, but freely rewrite example texts

## Bucket B: keep behavior, rewrite examples

Definition:

- tests whose assertion is still useful, but the query text is too specific,
  bulky, or historically inherited

Examples:

- complex multiline joins
- nested unions with comments and semicolons
- parser recovery around incomplete `ON` clauses

Action:

- rewrite from the minimal local scenario needed to prove the same behavior

## Bucket C: regression archive only

Definition:

- tests whose main role is “we once had this exact bug/query”
- historical or reference-shaped fixtures

Examples:

- `test_exact_java_query_structure`
- `test_complete_user_query_from_fixture`
- any test explicitly labeled `reference`

Action:

- keep separately as compatibility regressions if still valuable;
- do not use as specification input for clean-room rewrite slices

## Immediate practical next step

For Slice 0, the next useful action is:

1. create a short marker file or comment policy that labels tests in
   `sdbl_parser_tests.rs` as `A`, `B`, or `C`;
2. start by rewriting Bucket C first, because it gives the biggest legal payoff
   with the smallest effect on parser architecture;
3. only then start the lexer/parser replacement slices.

## Bottom line

The SDBL test corpus is not hopelessly contaminated, but it is not yet a
clean-room-ready baseline either.

The best current reading is:

- parser tests are the noisier and more provenance-sensitive half;
- `sdbl-hir` tests are more local and more worth preserving;
- large reference-shaped regression queries should be treated as rewrite-first
  material before the real clean-room lexer/parser work begins.
