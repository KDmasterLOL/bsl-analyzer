# ThisObjectAssign provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule is based on public platform semantics: `ЭтотОбъект` / `ThisObject` is a built-in read-only property in the relevant module contexts. The idea that assigning to a read-only built-in property is an error is not specific to any upstream project.

## Public sources

- Public platform semantics of `ThisObject` in compatibility mode `8.3.3+`.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It:

- consumes `BodyDiagnostic::ThisObjectAssign` from lowering;
- reports only direct assignment to `ЭтотОбъект` / `ThisObject`;
- is limited by metadata to common modules and form modules.

Property writes such as `ЭтотОбъект.Реквизит = ...` are intentionally outside the scope of this rule.

## Audit notes

- Rule idea: clean and platform-semantics-based.
- Docs were rewritten to distinguish this rule from other `ThisObject`-related style diagnostics such as redundant self-reference.
- Existing tests are local and cover Russian/English names, case-insensitive matching, direct assignment, and excluded property access.
