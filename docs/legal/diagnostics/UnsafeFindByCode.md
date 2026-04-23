# UnsafeFindByCode provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows from a public and obvious semantic constraint: searching by code is unsafe when code uniqueness is not guaranteed by metadata settings. That idea depends on the platform behavior of `FindByCode()` and on metadata options such as uniqueness control and code series.

## Public sources

- `v8std.ru/diagnostics/bslls/UnsafeFindByCode/` as a secondary public reference
- public platform semantics of `FindByCode()` and metadata settings that affect code uniqueness

## Audit result

The current implementation is local Rust code. It:

- recognizes supported manager collections (`Catalogs`, `ChartsOfCharacteristicTypes`, `ChartsOfAccounts`)
- resolves the target metadata object through the local configuration model
- checks local metadata flags such as `check_unique` and `code_series`
- builds a local explanation message based on the reason the lookup is unsafe

This is a metadata-driven semantic rule, not a parser-port rule.

## Important caveats

- The exact set of supported metadata types is a local implementation choice of this project.
- The rule depends on having configuration metadata available; without it, the diagnostic does not fire.

## Conclusion

`UnsafeFindByCode` looks like a strong permissive candidate. The rule is grounded in public platform semantics, and the current implementation is local and metadata-driven.
