# QueryParseError provenance

## Status

Candidate for `MIT OR Apache-2.0`, with an SDBL implementation caveat.

## Why this rule is probably clean

The underlying requirement is public and unsurprising: query text should be syntactically valid and suitable for further maintenance. That rationale is consistent with public 1C guidance on query formatting and maintainability.

## Public sources

- `#std437` "Работа с запросами. Оформление текстов запросов"
- `v8std.ru/diagnostics/bslls/QueryParseError/` as a secondary public reference

## Audit result

The current handler is local Rust code. It reports parse errors by inspecting the SDBL AST that is already available in `SdblQueryInfo.query_ast`, and it also handles one extra malformed pattern with a trailing dot in `ССЫЛКА Документ.` expressions.

This is not a literal Java port at the file level, but the implementation still depends on the current SDBL parsing pipeline.

## Important caveats

- The diagnostic depends on `parser` / `sdbl_hir` provenance for full confidence.
- The large regression fixture in `query_parse_error.rs` should still be reviewed separately if we want to eliminate all test-level borrowing risk.

## Conclusion

At the rule and docs level, `QueryParseError` looks like a good permissive candidate. At the implementation level, final confidence still depends on the broader SDBL parser audit and a later pass over the remaining regression fixtures.
