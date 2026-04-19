# Provenance: CachedPublic

## Status

Candidate for `MIT OR Apache-2.0` after cleanup of inherited docs/examples.

## Why this rule exists

This diagnostic follows directly from official 1C guidance on library
compatibility.

Primary standard:

- ITS / v8std `#std644`, section `3.6`

The rule is not about caching mechanics alone. It is about keeping the public
library interface in an ordinary common module so that consumers do not depend on
an implementation detail such as a specialized cached module.

## Audit result

### Production code

Current implementation in `crates/ide-diagnostics/src/handlers/cached_public.rs`
uses local metadata, region-tree, and item-tree infrastructure.

This favors permissive treatment of the current file:

- the logic is expressed in terms of local HIR/metadata queries;
- the rule is simple and standards-based;
- the current file no longer contains explicit `ported from` wording.

### Documentation

The local English documentation previously mirrored upstream wording very
closely. During this audit it was rewritten around the official 1C rationale.

Russian documentation already followed the same standard-based idea closely and
was updated with a public `v8std.ru` reference.

### Tests

The previous local tests reused the same small region fixture structure as the
upstream `CachedPublicDiagnostic.bsl` resource.

During this audit, that fixture was replaced with new local region names and
method names while preserving the same behavioral coverage.

## Remaining caveats

- earlier repository history still contains upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CachedPublic` is a good permissive candidate because:

- the rule is grounded in official 1C compatibility guidance;
- the current implementation is expressed through local analysis infrastructure;
- the most obvious borrowed docs and fixture text were replaced.
