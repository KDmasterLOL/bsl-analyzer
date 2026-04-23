# FunctionOutParameter provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic API and design rule. Avoiding output parameters in functions is a common readability and maintainability guideline, not a unique 1C-specific standard or original upstream concept.

## Public sources

- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It reports only when:

- the enclosing routine is a function;
- the parameter is passed by reference (no `Знач` / `Val`);
- the function directly assigns to that parameter name.

It does not report procedures, `Val` parameters, or property assignments on parameter objects.

## Audit notes

- Rule idea: clean and generic.
- Docs were rewritten to match the actual implementation scope.
- Existing tests are local and cover functions, procedures, `Val` parameters, case-insensitive matching, direct assignment only, and multiple violations.
