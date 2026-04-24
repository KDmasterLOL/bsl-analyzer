# SDBL Slice 1 — Clean-Room Attestation

**Status:** complete (2026-04-24).

This document attests the clean-room authorship of the Slice 1
material of the SDBL lexer, per the staged migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Scope

The paths claimed as clean-room Slice 1 authorship are:

- `crates/lexer/src/sdbl/mod.rs` — specifically:
  - the file-level docstring;
  - the variants of `SdblTokenKind` declared under the
    `CLEAN-ROOM Slice 1 — ITS-derived` banner, with their
    `#[regex(...)]` / `#[token(...)]` annotations and their inline
    provenance comments. The full Slice 1 list is: `Whitespace`,
    `Newline`, `Comment`, `LParen`, `RParen`, `Dot`, `Comma`,
    `Semicolon`, `Hash`, `Ampersand`, `Bar`, `Eq`, `Neq`, `Le`,
    `Lt`, `Ge`, `Gt`, `Plus`, `Minus`, `Star`, `Slash`, `Percent`,
    `Float`, `Decimal`, `Quote`, `String`, `Date`, `Ident`,
    `Parameter`.
- `crates/lexer/src/sdbl/strings_mode.rs` — the entire module
  (mini-spec header, public surface, and `scan` implementation).
- `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt`.
- `crates/lexer/tests/sdbl_golden_corpus.rs` — including the
  `expect-test` snapshot captured against the pre-refactor
  implementation.
- `crates/lexer/tests/sdbl_slice1_core.rs`.

The `LEGACY (Slices 2–5 pending)` section of `SdblTokenKind` (`Kw*`,
`Fn*`, `Mdo*`, `Vt*`, `Type*`, `Lit*`, `Op*`, `Period*`, and the
`Error` fallback) is explicitly **not** covered by this attestation;
those variants remain Tier B and will be re-derived by later slices.

Downstream files that consume SDBL tokens
(`crates/parser/src/lib.rs`, `crates/parser/src/sdbl_token_converter.rs`,
`crates/sdbl-hir/**`, `crates/parser/tests/sdbl_parser_tests.rs`)
were not modified in Slice 1; they continue to see the public surface
`lexer::sdbl::{tokenize_sdbl, SdblToken, SdblTokenKind}` unchanged.

## Sources consulted

The Slice 1 material was re-derived from:

1. 1C ITS documentation:
   - <https://its.1c.ru/db/pubqlang/content/12/hdoc> — lexical
     elements (whitespace, identifiers, numeric and string and date
     literals, separators, operators).
   - <https://its.1c.ru/db/pubqlang/content/10/hdoc> — query-language
     structure (parameter references, temp-table markers, operator
     semantics).
2. The local mini-spec at the top of
   `crates/lexer/src/sdbl/strings_mode.rs`, describing the BSL
   multiline-string convention as it surfaces inside SDBL strings
   embedded as BSL literals.
3. Universal regex primitives: decimal-digit character classes,
   the `\p{L}` Unicode-letter class, and simple literal tokens. The
   resulting regex texts are the natural expression of those
   primitives and would converge regardless of author. The claim
   made here is **independent derivation from the sources above**,
   not textual novelty of the primitive regexes.

## Non-consultation statement

During the authorship of the Slice 1 material the following sources
were not used as working text:

- the sibling `../bsl-parser` project — neither its grammar files
  nor its token inventory were consulted;
- any other third-party SDBL grammar, token inventory, or parser.

## Preserved pre-refactor behaviours

Two behaviours observed in the pre-clean-room scanner diverge from
what a strict reading of the ITS spec would produce. Both are
preserved bit-for-bit in Slice 1 so that the byte-identity golden
corpus stays green across the commit series, and both are
documented in the strings-mode mini-spec for a later-slice
follow-up:

1. The doubled-quote `""` escape inside a string resets the
   accumulation anchor past the pair; content scanned before `""`
   in the same run is not emitted as its own token. A spec-aligned
   treatment is explicitly deferred and will land with a
   justification-committed corpus bump.
2. Line comments are not in the upstream SDBL grammar; they are
   retained as a local tooling concession (see the per-variant
   provenance comment on `Comment` in
   `crates/lexer/src/sdbl/mod.rs`).

## Verification recipe

All of the following must be green before this attestation is
considered live:

1. `cargo test -p lexer` — inline unit tests, the byte-identity
   golden corpus, the clean-room Slice 1 acceptance tests, and the
   crate doctests.
2. `cargo test -p parser --test sdbl_parser_tests` — 123 SDBL
   parser tests.
3. `cargo test -p parser` — full parser test suite.
4. `cargo test -p sdbl-hir` — HIR lowering tests.
5. `cargo build --workspace --all-targets` — workspace build.
6. `cargo clippy -p lexer --all-targets --all-features -- -D warnings`
   — lexer clippy with warnings denied.

## Commit trail

- `f4a3c9ce` — C0: establish SDBL lexer byte-identity golden corpus
  baseline.
- `49aa192c` — C1: extract SDBL strings-mode into a dedicated pure
  module.
- `ac4cbad2` — C2: rewrite SDBL Slice 1 tokens and strings-mode
  clean-room from ITS.
- C3: this attestation, the `sdbl_slice1_core.rs` acceptance tests,
  and the Slice 1 status update in
  [`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Licensing note

The `crates/lexer` crate retains its `LGPL-3.0-or-later` license
until the full Slice 1 → Slice 5 migration is complete. Promoting
the crate to Tier A (`MIT OR Apache-2.0`) is explicitly out of
scope for Slice 1 and will happen once the last legacy variant
has been re-derived.

## Author attestation

The Slice 1 material listed above under **Scope** was authored as a
clean-room re-derivation from the sources listed under **Sources
consulted**, without using the `../bsl-parser` project or any other
third-party SDBL grammar as working text. This attestation applies
at the date recorded at the top of the document.
