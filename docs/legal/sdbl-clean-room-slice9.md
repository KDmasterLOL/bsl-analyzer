# SDBL Slice 9 — Clean-Room Attestation

**Status:** complete (2026-04-25).

This document attests the clean-room authorship of the Slice 9
material of the SDBL parser — the **JOIN family** surface
(predicate + clause body) — per the staged migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Scope

The paths claimed as clean-room Slice 9 authorship are:

- 2 functions in `crates/parser/src/grammar/sdbl/select.rs` under
  the `CLEAN-ROOM Slice 9 — JOIN family` banner:
  - `is_join_keyword` — JOIN-clause starter predicate. Returns
    true for any of the five bilingual starters (`LEFT`/`ЛЕВОЕ`,
    `RIGHT`/`ПРАВОЕ`, `FULL`/`ПОЛНОЕ`, `INNER`/`ВНУТРЕННЕЕ`,
    `JOIN`/`СОЕДИНЕНИЕ`). `OUTER`/`ВНЕШНЕЕ` is **not** a starter;
    it is consumed only inside `join_clause` after the optional
    join-type Ident.
  - `join_clause` — single `SdblJoinClause` parser. Reads
    `[type] [OUTER]? (JOIN|СОЕДИНЕНИЕ) data-source (ON|ПО)
    logical-expression`; bare `JOIN`/`СОЕДИНЕНИЕ` without an
    explicit type is accepted as implicit INNER per the SELECT
    mini-spec §JOIN clauses behavioural note.

- The clean-room banner block at the top of the Slice 9 section
  in `select.rs` (replacing the previous `LEGACY (Slices 9–11
  pending)` banner placement; the LEGACY banner itself was
  retained at `LEGACY (Slices 5, 11 pending)` for the still-
  pending items).

- The 15 Bucket-A gap-test functions in
  `crates/parser/tests/sdbl_parser_tests.rs` added in C0 plus
  the 17 spec-driven acceptance tests in
  `crates/parser/tests/sdbl_slice9_joins.rs` added in C3.

**1 NodeKind preserved bit-for-bit through the rewrite** (no
variant rename, no addition, no removal, no reorder in
`crates/syntax/src/syntax_kind.rs`):

`SdblJoinClause`.

**Function → NodeKind attribution map:**

| Function | Emits |
|---|---|
| `is_join_keyword` | (predicate, no NodeKind) |
| `join_clause` | `SdblJoinClause` |

**Child-attachment invariants** carried by Slice 9 that
downstream consumers depend on (parser-side AST-shape contracts):

1. **`SdblJoinClause::join_type()`** —
   `crates/syntax/src/ast.rs:1403-1437`. Reads direct tokens of
   the JOIN node combined with parent-source direct tokens and
   substring-matches `LEFT|ЛЕВОЕ`, `RIGHT|ПРАВОЕ`,
   `FULL|ПОЛНОЕ`. Defaults to `JoinType::Inner`. Slice 9 keeps
   the join-type Ident as a **direct token child** of
   `SdblJoinClause` (bumped via `p.bump()` after the keyword
   probe) and emits the bilingual EN/RU pairs in their lexer-
   canonical form.

2. **`SdblDataSource::join_clauses()`** —
   `crates/syntax/src/ast.rs:1342-1345`. Filters direct children
   for `SdblJoinClause`. Slice 9 `join_clause` runs exactly one
   `m.complete(NodeKind::SdblJoinClause)` per call (including on
   the missing-JOIN error path) and never abandons the marker.

3. **`SdblJoinClause::data_source()`** —
   `crates/syntax/src/ast.rs:1394-1396`.
   `find_map(SdblDataSource::cast)`. The JOIN'ed data source is
   a **direct child** of `SdblJoinClause`.

4. **HIR ON-condition reader** — `crates/sdbl-hir/src/lower/
   join_clause.rs:142-153`. Filters direct children for 7
   NodeKinds. Slice 9 calls `expressions::logical_expression(p)`
   which Slice 10a-attests always wraps in `SdblLogicalOrExpr`
   — covered by line 145 of the HIR reader.

5. **Nested JOIN — `lower_join_clause_recursive`** —
   `crates/sdbl-hir/src/lower/join_clause.rs:35-51`. Walks
   `join.data_source().join_clauses()` recursively. Slice 9
   keeps the JOIN'ed source attached as a direct child of
   `SdblJoinClause`, and Slice 8's attachment loop continues to
   own further-JOIN attachment to the inner source.

6. **`join_type()` parent-tokens fallback** — when the join-type
   keyword is ABSENT from `SdblJoinClause` direct tokens (bare
   JOIN), the helper at `ast.rs:1403-1437` walks up to the
   parent `SdblDataSource` and reads its direct tokens too.
   Slice 9 bumps the type Ident *inside* the marker, so the
   fallback only fires for genuine bare-JOIN inputs (where the
   substring match correctly defaults to `JoinType::Inner`).
   Acceptance test
   `test_slice9_chapter48_nested_join_inside_join` pins this
   behaviour: in `T1 LEFT JOIN T2 JOIN T3 …`, the inner bare
   JOIN's parent-tokens fallback walks to T2's `SdblDataSource`
   (which has no LEFT keyword), defaulting to Inner.

7. **FROM-side `SdblDataSource::join_clauses()` reader** at
   `crates/sdbl-hir/src/lower/from_clause.rs:36-72`. Reads
   `ds.subquery().is_some() && ds.join_clauses().next().is_some()`
   to emit `JoinWithSubQuery` (line 40) and similarly for
   virtual-table sources to emit `JoinWithVirtualTable`
   (line 65). Slice 9 ensures that after parsing
   `FROM (subquery) JOIN T ON ...`, the resulting
   `SdblDataSource` has BOTH `subquery()` Some AND
   `join_clauses().next()` Some. The HIR diagnostic emission
   for `JoinWithSubQuery`, `JoinWithVirtualTable`, and
   `LogicalOrInJoin` lives in
   `crates/ide-diagnostics/src/handlers/{join_with_sub_query.rs,
   join_with_virtual_table.rs,
   logical_or_in_join_query_section.rs}` — handler test suites
   not modified by Slice 9. Acceptance tests
   `test_slice9_inv7_subquery_join_shape`,
   `test_slice9_inv7_virtual_table_join_shape`, and
   `test_slice9_inv7_or_in_on_shape` pin the producer-side AST
   shape these consumers read.

## Sources consulted

The C2 author opened the following sources during the clean-room
rewrite. Each was consulted as a *specification* of the JOIN
grammar surface, not as a textual source for transcription.

### ITS pubqlang chapters (via the local dump)

Local dump path:
`/home/itrous/src/tools_migration/its/dump/html/`. The web URLs
under `https://its.1c.ru/db/pubqlang/` are paywalled and
WebFetch returns navigation stubs only; the dump is the
authoritative on-disk reference.

| Chapter | Evidence | Tier |
|---|---|---|
| `chapter_044.html` | `ВНУТРЕННЕЕ СОЕДИНЕНИЕ` listing + standalone `СОЕДИНЕНИЕ` reference. | A1 |
| `chapter_045.html` | `ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ` listing. | A1 |
| `chapter_046.html` | `ПРАВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ` listing (also one secondary `ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ` example). | A1 |
| `chapter_047.html` | `ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ` listing. | A1 |
| `chapter_048.html` | Chained / nested example listings (`ВНУТРЕННЕЕ СОЕДИНЕНИЕ` + `ЛЕВОЕ` chains). | A1 |

§ITS coverage verification — outcomes recorded at C2 author time:

- `ВНУТРЕННЕЕ СОЕДИНЕНИЕ` / chapter 44 — **verified yes**;
- `ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ` / chapter 45 — **verified yes**;
- `ПРАВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ` / chapter 46 — **verified yes**;
- `ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ` / chapter 47 — **verified yes**;
- bare standalone `СОЕДИНЕНИЕ` / chapter 44 — **verified yes**
  (Tier A1 — chapter 44 standalone reference);
- chained / nested examples / chapter 48 — **verified yes**;
- bare `ЛЕВОЕ`/`ПРАВОЕ`/`ПОЛНОЕ` without `ВНЕШНЕЕ` — **verified
  no** (no chapter prose attests OUTER optionality across
  chapters 45/46/47); these forms are pinned as Tier D local
  allowances under §Preserved behaviours #2.
- `ESCAPE/СПЕЦСИМВОЛ`-equivalent of the ON-clause — **N/A**
  (Slice 9 does not introduce its own predicates; the ON
  condition delegates entirely into Slice 10a's
  `logical_expression`).

### SELECT mini-spec

`docs/legal/sdbl-select-mini-spec.md` — the C0a-authored
clean-room reference for the SELECT grammar:

- §Data source (line 248) — the join-attachment loop boundary
  inside `data-source` (Slice 8 owns the loop; Slice 9 owns
  each call into `join-clause`);
- §JOIN clauses §Shape (lines 297–319) — the join-clause
  grammar production;
- §JOIN clauses behavioural note (line 318) — bare JOIN
  without explicit type is accepted and treated structurally
  as a valid join form (the source for the implicit-INNER
  default);
- §Recovery requirements item #6 (line 410) — incomplete
  join-condition-after-`ON`/`ПО` recovery policy
  (the mandate that the ON condition stays recoverable when
  incomplete; the parser delegates to
  `expressions::logical_expression`).

### Slice 2 lexer attestation

`docs/legal/sdbl-clean-room-slice2.md` — bilingual EN/RU keyword
vocabulary. Slice 2 attests the keyword pairs Slice 9 uses:
`LEFT/ЛЕВОЕ`, `RIGHT/ПРАВОЕ`, `FULL/ПОЛНОЕ`,
`INNER/ВНУТРЕННЕЕ`, `JOIN/СОЕДИНЕНИЕ`, `OUTER/ВНЕШНЕЕ`,
`ON/ПО`. The lexer canonicalises both sides to `TokenKind::Ident`
which Slice 9 probes via `at_keyword(...)`.

### Cross-slice neighbours

Slice 9 reads, but does not modify, the following neighbouring
attestations:

- `docs/legal/sdbl-clean-room-slice6.md` — query package +
  select entry;
- `docs/legal/sdbl-clean-room-slice7.md` — SELECT prefix;
- `docs/legal/sdbl-clean-room-slice8.md` — FROM sources and
  source chains, in particular the §AST-shape invariant #2 that
  `data_source` runs the `while is_join_keyword(p) {
  join_clause(p) }` attachment loop — Slice 9 owns the body of
  that loop;
- `docs/legal/sdbl-clean-room-slice10a.md` — the
  `expressions::logical_expression(p)` entry point Slice 9
  delegates the ON condition to;
- `docs/legal/sdbl-clean-room-slice10b.md` — the
  `column_or_function` clause-keyword recovery fix C2 precedent
  considered (and rejected) for the audit-gate decision under
  §Behaviour change.

## Non-consultation statement

The Slice 9 C2 clean-room rewrite did **not** consult:

- `../bsl-parser/*` (the third-party ANTLR-shaped grammar
  reference); the project root contains a checkout of this
  external grammar but it was not opened during C2 authoring;
- the pre-C1 function bodies of `is_join_keyword` and
  `join_clause` as a textual source — the bodies were available
  in the working tree (C1 split them out of LEGACY without
  textual change), but the C2 author re-derived the bodies from
  the cited sources without using the pre-C1 text as working
  copy;
- any third-party SDBL parser or grammar text.

The independent-derivation claim follows the same convention as
the prior Slice 1, 2, 6, 7, 8, 10a, 10b attestations: the
resulting event-parser shape is the natural expression of the
cited ITS chapters and the project's own event-parser
conventions; where the C2 clean-room implementation converges
with the pre-C1 implementation, that convergence is on the same
mini-spec specification both implementations follow, not
consultation of working text.

## Preserved pre-refactor behaviours

The two functions emit syntax trees with the same observable
shape as the pre-C1 implementation, modulo the bug-fix entries
under §Behaviour change. The non-trivial preserved behaviours
are:

1. **`is_join_keyword` does not accept `OUTER`/`ВНЕШНЕЕ` as a
   starter.** Adding `OUTER` to the starter set would change
   Slice 8's join-attachment-loop boundary (the loop test
   becomes unstably true at `OUTER` in malformed input, breaking
   the alias-/recovery boundary in `is_clause_keyword`). The
   C2 rewrite preserves the five-starter inventory.

2. **Bare `LEFT`/`RIGHT`/`FULL`/`INNER` (and RU equivalents)
   without `OUTER`/`ВНЕШНЕЕ` is accepted as a parser-tolerated
   form.** No ITS prose-note in chapters 45/46/47 attests OUTER
   optionality — the listings are exact. The parser bumps the
   type Ident unconditionally and only checks for OUTER as
   optional, so bare `ЛЕВОЕ СОЕДИНЕНИЕ` produces a `JoinType::
   Left` clause via substring match. This is preserved as a
   Tier D local IDE-recovery allowance, pinned by acceptance
   tests `test_slice9_d_bare_full_ru_local_allowance`,
   `test_slice9_d_bare_left_ru_local_allowance`,
   `test_slice9_d_bare_right_ru_local_allowance`.

3. **Bare `JOIN`/`СОЕДИНЕНИЕ` without explicit type is
   implicit INNER.** Source: SELECT mini-spec §JOIN clauses
   behavioural note (line 318) and ITS chapter 44 standalone
   `СОЕДИНЕНИЕ` reference. Pinned by
   `test_slice9_c_bare_join_en` (Tier C) and the C0 Bucket-A
   `test_slice9_bare_join_ru` (RU companion).

4. **Type Ident is bumped INSIDE the `SdblJoinClause` marker.**
   `SdblJoinClause::join_type()` reads direct tokens to
   substring-match `LEFT|ЛЕВОЕ`, `RIGHT|ПРАВОЕ`, `FULL|ПОЛНОЕ`.
   The C2 rewrite keeps the bump inside the marker so the helper
   gets the type token at the expected position. Consumer-side
   invariant #1.

5. **JOIN clause bumps `OUTER`/`ВНЕШНЕЕ` after any preceding
   type Ident, including `INNER`/`ВНУТРЕННЕЕ`.** Even though
   chapter 44 (INNER) does not list a `INNER OUTER` form, the
   parser tolerates it because the lexer's Slice 2 vocabulary
   accepts the keyword pair regardless of preceding context.
   This is the SQL-92 OUTER-keyword ergonomics convention and
   produces a recoverable parse for typo-paths like
   `INNER OUTER JOIN`.

6. **Mandatory `JOIN`/`СОЕДИНЕНИЕ` after type/OUTER probes.**
   Missing here: `Parser::error()` bumps the offending token
   into an ERROR child of the in-progress `SdblJoinClause`
   marker, then `m.complete(NodeKind::SdblJoinClause)` runs
   anyway and the function returns. The `Parse::errors()` list
   is **not** populated (`Parser::error()` only inserts an
   ERROR node into the tree). Pinned by audit-gate test
   `test_slice9_missing_join_keyword_current_behavior` in
   `sdbl_parser_tests.rs`. Recovery improvement (zero-width
   error mirroring Slice 10b's `column_or_function`) is
   deferred to the Slice 12 IDE-recovery rewrite — see
   §Behaviour change for the audit-gate decision rationale.

7. **Mandatory `ON`/`ПО` after the joined data source.**
   Missing here: `Parser::error()` bumps the offending token
   into an ERROR child and parsing falls through to
   `expressions::logical_expression(p)`, so the user's typed
   condition still lands inside the JOIN node (recovery policy
   from mini-spec §Recovery requirements item #6). Pinned by
   audit-gate test `test_slice9_missing_on_current_behavior`.

8. **`SdblDataSource` is a direct child of `SdblJoinClause`,
   not nested under an alias wrapper.** `data_source(p)` runs
   inside the same marker scope, so the `SdblDataSource` it
   completes attaches directly. Consumer-side invariant #3.

## Behaviour change

**None.**

The plan v9 §Pre-existing bug audit identified the two
`Parser::error()`-bumps (missing JOIN keyword and missing
ON/ПО) as audit-gate candidates with two options:

- **Option A FIX** — mirror Slice 10b `column_or_function`:
  emit a zero-width ERROR via `let err = p.start();
  err.complete(p, NodeKind::Error);` (which does not bump the
  next token), and flip the audit-gate tests #14/#15 in the
  same atomic commit as the fix; document under §Behaviour
  change.
- **Option B PRESERVE** — keep `p.error()` as-is (bump-on-
  error); document under §Preserved behaviours; audit-gate
  tests #14/#15 stay green.

The C2 author chose **Option B PRESERVE** for the following
reasons:

1. Both options leave bad recovery trees in the error case.
   For test #14 (`FROM T1 LEFT T2 ON A = B`), Option A keeps
   T2 in the FROM clause but still leaves it un-attached to a
   JOIN; Option B consumes T2 into an ERROR child but doesn't
   help downstream. The recovery-quality gap is marginal.
2. For test #15 (`FROM T1 JOIN T2 A = B`), Option A would
   allow `expressions::logical_expression` to parse the full
   `A = B` as the join condition (instead of just `B`), which
   is meaningfully better. But this single improvement does
   not justify a parser-side §Behaviour change entry when the
   broader IDE-recovery rewrite is already scoped under Slice
   12.
3. Slice 10b's `column_or_function` clause-keyword fix
   addressed a **production-correctness bug** (mid-typed
   `func(x, FROM ...)` hijacked the `FROM` keyword from the
   outer SELECT body, producing a noisy parse tree across the
   whole query). Slice 9's two error paths only affect local
   recovery quality on incomplete JOIN inputs — there is no
   downstream hijacking analogue.
4. Slice 12's scope explicitly owns IDE-recovery hardening
   across the parser surface. Splitting the fix between Slice
   9 and Slice 12 risks two §Behaviour change entries against
   the same code path; landing both fixes in Slice 12 makes
   the recovery story coherent.

The two `Parser::error()`-bumps therefore remain documented
under §Preserved behaviours items #6 and #7, and the audit-gate
tests
`test_slice9_missing_join_keyword_current_behavior` /
`test_slice9_missing_on_current_behavior` continue to lock
the current behaviour. The C0 audit gate's role flips from
"pre-rewrite regression gate" to "post-rewrite preservation
gate" without test edits.

## Verification recipe

Run each command in sequence; all must pass.

1. `cargo test -p parser --test sdbl_parser_tests` —
   178 passed (was 163 + 15 Slice 9 Bucket-A gap adds in C0).
2. `cargo test -p parser --test sdbl_slice6_package`
   (26 passed).
3. `cargo test -p parser --test sdbl_slice7_fields`
   (26 passed).
4. `cargo test -p parser --test sdbl_slice8_sources`
   (28 passed).
5. `cargo test -p parser --test sdbl_slice9_joins`
   (17 passed — the new C3 acceptance suite).
6. `cargo test -p parser --test sdbl_slice10a_backbone`
   (28 passed).
7. `cargo test -p parser --test sdbl_slice10b_predicates`
   (43 passed).
8. `cargo test -p parser --test sdbl_slice2_keywords`
   (45 passed).
9. `cargo test -p parser --test sdbl_golden_corpus`
   (23 passed).
10. `cargo test -p parser --test sdbl_slice1_core`
    (4 passed + ignored — pre-existing).
11. `cargo test -p parser` — full parser suite.
12. `cargo test -p sdbl-hir` — 204 HIR lowering tests.
13. `cargo test -p lexer` — full lexer suite.
14. `cargo test -p ide-db` — SDBL validation tests including
    `parse_sdbl` path.
15. `cargo test -p ide` — full IDE test suite.
16. `cargo test -p ide-diagnostics` — 1572 passed (+1 ignored,
    pre-existing) including the three Slice 9 producer-side
    consumer suites:
    `crates/ide-diagnostics/src/handlers/join_with_sub_query.rs`,
    `crates/ide-diagnostics/src/handlers/join_with_virtual_table.rs`,
    `crates/ide-diagnostics/src/handlers/logical_or_in_join_query_section.rs`.
17. `cargo test -p mcp-server` — 72 passed.
18. `cargo build --workspace --all-targets` — workspace build
    clean.
19. `cargo clippy --all-targets --all-features -- -D warnings`
    — workspace clippy clean (verified by pre-commit hook on
    every Slice 9 commit).

## Commit trail

Slice 9 landed across 4 logical phases. The base ref
`5b8168a6` is the Slice 9 C2 anchor; the immediate pre-Slice-9
boundary on `develop` is `4ab6f154` (the last Slice 10b commit
on the prior tail). The trail enumerated below names every
commit reachable from `git log --oneline --reverse
4ab6f154..HEAD` *except* the absolute-last one (see
Anti-Hilbert disclosure at the end of this section).

- **C0 — `de6820f8` (2026-04-25)**: audit SDBL Slice 9 tests
  and extend JOIN coverage with 15 Bucket-A gap-test functions
  in `sdbl_parser_tests.rs`. Tier classification per test
  pinned in the file header comment block:
  - 4 Tier A1 RU canonical listings (#1–#4: ВНУТРЕННЕЕ /
    ЛЕВОЕ ВНЕШНЕЕ / ПРАВОЕ ВНЕШНЕЕ / ПОЛНОЕ ВНЕШНЕЕ);
  - 2 Tier A2 OR Tier D candidates (#5 bare ПОЛНОЕ, #6 bare
    ЛЕВОЕ — final tier set by C2 author after chapter prose
    verification: classified as Tier D);
  - 2 Tier C / chapter 44 standalone (#7 bare JOIN EN, #8
    bare СОЕДИНЕНИЕ RU);
  - 2 chapter 48 chained / nested (#9, #10);
  - 3 parser-side AST-shape guards for HIR diagnostics (#11
    JoinWithSubQuery, #12 JoinWithVirtualTable, #13
    LogicalOrInJoin);
  - 2 audit-gate tests for `Parser::error()`-bumps (#14
    missing-JOIN-keyword, #15 missing-ON).
  Codex pair-review found one P2 (the `assert_clean_parse`
  helper must reject `SyntaxKind::ERROR` recovery nodes —
  `Parser::error()` does not populate `Parse::errors()`); the
  helper and inline clean-parse assertions were updated to
  scan the syntax tree for ERROR descendants in the same
  commit.
- **C1 — `dc10cd6c` (2026-04-25)**: split SDBL Slice 9 JOIN
  family from LEGACY into clean-room banner. Pure refactor:
  physically relocate `is_join_keyword` and `join_clause` from
  the previous `LEGACY (Slices 9–11 pending)` block to a new
  `CLEAN-ROOM Slice 9 — JOIN family` banner placed directly
  after the Slice 8 banner; update the LEGACY banner header
  to `LEGACY (Slices 5, 11 pending)` and drop the JOIN-family
  bullet; update Slice 8's `data_source` comment to reference
  the Slice 9 helper; attach 2 `// C1 placeholder — clean-
  room rewrite in C2` markers; extend `sdbl.rs` Provenance
  docstring with a "Slice 9 — clean-room (rewrite in
  progress)" bullet (no attestation citation per Slice 10b
  Round-7 finding precedent). No fixup commits.
- **C2 — `5b8168a6` (2026-04-25)**: rewrite SDBL Slice 9 JOIN
  family clean-room from ITS pubqlang chapters 44–48 and
  SELECT mini-spec. Replaces the 2 C1 placeholder comments
  with tiered (A1/B/C/D) per-function provenance comments;
  records §ITS coverage verification outcomes inline in the
  body comments (verified-yes for the four chapter listings +
  chapter 44 standalone + chapter 48 chained / nested
  examples; verified-no for bare LEFT/RIGHT/FULL OUTER
  optionality which becomes Tier D local allowance). Audit-
  gate decision: **Option B PRESERVE** for both
  `Parser::error()`-bumps (rationale in §Behaviour change).
  No fixup commits.
- **C3 — `9af02c0b` (2026-04-25)**: this attestation +
  17 spec-driven acceptance tests in
  `crates/parser/tests/sdbl_slice9_joins.rs` + master-doc
  flip in `docs/legal/sdbl-clean-room-slices.md` + module
  Provenance docstring flip to "complete (2026-04-25)" with
  attestation citation in `crates/parser/src/grammar/sdbl.rs`.

**Anti-Hilbert disclosure.** The very last commit on this
branch — the one that authors / amends this attestation
§Commit trail itself — is necessarily not named in this
enumeration: a Git commit cannot reference its own future
hash at write time. This anti-Hilbert property applies to
every legal/clean-room attestation that records its own
commit trail, and is shared with the prior Slice 1, 2, 6, 7,
8, 10a, 10b attestations in this project. A reviewer running
`git log --oneline --reverse 4ab6f154..HEAD` will always see
exactly one commit beyond the trail's last named hash: that
commit is the one that landed this attestation in its
current state, and it is the natural endpoint of the trail.

The phase totals (named hashes in the trail above): C0 1,
C1 1, C2 1, C3 1 anchor = 4 commits enumerated. The branch
HEAD adds one trailing commit (the one editing this trail to
name the C3 anchor + any subsequent fixups), per the Anti-
Hilbert disclosure above.

## Licensing note

The `crates/parser` crate retains its `LGPL-3.0-or-later`
license until the full Slice 6 → Slice 11 parser migration is
complete and Slice 13 reattaches `sdbl-hir`. Promoting the
crate to Tier A (`MIT OR Apache-2.0`) is explicitly out of
scope for Slice 9 and will happen once the last LEGACY-banner
function under `grammar/sdbl/expressions.rs` and
`grammar/sdbl/select.rs` has been re-derived (only Slices 5
and 11 remain after Slice 9 lands) and the HIR lowering
cascade in `sdbl-hir` has been cleaned up (Slice 13).

## Author attestation

The Slice 9 material listed above under **Scope** was authored
as a clean-room re-derivation from the sources listed under
**Sources consulted**, without using the `../bsl-parser`
project, the pre-C1 function bodies of `is_join_keyword` and
`join_clause` as working text, or any other third-party SDBL
parser. The independent-derivation claim follows the same
convention as Slices 1, 2, 6, 7, 8, 10a, 10b attestations: the
resulting event-parser shape is the natural expression of the
cited ITS chapters and the project's own event-parser
conventions; where the C2 clean-room implementation converges
with the pre-C1 implementation, that convergence is on the
same mini-spec specification both implementations follow, not
consultation of working text.

The author attests that:

- the two Slice 9 functions in `select.rs` were re-authored
  under the `CLEAN-ROOM Slice 9 — JOIN family` banner;
- no behaviour change was introduced by the rewrite — the
  audit-gate decision under §Behaviour change is **Option B
  PRESERVE** for both `Parser::error()`-bumps, with recovery
  hardening deferred to Slice 12;
- the single NodeKind emitted by `join_clause`
  (`SdblJoinClause`) retains its pre-C1 child-attachment
  shape so all downstream consumers (the
  `SdblJoinClause::join_type()` parent-tokens fallback at
  `ast.rs:1403-1437`, the HIR ON-condition reader at
  `sdbl-hir/src/lower/join_clause.rs:142-153`, the FROM-side
  `JoinWithSubQuery`/`JoinWithVirtualTable` shape readers at
  `from_clause.rs:36-72`, the `LogicalOrInJoin` shape reader
  at `join_clause.rs:188`, and the recursive
  `lower_join_clause_recursive` at `join_clause.rs:35-51`)
  continue to work without modification;
- the verification recipe was run end-to-end and all 19 steps
  pass.

— Authored by the Slice 9 C2 implementation, attested by the
Slice 9 C3 commit (2026-04-25).
