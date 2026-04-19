# Provenance: DeprecatedFind

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in published 1C migration guidance for platform
version 8.3.

Primary source:

- 1C migration guide section on method and property renames

That guidance explicitly lists the global method rename:

- `Найти` / `Find` -> `СтрНайти` / `StrFind`

The rule is API-based: the old global method remains for compatibility, but new
code should use the replacement string-search method.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/deprecated_find.rs` is local and HIR-based:

- the handler receives a local HIR diagnostic when the deprecated global method
  is used;
- it formats replacement text locally for Russian and English spellings;
- collection method calls such as `Collection.Find(...)` are excluded by the
  underlying HIR detection.

This favors permissive treatment because the rule follows published platform
migration guidance and the implementation is tied to local HIR infrastructure.

### Documentation

Public documentation was rewritten during this audit to avoid inherited English
wording and to explain the ambiguity of the deprecated global method.

### Tests

Current tests are local and inline, covering:

- deprecated Russian spelling;
- deprecated English spelling;
- exclusion of collection methods;
- case-insensitive matching;
- deprecated calls both inside a procedure and at module top-level.

During this audit, a provenance-oriented fixture comment was replaced with a
neutral description of the same scenario.

## Remaining caveats

- the diagnostic idea overlaps with public `bsl-ls` material because both tools
  reflect the same published platform migration rule;
- deeper provenance of the underlying HIR detection still depends on the
  broader audit of `hir` and lowering logic;
- repository history may still contain earlier upstream-aligned wording.

## Conclusion

`DeprecatedFind` is a good permissive candidate because:

- the rule directly follows from published 1C migration guidance;
- the current implementation is local and HIR-based;
- the active docs and tests do not require retaining copyleft treatment.
