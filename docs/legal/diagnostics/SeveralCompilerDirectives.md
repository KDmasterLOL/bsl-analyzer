# SeveralCompilerDirectives provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule is based on public BSL syntax and compiler-directive semantics. The idea that one module item should not carry multiple compilation directives is a language/platform rule, not an original upstream invention.

## Public sources

- Public BSL syntax and compiler-directive semantics.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and item-tree based. It:

- iterates top-level procedures, functions, and variables;
- checks the number of collected annotations for each item;
- reports any item with more than one directive annotation.

This is a straightforward syntax-level detector rather than a complex semantic rule.

## Audit notes

- Rule idea: clean and language-based.
- Docs were updated to match the real implementation scope, including the fact that comments and blank lines between directives do not matter.
- Existing tests are local and cover variables and methods with duplicated directives, mixed directives, single-directive cases, and no-directive cases.
