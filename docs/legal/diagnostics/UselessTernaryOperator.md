# UselessTernaryOperator provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic simplification rule for boolean expressions. The idea that a ternary operator becomes redundant when it only wraps boolean constants is not specific to any upstream project.

## Public sources

- General simplification guidance for boolean expressions.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and syntax-based. It:

- traverses AST ternary nodes directly;
- reports ternaries whose condition is a boolean literal;
- also reports ternaries where both branches are boolean literals;
- intentionally skips mixed cases where only one branch is boolean.

This is a small, targeted simplification rule rather than a full boolean-equivalence analyzer.

## Audit notes

- Rule idea: clean and generic.
- Docs were corrected to match the real implementation scope and explicitly document what it does not catch.
- Existing tests are local and cover direct/inverted useless ternaries, boolean-literal conditions, valid mixed cases, and the broader regression fixture.
