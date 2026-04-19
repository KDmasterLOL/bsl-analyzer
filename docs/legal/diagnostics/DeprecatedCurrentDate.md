# Provenance: DeprecatedCurrentDate

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in published 1C guidance about working across time
zones in client-server systems.

Primary source:

- ITS / v8std `#std643`

The rule is behavioral and platform-based: `CurrentDate()` / `ТекущаяДата()`
returns machine-local time, while `CurrentSessionDate()` /
`ТекущаяДатаСеанса()` aligns the value with the user session timezone.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/deprecated_current_date.rs` is local and
HIR-based:

- the handler receives a local HIR diagnostic when a deprecated global method
  is used;
- it formats the replacement message locally for Russian and English spellings;
- method calls on objects are excluded by the underlying HIR detection.

This favors permissive treatment because the rule follows public 1C timezone
guidance and the implementation is tied to local HIR infrastructure.

### Documentation

Public documentation was rewritten during this audit to avoid inherited English
wording and to explain the timezone rationale using public 1C materials.

### Tests

Current tests are local and inline, covering:

- deprecated Russian spelling;
- deprecated English spelling;
- exclusion of object methods;
- case-insensitive matching;
- mixed Russian and English procedures in one file.

During this audit, a provenance-oriented fixture comment was replaced with a
neutral description of the same scenario.

## Remaining caveats

- the diagnostic idea overlaps with public `bsl-ls` material because both tools
  reflect the same published 1C rule;
- deeper provenance of the underlying HIR detection still depends on the
  broader audit of `hir` and lowering logic;
- repository history may still contain earlier upstream-aligned wording.

## Conclusion

`DeprecatedCurrentDate` is a good permissive candidate because:

- the rule directly follows from public `#std643`;
- the current implementation is local and HIR-based;
- the active docs and tests do not require retaining copyleft treatment.
