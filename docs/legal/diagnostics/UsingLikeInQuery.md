# UsingLikeInQuery provenance

## Status

Candidate for `MIT OR Apache-2.0`, with an SDBL implementation caveat.

## Why this rule is probably clean

The rule is based on a public technical concern: use of `LIKE` / `ПОДОБНО` in queries can behave differently across DBMS implementations and should be treated with caution. That idea is reflected in public 1C guidance and is not a unique analyzer-specific invention.

## Public sources

- `#std726` "Особенности использования в запросах оператора ПОДОБНО"
- developer documentation for the string pattern operator

## Audit result

The current handler is local Rust code, but it is only a thin dispatch layer over `sdbl_hir::SdblDiagnostic::LikeUsage` (consumes both `LikeUsageKind::Allowed` and `LikeUsageKind::Incorrect`; the latter is what additionally fires `IncorrectUseLikeInQuery`).

The important behavioral point is that the current implementation is intentionally conservative: it reports every detected `LIKE` / `ПОДОБНО` occurrence rather than trying to distinguish “safe” and “unsafe” forms.

## Important caveats

- Final licensing confidence depends on the broader provenance of `parser` and `sdbl_hir`.
- The current implementation is broader than some prose descriptions of the rule, because it flags all `LIKE` usages.

## Conclusion

At the rule and docs level, `UsingLikeInQuery` looks like a good permissive candidate. At the implementation level, final confidence still depends on the broader SDBL parser audit.
