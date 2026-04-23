# MissingReturnedValueDescription provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule directly follows public 1C documentation standards. `#std453` explicitly describes the `Возвращаемое значение` section for functions and its absence for procedures, so the diagnostic idea is standards-based rather than an original upstream invention.

## Public sources

- `#std453` Procedures and functions description.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR/doc-comment-based. It validates:

- missing returned-value documentation for export functions;
- unexpected returned-value documentation on procedures;
- missing text descriptions for returned types when `allowShortDescriptionReturnValues = false`.

It also skips hyperlink-style comments such as `См. ДругойМетод()`.

## Audit notes

- Rule idea: clean and standards-based.
- Docs were rewritten to capture the export-function scope and strict-mode nuance.
- Existing tests are local and cover export vs non-export functions, procedures, hyperlink references, strict mode, multiple returned types, and compatibility cases.
