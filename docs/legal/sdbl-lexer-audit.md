# SDBL Lexer Audit

> **Superseded on current state — retained as historical record.** This note was
> written in April 2026 without access to the upstream grammar. Its conclusion
> that the token inventory was derived is now proven from this repository's own
> history. Its risk assessment of the code describes the tree before Slices 1,
> 2, 2-addendum and 3a landed and overstates what remains today. See
> `sdbl-provenance-2026-07-audit.md`.

## Scope

This note records a focused provenance assessment for:

- `crates/lexer/src/sdbl.rs`

The upstream comparison baseline is:

- `../bsl-parser/src/main/antlr/SDBLLexer.g4`

## High-level conclusion

`crates/lexer/src/sdbl.rs` currently looks like a **high-risk derived layer**.

It is not a literal ANTLR copy, because:

- it is implemented with `logos` and custom Rust code rather than ANTLR;
- many token names are collapsed or simplified;
- string handling is partly custom and not rule-for-rule identical.

However, the overall evidence still points to substantial derivation from the
upstream lexer work:

- the bilingual token inventory is very close in scope;
- several token families and special lexer states clearly mirror ANTLR modes;
- the concrete organization of metadata object types, virtual tables, functions,
  and special contexts is too specific to treat as an independently invented
  catalog by default.

## Main evidence of upstream dependence

## 1. Token vocabulary overlap

The local lexer contains the same broad token families as `SDBLLexer.g4`:

- core query clauses;
- logical and comparison operators;
- CASE / WHEN / THEN / ELSE / END;
- aggregate and date/time functions;
- metadata object types;
- virtual table suffixes;
- period types;
- literals and parameters;
- temporary-table and brace/dot special handling.

At the idea level, individual keywords are not protectable by themselves.
But the **selection, categorization, and organization** of such a large
bilingual token universe is still a provenance signal.

## 2. Special lexer states closely track ANTLR modes

Upstream `SDBLLexer.g4` uses multiple modes:

- `STRINGS`
- `PARAMETER_MODE`
- `DOT_MODE`
- `BRACE_MODE`
- `ID_MODE`
- `EXTERNAL_DATA_SOURCE_MODE`

The local lexer does not reuse ANTLR syntax, but it still reproduces the same
special-context design in practical terms:

- custom string tokenization outside normal logos flow;
- parameter token handling;
- dot-sensitive virtual table / property handling;
- brace handling;
- identifier-after-keyword handling;
- external data source mode.

That level of structural overlap is one of the strongest provenance signals in
this file.

## 3. Highly specific token categories

Several categories go beyond “obvious SQL keywords” and point to prior
grammar-driven vocabulary work:

- metadata object kinds (`Catalog`, `Document`, `ChartOfAccounts`, etc.);
- virtual tables (`SliceFirst`, `BalanceAndTurnovers`, `DrCrTurnovers`, etc.);
- type literals and special functions (`EmptyRef`, `RefPresentation`, etc.);
- period names and query-specific helper functions.

This is exactly the kind of inventory that is expensive to assemble from scratch
and therefore difficult to treat as presumptively independent.

## Local transformations and favorable signals

Despite the high-risk conclusion, this file also contains real local work.

## 1. `logos`-based implementation

The lexer is not an ANTLR grammar transcription. It is expressed as:

- a Rust enum with `#[regex]` / `#[token]` attributes;
- case-insensitive regexes for Russian and English variants;
- plain Rust token structs and tokenization helpers.

This is a real implementation rewrite, even if the vocabulary likely came from
upstream work.

## 2. Deliberate simplifications

The local lexer intentionally collapses or merges some distinctions, for example:

- `KwOnOrBy` merges `ON` / `BY` / `ПО`;
- many downstream parser distinctions are deferred to parser logic instead of
  being encoded as separate lexer states or token rules;
- some period/function ambiguities are handled via priority and parser context.

Those are local design choices, not a direct ANTLR port.

## 3. Custom string handling

`tokenize_sdbl` and `tokenize_strings_mode` implement string tokenization
outside the normal logos flow.

This looks local and pragmatic:

- it supports the way SDBL is extracted from BSL multiline strings;
- it intentionally differs from a straight ANTLR-style `STRINGS` mode;
- tests document behavior around embedded newlines and split string fragments.

## 4. Explicitly non-standard convenience behavior

The local lexer accepts line comments even though SDBL itself does not formally
define them, and the file comments explicitly say this is done for robustness
and developer convenience.

That is another local policy choice rather than inherited grammar text.

## Assessment by sublayer

### Token inventory

Current assessment: **high risk**

Reason:

- the bilingual vocabulary and category breakdown are too close to upstream
  lexer work to treat as clean by default.

### Mode / context design

Current assessment: **high risk**

Reason:

- the same special contexts appear in both implementations;
- this is one of the strongest structural overlaps with `SDBLLexer.g4`.

### Rust implementation mechanics

Current assessment: **medium risk**

Reason:

- actual Rust code, `logos` annotations, custom helper functions, and string
  splitting logic are local implementation work;
- but they sit on top of a likely derived token inventory.

## Practical licensing conclusion

Today, `crates/lexer/src/sdbl.rs` should remain in the **copyleft-risk bucket**.

The right way to think about it is:

- **implementation form:** partly local
- **token universe and structure:** still likely derived

So this file is not a good immediate `MIT OR Apache-2.0` candidate.

## Best future rewrite strategy

If the goal is a clean-room replacement, the rewrite should preserve only the
minimum functional intent:

1. define a new SDBL token inventory from primary 1C language behavior and
   independently authored test cases;
2. avoid using `SDBLLexer.g4` as the working text during the rewrite;
3. keep useful local ideas that are not themselves provenance-heavy:
   - `logos` implementation style,
   - `KwOnOrBy`-style pragmatic token merging if still desired,
   - custom string extraction flow,
   - explicit convenience handling for extracted query text.

## Bottom line

`crates/lexer/src/sdbl.rs` is not a raw copy of upstream ANTLR code, but it is
still too close in vocabulary structure and context design to call it clean.

For licensing purposes, it should be treated as one of the main remaining
copyleft blockers alongside the SDBL grammar files in `crates/parser`.
