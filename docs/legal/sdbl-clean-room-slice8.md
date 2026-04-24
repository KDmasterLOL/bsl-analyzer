# SDBL Slice 8 — Clean-Room Attestation

**Status:** complete (2026-04-25).

This document attests the clean-room authorship of the Slice 8
material of the SDBL parser — the FROM clause and source chains
covering table references, subqueries in FROM, parameter sources, and
source aliases — per the staged migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Scope

The paths claimed as clean-room Slice 8 authorship are:

- `crates/parser/src/grammar/sdbl.rs` — specifically the Slice 8
  addition to the module-level `## Provenance` docstring (the `Slice 8
  — clean-room` bullet with its per-function enumeration and the
  replacement `Slices 9–11 pending` bullet listing the remaining
  LEGACY-banner functions).
- `crates/parser/src/grammar/sdbl/select.rs` — specifically the 5
  functions declared under the `CLEAN-ROOM Slice 8 — FROM sources and
  source chains` banner, with their per-function provenance comments:
    - `is_data_source_start` — event-parser predicate for FROM-list
      head detection; accepts `LParen` (subquery-source), `Ampersand`
      (parameter-source), or non-keyword `Ident` (table-ref).
    - `from_clause` — `(FROM|ИЗ) data-source (',' data-source)*`
      delegating to the Tier B `parse_delimited_list` helper with
      `LIST_RECOVERY` so incomplete or empty list positions produce
      recoverable error nodes.
    - `data_source` — `primary-source alias? join-clause*` with
      subquery vs. table-ref dispatch on leading `LParen`, the
      combined alias guard `(at_sdbl_keyword("AS", "КАК") ||
      is_identifier_token) && !is_clause_keyword`, and the Tier B
      `join_clause` attachment loop (Slice 9 target).
    - `table_ref` — parameter-source branch (`Ampersand + identifier`
      wrapped in `SdblParameter`) or identifier chain with DOT
      recovery (empty `Error` sub-node inserted when the next token
      after `.` is not an identifier or is a clause / `AS` / `КАК`
      keyword); VT method-call arguments delegated to Tier B
      `virtual_table_args_legacy` (Slice 5 target).
    - `source_alias` — `(AS | КАК)? identifier` at the FROM-clause
      data-source / table-ref site; empty `Error` sub-node when a
      clause keyword appears in the alias position, so the enclosing
      clause loop can consume it at the next level up.
- `crates/parser/tests/sdbl_slice8_sources.rs` — the new spec-driven
  acceptance test file authored against the ITS sources below, not
  against the existing `parse_sdbl` output.

The following 5 `SyntaxKind` node kinds are locked in place by
Slice 8 (no rename, no addition, no removal, no enum reorder):

- `SdblFromClause`
- `SdblDataSource`
- `SdblTableRef`
- `SdblParameter`
- `SdblAlias`

`SdblAlias` is shared with the Slice 7 `selected_field_alias`
call-site; its identity is load-bearing for the
`AssignAliasFieldsInQuery` diagnostic gate and is preserved bit-for-bit
by Slice 8 as well.

**Child-attachment invariants locked by Slice 8** (shape contract of
`SdblDataSource`, carried by Slice 8 even when the child node kinds
belong to other slices):

- `SdblJoinClause` (Slice 9 NodeKind) is a direct child of
  `SdblDataSource` — consumed by
  `crates/syntax/src/ast.rs:1343` `SdblDataSource::join_clauses()`,
  `crates/sdbl-hir/src/lower/from_clause.rs:38-49`, and
  `crates/sdbl-hir/src/lower/join_clause.rs:23-29`.
- `SdblSubquery` inside the subquery-source path of `data_source` is
  a direct child of `SdblDataSource` — consumed by
  `crates/syntax/src/ast.rs:1332-1335` `SdblDataSource::subquery()`.
- `SdblAlias` inside `SdblDataSource` is a direct child (not a
  grandchild via an intermediate wrapper) — consumed by
  `crates/syntax/src/ast.rs:1338` `SdblDataSource::alias()`.
- `SdblDataSource` is a direct child of `SdblFromClause` —
  `SdblFromClause::data_sources()` at
  `crates/syntax/src/ast.rs:1299-1300` walks direct children, not
  descendants; `sdbl-hir/src/lower/from_clause.rs:31` lowers via
  `from.data_sources()`.
- `SdblParameter` is a direct child of `SdblTableRef` on the
  parameter-source path — consumed by
  `crates/sdbl-hir/src/lower/expr/mod.rs:52` `lower_parameter`.

**AST-shape invariants locked by Slice 8** (ordering / direct-child
contracts that HIR lowering reads beyond NodeKind identity):

1. `SdblTableRef` table-path IDENT ordering — every identifier in the
   dot-separated chain is a direct IDENT token child of
   `SdblTableRef`, in source order, not wrapped in a sub-node.
   Consumer: `sdbl-hir/src/lower/from_clause.rs:381-393`
   (`parse_table_name`).
2. `SdblTableRef` VT-param expression children stay inside
   `SdblTableRef` — the C1 extraction into
   `virtual_table_args_legacy` opens no new wrapper marker, so VT
   argument expressions and `SdblMissingArg` / `ERROR` nodes remain
   direct children of `SdblTableRef`. Consumer:
   `sdbl-hir/src/lower/from_clause.rs:283-306`.
3. IDENT token ranges under `SdblTableRef` drive source-map
   emission. Consumer: `sdbl-hir/src/lower/from_clause.rs:183-193`.
4. `SdblAlias` name extraction reads the last non-AS/КАК direct
   IDENT token. Consumer: `syntax/src/ast.rs:1232-1249` —
   `SdblAlias::identifier()` and `SdblAlias::name()`.
5. `SdblParameter` sub-node is direct child of `SdblTableRef` on the
   parameter-source path. (Listed above under child-attachment
   invariants; repeated here for completeness as an AST-shape
   constraint.)
6. `SdblDataSource` is a direct child of `SdblFromClause`.
7. `L_PAREN` is a direct token child of `SdblTableRef` for VT-call
   forms. Consumer:
   `sdbl-hir/src/lower/from_clause.rs:1108-1111`
   (`check_virtual_table_params`) — the helper scans only for a
   direct `L_PAREN` token; `R_PAREN` is not HIR-scanned but is kept
   as a direct token child by the C1 extraction as defensive
   preservation.

**Also in Slice 8 deliverable (not Slice-8-attested):**

- `virtual_table_args_legacy` — born in C1 as a pure-refactor
  extraction of the VT method-call body from the pre-C1 `table_ref`.
  The extracted body is bit-identical to the pre-C1 span; the helper
  owns the leading `if p.at(TokenKind::LParen)` guard so the call
  site in `table_ref` is unconditional. It lives under the LEGACY
  banner and its clean-room rewrite is deferred to Slice 5 (virtual
  table and external-source handling).

The `LEGACY (Slices 9–11 pending)` portion of `select.rs`
(`select_tail_clauses`, `query_body_clauses`,
`virtual_table_args_legacy`, `where_clause`, `is_identifier_token`,
`is_clause_keyword`, `is_join_keyword`, `join_clause`,
`is_limitation_keyword`, `limitations`, `top_clause`,
`group_by_clause`, `order_by_clause`, `order_by_item`,
`having_clause`, `for_update_clause`, `index_by_clause`,
`autoorder_clause`, `totals_by_clause`) remains explicitly **not**
covered by this attestation.

Downstream consumers of the 5 Slice 8 node kinds
(`crates/parser/src/sdbl_token_converter.rs`,
`crates/parser/src/lib.rs`, `crates/parser/src/event.rs`,
`crates/syntax/src/syntax_kind.rs`, `crates/syntax/src/ast.rs`,
`crates/sdbl-hir/src/lower/**`,
`crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs`,
`crates/ide-db/src/database_impl_tests.rs`,
`crates/ide/tests/sdbl_completion_integration_test.rs`,
`crates/mcp-server/src/tools/query.rs`) were not modified in Slice 8;
they continue to see the public surfaces `parser::parse_sdbl(&str) ->
syntax::Parse<SyntaxNode>`,
`parser::parse_sdbl_with_shared_cache(&str)`, and the 5 locked
`SyntaxKind` variants unchanged.

## Sources consulted

The Slice 8 material was re-derived from:

1. 1C ITS documentation:
   - <https://its.1c.ru/db/pubqlang/content/10/hdoc> — query-language
     structure: the FROM clause shape
     (`from-clause := (FROM|ИЗ) data-source (',' data-source)*`), the
     data-source shape (`primary-source alias? join-clause*`),
     primary-source alternatives
     (`subquery-source | table-ref | parameter-source`), the
     subquery-source wrapping (`'(' subquery ')' alias?`), the
     parameter-source lexical shape (`'&' identifier`), the
     metadata-object identifier chain (`identifier ('.' identifier)*`)
     for table references, and the alias grammar
     (`(AS | КАК)? identifier`).
   - <https://its.1c.ru/db/pubqlang/content/12/hdoc> — lexical
     elements: bilingual FROM / ИЗ, AS / КАК keyword vocabulary; the
     Ampersand `&` parameter prefix lexeme; the identifier
     longest-match rule that lets `is_clause_keyword` separate a
     clause keyword from an identifier in both the DOT-chain guard
     in `table_ref` and the alias position in `source_alias`.
2. The local SDBL SELECT mini-spec at
   [`sdbl-select-mini-spec.md`](sdbl-select-mini-spec.md), which
   already carries the FROM clause production, the data-source
   productions, the subquery-source and parameter-source forms, the
   alias grammar with the `[AS|КАК]?` optional keyword, the
   identifier-chain shape for table references, the virtual-table
   argument behaviour (empty arguments valid for compatibility), and
   the recovery expectations for an IDE parser. The mini-spec is the
   project's own phrasing of the ITS rules, clean-room from the
   `../bsl-parser` grammar.
3. The Slice 1, Slice 2, Slice 6, and Slice 7 clean-room material
   already present in `crates/lexer/src/sdbl/mod.rs` and
   `crates/parser/src/grammar/sdbl.rs` /
   `crates/parser/src/grammar/sdbl/select.rs` — consulted only for
   the shape of per-function provenance comments, the CLEAN-ROOM /
   LEGACY banner layout, and the project's event-parser conventions
   (marker `p.start()` / `m.complete(...)`, `p.bump()`,
   `p.skip_trivia()`, `p.at_keyword()`, `p.at(TokenKind::...)`,
   `p.eat(TokenKind::...)`, `p.expect(...)`, `at_sdbl_keyword`,
   `eat_sdbl_keyword`, `parse_delimited_list`, `LIST_RECOVERY`,
   `p.check_iteration_limit()`). The Tier B helpers
   `virtual_table_args_legacy`, `is_clause_keyword`,
   `is_identifier_token`, `is_join_keyword`, and `join_clause` are
   pre-existing project prior art and are dispatched from the
   clean-room Slice 8 bodies without being re-authored.

The resulting event-parser shape for the 5 Slice 8 functions is the
natural expression of the ITS grammar-shape rules and the project's
own event-parser conventions, and would converge regardless of
author. The claim made here is **independent derivation from the
sources above**, not textual novelty.

## Non-consultation statement

During the authorship of the Slice 8 material the following sources
were not used as working text:

- the sibling `../bsl-parser` project — neither its grammar files nor
  its parser implementation were consulted;
- the pre-C1 function bodies of the 5 Slice 8 functions — the C1
  commit performed a pure-refactor extraction of the VT method-call
  body into `virtual_table_args_legacy`, a rename of
  `source_alias_legacy` → `source_alias`, and a banner reorder to
  make all 5 Slice 8 functions contiguous under the
  `CLEAN-ROOM Slice 8` banner; but the C2 body text of each of the 5
  functions was re-derived against the ITS sources and the project's
  mini-spec rather than copied from C1 verbatim;
- any other third-party SDBL parser, grammar, or event-tree
  implementation.

The 132 SDBL parser integration tests in `sdbl_parser_tests.rs`
(128 pre-existing + 4 Slice 8 Bucket-A gap tests added in C0), the
26 Slice 6 acceptance tests in `sdbl_slice6_package.rs`, the 26
Slice 7 acceptance tests in `sdbl_slice7_fields.rs`, the 28 Slice 8
acceptance tests in `sdbl_slice8_sources.rs` (this slice), the 204
HIR lowering tests in `sdbl-hir`, the 1572 diagnostic tests in
`ide-diagnostics` (which includes the `AssignAliasFieldsInQuery`
gate), the SDBL tests in `ide-db`, and the nested UNION package
scenarios in the `ide` `sdbl_completion_integration_test` form the
regression gate for Slice 8. They cover the locked compatibility
surfaces (public API, NodeKind identity, child-attachment
invariants, AST-shape invariants, bilingual keyword acceptance,
recovery shapes for incomplete input) and a sampled regression
corpus of accepted inputs; they do not constitute a byte-identity
golden corpus across the full SDBL input space. At parser level
there is no byte-identity golden corpus analogous to the lexer's;
the test suite is the gate, and behaviour is preserved within the
sampled surface rather than proven equivalent across all inputs.

## Preserved pre-refactor behaviours

Seven behaviours observed in the pre-clean-room parser are not
directly derivable from a strict reading of the ITS spec alone and
are preserved bit-for-bit in Slice 8:

1. **`selected_field_alias` (Slice 7) and `source_alias` (Slice 8)
   remain two distinct functions with behaviorally equivalent
   bodies.** Both emit `SdblAlias`; both accept the
   `(AS | КАК)? identifier` form including the bare-identifier
   implicit-alias form and the `AS` / `КАК` without an identifier
   recovery path. Scope discipline: a merge would edit Slice 7-
   attested code for an ergonomic gain. Whether re-merge happens is
   a Slice 12 (recovery and IDE allowances) decision.

2. **`table_ref` emits `SdblParameter` as a sub-node of
   `SdblTableRef` for the `&Ident` parameter-source path.** This is
   the load-bearing invariant for `sdbl-hir/src/lower/expr/mod.rs:52`
   `lower_parameter`. Preserved bit-for-bit.

3. **`data_source` alias-call guard:
   `(at_sdbl_keyword("AS", "КАК") || is_identifier_token) &&
   !is_clause_keyword`.** This guard blocks alias consumption when
   the next token is a clause keyword, preserving the
   `(SELECT 1)\nFROM`-style recovery where the alias position is
   followed by another clause keyword (the enclosing clause loop
   must be allowed to consume that keyword at the next level up).

4. **`table_ref` DOT-chain recovery emits empty `Error` sub-nodes
   for non-Ident tokens after `.` and for clause / AS / КАК keywords
   after `.`.** This is IDE-recovery glue for incomplete qualified
   names while the user is typing. The ITS spec does not mandate
   specific recovery shapes, but the mini-spec §Recovery
   requirements #4 expects recoverable incomplete table references,
   and existing parser tests exercise the current shape.

5. **`data_source`'s JOIN attachment loop stays in Slice 8, but the
   JOIN body does not.** The loop `while is_join_keyword(p) {
   join_clause(p); p.skip_trivia(); }` is the data-source grammar's
   attachment point per mini-spec `data-source := primary-source
   alias? join-clause*`. Slice 8 clean-rooms the attachment;
   `is_join_keyword` and `join_clause` bodies remain Tier B until
   Slice 9.

6. **`virtual_table_args_legacy` extraction is
   pure-behaviour-preserving.** The extracted body includes
   `SdblMissingArg` emission for empty-comma / trailing-comma
   positions and `recover_to_delimiter_vt` recovery. No logic change
   in C1; the rewrite is deferred to Slice 5. The `select.rs` inline
   `mod tests` (`test_tuple_in_virtual_table_params`,
   `test_virtual_table_debug`, etc.) are the regression gate, and
   all sdbl-hir VT-params tests (which read direct `L_PAREN` /
   expression children of `SdblTableRef`) stay green.

7. **`table_ref` parameter-source branch admits a bare `&` without
   identifier as an IDE-recovery allowance.** The identifier bump is
   guarded by `if p.at(TokenKind::Ident)`, not required by
   `p.expect`, so an incomplete `ВЫБРАТЬ * ИЗ &` completes the
   `SdblParameter` / `SdblTableRef` markers without aborting the
   enclosing query — the user can keep typing the parameter name
   with the markers already open. The mini-spec
   `parameter-source := '&' identifier` declares the identifier
   mandatory; the parser preserves the pre-C1 IDE-recovery allowance
   explicitly.

## Verification recipe

All of the following must be green before this attestation is
considered live:

1. `cargo test -p parser --test sdbl_parser_tests` — 132 SDBL parser
   tests (128 pre-existing + 4 Slice 8 gap tests added in C0).
2. `cargo test -p parser --test sdbl_slice6_package` — 26 Slice 6
   acceptance tests (regression gate).
3. `cargo test -p parser --test sdbl_slice7_fields` — 26 Slice 7
   acceptance tests (regression gate).
4. `cargo test -p parser --test sdbl_slice8_sources` — 28 Slice 8
   spec-driven acceptance tests (this attestation's primary gate).
5. `cargo test -p parser` — full parser suite (integration tests +
   inline `mod tests` in `select.rs` including
   `test_tuple_in_virtual_table_params`).
6. `cargo test -p sdbl-hir` — 204 HIR lowering tests, including the
   `AliasWithoutAsKeyword` path at
   `crates/sdbl-hir/src/lower/diagnostics.rs:593-621` and the
   VT-params check at
   `crates/sdbl-hir/src/lower/from_clause.rs:1108-1111`.
7. `cargo test -p lexer` — lexer tests (Slices 1 + 2 regression
   gate).
8. `cargo test -p ide-db` — SDBL validation tests including the one
   at `crates/ide-db/src/database_impl_tests.rs` that parses SDBL
   through `parse_sdbl`.
9. `cargo test -p ide --test sdbl_completion_integration_test` —
   nested UNION + FROM scenarios.
10. `cargo test -p ide` — full IDE test suite.
11. `cargo test -p ide-diagnostics` — 1572 diagnostic tests
    including the hard `AssignAliasFieldsInQuery` regression gate.
12. `cargo test -p mcp-server` — MCP server regression gate; the
    `parse_sdbl` consumer at `crates/mcp-server/src/tools/query.rs`
    is exercised by the MCP tool tests.
13. `cargo build --workspace --all-targets` — workspace build.
14. `cargo clippy -p parser --all-targets --all-features --
    -D warnings` — parser clippy with warnings denied.

## Commit trail

- `078dd808` (2026-04-25) — C0: audit SDBL Slice 8 test buckets in
  `sdbl_parser_tests.rs`; add four Bucket-A regression gates for
  Slice 8 coverage gaps
  (`test_slice8_from_multi_source_with_bare_alias`,
  `test_slice8_russian_subquery_source_with_alias`,
  `test_slice8_temp_table_source_across_package_boundary`,
  `test_slice8_parameter_source_without_alias`). Each gap test
  carries structural assertions (SdblDataSource / SdblAlias /
  SdblSubquery / SdblParameter / SdblTableRef) and a full-input
  consumption check (`root.text() == input`) so a regression that
  drops trailing input cannot silently pass. No production-code
  changes.
- `1be6dd69` (2026-04-25) — C1: pre-refactor extraction of the
  virtual-table method-call body from the pre-C1 `table_ref` into
  a new private helper `virtual_table_args_legacy` (bit-identical
  body; helper owns the leading `if p.at(LParen)` guard so the call
  site is unconditional); rename `source_alias_legacy` →
  `source_alias`; reorder so all 5 Slice 8 functions sit
  contiguously under a new `CLEAN-ROOM Slice 8` banner with an
  explicit C1-placeholder preamble; LEGACY banner enumeration
  updated to name `virtual_table_args_legacy` as a Slice 5 target
  and renamed `Slices 8–11 pending` → `Slices 9–11 pending`;
  module-level `## Provenance` docstring in `sdbl.rs` extended with
  the Slice 8 bullet. No logic change. This commit is the safe
  revert boundary for the clean-room rewrite.
- `85b4005e` (2026-04-25) — C2: re-author the 5 Slice 8 function
  bodies and rustdoc from ITS pubqlang/10, pubqlang/12, the local
  `sdbl-select-mini-spec.md`, and the project's event-parser
  conventions; attach one per-function provenance comment each;
  update the module docstring in `sdbl.rs` and the Slice 8 banner in
  `select.rs` to reflect the C1 + C2 landed state with explicit
  wording that the Slice 8 attestation is authored in C3 and must
  not be cited as current evidence until this document lands.
- C3 (2026-04-25): this attestation, the `sdbl_slice8_sources.rs`
  acceptance tests, and the Slice 8 status update in
  [`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Licensing note

The `crates/parser` crate retains its `LGPL-3.0-or-later` license
until the full Slice 6 → Slice 11 parser migration is complete and
Slice 13 reattaches `sdbl-hir`. Promoting the crate to Tier A
(`MIT OR Apache-2.0`) is explicitly out of scope for Slice 8 and will
happen once the last LEGACY-banner function under
`grammar/sdbl/select.rs` has been re-derived and the HIR lowering
cascade in `sdbl-hir` has been cleaned up.

## Author attestation

The Slice 8 material listed above under **Scope** was authored as a
clean-room re-derivation from the sources listed under **Sources
consulted**, without using the `../bsl-parser` project, the pre-C1
function bodies of the 5 Slice 8 functions, or any other third-party
SDBL parser as working text. This attestation applies at the date
recorded at the top of the document.
