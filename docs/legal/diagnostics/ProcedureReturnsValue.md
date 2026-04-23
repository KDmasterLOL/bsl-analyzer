# Provenance: ProcedureReturnsValue

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic language-correctness rule.

The idea is straightforward and comes directly from BSL language semantics:
functions may return values, procedures may not. This is not tied to a unique
protectable upstream concept or to a specific `v8std` standard page.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/procedure_returns_value.rs`
is very small and local:

- HIR lowering identifies return-with-value inside a procedure body;
- the handler turns that local body diagnostic into a user-facing error.

This strongly favors permissive treatment because the implementation is a direct
local wrapper over a basic language rule.

### Documentation

RU/EN documentation was rewritten during this audit to describe the rule in
simple local wording without relying on upstream phrasing.

### Tests

Current tests are local inline fixtures covering:

- invalid return-with-value in procedures;
- valid bare `Возврат;` in procedures;
- valid return values in functions;
- the semicolon-omission edge case before `КонецЕсли`;
- a larger mixed fixture with several expected hits.

The test suite is embedded directly in the Rust module.

## Remaining caveats

- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`ProcedureReturnsValue` is a strong permissive candidate because it is a generic
language-correctness rule with local implementation, local tests, and now-local
documentation.
