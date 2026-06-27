# Provenance: DeprecatedMessage

## Status

Candidate for `MIT OR Apache-2.0`.

Historical note (2026-06-27): public diagnostic code `DeprecatedMessage` is no
longer active; it is folded into `DeprecatedPlatformApi`. This file is retained
as legal provenance history for the folded implementation.

## Why this rule exists

This diagnostic is grounded in published 1C guidance about user notifications.

Primary source:

- ITS / v8std `#std418`

The rule is API- and UX-based: instead of the global method
`Сообщить()` / `Message()`, user-facing messages should be emitted through the
`СообщениеПользователю` / `UserMessage` object or standard-library helpers
built on top of it.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/deprecated_message.rs` is local and
HIR-based:

- the handler receives a local HIR diagnostic when the deprecated global method
  is used;
- it formats replacement text locally for Russian and English spellings;
- object method calls are excluded by the underlying HIR detection.

This favors permissive treatment because the rule follows public 1C standards
and the implementation is tied to local HIR infrastructure.

### Documentation

Public documentation was rewritten during this audit to avoid inherited English
wording and to explain the preferred `UserMessage`-based approach in local
terms.

### Tests

Current tests are local and inline, covering:

- deprecated Russian spelling;
- deprecated English spelling;
- exclusion of object methods;
- case-insensitive matching;
- deprecated calls inside blocks and at module level.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- the diagnostic idea overlaps with public `bsl-ls` material because both tools
  reflect the same published 1C standard;
- deeper provenance of the underlying HIR detection still depends on the
  broader audit of `hir` and lowering logic;
- repository history may still contain earlier upstream-aligned wording.

## Conclusion

`DeprecatedMessage` is a good permissive candidate because:

- the rule directly follows from public `#std418`;
- the current implementation is local and HIR-based;
- the active docs and tests do not require retaining copyleft treatment.
