# YoLetterUsage provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule directly follows public 1C module text guidelines. The requirement to avoid the Russian letter `ё` in source code comes from `#std456`, so the diagnostic idea is standards-based rather than an original upstream invention.

## Public sources

- `#std456` Module texts.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and token-based:

- it scans syntax tokens;
- it reports only `IDENT` tokens containing `ё` or `Ё`;
- it ignores string literals, comments, and other non-identifier text.

This is a local implementation detail and narrower than a generic "search for `ё` everywhere" rule.

## Audit notes

- Rule idea: clean and standards-based.
- Docs were rewritten to match the actual implementation scope.
- Existing tests are local and exercise identifier-only behavior.
