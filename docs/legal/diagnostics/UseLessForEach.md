# UseLessForEach provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic maintainability rule about pointless collection iteration. The idea that a `For Each` loop is suspicious when its iterator is never used is not specific to any upstream project.

## Public sources

- General loop-readability and maintainability guidance.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It:

- consumes lowering-time diagnostics for loops with an unused iterator;
- treats iterator property access, argument passing, assignments, conditions, and method calls as valid usage patterns;
- adds a local suppression when the iterator name collides with a module-level variable name, to avoid a known false-positive pattern.

## Audit notes

- Rule idea: clean and generic.
- Docs were expanded to describe the real scope instead of presenting the rule as a simplistic text check.
- Existing tests are local and cover unused iterators, valid usage patterns, chained access, conditional usage, and the module-variable-name suppression case.
