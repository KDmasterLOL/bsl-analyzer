# FunctionReturnsSamePrimitive provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic API and design rule. Flagging functions that always return the same primitive literal is a common maintainability idea, not a unique 1C-specific standard or original upstream concept.

## Public sources

- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It reports only when:

- the function has multiple return statements;
- all collected returns are the same primitive literal;
- the function is not excluded by the local attachable-function heuristic.

It does not report functions that return the same variable or the same computed expression.

## Audit notes

- Rule idea: clean and generic.
- Docs were rewritten to match the actual implementation scope.
- Existing tests are local and cover booleans, strings, numbers, `Null`, case-insensitive literal matching, single-return functions, variable returns, and attachable-function exclusion.
