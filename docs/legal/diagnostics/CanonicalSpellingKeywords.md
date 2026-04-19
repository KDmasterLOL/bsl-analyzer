# Provenance: CanonicalSpellingKeywords

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic follows directly from official 1C language style guidance.

Primary sources:

- ITS / v8std `#std441`: canonical spelling of built-in language keywords
- public diagnostic mapping `ACC 1248`

The canonical forms themselves are language facts: they come from the platform
syntax and documentation, not from a project-specific invention.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/canonical_spelling_keywords.rs`
is a local token-based checker over the project's own syntax tree.

This strongly favors permissive treatment:

- the rule is fully standards-based;
- the implementation is straightforward local token inspection;
- fixes are generated through local edit infrastructure.

### Documentation

Local documentation was updated during this audit to reference official and
public 1C sources directly.

The keyword tables are retained as a reference list of canonical language forms.
Their content is best understood as language specification data rather than as
creative project-specific material.

### Tests

Current local tests are granular token-level scenarios expressed directly in the
Rust test module. They do not rely on a copied upstream fixture file.

## Remaining caveats

- earlier repository history may still contain wording close to upstream docs;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CanonicalSpellingKeywords` is one of the clearest permissive candidates because:

- the rule comes straight from official 1C language guidance;
- the canonical forms are properties of the language itself;
- the current implementation and tests are local.
