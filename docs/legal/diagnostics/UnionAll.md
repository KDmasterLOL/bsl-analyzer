# UnionAll

## Status

Promising candidate for `MIT OR Apache-2.0`, with an implementation caveat.

## Why this rule is probably clean

The rule itself follows a public 1C standard: queries should generally use `UNION ALL` / `ОБЪЕДИНИТЬ ВСЕ`, while plain `UNION` / `ОБЪЕДИНИТЬ` should be reserved for cases where duplicate elimination is actually required.

That recommendation comes from the documented behavior and cost model of the query language, not from a unique design created by `bsl-language-server`.

## Public sources

- ITS / v8std `#std434`: usage of `UNION` and `UNION ALL` in queries.

## Implementation audit notes

Current Rust code does not try to prove whether duplicate elimination is semantically required. It simply reports `sdbl_hir::SdblDiagnostic::UnionWithoutAll`, that is, every parsed `UNION` / `ОБЪЕДИНИТЬ` occurrence without `ALL` / `ВСЕ`.

That behavior is still consistent with a conservative lint based on the public standard, but it means the implementation is intentionally simpler than the full prose guidance.

## Remaining caveat

This is an SDBL diagnostic. Rule-level provenance looks clean, but final implementation-level confidence still depends on the broader audit of `parser`, `lexer` SDBL support, and `sdbl_hir`.

## Conclusion

`UnionAll` looks clean at the rule level and is a good future permissive candidate, but it should remain marked with the general SDBL implementation caveat until the parser / `sdbl_hir` audit is completed.
