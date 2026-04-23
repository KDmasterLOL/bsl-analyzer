# MissingTemporaryFileDeletion provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule is grounded in public 1C guidance about explicit cleanup of temporary files created through platform APIs. The recommendation to remove temporary files after use comes from public standards and platform documentation, not from an original upstream idea.

## Public sources

- `#std542` File system access from application code.
- Public `v8-code-style` and `v8std.ru` pages for this rule, used only as secondary references.

## Implementation notes

The current implementation is local and HIR/CFG-based. It:

- finds `ПолучитьИмяВременногоФайла` / `GetTempFileName` calls;
- distinguishes assigned calls from inline usage;
- for assigned calls, searches for a reachable later cleanup call that uses the same variable;
- treats both deletion and move methods as cleanup according to the configurable `searchDeleteFileMethod` regex.

Inline usage is always reported because the cleanup target cannot be tracked reliably.

## Audit notes

- Rule idea: clean and public-guidance-based.
- The current behavior is narrower and more specific than a generic "all temporary files must be deleted" statement: it only tracks `GetTempFileName` usage and configured cleanup methods.
- Existing tests are local and cover default/extended/restrictive config, inline usage, bilingual names, case-insensitive matching, module-qualified cleanup methods, and CFG-aware reachability.
