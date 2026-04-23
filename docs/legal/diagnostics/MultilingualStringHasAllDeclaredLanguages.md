# MultilingualStringHasAllDeclaredLanguages provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule is grounded in public localization guidance and the public semantics of `НСтр` / `NStr`. The idea that a multilingual string should provide text for each declared language is not specific to any upstream project.

## Public sources

- `#std763` Localization requirements.
- Public semantics of `НСтр` / `NStr`.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and configuration-driven. It:

- reads required languages from the `declaredLanguages` setting via `NstrConfig`;
- scans syntax tokens for `НСтр` / `NStr` calls;
- extracts language keys from the first literal argument;
- reports missing configured languages, including empty-argument cases;
- skips some `СтрШаблон` / `StrTemplate` patterns and variable flows to reduce false positives.

This means the current behavior is not a full semantic validator for all multilingual-string scenarios; it is a practical syntax-level rule with targeted exceptions.

## Audit notes

- Rule idea: clean and standards-based.
- Docs were expanded to reflect the real behavior: config-driven language set, default `ru`, and explicit template-related skips.
- Existing tests are local and cover `ru`-only mode, `ru,en` mode, empty `НСтр()`, malformed strings, direct and indirect `СтрШаблон` cases, and multiline literals.
