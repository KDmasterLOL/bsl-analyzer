# SDBL Slice 6 — Clean-Room Attestation

**Status:** complete (2026-04-24).

This document attests the clean-room authorship of the Slice 6
material of the SDBL parser root and package skeleton, per the staged
migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Scope

The paths claimed as clean-room Slice 6 authorship are:

- `crates/parser/src/grammar/sdbl.rs` — specifically:
  - the module-level `## Provenance` docstring block for Slice 6;
  - the functions declared under the `CLEAN-ROOM Slice 6 — query
    package, DROP, select entry` banner, with their per-function
    provenance comments. The full Slice 6 list here is 3 functions:
    - `query_package` — entry point `query-package := query-item (';'
      query-item)* ';'?`, plus a parser-tolerance rule for empty and
      trivia-only input.
    - `queries` — dispatcher between SELECT and DROP statements.
    - `drop_table_query` — `DROP` / `УНИЧТОЖИТЬ` statement for a
      temporary table named by a single identifier.
- `crates/parser/src/grammar/sdbl/select.rs` — specifically the 3
  functions under the `CLEAN-ROOM Slice 6 — select entry wrapper,
  subquery, UNION clause` banner, with their per-function provenance
  comments:
    - `select_query` — post-split wrapper that opens `SdblSelectQuery`
      around `subquery()` and `select_tail_clauses()`.
    - `subquery` — main query body plus the `UNION` clause loop;
      terminates at `;`, on any non-UNION token, or at EOF.
    - `union_clause` — `UNION [ALL] query`; bundles `UNION` and `UNION
      ALL` into a single `SdblUnionClause` node kind (see § Preserved
      pre-refactor behaviours).
- `crates/parser/tests/sdbl_slice6_package.rs` — the new spec-driven
  acceptance test file authored against the ITS sources below, not
  against the existing `parse_sdbl` output.

The following 5 `SyntaxKind` node kinds are locked in place by Slice 6
(no rename, no addition, no removal, no enum reorder):

- `SdblQueryPackage`
- `SdblSelectQuery`
- `SdblDropQuery`
- `SdblSubquery`
- `SdblUnionClause`

The `LEGACY (Slices 7–11 pending)` portion of `select.rs`
(`select_tail_clauses` and everything below it — `query`, SELECT
fields, `FROM`, `JOIN` family, `WHERE`, `GROUP`, `ORDER`, `TOTALS`,
`FOR UPDATE`, `INDEX BY`, expression glue, recovery helpers) remains
explicitly **not** covered by this attestation; those functions stay
Tier B and will be re-derived by Slices 7–11.

Downstream consumers of the 5 Slice 6 node kinds
(`crates/parser/src/sdbl_token_converter.rs`,
`crates/parser/src/lib.rs`, `crates/parser/src/event.rs`,
`crates/syntax/src/syntax_kind.rs`, `crates/syntax/src/ast.rs`,
`crates/sdbl-hir/src/lower/mod.rs`,
`crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs`,
`crates/ide-db/src/database_impl_tests.rs`,
`crates/ide/tests/sdbl_completion_integration_test.rs`,
`crates/mcp-server/src/tools/query.rs`) were not modified in Slice 6;
they continue to see the public surfaces `parser::parse_sdbl(&str) ->
syntax::Parse<SyntaxNode>`, `parser::parse_sdbl_with_shared_cache(&str)`,
and the 5 locked `SyntaxKind` variants unchanged.

`SdblUnionClause::has_all()` at `crates/syntax/src/ast.rs:1035` is the
post-parse helper that distinguishes `UNION` from `UNION ALL` by
scanning IDENT tokens inside the clause for `"ALL"` / `"ВСЕ"`. The
helper's sole current consumer is
`crates/sdbl-hir/src/lower/mod.rs:173-175` (the ALL-recording path).

## Sources consulted

The Slice 6 material was re-derived from:

1. 1C ITS documentation:
   - <https://its.1c.ru/db/pubqlang/content/10/hdoc> — query-language
     structure: the query package shape (one or more query items
     separated by `;`, optional trailing `;`), the `SELECT` entry
     point, subquery scope vs package scope, the `UNION` / `UNION ALL`
     skeleton.
   - <https://its.1c.ru/db/pubqlang/content/12/hdoc> — lexical
     elements: the bilingual keyword vocabulary for `DROP` /
     `УНИЧТОЖИТЬ`, `UNION` / `ОБЪЕДИНИТЬ`, `ALL` / `ВСЕ`, `SELECT` /
     `ВЫБРАТЬ`, and the identifier longest-match rule that separates
     identifiers from keywords.
   - <https://its.1c.ru/db/pubqlang/content/51/hdoc/h47> —
     temporary-table lifecycle: `DROP` / `УНИЧТОЖИТЬ` terminates the
     lifetime of a temporary table named by a single identifier.
2. The local SDBL SELECT mini-spec at
   [`sdbl-select-mini-spec.md`](sdbl-select-mini-spec.md), which
   already carries the query-package / subquery / UNION grammar shape
   in the project's own phrasing, clean-room from the ITS sources
   above.
3. The Slice 1 and Slice 2 clean-room material already present in
   `crates/lexer/src/sdbl/mod.rs` — consulted only for the shape of
   per-function provenance comments and the project's event-parser
   conventions (marker start/complete, `p.bump()`, `p.skip_trivia()`,
   `p.at_keyword()`, `at_sdbl_keyword`, `eat_sdbl_keyword`,
   `p.check_iteration_limit()`).

The resulting event-parser shape for the 6 Slice 6 functions is the
natural expression of the ITS grammar-shape rules and the project's
own event-parser conventions, and would converge regardless of author.
The claim made here is **independent derivation from the sources
above**, not textual novelty.

## Non-consultation statement

During the authorship of the Slice 6 material the following sources
were not used as working text:

- the sibling `../bsl-parser` project — neither its grammar files nor
  its parser implementation were consulted;
- the pre-C1 function bodies of the 6 Slice 6 functions — the C1
  commit extracted the `AUTOORDER / ORDER BY / TOTALS BY` tail-clause
  loop from `select_query` into `select_tail_clauses` as a pure
  refactor so the post-C1 `select_query` wrapper could be attested
  independently, but the body text of the wrapper, `subquery`,
  `union_clause`, `query_package`, `queries`, `drop_table_query` was
  re-derived against the ITS sources and the project's mini-spec
  rather than copied from C1 verbatim;
- any other third-party SDBL parser, grammar, or event-tree
  implementation.

The 123 pre-existing SDBL parser tests in `sdbl_parser_tests.rs`, the
82+ (204 as of C2) tests in `sdbl-hir`, the SDBL tests in `ide-db`,
and the nested UNION package scenarios in the `ide`
`sdbl_completion_integration_test` form the regression gate that the
re-derived functions accept exactly the same input set as the
pre-refactor implementation. At parser level there is no
byte-identity golden corpus analogous to the lexer's; the test suite
is the gate.

## Preserved pre-refactor behaviours

Three behaviours observed in the pre-clean-room parser diverge from
what a strict reading of the ITS spec would produce and are preserved
bit-for-bit in Slice 6:

1. **`SdblUnionClause` bundles `UNION` and `UNION ALL` into a single
   node kind.** The optional `ALL` / `ВСЕ` keyword is consumed into
   the clause node's token stream but not projected into a distinct
   node kind. `SdblUnionClause::has_all()` at
   `crates/syntax/src/ast.rs:1035` scans IDENT tokens for
   `"ALL"` / `"ВСЕ"` post-parse, and
   `crates/sdbl-hir/src/lower/mod.rs:173-175` consumes that helper in
   the ALL-recording path.

   A true split (`SdblUnionClause` vs `SdblUnionAllClause`) is an
   improvement for semantic layers but is a breaking AST-surface
   change with three existing touchpoints —
   `crates/syntax/src/syntax_kind.rs` (new variant, changes the u16
   discriminant layout), `crates/syntax/src/ast.rs` (new AST
   wrapper), and `crates/sdbl-hir/src/lower/mod.rs` (replace
   `has_all()` with a pattern match on two distinct node kinds) —
   plus a design-risk surface for future diagnostics/IDE handlers
   that might want to observe the `ALL` modifier. These files are
   out of Slice 6 scope per the File-ownership map in
   [`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md) (Slice 6
   is parser-only). The split is naturally re-visited in Slice 13
   (sdbl-hir reattachment), where AST + HIR + downstream edits are
   in scope.

2. **`drop_table_query` identifier-recovery path is local-preserved.**
   The body emits an `SdblDropQuery` node with exactly one identifier
   child when the identifier is present and an `Error` sub-node when
   it is not, so the IDE can reason about `DROP` / `УНИЧТОЖИТЬ`
   statements under partial typing. The ITS sources identified during
   Slice 6 research (pubqlang/10, /12, /51 h47) describe the `DROP
   <temp-table>` syntax and lifecycle but do not spell the exact
   grammar production; the minimal `DROP <ident>` shape used here is
   spec-derivable. A tightened rewrite with a specific ITS sub-page
   citation is expected when Slice 3 promotes the `KwDrop` lexer
   variant out of the LEGACY banner.

3. **`sdbl_parser_tests.rs:737
   (test_exact_extracted_query_from_logs) is retained as Bucket C.**
   The test encodes a specific error-recovery invariant — the package
   boundary (`;` separator) must still yield two `SdblSelectQuery`
   children even when the first query has a malformed `ON` expression
   — that is not spec-derivable from the ITS sources above. The other
   two Bucket-C tests on the Slice 6 surface
   (`sdbl_parser_tests.rs:480 (test_double_union_all_queries_with_aliases)`
   and
   `sdbl_parser_tests.rs:692 (test_into_clause_with_union_and_semicolon_separator)`)
   were rewritten on spec-driven Товары / Услуги / ВыбранныеТовары /
   ЦеныТоваров schemas and promoted to Bucket B in C0.

## Verification recipe

All of the following must be green before this attestation is
considered live:

1. `cargo test -p parser --test sdbl_parser_tests` — 126 SDBL parser
   tests (123 pre-existing + 3 Slice 6 gap tests added in C0).
2. `cargo test -p parser --test sdbl_slice6_package` — 26 spec-driven
   Slice 6 acceptance tests (this attestation's primary gate).
3. `cargo test -p parser` — full parser test suite (integration tests
   included).
4. `cargo test -p sdbl-hir` — HIR lowering tests; `SdblUnionClause::
   has_all()` consumption at `crates/sdbl-hir/src/lower/mod.rs:173-175`
   exercised by UNION tests.
5. `cargo test -p lexer` — lexer tests (Slices 1 + 2 regression gate).
6. `cargo test -p ide-db` — SDBL validation tests including the one
   at `crates/ide-db/src/database_impl_tests.rs:505` that parses SDBL
   through `parse_sdbl`.
7. `cargo test -p ide --test sdbl_completion_integration_test` —
   nested UNION package scenarios at
   `crates/ide/tests/sdbl_completion_integration_test.rs:49`.
8. `cargo test -p ide` — full IDE test suite.
9. `cargo build --workspace --all-targets` — workspace build.
10. `cargo clippy -p parser --all-targets --all-features -- -D warnings`
    — parser clippy with warnings denied.

## Commit trail

- `cd709cac` (2026-04-24) — C0: audit SDBL Slice 6 test buckets in
  `sdbl_parser_tests.rs`; rewrite two Bucket-C tests
  (`test_double_union_all_queries_with_aliases`,
  `test_into_clause_with_union_and_semicolon_separator`) on spec-
  driven input and promote them to Bucket B; preserve the third
  (`test_exact_extracted_query_from_logs`) as Bucket C with explicit
  preserved-behaviour rationale; add three Slice 6 gap tests
  (`test_package_with_three_statements`,
  `test_subquery_in_where_with_outer_union`,
  `test_drop_mid_package_after_union`). 139 insertions, 35 deletions
  in `sdbl_parser_tests.rs` only. No production-code changes.
- `1acb9875` (2026-04-24) — C1: pre-refactor extraction of the
  AUTOORDER / ORDER BY / TOTALS BY tail-clause loop from
  `select_query` into a new private helper `select_tail_clauses`,
  leaving `select_query` as a 4-LOC wrapper; insert the module-level
  `## Provenance` docstring in `sdbl.rs` and the CLEAN-ROOM Slice 6
  banners above the 6 Slice-6 functions in `sdbl.rs` and `select.rs`;
  insert the LEGACY (Slices 7–11 pending) banner immediately after
  `union_clause`. No logic change. This commit is the safe revert
  boundary for the clean-room rewrite.
- `66a210a1` (2026-04-24) — C2: re-author the 6 Slice 6 function
  bodies and rustdoc from ITS pubqlang/10, pubqlang/12, and
  pubqlang/51 h47 plus the project's event-parser conventions;
  attach per-function provenance comments; tighten the module-level
  `## Provenance` docstring to state the authoring discipline. Pair
  review (gpt-5.5, reasoning high) round-1 PASS with two LOW nits
  addressed before commit (parser-tolerance rustdoc on
  `query_package`, pubqlang/51 h47 citation on `drop_table_query`).
- C3 (2026-04-24): this attestation, the `sdbl_slice6_package.rs`
  acceptance tests, and the Slice 6 status update in
  [`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Licensing note

The `crates/parser` crate retains its `LGPL-3.0-or-later` license
until the full Slice 6 → Slice 11 parser migration is complete and
Slice 13 reattaches `sdbl-hir`. Promoting the crate to Tier A
(`MIT OR Apache-2.0`) is explicitly out of scope for Slice 6 and will
happen once the last LEGACY-banner function under
`grammar/sdbl/select.rs` has been re-derived and the HIR lowering
cascade in `sdbl-hir` has been cleaned up.

## Author attestation

The Slice 6 material listed above under **Scope** was authored as a
clean-room re-derivation from the sources listed under **Sources
consulted**, without using the `../bsl-parser` project, the pre-C1
function bodies of the 6 Slice 6 functions, or any other third-party
SDBL parser as working text. This attestation applies at the date
recorded at the top of the document.
