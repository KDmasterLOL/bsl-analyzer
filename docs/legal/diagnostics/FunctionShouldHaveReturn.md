# FunctionShouldHaveReturn provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule follows basic BSL language semantics. A function is expected to return a value, so flagging a function with no `Возврат` / `Return` at all is not a unique upstream idea but a direct consequence of the language model.

## Public sources

- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It reports only when:

- the routine is a function;
- no `Возврат` / `Return` statements were found in it.

It does not guarantee that all control-flow paths return a value; that concern is handled by separate diagnostics.

## Audit notes

- Rule idea: clean and language-semantic.
- Docs were rewritten to distinguish this rule from path-completeness checks.
- Existing tests are local and cover functions with and without returns, procedures, conditional returns, multiple routines, and English syntax.
