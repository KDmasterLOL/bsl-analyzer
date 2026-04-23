# DoubleNegatives provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic readability and refactoring rule. Detecting double negatives is a well-known code-quality idea and does not depend on any unique 1C-specific standard or original upstream concept.

## Public sources

- Martin Fowler's refactoring catalog entry "Remove Double Negative".

## Implementation notes

The current implementation is local and AST-based. It reports three structural patterns:

- `НЕ (НЕ X)`
- `НЕ (X <> Y)`
- `(НЕ X) <> Y`

It also applies local filters:

- expressions containing logical `И` / `ИЛИ` inside the candidate are skipped;
- one parse-error-shaped edge case ending with `=` is skipped.

## Audit notes

- Rule idea: clean and generic.
- Docs were rewritten to match the actual implementation scope and filters.
- Existing tests are local and cover both positive detections and deliberate skips.
