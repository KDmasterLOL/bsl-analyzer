# SDBL Slice 7-addendum — Clean-Room Attestation

**Status:** complete (2026-04-26).

This document attests the clean-room authorship of the Slice
7-addendum material of the SDBL parser — the **SELECT-prefix
qualifier helpers** (`is_identifier_token`,
`is_limitation_keyword`, `limitations`, `top_clause`) — per the
staged migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

The Slice 7-addendum is a deferred follow-up to the Slice 7
clean-room (which landed 2026-04-25 and explicitly excluded the
SELECT-prefix qualifier helpers from its scope). After Slice 11
landed and shrank the residual `LEGACY (Slices 5, 11 pending)`
banner in `crates/parser/src/grammar/sdbl/select.rs` to
`LEGACY (Slice 5 + SELECT limitation helpers pending)`, this
addendum closes the four limitation-helper functions and
shrinks the LEGACY banner to its true `LEGACY (Slice 5
pending)` form, leaving only the Slice 5 target
`virtual_table_args_legacy` in the residual block.

## Scope

The paths claimed as clean-room Slice 7-addendum authorship
are:

- 4 functions in
  `crates/parser/src/grammar/sdbl/select.rs` under the new
  `CLEAN-ROOM Slice 7-addendum — SELECT prefix qualifiers`
  banner:

  - `is_identifier_token` — Tier C/B local parser contract:
    `p.at(TokenKind::Ident)` predicate consumed cross-slice by
    Slice 7 alias-scan (`selected_field_alias` at
    `select.rs:357, 370`) and Slice 8 source-alias guard
    (`source_alias` at `select.rs:582, 600`). The Slice 8
    attestation at
    `docs/legal/sdbl-clean-room-slice8.md:264-269` preserves
    the alias-scan guard as a behaviour contract.
  - `is_limitation_keyword` — `fn(&Parser) -> bool` predicate
    matching the bilingual SELECT-prefix qualifier vocabulary
    (DISTINCT / РАЗЛИЧНЫЕ, TOP / ПЕРВЫЕ, ALLOWED / РАЗРЕШЕННЫЕ).
  - `limitations` — main entry; emits `SdblLimitations` as a
    flat sequence of bare keyword tokens (DISTINCT, ALLOWED)
    and `SdblTopClause` wrapper nodes (one per TOP qualifier)
    as direct children. Any-order qualifier acceptance + duplicate-
    qualifier loop tolerance preserved as IDE-recovery
    allowances Q1/Q2.
  - `top_clause` — helper consumed by `limitations` for each
    `TOP` / `ПЕРВЫЕ` qualifier; emits `SdblTopClause` with the
    keyword token + Decimal count token (or an ERROR sub-node
    if the count is missing per IDE-recovery allowance Q3).

- The clean-room banner block at the new
  `CLEAN-ROOM Slice 7-addendum — SELECT prefix qualifiers`
  position in `select.rs`. The residual LEGACY banner shrinks
  from `LEGACY (Slice 5 + SELECT limitation helpers pending)`
  to `LEGACY (Slice 5 pending)`, containing only
  `virtual_table_args_legacy`.

- The §Limitations §AST-shape contract / §Deferred semantic
  constraint / §IDE-recovery allowances / §Tier classification
  / §ITS coverage subsections, plus the three new rows in the
  §ITS coverage verification table (DISTINCT, TOP, ALLOWED),
  plus the §Non-consultation statement (Slice 7-addendum
  reaffirmation) of `docs/legal/sdbl-select-mini-spec.md` —
  all added in C0.

- The 5 Bucket-A gap-test functions in
  `crates/parser/tests/sdbl_parser_tests.rs` added in C0 plus
  the spec-driven acceptance tests in
  `crates/parser/tests/sdbl_slice7_addendum_limitations.rs`
  added in C3.

**2 NodeKinds preserved bit-for-bit through the rewrite** (no
variant rename, no addition, no removal, no reorder in
`crates/syntax/src/syntax_kind.rs`):

`SdblLimitations`, `SdblTopClause`.

**Function → NodeKind attribution map:**

| Function | Emits |
|---|---|
| `is_identifier_token` | (predicate, no NodeKind) |
| `is_limitation_keyword` | (predicate, no NodeKind) |
| `limitations` | `SdblLimitations` |
| `top_clause` | `SdblTopClause` (called by `limitations` for each TOP qualifier) |

## Tier classification

Per codex Round-2 finding 2 (MED) and the post-Round-3
v8327doc Глава 8 discovery, the addendum carries an explicit
§Tier classification section. Tier scheme per Slice 9
precedent — A1 = ITS canonical listing, A2 = ITS prose-note,
B = lexer Slice 2 attested keyword pair, C = SELECT mini-spec,
D = local IDE-recovery allowance.

| Function | Tier source map |
|---|---|
| `is_identifier_token` | **C/B local parser contract** — body `p.at(TokenKind::Ident)` is trivially derivable from the project's event-parser conventions; load-bearing semantics inherited from Slice 7 alias-scan + Slice 8 source-alias guard contracts cross-referenced at `docs/legal/sdbl-clean-room-slice8.md:264-269`. |
| `is_limitation_keyword` | **A1** + B (lexer Slice 2 / Slice 2 LEGACY-attested keyword pairs DISTINCT / РАЗЛИЧНЫЕ, TOP / ПЕРВЫЕ, ALLOWED / РАЗРЕШЕННЫЕ). Primary Tier A1 source: v8327doc Глава 8 §<Описание запроса> at `page.html:1320` canonical EBNF skeleton placing all three qualifiers in their canonical first three SELECT-prefix slots; bilingual word-list at `:1030-1034` (РАЗЛИЧНЫЕ ↔ DISTINCT), `:1040-1044` (РАЗРЕШЕННЫЕ ↔ ALLOWED), `:920-924` (ПЕРВЫЕ ↔ TOP). |
| `limitations` | **A1** + B + C. Primary Tier A1 source: v8327doc Глава 8 at `page.html:1320` canonical EBNF + `:1331-1356` prose semantics for РАЗРЕШЕННЫЕ / РАЗЛИЧНЫЕ / ПЕРВЫЕ. Secondary corroborating sources: pubqlang chapters 19/20/57 (textbook companion). C = mini-spec §Limitations any-order acceptance + duplicate-qualifier loop tolerance contract. |
| `top_clause` | **A1** + B + C + D. Primary Tier A1 source: v8327doc Глава 8 at `page.html:1320` canonical EBNF `[ПЕРВЫЕ <Количество>]` slot + `:1350-1356` prose covering limit, ordering interaction with subsequent ORDER BY, and nested-query support. Secondary: pubqlang `chapter_019.html:19, 28`. D = §IDE-recovery allowance Q3 (missing-decimal recovery shape; Slice 12 owns the recovery-quality fix). |

**ALLOWED / РАЗРЕШЕННЫЕ Tier classification — verbatim
disclosure** (per codex Round-2 finding 2 + post-Round-3
reclassification): The user downloaded the v8.3.27 Developer's
Reference Глава 8 «Работа с запросами» from
`https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453` and
saved a local snapshot at
`/home/itrous/src/tools_migration/its/dump/its_db_v8327doc_bookmark_dev_TI000000453/page.html`
during the C0 review window. Line 1320 of that page carries
the canonical EBNF skeleton placing `[РАЗРЕШЕННЫЕ]` in the
first SELECT-prefix slot; lines 1331-1344 give full prose
semantics covering RLS scope, top-level-only constraint
(see §Deferred semantic constraint below), propagation into
subqueries, and interaction with ЧТЕНИЕ rights; the bilingual
word-list at `:1040-1044` maps РАЗРЕШЕННЫЕ ↔ ALLOWED. The
pubqlang dump's `chapter_057.html:50` UI-checkbox prose is a
secondary corroborating reference only.

The first-pass codex adversarial review (Rounds 1–3) had
classified ALLOWED as Tier D / B-contested under the
assumption that the pubqlang dump was the only available SDBL
grammar source (no canonical `ВЫБРАТЬ РАЗРЕШЕННЫЕ` example
was found in the 161 local pubqlang chapter files). After the
v8327doc Глава 8 download landed, ALLOWED was reclassified to
**Tier A1**: the developer's reference is the primary SDBL
grammar specification and lists РАЗРЕШЕННЫЕ in the canonical
first-SELECT-prefix slot with full prose semantics.

**Deferred semantic constraint** (codex Round-4 finding 4):
v8327doc Глава 8 at `page.html:1336` constrains РАЗРЕШЕННЫЕ to
the top-level `ВЫБРАТЬ` only (the keyword may appear only in
the top-level SELECT and propagates to nested subqueries).
The current parser's `query()` at
`crates/parser/src/grammar/sdbl/select.rs:279-307` calls
`limitations()` for every query body it parses, including
nested subqueries — it does NOT enforce the top-level-only
constraint at parser level. The Slice 7-addendum **preserves**
this; any semantic restriction (HIR-level or
IDE-diagnostic-level enforcement) is deferred to a future slice
(Slice 13 HIR reattachment, or a dedicated diagnostic).

## Sources consulted

The Slice 7-addendum material was authored from:

1. **Primary** SDBL grammar specification: v8.3.27 Developer's
   Reference Глава 8 «Работа с запросами» —
   `https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453`.
   Locally saved snapshot at
   `/home/itrous/src/tools_migration/its/dump/its_db_v8327doc_bookmark_dev_TI000000453/page.html`
   for line-numbered reviewer convenience; the canonical
   citation target is the public URL above. Specifically:
   - `page.html:1320` — canonical EBNF skeleton for `<Описание
     запроса>` placing РАЗРЕШЕННЫЕ, РАЗЛИЧНЫЕ, ПЕРВЫЕ
     `<Количество>` in the first three optional SELECT-prefix
     slots.
   - `page.html:1331-1344` — RLS-scope prose for РАЗРЕШЕННЫЕ
     (top-level-only constraint; propagation into subqueries;
     interaction with ЧТЕНИЕ rights).
   - `page.html:1346-1348` — duplicate-elimination prose for
     РАЗЛИЧНЫЕ.
   - `page.html:1350-1356` — limit / ordering / nested-query
     prose for ПЕРВЫЕ.
   - `page.html:1030-1034` — bilingual pair РАЗЛИЧНЫЕ ↔
     DISTINCT.
   - `page.html:1040-1044` — bilingual pair РАЗРЕШЕННЫЕ ↔
     ALLOWED.
   - `page.html:920-924` — bilingual pair ПЕРВЫЕ ↔ TOP.

2. **Secondary corroborating** ITS pubqlang dump (textbook
   companion) at
   `/home/itrous/src/tools_migration/its/dump/html/`:
   - `chapter_019.html:19, 28` — TOP / ПЕРВЫЕ canonical
     demonstrative `ВЫБРАТЬ ПЕРВЫЕ 3` example.
   - `chapter_020.html:18, 29, 38, 42` — DISTINCT / РАЗЛИЧНЫЕ
     canonical demonstrative example + DISTINCT × ORDER BY
     interaction.
   - `chapter_057.html:50` — UI-checkbox prose for "Разрешенные"
     in the query-designer GUI (corroborating only).

3. The C0-extended SDBL select mini-spec at
   [`sdbl-select-mini-spec.md`](sdbl-select-mini-spec.md)
   §Limitations — full AST-shape contract, IDE-recovery
   allowances Q1/Q2/Q3, Tier classification, ITS coverage,
   Deferred semantic constraint, plus the three new rows in
   the §ITS coverage verification table.

4. The lexer Slice 2 attestation
   ([`sdbl-clean-room-slice2.md`](sdbl-clean-room-slice2.md))
   for bilingual keyword pair attestations (Slice-2-LEGACY
   KwAllowed at `crates/lexer/src/sdbl/mod.rs:470, 494`).

5. The Slice 1/2/6/7/8/9/10a/10b/11 clean-room attestations
   for event-parser conventions and AST-shape contracts (in
   particular the Slice 8 attestation at
   [`sdbl-clean-room-slice8.md:264-269`](sdbl-clean-room-slice8.md)
   for the `is_identifier_token` cross-slice contract).

6. The HIR consumer code at
   `crates/sdbl-hir/src/lower/select_fields.rs:45-90` for
   read-only documentation of consumer-side DISTINCT / TOP
   extraction (no ALLOWED consumer exists at HIR level).

7. The IDE-diagnostics test gates at
   `crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs:442-491,
   514-528` for cross-checking any-order qualifier acceptance.

## Non-consultation statement

The author did NOT consult `../bsl-parser/*` or any pre-C1
textual transcription of the 4 Slice-7-addendum parser
function bodies (`is_identifier_token`, `is_limitation_keyword`,
`limitations`, `top_clause`) as working text during C0 / C2 /
C3 authoring. The C1 commit physically relocated the four
functions out of the LEGACY block into the new
`CLEAN-ROOM Slice 7-addendum — SELECT prefix qualifiers`
banner with `// C1 placeholder — clean-room rewrite in C2`
markers; C2 re-derived each function body from the cited
sources without using the C1 placeholder bodies as working
text.

The 8 codex adversarial-review rounds on the implementation
plan (R1: 9 findings; R2: 6; R3: 0; R4: 5; R5: 2; R6: 3;
R7: 3; R8: 0) verified the v8327doc Глава 8 ALLOWED Tier-A1
reclassification, confirmed plan / mini-spec / test
consistency, and issued the
"IMPLEMENTATION-READY-V6" verdict before C0 lands. A separate
codex review pass on the C2 diff (one Minor finding —
bilingual word-list line numbers — addressed in C2 itself)
confirmed the four function bodies are faithful to the
mini-spec shape and v8327doc EBNF/prose with no behavioural
defect.

## Preserved pre-refactor behaviours

The C2 clean-room rewrite preserves three behaviours of the
pre-refactor parser explicitly as IDE-recovery allowances
(documented in `sdbl-select-mini-spec.md` §Limitations
§IDE-recovery allowances and quoted as Q1 / Q2 / Q3 in the
plan):

1. **Q1: any-order qualifier acceptance.** DISTINCT, TOP, and
   ALLOWED are accepted in any source-order permutation via
   the `while is_limitation_keyword(p)` loop in `limitations`.
   The parser does not enforce a canonical permutation
   (v8327doc EBNF suggests ALLOWED → DISTINCT → TOP, but
   pubqlang demonstrative examples and the
   `assign_alias_fields_in_query.rs:514-528` HIR-consumer
   gates confirm tolerance of arbitrary orderings such as
   `ВЫБРАТЬ РАЗЛИЧНЫЕ ПЕРВЫЕ 10 …` and
   `SELECT TOP 50 DISTINCT …`).

2. **Q2: duplicate-qualifier loop tolerance.** Input
   `ВЫБРАТЬ РАЗЛИЧНЫЕ РАЗЛИЧНЫЕ A` is accepted (the loop body
   re-enters on every `is_limitation_keyword` hit without
   deduplication); semantic uniqueness is not enforced at
   parser level. The HIR consumer extracts DISTINCT and TOP
   without ordering or duplicate-qualifier legality checks.

3. **Q3: missing-TOP-count recovery shape.** `top_clause`
   calls `p.expect(TokenKind::Decimal)` at `select.rs:1635`;
   when the next non-trivia token is not a Decimal,
   `Parser::expect` invokes `Parser::error` at
   `parser.rs:160-166`, which **bumps** the next token into
   an `ERROR` sub-node attached as a direct child of
   `SdblTopClause`. For input `ВЫБРАТЬ ПЕРВЫЕ A ИЗ Т`, the
   `A` Ident is consumed into the ERROR child; the
   limitations loop then exits because the following `ИЗ` is
   not a limitation keyword; the outer `selected_fields`
   parser then consumes `ИЗ Т` as a bare `SdblColumnRef` +
   `SdblAlias` (no `SdblFromClause` is emitted). A tighter
   recovery (recognise FROM / clause-keyword boundary, emit
   empty error sub-node instead of consuming) is deferred to
   Slice 12.

## Behaviour change

**None.** The Slice 7-addendum carries no behavioural fixes
beyond the three preserved-quirk documentation items above.

The Tier-A1 source v8327doc Глава 8 prose at
`page.html:1336` carries an additional semantic constraint
(top-level-only `РАЗРЕШЕННЫЕ`), but the parser does NOT
enforce this constraint and the Slice 7-addendum preserves
that — see §Tier classification §Deferred semantic constraint
above. Enforcement is deferred to a future slice (Slice 13
HIR reattachment or a dedicated IDE diagnostic).

The C2 author dropped the empty
`if !p.expect(TokenKind::Decimal) { /* Error recovery: complete
anyway */ }` no-op block in `top_clause`. This is purely
stylistic: `Parser::expect` preserves the same side effect
(invoke `Parser::error` on miss; bump next token into ERROR
sub-node) regardless of whether the caller inspects the bool
return. Codex C2 review confirmed this as within clean-room
latitude.

## Verification recipe

All of the following must be green before this attestation is
considered live (per Slice 7-addendum plan §Verification
recipe — explicit 25-step recipe written for the 4-commit C0 /
C1 / C2 / C3 flow, NOT inherited from Slice 11's
C0a/C0b-shaped flow):

1. `cargo test -p parser --test sdbl_parser_tests` — 197 tests
   (192 pre-existing + 5 Bucket-A gap tests added in C0).
2. `cargo test -p parser --test sdbl_slice6_package` — 26 tests.
3. `cargo test -p parser --test sdbl_slice7_fields` — 26 tests.
4. `cargo test -p parser --test sdbl_slice8_sources` — 28 tests.
5. `cargo test -p parser --test sdbl_slice9_joins` — 17 tests.
6. `cargo test -p parser --test sdbl_slice10a_backbone` — 28 tests.
7. `cargo test -p parser --test sdbl_slice10b_predicates` — 43 tests.
8. `cargo test -p parser --test sdbl_slice11_clauses` — 35 tests.
9. `cargo test -p parser --test sdbl_slice7_addendum_limitations`
   — new in C3; 13 spec-driven acceptance tests.
10. `cargo test -p parser --test sdbl_slice2_keywords` — 45 tests.
11. `cargo test -p parser --test sdbl_golden_corpus` — 23 tests.
12. `cargo test -p parser --test sdbl_slice1_core` — 4 tests.
13. `cargo test -p parser` — full parser suite.
14. `cargo test -p sdbl-hir` — 204+ HIR lowering tests
    (including `assign_alias_fields_in_query` HIR gate
    continues consuming `SdblLimitations` / `SdblTopClause`
    unchanged).
15. `cargo test -p lexer` — Slices 1 + 2 regression gate.
16. `cargo test -p ide-db` — SDBL validation tests.
17. `cargo test -p ide --test sdbl_completion_integration_test`
    — subquery + UNION scenarios.
18. `cargo test -p ide` — full IDE test suite.
19. `cargo test -p ide-diagnostics` — 1572 tests including the
    `test_top_clause_*` gates at
    `assign_alias_fields_in_query.rs:442-491` and the
    `test_distinct_top_combination` /
    `test_top_distinct_order` gates at
    `assign_alias_fields_in_query.rs:514-528`.
20. `cargo test -p mcp-server` — 72 tests.
21. `cargo build --workspace --all-targets` — workspace build.
22. `cargo clippy -p parser --all-targets --all-features --
    -D warnings` — parser clippy clean.
23. `cargo clippy -p lexer --all-targets --all-features --
    -D warnings` — lexer clippy clean.
24. `git log --follow crates/parser/src/grammar/sdbl/select.rs`
    — shows C1 + C2 as separate commits; C0 and C3 do not
    touch this file.
25. `git diff develop..HEAD --stat` — exactly 7 files:
    `crates/parser/src/grammar/sdbl.rs`,
    `crates/parser/src/grammar/sdbl/select.rs`,
    `crates/parser/tests/sdbl_parser_tests.rs`,
    `crates/parser/tests/sdbl_slice7_addendum_limitations.rs`,
    `docs/legal/sdbl-clean-room-slice7-addendum.md`,
    `docs/legal/sdbl-clean-room-slices.md`,
    `docs/legal/sdbl-select-mini-spec.md`. No other paths.

## Commit trail

- `ac1815e2` (2026-04-26) — C0: extend SDBL select mini-spec
  §Limitations with full DISTINCT / TOP / ALLOWED AST-shape
  contract + IDE-recovery allowances Q1/Q2/Q3 + Tier
  classification + ITS coverage verification table extension
  + Slice-7-addendum reaffirmation of the §Non-consultation
  statement; add 5 Bucket-A gap-test functions in
  `sdbl_parser_tests.rs` (192 → 197 tests); v8327doc Глава 8
  cited as primary source with pubqlang chapters 19/20/57 as
  secondary corroborating sources. No production-code changes.
- `49038d24` (2026-04-26) — C1: pure-refactor relocation of
  the 4 Slice-7-addendum functions out of the legacy
  `LEGACY (Slice 5 + SELECT limitation helpers pending)`
  block into a new
  `CLEAN-ROOM Slice 7-addendum — SELECT prefix qualifiers`
  banner; LEGACY banner header shrunk to `LEGACY (Slice 5
  pending)`; per-function `// C1 placeholder — clean-room
  rewrite in C2` markers attached; module-level
  `## Provenance` docstring in `sdbl.rs` extended with the
  Slice 7-addendum bullet (no attestation citation per
  forward-reference prohibition; flipped to "complete" in C3).
  This commit is the safe revert boundary for the clean-room
  rewrite.
- `4035e22d` (2026-04-26) — C2: re-author the 4 Slice
  7-addendum function bodies and rustdoc from v8327doc Глава 8
  + pubqlang chapters 19/20/57 + the C0-extended select
  mini-spec + the Slice 8 attestation cross-reference for
  `is_identifier_token`; attach one per-function provenance
  comment each; address the codex C2-review Minor finding
  (bilingual word-list line citations corrected from
  `:1024-1046` to `:1030-1034` / `:1040-1044` / `:920-924`);
  drop the empty `if !p.expect(...) { }` no-op block in
  `top_clause` (within clean-room latitude per codex C2-review
  confirmation).
- `cb383521` (2026-04-26) — C3: this attestation, the
  `sdbl_slice7_addendum_limitations.rs` acceptance tests, the
  Slice 7-addendum status block addition in
  [`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md),
  and the `sdbl.rs` Provenance docstring flip to "complete
  (landed with C3 2026-04-26)" with attestation citation.
- `<HILBERT_COMMIT>` (2026-04-26) — Anti-Hilbert close-out:
  replaced the `<THIS_COMMIT>` placeholder in the §Commit
  trail entry above with the actual C3 SHA `cb383521`. This
  amendment commit is itself self-referential by design (it is
  the only commit whose own SHA does not appear in the
  attestation §Commit trail) — mirrors the Slice 11
  attestation Anti-Hilbert close-out at commit `83267131`.

## Licensing note

The `crates/parser` crate retains its `LGPL-3.0-or-later`
license until the full Slice 6 → Slice 11 → Slice 5 parser
migration is complete and Slice 13 reattaches `sdbl-hir`.
Promoting the crate to Tier A (`MIT OR Apache-2.0`) is
explicitly out of scope for the Slice 7-addendum and will
happen once the last LEGACY-banner function under
`grammar/sdbl/select.rs` (`virtual_table_args_legacy`) has been
re-derived by Slice 5 and the HIR lowering cascade in
`sdbl-hir` has been cleaned up.

The lexer token `KwAllowed` (along with `KwOverall`, `KwOnly`,
`KwPeriods`, `KwHierarchy`, `KwAutoOrder`, `KwAsc`, `KwDesc`,
`KwFor`, `KwUpdate`, `KwIndex`) remains in the lexer
`LEGACY (Slices 3-5 pending)` block at
`crates/lexer/src/sdbl/mod.rs:470-495`. The Slice 7-addendum
cleans the **parser helper bodies only**; lexical promotion to
clean-room is Slice 3 territory.

## Author attestation

The Slice 7-addendum material listed above under **Scope** was
authored as a clean-room re-derivation from the sources listed
under **Sources consulted**, without using the `../bsl-parser`
project, the pre-C1 function bodies of the 4 Slice 7-addendum
functions, or any other third-party SDBL parser as working
text. This attestation applies at the date recorded at the top
of the document.
