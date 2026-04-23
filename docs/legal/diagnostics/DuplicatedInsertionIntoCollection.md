# DuplicatedInsertionIntoCollection provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic bug-pattern and readability rule. Detecting repeated insertion of the same value or key into a collection does not rely on a unique 1C-specific standard or on an original upstream concept.

## Public sources

- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It uses its own post-lowering analysis:

- tracks collection insertions per body;
- hashes receiver and argument expressions structurally;
- tracks variable generations to distinguish old and reassigned values;
- treats `Add` and `Insert` differently;
- applies local flow heuristics around `return`, `raise`, loop breaks, and nested scopes.

This is not a parser-port or a thin translation of an upstream visitor.

## Audit notes

- Rule idea: clean and generic.
- EN docs were written from scratch; RU docs were updated to match the actual implementation details.
- Existing tests are local and cover control-flow, reassignment, preprocessor, special literals, and configuration behavior.
