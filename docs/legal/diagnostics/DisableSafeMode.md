# DisableSafeMode provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule follows public 1C security guidance around safe mode. Detecting calls that disable safe mode or explicitly allow its disabling is a security policy derived from public platform behavior and standards, not an original upstream idea.

## Public sources

- 1C developer documentation on safe mode.
- `#std669` Restriction on execution of external code.
- `#std678` Server API security.
- `#std770` Restrictions on the use of Execute and Eval on the server.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It reports global calls to:

- `УстановитьБезопасныйРежим` / `SetSafeMode` when the argument is not statically `Истина` / `True`;
- `УстановитьОтключениеБезопасногоРежима` / `SetSafeModeDisabled` when the argument is not statically `Ложь` / `False`.

Object-qualified calls are intentionally ignored by the current logic.

## Audit notes

- Rule idea: clean and security-driven.
- Docs were rewritten to match the actual implementation behavior.
- Existing tests are local and cover direct, variable-based, bilingual, and qualified-call scenarios.
