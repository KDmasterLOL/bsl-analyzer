# SDBL Slice 2 — Clean-Room Attestation

**Status:** complete (2026-04-24).

This document attests the clean-room authorship of the Slice 2
material of the SDBL lexer, per the staged migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Scope

The paths claimed as clean-room Slice 2 authorship are:

- `crates/lexer/src/sdbl/mod.rs` — specifically:
  - the file-level docstring bullet added for Slice 2;
  - the variants of `SdblTokenKind` declared under the
    `CLEAN-ROOM Slice 2 — structural keyword vocabulary (ITS
    pubqlang/10, /12)` banner, with their `#[regex(...)]`
    annotations, their per-variant ITS provenance comments, the
    top-of-block convenience index, and the preserved-behaviour
    comment block on `KwOnOrBy`. The full Slice 2 list is 35
    variants:
    - **Clause starters (12):** `KwSelect`, `KwFrom`, `KwInto`,
      `KwWhere`, `KwGroup`, `KwOrder`, `KwHaving`, `KwTotals`,
      `KwUnion`, `KwAll`, `KwDistinct`, `KwTop`.
    - **Join family (7):** `KwJoin`, `KwInner`, `KwLeft`,
      `KwRight`, `KwFull`, `KwOuter`, `KwOnOrBy`.
    - **Aliasing & predicates (5):** `KwAs`, `KwIn`, `KwBetween`,
      `KwLike`, `KwIs`.
    - **CASE family (5):** `KwCase`, `KwWhen`, `KwThen`, `KwElse`,
      `KwEnd`.
    - **Logical operators & literals (6):** `OpAnd`, `OpOr`,
      `OpNot`, `LitTrue`, `LitFalse`, `LitNull`.
- `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt` — the six
  new corpus entries (051–056) that close the bilingual-coverage
  blind spots for Slice 2 variants before the regex rewrite.
- `crates/lexer/tests/sdbl_golden_corpus.rs` — the snapshot
  regenerated against the extended corpus.
- `crates/lexer/tests/sdbl_slice2_keywords.rs` — the new
  spec-driven acceptance test file.

The original `LEGACY (Slices 3–5 pending)` section of `SdblTokenKind`
remained explicitly **not** covered by this Slice 2 attestation. The
17 clause-keyword variants from that legacy block (`KwDrop`,
`KwAutoOrder`, `KwAsc`, `KwDesc`, `KwHierarchy`, `KwAllowed`,
`KwFor`, `KwUpdate`, `KwIndex`, `KwOnly`, `KwOverall`, `KwPeriods`,
`KwEscape`, `KwRefs`, `KwCast`, `KwType`, `KwValue`) were claimed
clean-room by the **Slice 2-addendum** (landed 2026-05-07; see
[`sdbl-clean-room-slice2-addendum.md`](sdbl-clean-room-slice2-addendum.md)).
The remaining LEGACY-block variants (`Fn*`, `Mdo*`, `Vt*`, `Type*`,
`LitUndefined`, `Period*`, and the `Error` fallback) stay Tier B and
will be re-derived by Slices 3, 4, and 5.

Downstream files that consume SDBL tokens
(`crates/parser/src/lib.rs`, `crates/parser/src/sdbl_token_converter.rs`,
`crates/parser/src/grammar/sdbl/**`, `crates/sdbl-hir/**`,
`crates/parser/tests/sdbl_parser_tests.rs`) were not modified in
Slice 2; they continue to see the public surface
`lexer::sdbl::{tokenize_sdbl, SdblToken, SdblTokenKind}` unchanged.

`KwAs` is used by the parser in both field-alias position and inside
`CAST` / `ВЫРАЗИТЬ` expressions; the lexer emits a single merged
token in both positions, and the parser disambiguates by context
(`crates/parser/src/grammar/sdbl/expressions.rs` around the `CAST`
entry point, and `crates/parser/src/grammar/sdbl/select.rs` around
the select-list and FROM-source alias positions). This merge is
preserved; no lexer-side split is performed in Slice 2.

`LitNull` is emitted as `SdblTokenKind::LitNull`, but the token
converter (`crates/parser/src/sdbl_token_converter.rs`) maps it onto
`TokenKind::Ident` so the grammar can re-check it by text
(`p.at_keyword("NULL")`). This mapping is preserved, not touched;
the `LitNull` kind is retained so downstream HIR lowering can keep
the semantic distinction.

## Sources consulted

The Slice 2 material was re-derived from:

1. 1C ITS documentation:
   - <https://its.1c.ru/db/pubqlang/content/10/hdoc> — query-language
     structure: SELECT / FROM / WHERE / GROUP / ORDER / HAVING /
     TOTALS / UNION / ALL / DISTINCT / TOP / INTO clause starters,
     the join clause family and its modifiers, field aliasing, basic
     predicates (IN, BETWEEN, LIKE, IS), and the conditional (CASE)
     expression.
   - <https://its.1c.ru/db/pubqlang/content/12/hdoc> — lexical
     elements: logical operators (AND / OR / NOT), boolean literals
     (TRUE / FALSE), the NULL literal, and the identifier
     longest-match rule that protects the keyword vocabulary from
     collisions with user identifiers.
2. The Slice 1 clean-room material already present in
   `crates/lexer/src/sdbl/mod.rs` above the Slice 2 banner —
   consulted only for the existing `Ident` regex (priority = 1) and
   for the shape of the per-variant provenance comment format.

The resulting `#[regex(...)]` patterns take the form
`(?i)<russian>|(?i)<english>` for every bilingual keyword, and
`(?i)null` for `LitNull`. These patterns are the natural expression
of the ITS rule "each keyword has a Russian and an English spelling,
both case-insensitive" and would converge regardless of author. The
claim made here is **independent derivation from the sources above**,
not textual novelty of the bilingual-alternation pattern.

## Non-consultation statement

During the authorship of the Slice 2 material the following sources
were not used as working text:

- the sibling `../bsl-parser` project — neither its grammar files
  nor its token inventory were consulted;
- the pre-clean-room regex text of the Slice 2 variants themselves —
  the `#[regex]` attribute bodies present in the LEGACY block prior
  to this slice were not consulted during the re-derivation;
- any other third-party SDBL grammar, token inventory, or parser.

The extended byte-identity golden corpus
(`crates/lexer/tests/sdbl_golden_corpus.rs`) is the verification
gate that the re-derived patterns accept exactly the same text set
as the pre-refactor implementation; the corpus was extended in
commit `43ccd80e` before the re-derivation to close the two
confirmed bilingual blind spots (`КАК`, `ON`) plus the wider set
flagged by pair review.

## Preserved pre-refactor behaviours

One behaviour observed in the pre-clean-room lexer diverges from
what a strict reading of the ITS spec would produce and is
preserved bit-for-bit in Slice 2:

1. `KwOnOrBy` bundles the join-`ON` keyword, the grouping / sorting
   `BY` keyword, and the Russian `ПО` (which straddles both roles)
   into a single token kind. The parser disambiguates by token
   text: `crates/parser/src/grammar/sdbl/select.rs` uses
   `at_sdbl_keyword(p, "ON", "ПО")` at its join-clause entry and
   `at_sdbl_keyword(p, "BY", "ПО")` at every `BY`-expecting clause
   (GROUP BY, ORDER BY, TOTALS BY, INDEX BY, and their variants).

   The rationale for deferring the split is scope discipline, not
   parser complexity: splitting `KwOnOrBy` into `KwOn` + `KwBy`
   requires adding and removing enum variants, which cascades into
   the 26-entry `Kw* → TokenKind::Ident` mapping in
   `crates/parser/src/sdbl_token_converter.rs`. That file is
   explicitly out of scope for Slice 2 per the staged plan in
   [`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md); the
   split will happen naturally in Slice 9 (joins) and/or Slice 11
   (clauses after FROM) where converter edits are in scope.

## Verification recipe

All of the following must be green before this attestation is
considered live:

1. `cargo test -p lexer` — inline unit tests, the byte-identity
   golden corpus, the clean-room Slice 1 acceptance tests, the new
   Slice 2 keyword acceptance tests, and the crate doctests.
2. `cargo test -p parser --test sdbl_parser_tests` — 123 SDBL
   parser tests (token kinds and accepted text set unchanged).
3. `cargo test -p parser` — full parser test suite.
4. `cargo test -p sdbl-hir` — HIR lowering tests (token identity
   unchanged).
5. `cargo build --workspace --all-targets` — workspace build.
6. `cargo clippy -p lexer --all-targets --all-features -- -D warnings`
   — lexer clippy with warnings denied.

## Commit trail

- `3da0f41d` (2026-04-24) — C0: extend SDBL golden corpus with
  Slice 2 bilingual coverage (entries 051–057 covering `КАК`,
  `ON`, Russian clause vocabulary, Russian predicates / logical
  operators / CASE family, English `TOTALS`, the Russian
  join-family modifiers, and English `LEFT JOIN`).
- `bc8fd550` (2026-04-24) — C1: move the 35 Slice 2 variants
  under the `CLEAN-ROOM Slice 2` banner grouped into five
  labeled sub-sections. No regex or variant-name change; golden
  corpus passes unchanged. This commit is the safe revert
  boundary for the clean-room rewrite.
- `ea0e34d2` (2026-04-24) — C2: rewrite the 35 Slice 2
  `#[regex]` attributes clean-room from ITS, attach per-variant
  provenance comments, and populate the top-of-block convenience
  index. Golden corpus passes unchanged (pattern identity is the
  natural outcome of the bilingual-alternation rule).
- C3 (2026-04-24): this attestation, the `sdbl_slice2_keywords.rs`
  acceptance tests, and the Slice 2 status update in
  [`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Licensing note

The `crates/lexer` crate retains its `LGPL-3.0-or-later` license
until the full Slice 1 → Slice 5 migration is complete. Promoting
the crate to Tier A (`MIT OR Apache-2.0`) is explicitly out of
scope for Slice 2 and will happen once the last legacy variant
(currently the `Fn*`, `Mdo*`, `Vt*`, `Type*`, `LitUndefined`,
`Period*`, and long-tail `Kw*` variants listed under Scope) has
been re-derived.

## Author attestation

The Slice 2 material listed above under **Scope** was authored as a
clean-room re-derivation from the sources listed under **Sources
consulted**, without using the `../bsl-parser` project, the
pre-clean-room regex text of the Slice 2 variants, or any other
third-party SDBL grammar as working text. This attestation applies
at the date recorded at the top of the document.
