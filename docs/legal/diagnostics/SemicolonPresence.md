# SemicolonPresence provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule is based on public BSL syntax and public 1C style guidance for module texts. The requirement to delimit statements with semicolons is part of the language and common coding style, not an original upstream idea.

## Public sources

- Public BSL language semantics.
- `#std456` Module texts.
- `v8std.ru` language reference, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It:

- receives `BodyDiagnostic::MissingSemicolon` from AST → HIR lowering;
- reports only statements that reached lowering without a trailing semicolon;
- skips labels, empty statements, and statements that already contain parse errors;
- provides a local quick-fix that inserts `;`.

## Audit notes

- Rule idea: clean and language-based.
- Current behavior is intentionally narrow and syntax-oriented; it does not try to re-validate every parser edge case from scratch.
- Existing tests are local and cover regular statements, `EndIf`, labels, parse-error exclusions, and omitted semicolons before closing constructs.
