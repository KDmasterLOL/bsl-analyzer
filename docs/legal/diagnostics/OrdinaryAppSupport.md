# Provenance: OrdinaryAppSupport

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic has a public standards-based rationale.

`#std467` ("General configuration requirements") supports the expectation that a
configuration should correctly handle ordinary-application compatibility
settings. The current diagnostic turns that public guidance into two concrete
configuration checks.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/ordinary_app_support.rs`
is local and metadata-based:

- it runs only when local analyzer config enables `ordinary_app_support`;
- it limits itself to `SessionModule` files;
- it loads configuration metadata locally and checks two boolean properties;
- it emits local diagnostics at the module header range.

This strongly favors permissive treatment because the implementation is a local
standards-checking layer over public configuration guidance.

### Documentation

RU/EN documentation was rewritten during this audit to reflect the actual scope:
`SessionModule` only and gated by `ordinary_app_support`.

### Tests

Current tests are local fixture-based checks covering:

- a `SessionModule` with unsupported configuration settings;
- disabled analyzer config;
- a non-session module that should not be checked.

The test suite is embedded directly in the Rust module.

## Remaining caveats

- the decision to gate the rule behind `ordinary_app_support` is a local product
  choice, not something mandated by the standard itself;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`OrdinaryAppSupport` is a strong permissive candidate because it implements a
public 1C configuration recommendation through local metadata checks, with local
tests and now-local documentation.
