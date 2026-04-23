# MissingParameterDescription provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule directly follows public 1C documentation standards. `#std453` explicitly describes how method parameters should be documented, so the diagnostic idea is standards-based rather than an original upstream invention.

## Public sources

- `#std453` Procedures and functions description.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and metadata/comment-based. It validates:

- missing parameter descriptions;
- extra descriptions for nonexistent parameters;
- duplicate descriptions;
- incorrect description order.

It also skips hyperlink-style comments such as `См. ДругойМетод()`.

## Audit notes

- Rule idea: clean and standards-based.
- Docs were rewritten to match the actual validation scope.
- Existing tests are local and cover missing docs, extra docs, duplicates, ordering, case-insensitive matching, hyperlink references, and a large compatibility fixture.
