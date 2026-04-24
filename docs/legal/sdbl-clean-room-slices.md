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

### Goal

Separate the most provenance-sensitive catalogs into their own owned tables.

### Scope

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

### Goal

Rebuild the smallest useful `SELECT` body.

### Scope

- selected fields
- asterisk fields
- aliases
- `INTO` / `ПОМЕСТИТЬ`

### Files

- `crates/parser/src/grammar/sdbl/select.rs`
- tests that power `AssignAliasFieldsInQuery`

## Slice 8: FROM sources and source chains

### Goal

Rebuild source parsing independently from full expression complexity.

### Scope

- table references
- subqueries in `FROM`
- parameter sources
- source aliases

### Files

- `crates/parser/src/grammar/sdbl/select.rs`

## Slice 9: JOIN family

### Goal

Rebuild join parsing as a dedicated isolated subsystem.

### Scope

- join modifiers
- join chaining
- `ON` / `ПО`
- join-source attachment rules

### Files

- `crates/parser/src/grammar/sdbl/select.rs`

## Slice 10: expression minimum

### Goal

Rebuild only the expression surface needed for practical query parsing.

### Scope

- literals
- identifiers
- qualified names
- unary operators
- binary operator precedence
- function calls
- parenthesized expressions
- CASE expression

### Files

- `crates/parser/src/grammar/sdbl/expressions.rs`

### Notes

This slice should intentionally target a local precedence parser or another
explicit local shape, not an ANTLR-style reproduction.

## Slice 11: clauses after FROM

### Goal

Rebuild trailing query clauses once field/source/expression foundations are in
place.

### Scope

- `WHERE`
- `GROUP BY`
- `HAVING`
- `ORDER BY`
- `AUTOORDER`
- `TOTALS ... BY`
- `FOR UPDATE`
- `INDEX BY`

### Files

- `crates/parser/src/grammar/sdbl/select.rs`

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
