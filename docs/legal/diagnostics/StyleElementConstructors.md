# StyleElementConstructors provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule is grounded in public 1C style guidance about using style elements instead of hardcoded visual constructors. The idea is standards-based and not specific to any upstream project.

## Public sources

- `#std667` Style elements.
- Public `v8-code-style` guidance for `new-color` / `new-font`.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It:

- consumes lowering-time diagnostics for style-related constructors;
- reports direct constructor calls for `Color`, `Font`, and `Border`;
- reports both typed constructors and string-based constructor forms like `New("Color", ...)`;
- also catches nested constructor usage inside other expressions.

This is a focused detector for a small set of style-constructor patterns, not a full validator of all UI styling practices.

## Audit notes

- Rule idea: clean and standards-based.
- Docs were expanded to describe the actual narrow constructor set the implementation detects.
- Existing tests are local and cover Russian and English constructor names, string-based constructors, nested constructors, and non-style constructors that should not trigger.
