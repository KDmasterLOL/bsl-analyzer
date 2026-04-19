# Provenance: CommentedCode

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic follows the published 1C guidance on module text hygiene.

Primary source:

- ITS / v8std `#std456`

The rule is general and organizational: dead code, debugging leftovers, and
temporary service comments should not stay in committed module text.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/commented_code.rs`
is based on local token grouping and local heuristics:

- comment tokens are collected from the local syntax tree;
- consecutive comments are grouped using local line analysis;
- code-like fragments are detected through local text heuristics and
  configuration.

This supports permissive treatment because the rule is standards-based and the
current implementation is expressed through local token-oriented logic rather
than a copied parser visitor.

### Documentation

Both public documentation pages were rewritten during this audit to describe the
rule from code hygiene and standard requirements instead of inherited phrasing.

### Tests

Several test fixtures with characteristic business-specific names were replaced
with new local examples while preserving the same behavioral coverage:

- single commented assignment;
- multi-line commented block;
- commented procedure declaration;
- consecutive commented statements;
- range trimming around descriptive wrapper comments.

## Remaining caveats

- heuristic similarity to older implementations can still exist conceptually;
- repository history may still contain earlier upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CommentedCode` is a good permissive candidate because:

- the rule follows from published module-text guidance;
- the current implementation is local and token-based;
- the most obvious inherited docs and characteristic fixtures were replaced.
