# ReservedParameterNames provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule is based on public naming guidance and ordinary scope-shadowing behavior in BSL. The idea that a parameter should not hide a reserved platform name is not specific to any upstream project.

## Public sources

- `#std640` Procedure and function parameters.
- `#std454` Rules for generating variable names.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and configuration-driven. It:

- walks top-level procedures and functions from the item tree;
- checks parameter names against the configured `reservedWords` array;
- compares names case-insensitively;
- requires exact matches rather than partial matches.

If `reservedWords` is empty, the rule intentionally produces no diagnostics.

## Audit notes

- Rule idea: clean and standards-based.
- Docs were corrected to match the real implementation: the code uses a configured string array, not a regex.
- Existing tests are local and cover empty config, exact matches, case-insensitive matching, multiple words, partial non-matches, and function parameters.
