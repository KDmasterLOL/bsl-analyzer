# SelfAssign provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic static-analysis rule about assignments that have no effect. The idea that `a = a` or `Obj.X = Obj.X` is suspicious is not specific to any upstream project.

## Public sources

- Public BSL assignment semantics.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It:

- consumes `BodyDiagnostic::SelfAssign` emitted during lowering;
- reports self-assignment of simple paths and property paths;
- matches identifiers case-insensitively, consistent with BSL semantics.

This handler is intentionally thin because the actual detection lives in local HIR lowering.

## Audit notes

- Rule idea: clean and generic.
- Docs were expanded to match the real scope of the implementation instead of only mentioning variable-to-itself assignments.
- Existing tests are local and cover simple self-assignment, case-insensitive matching, property self-assignment, and non-self cases.
