# Provenance: LatinAndCyrillicSymbolInWord

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic readability and correctness rule.

It warns about identifiers that mix Latin and Cyrillic characters in the same
word, which is a well-known source of confusion because some letters are
visually similar but belong to different alphabets.

There is no direct normative 1C standard source for this exact rule. The idea is
generic and language-agnostic.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/latin_and_cyrillic_symbol_in_word.rs` is
local and nontrivial:

- it performs its own mixed-script detection;
- it has a local optimization path for quick byte-level rejection;
- it checks several identifier categories in the syntax tree;
- it supports local configuration for excluded words and for allowed
  trailing-part patterns such as `HTTPСоединение`.

This strongly favors permissive treatment because the implementation is clearly
local and not tied to a public standard text.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
as a generic mixed-script readability diagnostic, without relying on borrowed
wording.

### Tests

Current tests are local inline Rust scenarios covering:

- mixed-script identifiers in different syntax positions;
- exclusion-list behavior;
- short identifiers that should be ignored;
- pure Cyrillic and pure Latin identifiers;
- allowed trailing-part mixed names.

## Important caveat

There is no strong public normative source to rely on here, so the legal basis
comes from independent local implementation rather than from “rule copied from a
standard”.

That is still a good position for permissive licensing, because generic ideas
and readability concerns are not exclusive to one upstream project.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`LatinAndCyrillicSymbolInWord` is a strong permissive candidate because it is a
generic readability rule with a clearly local implementation, local tests, and
rewritten documentation.
