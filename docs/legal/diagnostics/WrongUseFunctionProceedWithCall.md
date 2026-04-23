# WrongUseFunctionProceedWithCall provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

The rule follows public 1C extension semantics. `ПродолжитьВызов` / `ProceedWithCall` is tied to interception methods in configuration extensions, so using it outside the supported extension context is a platform misuse rather than an original upstream idea.

## Public sources

- 1C documentation on configuration extensions and module annotations.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based:

- the diagnostic is emitted during lowering when the code sees a global call to `ПродолжитьВызов` / `ProceedWithCall`;
- the enclosing method must be marked with `&Вместо`;
- calls from ordinary methods and from `&Перед` / `&После` methods are reported.

This implementation does not depend on parser-derived query logic, copied tests, or borrowed metadata fixtures.

## Audit notes

- Rule idea: clean.
- Docs were rewritten to match the actual implementation scope.
- Existing tests are local and exercise the project's own HIR-based behavior.
