# Track 6.1 — Closure document

Closure record for Track 6.1 «Parser UX»
(architectural mini-track inside ROADMAP §Track 6).

## Status

- **Status:** CLOSED.
- **Date:** 2026-05-13.
- **Scope:** Structured BSL parse errors (`ParseError`) and precise SDBL
  query parse ranges (`QueryParseError`) landed across parser, syntax,
  hir-def, and ide-diagnostics.
- **Test count at closure:** parser tests: 561; ide-diagnostics tests: 1717.
- **Gate:** all post-D.2 test suites were green before this closure slice.

## Scope and motivation

Track 6.1 replaced generic parser diagnostics with structured error payloads.
The old BSL handler rendered the hardcoded message
`Ошибка разбора исходного кода`. The old SDBL handler rendered the hardcoded
message `Текст запроса содержит ошибки` and placed diagnostics on the whole
BSL literal.

The landed behavior now preserves parser intent:

- BSL parse diagnostics render `ParseError::format_ru()` output.
- SDBL parse diagnostics render the same structured payload style.
- Query ranges are projected back from SDBL text into BSL-literal coordinates.
- IDE diagnostics point at the offending token or missing-token insertion
  point, not the full query literal.

This closure document describes the implementation that landed, not the
original plan text.

## Architecture summary

- New crate `parser-error` owns `ParseError` and `RecoveryKind`.
- `RecoveryKind` models range semantics: `BumpToken`, `MissingToken`,
  `RecoverySpan`, and `Custom`.
- Parser events carry structured payloads through `Event::Error` and
  `Event::ErrorWithSpan`.
- `SyntaxError` stores the structured payload in the syntax tree error stream.
- Parser sink computes ranges with `compute_error_range` based on
  `RecoveryKind`.
- BSL grammar emits via `error_unexpected`, `error_custom`, and
  `emit_error_at_marker`.
- SDBL grammar emits the same structured payloads instead of bare `p.error()`
  or direct `NodeKind::Error` recovery markers.
- Inverse SDBL-to-BSL projection lives in
  `syntax::sdbl_query::map_range_query_to_literal`.
- HIR lowering populates `SdblQueryInfo::error_ranges_in_bsl`.
- `parse_error` consumes `parse.errors()` and renders
  `ParseError::format_ru()`.
- `query_parse_error` consumes `error_ranges_in_bsl` and emits precise
  BSL-coordinate diagnostics.

## Slice-by-slice summary

| Slice | Commit(s) | Landed result |
|---|---|---|
| A.1 | `8ddf8ccb` | Introduced `parser-error` crate with `ParseError` and `RecoveryKind`. |
| A.2 | `703300c4` | Added `Event::Error` and `Event::ErrorWithSpan` variants. |
| A.3 | `565918fb` | Rewired `SyntaxError` as the structured carrier through syntax. |
| B.1 | `f762aec7` | Upgraded `Parser::expect` and added structured helper facade. |
| B.2 | `d3f1543c`, `f1fc00ff` | Wired sink event-loop range computation and fixed parser-test bandaids honestly. |
| B.3 | `512adc79`, `21199d55` | Migrated 13 BSL grammar sites to structured emit; follow-up fixed bare-ident recovery wording. |
| C.1 | `a019462a`, `7d4ba69a` | Migrated 25 SDBL grammar sites; follow-up added empty-span guards and EOF-aware custom recovery. |
| C.2 | `ee69cddf`, `a5223483` | Added and polished `map_range_query_to_literal` inverse projection helper. |
| C.3 attempt | `90bf5427` -> `66a63ebd` | Initial unprompted implementation was reverted. |
| C.3 redo | `65202959` | Populated `error_ranges_in_bsl` with `(TextRange, ParseError)` and trailing-dot synthetic detection. |
| D.1 | `78c208e8` | Switched `parse_error` handler to structured rendering. |
| D.2 | `a040b9a1` | Switched `query_parse_error` handler to precise BSL ranges and rebased snapshots. |

## Handler outcomes

`ParseError` audit-card requirement:

- structured expected-token errors landed;
- actual-token reporting is represented in `Expected` / `Unexpected`;
- recovery taxonomy controls range shape;
- handler no longer scans `ERROR` nodes for the primary diagnostic surface.

`QueryParseError` audit-card requirement:

- SDBL parser errors now keep structured payloads;
- ranges are projected from query text to literal coordinates;
- trailing-dot synthetic query errors are emitted during lowering;
- handler no longer performs consumer-side trailing-dot detection;
- 9 affected snapshots were rebased, each new range being a sub-range of the
  old whole-literal diagnostic range.

## Coordination retrospective

Planning had 4 rounds of plan-level Codex review. During implementation,
8 of 12 slices received per-slice adversarial review.

One coordination incident is part of the closure record: commit `90bf5427`
implemented C.3 without an explicit request and introduced a type-signature
regression. It was reverted by `66a63ebd` and redone correctly as
`65202959`.

Lesson: every Codex dispatch needs an explicit scope fence.

## Known plan-vs-implementation discrepancy

Plan §3.1.1 showed the example message
`Ожидалось 'Тогда', встречено: конец файла` with a colon after
`встречено`. The actual `parser_error::format_ru()` source of truth renders
single-expected errors as:

`Ожидалось 'Тогда', встречено конец файла`

The implementation is authoritative. The plan example was cosmetically
inaccurate.

## Out of scope

Deferred from Track 6.1:

- bilingual diagnostic messages (RU/EN);
- quick-fixes;
- LSP `relatedInformation`.

## Acceptance evidence

| Gate | Result |
|---|---|
| Parser structured payload foundation exists | done: `parser-error`, parser events, `SyntaxError` |
| BSL parse diagnostics use structured rendering | done: `parse.errors()` + `ParseError::format_ru()` |
| SDBL query diagnostics use precise BSL ranges | done: `error_ranges_in_bsl` |
| Parser tests | 561 green post-D.2 |
| ide-diagnostics tests | 1717 green post-D.2 |
| Closure docs | this Phase E.1 commit |
