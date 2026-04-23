# MultilingualStringUsingWithTemplate provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule is grounded in public localization guidance and the public behavior of `НСтр` / `NStr` and `СтрШаблон` / `StrTemplate`. The idea that a template string should exist for each required language is not specific to any upstream project.

## Public sources

- `#std763` Localization requirements.
- Public semantics of `НСтр` / `NStr` and `СтрШаблон` / `StrTemplate`.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and configuration-driven. It:

- reads required languages from the `declaredLanguages` setting via `NstrConfig`;
- scans syntax for `НСтр` / `NStr` calls;
- keeps only calls used directly in `СтрШаблон` / `StrTemplate`, or assigned to variables later used there;
- extracts language keys from the first literal argument and reports missing configured languages;
- reports empty-argument calls in template context as missing all declared languages.

This is a practical syntax-level rule for template-related multilingual strings, not a full validator for every `НСтр` usage.

## Audit notes

- Rule idea: clean and standards-based.
- Docs were corrected to match the real scope: template context only, plus indirect variable-to-template flow.
- Existing tests are local and cover `ru`-only mode, `ru,en` mode, direct and indirect template usage, empty `НСтр()`, and exclusion of `НСтр` outside template context.
