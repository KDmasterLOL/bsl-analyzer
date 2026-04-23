# Provenance: InvalidCharacterInFile

## Status

Good candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in public 1C guidance on module texts.

Primary source:

- ITS / v8std `#std456`: module texts

The rule rationale is straightforward and public: module source should not
contain confusing or invalid text characters that break readability, search, or
syntax.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/invalid_character_in_file.rs` is local and
token-based:

- it scans parsed tokens from the local syntax tree;
- it checks a local list of forbidden dash-like Unicode characters plus
  non-breaking space;
- it emits different messages for invalid dash characters and invalid spaces.

This favors permissive treatment because the implementation is local and the
rule basis comes from public style guidance.

### Important scope caveat

Current implementation is narrower than the broad wording of `#std456`.

It currently scans only `STRING`, `COMMENT`, and `ERROR` tokens, not every
possible character position in the file. So the standard basis is strong, but
the exact detection surface is the local `bsl-analyzer` implementation.

### Documentation

Local RU/EN documentation was rewritten during this audit to cite `#std456`
directly and to describe the detected character classes more precisely.

### Tests

Current tests are local inline Rust scenarios covering:

- all supported illegal dash variants;
- non-breaking spaces in comments and strings;
- mixed invalid characters;
- correct ordinary hyphen-minus and space cases.

The tests are local and do not depend on an external fixture file.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  parser / syntax infrastructure.

## Conclusion

`InvalidCharacterInFile` is a good permissive candidate because it is grounded
in a public 1C text-formatting standard and the current implementation/docs/tests
are local, with the main caveat being that the exact detection surface is
implementation-specific.
