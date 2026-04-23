# SelectTopWithoutOrderBy provenance

## Status

Promising candidate for `MIT OR Apache-2.0`, but final confidence still depends on the SDBL parser and `sdbl_hir` audit.

## Why this rule is probably clean

The underlying rule comes directly from public 1C guidance: using `ПЕРВЫЕ` / `TOP` without explicit ordering leads to nondeterministic results, except for narrow cases where order does not matter and only one row is expected. This is a public query-behavior rule, not a unique analyzer-specific idea.

## Public sources

- `#std412` "Упорядочивание результатов запроса"
- `v8std.ru/std/412/` as a public secondary reference

## Audit result

The current implementation is local Rust code that consumes `sdbl_hir::SdblDiagnostic::SelectTopWithoutOrderBy` and applies project-specific reporting policy:

- `TOP N` inside `UNION` is always reported;
- `TOP N` with `N > 1` is reported without `ORDER BY`;
- `TOP 1` and `TOP 0` can be skipped depending on `skipSelectTopOne` and the presence of `WHERE`.

## Important caveats

- This is an SDBL diagnostic, so final licensing confidence still depends on the broader audit of `parser` and `sdbl_hir`.
- The implementation is narrower and more operational than the full public standard text. It only covers the concrete cases encoded by `sdbl_hir`.
- The `skipSelectTopOne` configuration flag is a local project policy choice.

## Conclusion

`SelectTopWithoutOrderBy` looks like a good future permissive candidate at the rule level, but implementation-level confidence should remain tied to the remaining SDBL provenance audit.
