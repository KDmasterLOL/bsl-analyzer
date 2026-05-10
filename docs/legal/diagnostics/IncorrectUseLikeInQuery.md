# Provenance: IncorrectUseLikeInQuery

## Status

Good candidate for `MIT OR Apache-2.0` at the rule level, with an SDBL
implementation caveat.

## Why this rule exists

This diagnostic is explicitly grounded in a public 1C standard.

Primary source:

- ITS / v8std `#std726`: features of using the `ПОДОБНО` / `LIKE` operator in
  queries

The standard directly says that the pattern side of `ПОДОБНО` should use only:

- a constant string literal;
- a query parameter.

It also explicitly prohibits building the pattern via calculations or
concatenation in the query text.

## Audit result

### Production code

Current diagnostic-layer implementation in
`crates/ide-diagnostics/src/handlers/incorrect_use_like_in_query.rs` is local:

- `sdbl_hir` emits `SdblDiagnostic::LikeUsage { kind: LikeUsageKind::Incorrect }` with an SDBL range (Track 2 §4 Slice 4: a single variant carries both `UsingLikeInQuery` and `IncorrectUseLikeInQuery` BSL-LS rules, discriminated by `kind`);
- the IDE layer maps that range back into the BSL source and formats the
  message;
- local tests cover valid and invalid pattern forms.

### Documentation

Local RU/EN documentation was rewritten during this audit to cite `#std726`
directly and to align the examples with the exact current rule.

### Tests

Current tests are local inline Rust scenarios embedded in the handler module.
During this audit, the last remaining `from_fixture` naming trail was removed.

## Important caveat

Even though the rule itself is clearly public and standard-based, the current
implementation depends on the SDBL parser and `sdbl_hir` layers, which are
still under provenance audit.

This means the diagnostic should stay in the same bucket as other SDBL-based
rules:

- the rule concept is clean;
- the diagnostic layer is local;
- full repository-wide permissive treatment still depends on clearing the SDBL
  infrastructure.

## Residual risk

Residual risk is medium.

- low for the rule concept itself, because `#std726` is explicit;
- medium for repository-wide relicensing, because of the unresolved SDBL parser
  provenance.

## Conclusion

Keep `IncorrectUseLikeInQuery` in the `rule is clean, implementation depends on
SDBL audit` bucket for now.
