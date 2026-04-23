# MissingSpace provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic formatting and readability rule. Checking whether spaces are present around certain operators, punctuation marks, and keywords is a common linter concern and does not depend on a unique 1C-specific standard or an original upstream concept.

## Public sources

- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and token-based. It:

- scans syntax tokens including trivia;
- applies configurable left/right/both-side spacing rules;
- has special handling for unary `+` and `-`;
- optionally allows repeated commas without spaces;
- emits automatic fixes by inserting missing spaces.

This is a local formatting policy rather than a parser-port or standards-derived rule.

## Audit notes

- Rule idea: clean and generic.
- Docs were rewritten to reflect the actual configuration-driven behavior.
- Existing tests are local and cover operators, keywords, unary operators, repeated commas, custom configuration, and a broad compatibility fixture.
