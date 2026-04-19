# Provenance: DenyIncompleteValues

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in public 1C platform documentation about register
dimension properties and built-in completeness checks.

Primary sources:

- ITS `pubv8devui` chapter on fill checking
- 1C developer documentation for register dimension properties

The rule is metadata-based: when a register dimension should not accept empty
values, the platform flag `DenyIncompleteValues` is the built-in way to enforce
that constraint.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/deny_incomplete_values.rs` is local and
metadata-driven:

- it reads register metadata from the local module metadata model;
- it iterates dimensions and checks only the `deny_incomplete_values` flag;
- it formats the register type locally for the message.

This favors permissive treatment because the rule follows public platform
metadata semantics and the implementation is a straightforward local check.

### Documentation

Public documentation was rewritten during this audit to avoid inherited English
wording and to explain the rule in project-local language.

### Tests

Current tests are local and synthetic:

- dimension without the flag;
- dimension with the flag;
- multiple dimensions with mixed values;
- non-register metadata;
- disabled diagnostic;
- register type name formatting;
- short file range handling.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- the diagnostic idea overlaps with public `bsl-ls` material because the rule is
  a simple reflection of the same metadata property;
- repository history may still contain earlier upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`DenyIncompleteValues` is a good permissive candidate because:

- the rule follows public 1C metadata documentation;
- the current implementation is local and metadata-driven;
- the active docs and tests do not require retaining copyleft treatment.
