# SDBL Slice 7 — Clean-Room Attestation

**Status:** complete (2026-04-25).

This document attests the clean-room authorship of the Slice 7
material of the SDBL parser — the SELECT prefix covering the field
list, aliases, and INTO / ПОМЕСТИТЬ — per the staged migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Scope

The paths claimed as clean-room Slice 7 authorship are:

- `crates/parser/src/grammar/sdbl.rs` — specifically the Slice 7
  addition to the module-level `## Provenance` docstring (the `Slice 7
  — clean-room` bullet with its per-function enumeration and the
  follow-up `Slices 8–11 pending` bullet).
- `crates/parser/src/grammar/sdbl/select.rs` — specifically the 8
  functions declared under the `CLEAN-ROOM Slice 7 — SELECT prefix:
  field list, aliases, INTO` banner, with their per-function
  provenance comments:
    - `query` — SELECT header wrapper: `SELECT limitations?
      selected-fields into-clause?` followed by a delegation to the
      LEGACY `query_body_clauses` helper.
    - `selected_fields` — comma-separated selected-field list via the
      event-parser `parse_delimited_list` helper with
      `LIST_RECOVERY`.
    - `selected_field` — single selected field: asterisk field or
      expression with an optional alias, plus a local recovery hop
      through `recover_field_to_alias_or_delimiter` for unexpected
      post-expression tokens.
    - `is_field_start` — predicate for field-list head detection;
      accepts asterisk-start tokens or expression-start tokens.
    - `is_asterisk_start` — predicate for asterisk-field start; matches
      a literal `*` or a single-segment `Ident . *` lookahead.
    - `asterisk_field` — asterisk-field body; once entered, consumes
      any number of `Ident .` prefix segments before the mandatory `*`.
    - `selected_field_alias` — `(AS | КАК)? identifier` at the
      selected-field site; bare-identifier alias is accepted
      structurally and a clause-keyword guard prevents accidental
      consumption of `ИЗ` / `FROM` as the alias name.
    - `into_clause` — `(INTO | ПОМЕСТИТЬ) identifier` destination for
      temporary tables; wraps the identifier in an
      `SdblTempTableName` sub-node and emits a recoverable parse error
      when the identifier is missing.
- `crates/parser/tests/sdbl_slice7_fields.rs` — the new spec-driven
  acceptance test file authored against the ITS sources below, not
  against the existing `parse_sdbl` output.

The following 6 `SyntaxKind` node kinds are locked in place by Slice 7
(no rename, no addition, no removal, no enum reorder):

- `SdblQuery`
- `SdblFieldList`
- `SdblSelectedField`
- `SdblAlias`
- `SdblAsteriskField`
- `SdblIntoClause`

`SdblTempTableName` is also emitted by `into_clause` but is a
pre-existing node kind whose identity is preserved bit-for-bit; it is
listed here only for completeness.

**Also in Slice 7 deliverable (not Slice-7-attested):**

- `query_body_clauses` — born in C1 as a pure-refactor extraction of
  the FROM → ORDER BY dispatch tail of the pre-C1 `query()` body. It
  lives under the LEGACY banner and its clean-room rewrite is deferred
  to Slices 8 (FROM / `data_source`), 9 (JOIN via `data_source`), and
  11 (WHERE / GROUP / HAVING / FOR UPDATE / INDEX BY / ORDER BY).
- `source_alias_legacy` — born in C1 as the LEGACY twin of the split
  `alias()` helper. Its body was bit-identical to `selected_field_alias`
  at C1; after the C2 clean-room polish on the Slice 7 side (a minor
  `let _ = p.expect(TokenKind::Ident)` tightening that discarded an
  empty `if !p.expect(...) {}` recovery block), the two helpers are
  no longer textually identical but remain behaviorally equivalent —
  `source_alias_legacy` preserves the pre-C1 shape as scope discipline.
  Its call sites sit inside `data_source` (Slice 8 scope: both the
  subquery-source and table-ref alias positions). Clean-room rewrite
  deferred to Slice 8; whether Slice 8 re-merges the two helpers back
  into a unified `alias()` is a Slice 8 decision, not a Slice 7 one.

The `LEGACY (Slices 8–11 pending)` portion of `select.rs`
(`select_tail_clauses`, `query_body_clauses`, `source_alias_legacy`,
`from_clause`, `data_source`, `table_ref`, `where_clause`,
`group_by_clause`, `order_by_clause`, `order_by_item`, `having_clause`,
`for_update_clause`, `index_by_clause`, `autoorder_clause`,
`totals_by_clause`, `limitations`, `top_clause`, `is_limitation_keyword`,
`join_clause`, `is_join_keyword`, `is_clause_keyword`,
`is_identifier_token`, and the pre-Slice-6 helpers `at_sdbl_keyword`,
`eat_sdbl_keyword`, `is_data_source_start`,
`recover_field_to_alias_or_delimiter`, `recover_to_delimiter_vt`)
remains explicitly **not** covered by this attestation.

Downstream consumers of the 6 Slice 7 node kinds
(`crates/parser/src/sdbl_token_converter.rs`,
`crates/parser/src/lib.rs`, `crates/parser/src/event.rs`,
`crates/syntax/src/syntax_kind.rs`, `crates/syntax/src/ast.rs`,
`crates/sdbl-hir/src/lower/**`,
`crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs`,
`crates/ide-db/src/database_impl_tests.rs`,
`crates/ide/tests/sdbl_completion_integration_test.rs`,
`crates/mcp-server/src/tools/query.rs`) were not modified in Slice 7;
they continue to see the public surfaces `parser::parse_sdbl(&str) ->
syntax::Parse<SyntaxNode>`, `parser::parse_sdbl_with_shared_cache(&str)`,
and the 6 locked `SyntaxKind` variants unchanged.

The `AssignAliasFieldsInQuery` diagnostic at
`crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs`
does not walk `SdblSelectedField` / `SdblAlias` directly; it consumes
the HIR diagnostic `AliasWithoutAsKeyword` emitted from
`crates/sdbl-hir/src/lower/diagnostics.rs:593-621`, which in turn walks
the AST via `crates/sdbl-hir/src/lower/select_fields.rs:28-29`
(field-list dispatch) and `:112-187` (alias extraction). Preserving
the AST-node identity for `SdblSelectedField` + `SdblAlias` bit-for-bit
is therefore the load-bearing invariant for this diagnostic gate.

## Sources consulted

The Slice 7 material was re-derived from:

1. 1C ITS documentation:
   - <https://its.1c.ru/db/pubqlang/content/10/hdoc> — query-language
     structure: the `SELECT ... selected-fields INTO? FROM? ...`
     skeleton of a single query, the selected-field list shape
     (`selectedField (COMMA selectedField)*`), the asterisk-field
     forms (`*`, `Ident . *`), the alias grammar
     (`(AS | КАК)? identifier`), and the INTO / ПОМЕСТИТЬ destination
     for a temporary table.
   - <https://its.1c.ru/db/pubqlang/content/12/hdoc> — lexical
     elements: the bilingual keyword vocabulary for `SELECT` /
     `ВЫБРАТЬ`, `INTO` / `ПОМЕСТИТЬ`, `AS` / `КАК`, and the
     identifier longest-match rule that separates identifiers from
     clause keywords (the pragma behind the `is_clause_keyword` guard
     in `selected_field_alias` and `selected_field`).
   - <https://its.1c.ru/db/pubqlang/content/51/hdoc/h47> —
     temporary-table lifecycle: `INTO` / `ПОМЕСТИТЬ` names a
     temporary table by a single identifier, paired with the `DROP` /
     `УНИЧТОЖИТЬ` terminator covered in Slice 6.
2. The local SDBL SELECT mini-spec at
   [`sdbl-select-mini-spec.md`](sdbl-select-mini-spec.md), which
   already carries the query-body skeleton, the selected-fields
   production, the asterisk-field forms (including the
   project-side note that multi-segment forms are supported by the
   asterisk-field body when entered through expression parsing), the
   alias grammar with the `[AS|КАК]?` optional keyword, the INTO
   clause, and the recovery expectations for an IDE parser. The
   mini-spec is the project's own phrasing of the ITS rules, clean-room
   from the `../bsl-parser` grammar.
3. The Slice 1, Slice 2, and Slice 6 clean-room material already
   present in `crates/lexer/src/sdbl/mod.rs` and
   `crates/parser/src/grammar/sdbl.rs` /
   `crates/parser/src/grammar/sdbl/select.rs` — consulted only for the
   shape of per-function provenance comments, the CLEAN-ROOM / LEGACY
   banner layout, and the project's event-parser conventions (marker
   `p.start()` / `m.complete(...)`, `p.bump()`, `p.skip_trivia()`,
   `p.at_keyword()`, `p.at(TokenKind::...)`, `p.nth(...)`,
   `p.expect(...)`, `at_sdbl_keyword`, `eat_sdbl_keyword`,
   `parse_delimited_list`, `LIST_RECOVERY`,
   `p.check_iteration_limit()`). The Tier B recovery helper
   `recover_field_to_alias_or_delimiter` is pre-existing project prior
   art and is dispatched from the clean-room `selected_field` body
   without being re-authored.

The resulting event-parser shape for the 8 Slice 7 functions is the
natural expression of the ITS grammar-shape rules and the project's
own event-parser conventions, and would converge regardless of author.
The claim made here is **independent derivation from the sources
above**, not textual novelty.

## Non-consultation statement

During the authorship of the Slice 7 material the following sources
were not used as working text:

- the sibling `../bsl-parser` project — neither its grammar files nor
  its parser implementation were consulted;
- the pre-C1 function bodies of the 8 Slice 7 functions — the C1
  commit performed a pure-refactor extraction of the FROM → ORDER BY
  tail of `query()` into `query_body_clauses`, a call-site split of
  the dual-use `fn alias()` into `selected_field_alias` +
  `source_alias_legacy` (bit-identical bodies, different call-site
  ownership), and a banner reorder to make all 8 Slice 7 functions
  contiguous under the `CLEAN-ROOM Slice 7` banner; but the C2 body
  text of each of the 8 functions was re-derived against the ITS
  sources and the project's mini-spec rather than copied from C1
  verbatim;
- any other third-party SDBL parser, grammar, or event-tree
  implementation.

The 128 SDBL parser integration tests in `sdbl_parser_tests.rs`
(126 pre-existing + 2 Slice 7 Bucket-A gap tests added in C0), the 26
Slice 6 acceptance tests in `sdbl_slice6_package.rs`, the 204 HIR
lowering tests in `sdbl-hir`, the 1572 diagnostic tests in
`ide-diagnostics` (which includes the `AssignAliasFieldsInQuery` gate),
the SDBL tests in `ide-db`, and the nested UNION package scenarios in
the `ide` `sdbl_completion_integration_test` form the regression gate
that the re-derived 8 functions accept exactly the same input set and
produce exactly the same AST as the pre-refactor implementation. At
parser level there is no byte-identity golden corpus analogous to the
lexer's; the test suite is the gate.

## Preserved pre-refactor behaviours

Six behaviours observed in the pre-clean-room parser are not directly
derivable from a strict reading of the ITS spec alone and are
preserved bit-for-bit in Slice 7:

1. **`selected_fields` dispatches to `parse_delimited_list`.** The
   event-parser helper `super::expressions::parse_delimited_list` with
   `super::LIST_RECOVERY` is Tier B (Slice 10 target) project prior
   art, not grammar text. Its role in `selected_fields` is to allow
   incomplete or empty list elements to produce recoverable error
   nodes rather than abort the surrounding parse — an IDE recovery
   contract covered by §Recovery requirements of the mini-spec but
   whose concrete helper shape is local. The clean-room `selected_fields`
   body continues to dispatch into this helper; the helper itself is
   not re-authored in Slice 7.

2. **The dual-use `alias()` helper was call-site-split in C1 into
   `selected_field_alias` (Slice 7) and `source_alias_legacy` (LEGACY,
   Slice 8 target).** Both helpers emit `SdblAlias`, so NodeKind
   identity is preserved for the `AssignAliasFieldsInQuery` diagnostic
   gate. Both accept the `(AS | КАК)? identifier` form, including the
   bare-identifier implicit-alias form and the `AS` / `КАК` without an
   identifier recovery path (empty `Error` sub-node). The bodies were
   bit-identical in C1. After the C2 clean-room polish on the Slice 7
   side (`let _ = p.expect(TokenKind::Ident)` replacing an empty
   `if !p.expect(...) {}` recovery block in `selected_field_alias`),
   the two helpers are no longer textually identical but remain
   behaviorally equivalent; the Slice 8 side (`source_alias_legacy`)
   preserves the pre-C1 shape as scope discipline. Whether Slice 8
   re-merges the two helpers back into a unified clean-room `alias()`
   is a Slice 8 decision.

3. **`asterisk_field` accepts `*` and single-segment `Ident.*` via
   `is_asterisk_start`; multi-segment `Catalog.Products.*` and
   `#Temp.*` are NOT directly accepted by the asterisk-start
   predicate.** The `is_asterisk_start` lookahead checks for
   `TokenKind::Star` or the exact pattern `Ident Dot Star`; it does
   not check for `Ident Dot Ident Dot Star` (multi-segment) and it
   does not check for `Hash Ident Dot Star` (temp-table-prefixed
   asterisk). This matches the narrower predicate surface of the
   pre-C1 parser; the `asterisk_field` body itself consumes arbitrary
   `Ident .` prefix segments once entered, but the entry point is
   narrower than the ITS-implied grammar would permit. A multi-segment
   qualified asterisk reaches `asterisk_field` only through expression
   parsing, and a temp-table-prefixed asterisk does not reach
   `asterisk_field` at all because `Hash` is not in
   `is_expression_start` either. The acceptance tests in
   `sdbl_slice7_fields.rs` include negative tests that pin this
   narrower surface.

4. **`selected_field` recovery via `recover_field_to_alias_or_delimiter`
   is Tier B IDE-recovery glue.** When expression parsing stops before
   reaching an alias start, a list delimiter, or a clause keyword
   (e.g. an unsupported SDBL construct such as `CASE` inside
   arithmetic), the Tier B helper consumes the unexpected span into an
   `Error` sub-node so the rest of the field list can still parse.
   The helper is not re-authored in Slice 7 and its contract is
   local-preserved; its clean-room rewrite is deferred to Slice 12
   (recovery and IDE allowances) or earlier as needed.

5. **`into_clause` missing-identifier recovery emits an ERROR
   sub-node that consumes one token.** When the identifier after
   `INTO` / `ПОМЕСТИТЬ` is missing, `into_clause` calls `p.error()`.
   The project-level `Parser::error` implementation creates an ERROR
   marker that bumps the current token, so a following `;`, keyword,
   or arbitrary token is absorbed into an ERROR sub-node inside
   `SdblIntoClause`. The clause still carries no
   `SdblTempTableName` child, which is the load-bearing invariant for
   sdbl-hir's temp-table resolution. This matches the pre-C1 parser;
   a tighter recovery (e.g. treating `;` as an outer package boundary
   while emitting an empty ERROR marker at the missing-ident site)
   is deferred to Slice 12.

6. **`AssignAliasFieldsInQuery` diagnostic gate runs via the HIR path,
   not via direct AST walk.** The diagnostic at
   `crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs`
   consumes the HIR diagnostic `AliasWithoutAsKeyword` emitted from
   `crates/sdbl-hir/src/lower/diagnostics.rs:593-621`, which in turn
   walks the AST via `crates/sdbl-hir/src/lower/select_fields.rs:28-29`
   (field-list dispatch) and `:112-187` (alias extraction). NodeKind
   identity for `SdblSelectedField` + `SdblAlias` is therefore the
   load-bearing invariant for this gate and is preserved bit-for-bit
   by Slice 7 (neither C1 nor C2 touched `syntax_kind.rs`, `ast.rs`,
   or any of the consumer crates).

## Behaviour change (NOT preserved)

One pre-refactor behaviour is **not** preserved bit-for-bit by
Slice 7 — it is a deliberate post-landing bug fix:

- **`recover_field_to_alias_or_delimiter` stops at clause keywords,
  Semicolons, and EOF at any nesting depth (Slice 12 post-landing
  fix).** Pre-Slice-12 the helper at
  `crates/parser/src/grammar/sdbl/select.rs:45-128` wrapped ALL six
  stop conditions (alias keyword `AS`/`КАК`, Comma, Semicolon,
  RParen, clause keyword, EOF) inside
  `if case_depth == 0 && paren_depth == 0`. Two distinct bugs
  resulted at depth>0 inside an unterminated nested `(...)` or
  `CASE ... END`:

  1. **Clause keyword bug** — the outer query's clause keyword
     (FROM / WHERE / ...) was silently bumped into the recovery
     `Error` node. Analogous to the Slice 8-addendum post-C3 fix
     `7e4f6a9e` for `recover_to_delimiter_vt` and the Slice 12 F1
     fix `9d418084` for `recover_to_delimiter`.
  2. **EOF spin bug** — at depth>0 the gate did not enter, so
     `p.at_end()` was never checked; `p.bump()` is a no-op at EOF
     (`crates/parser/src/parser.rs:111-117`), so the helper spun
     until `Parser::check_iteration_limit` panicked with
     "iteration limit exceeded". This is a strictly worse failure
     mode than the clause-keyword consumption — instead of a quiet
     parse loss it produces a hard panic on certain unterminated
     inputs.

  Slice 12 (commit `80a3129c`, 2026-04-26) lifted three stop
  conditions out of the depth gate to fire at any nesting depth:
  clause keywords, Semicolon, and EOF. Three remain inside the
  depth gate as legitimate continuation tokens at depth>0:
  `AS`/`КАК` (nested alias is fine inside `CASE x WHEN ... AS y ...`),
  Comma (function-call args, CASE branches), and depth-0 RParen
  (the helper at lines 77-82 already bumps RParen at depth>0, so
  the top-level RParen check is the depth-0 case only).

  The `consumed_any` / `err.abandon` invariant at lines 49,
  122-127 is preserved unchanged.

  Regression gates in
  `crates/parser/tests/sdbl_slice7_fields.rs`:
  - `test_slice7_field_recovery_stops_on_clause_keyword_at_any_depth_ru`
    and `_en` use the trigger input `ВЫБРАТЬ A ( ИЗ T2 КАК Т`
    (and EN equivalent) to assert the outer FROM clause is
    preserved and no `Error` sub-node contains the clause
    keyword.
  - `test_slice7_field_recovery_breaks_at_eof_inside_unterminated_paren`
    is an audit-gate against the iteration-limit-panic regression:
    input `ВЫБРАТЬ A (` must not panic.

  Companion fix for `recover_to_delimiter` is documented under the
  Slice 10a attestation §Behaviour change (commit `9d418084`).
  All three SDBL recovery helpers
  (`recover_to_delimiter_vt`, `recover_to_delimiter`,
  `recover_field_to_alias_or_delimiter`) now share the same
  clause-keyword-at-any-depth contract.

## Verification recipe

All of the following must be green before this attestation is
considered live:

1. `cargo test -p parser --test sdbl_parser_tests` — 128 SDBL parser
   tests (126 pre-existing + 2 Slice 7 gap tests added in C0).
2. `cargo test -p parser --test sdbl_slice6_package` — 26 Slice 6
   acceptance tests (regression gate).
3. `cargo test -p parser --test sdbl_slice7_fields` — Slice 7
   spec-driven acceptance tests (this attestation's primary gate).
4. `cargo test -p parser` — full parser suite (integration tests
   included).
5. `cargo test -p sdbl-hir` — 204 HIR lowering tests, including the
   `AliasWithoutAsKeyword` path at
   `crates/sdbl-hir/src/lower/diagnostics.rs:593-621`.
6. `cargo test -p lexer` — lexer tests (Slices 1 + 2 regression gate).
7. `cargo test -p ide-db` — SDBL validation tests including the one at
   `crates/ide-db/src/database_impl_tests.rs:505` that parses SDBL
   through `parse_sdbl`.
8. `cargo test -p ide --test sdbl_completion_integration_test` —
   nested UNION package scenarios.
9. `cargo test -p ide` — full IDE test suite.
10. `cargo test -p ide-diagnostics` — 1572 diagnostic tests including
    the hard `AssignAliasFieldsInQuery` regression gate.
11. `cargo build --workspace --all-targets` — workspace build.
12. `cargo clippy -p parser --all-targets --all-features -- -D warnings`
    — parser clippy with warnings denied.

## Commit trail

- `062d0a72` (2026-04-25) — C0: audit SDBL Slice 7 test buckets in
  `sdbl_parser_tests.rs`; add two Bucket-A regression gates for
  Slice 7 coverage gaps (`test_russian_table_asterisk` for the
  Russian-identifier asterisk form and `test_russian_into_simple` for
  minimal Russian `ПОМЕСТИТЬ`); the three Bucket-C tests on the
  Slice 7 surface (`sdbl_parser_tests.rs:747`, `:805`, `:1043`) stay
  as-is under their existing Bucket-C preservation rationale (see
  the Slice 6 attestation precedent for `:737`). 17 insertions, 0
  deletions in `sdbl_parser_tests.rs` only. No production-code
  changes.
- `2e091d85` (2026-04-25) — C1: pre-refactor extraction of the
  FROM → ORDER BY tail of `query()` into a new private helper
  `query_body_clauses` under the LEGACY banner; call-site split of
  `fn alias` into `selected_field_alias` (Slice 7) and
  `source_alias_legacy` (LEGACY) with bit-identical bodies; reorder
  so all 8 Slice 7 functions sit contiguously under a new
  `CLEAN-ROOM Slice 7 — SELECT prefix: field list, aliases, INTO`
  banner; `is_field_start` moved down from pre-Slice-6 position into
  the Slice 7 block; `into_clause` moved up from LEGACY into the
  Slice 7 block; LEGACY banner enumeration updated to name the new
  helpers and their deferred slice ownership; module-level
  `## Provenance` docstring in `sdbl.rs` extended with the Slice 7
  bullet. No logic change. This commit is the safe revert boundary
  for the clean-room rewrite.
- `a22d98a7` (2026-04-25) — C2: re-author the 8 Slice 7 function
  bodies and rustdoc from ITS pubqlang/10, pubqlang/12, pubqlang/51
  h47, the local `sdbl-select-mini-spec.md`, and the project's
  event-parser conventions; attach one per-function provenance
  comment each; remove stale "Phase 1 / Phase 2" development
  annotations from `query()`; collapse one `clippy::collapsible_if`
  in `selected_field` alias-dispatch (behaviorally identical to the
  pre-C2 form and matches the pattern already used in `data_source`).
- C3 (2026-04-25): this attestation, the `sdbl_slice7_fields.rs`
  acceptance tests, and the Slice 7 status update in
  [`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Licensing note

The `crates/parser` crate retains its `LGPL-3.0-or-later` license
until the full Slice 6 → Slice 11 parser migration is complete and
Slice 13 reattaches `sdbl-hir`. Promoting the crate to Tier A
(`MIT OR Apache-2.0`) is explicitly out of scope for Slice 7 and will
happen once the last LEGACY-banner function under
`grammar/sdbl/select.rs` has been re-derived and the HIR lowering
cascade in `sdbl-hir` has been cleaned up.

## Author attestation

The Slice 7 material listed above under **Scope** was authored as a
clean-room re-derivation from the sources listed under **Sources
consulted**, without using the `../bsl-parser` project, the pre-C1
function bodies of the 8 Slice 7 functions, or any other third-party
SDBL parser as working text. This attestation applies at the date
recorded at the top of the document.
