# Provenance: DeprecatedAttributes8312

## Status

Candidate for `MIT OR Apache-2.0`.

Historical note (2026-06-27): public diagnostic code `DeprecatedAttributes8312`
is no longer active; it is folded into `DeprecatedPlatformApi`. This file is
retained as legal provenance history for the folded implementation.

## Why this rule exists

This diagnostic is grounded in public platform change documentation for
1C:Enterprise `8.3.12`.

Primary source:

- official 8.3.12 platform changelog

The rule is version- and API-based: it flags names that the platform itself
marked as deprecated and suggests newer replacements.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/deprecated_attributes_8312.rs` is local
and HIR-based:

- deprecated names are classified through local `hir::DeprecatedKind8312`;
- replacement text is maintained in a local lookup map;
- the handler only formats user-facing diagnostics from local HIR findings.

This favors permissive treatment because the rule follows public platform API
changes and the implementation is tied to local HIR infrastructure.

### Documentation

Public documentation was rewritten during this audit to avoid inherited English
wording and to describe the rule as a migration aid for the public 8.3.12 API
changes.

### Tests

Current tests are local and inline, covering:

- deprecated chart plot area attributes in Russian and English;
- chart attributes and methods;
- deprecated global methods;
- deprecated enum names and enum values.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- the catalog of deprecated names naturally overlaps with public `bsl-ls`
  material because both tools reflect the same 8.3.12 platform changes;
- deeper provenance of the underlying HIR detection still depends on the
  broader audit of `hir` and lowering logic;
- repository history may still contain earlier upstream-aligned wording.

## Conclusion

`DeprecatedAttributes8312` is a good permissive candidate because:

- the rule directly follows from public platform deprecations;
- the current implementation is local and HIR-based;
- the active docs and tests do not require retaining copyleft treatment.
