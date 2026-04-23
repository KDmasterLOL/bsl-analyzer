# Provenance: NestedStatements

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic readability and complexity rule.

The idea that deeply nested control flow reduces maintainability is widely used
in static analysis. A suitable public generic source here is Sonar `RSPEC-134`,
which covers excessive nesting of control-flow statements.

There is no special 1C-specific normative dependency for this rule.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/nested_statements.rs`
is small and local:

- HIR lowering computes nesting depth and statement range;
- the handler applies local configuration via `maxAllowedLevel`;
- the resulting diagnostic message and severity/tags are produced locally.

This strongly favors permissive treatment because the implementation is a thin
local policy layer over a generic static-analysis concept.

### Documentation

RU/EN documentation was rewritten during this audit to describe the current rule
in local wording and to point to a generic public source instead of upstream
diagnostic text.

### Tests

Current tests are local fixture-style cases covering:

- no nesting;
- boundary behavior at the default maximum level;
- violations above the threshold;
- mixed `IF` / `WHILE` / `FOR` / `TRY` scenarios;
- custom `maxAllowedLevel` configuration.

The test suite is embedded directly in the Rust module.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`NestedStatements` is a strong permissive candidate because it is a generic
complexity rule with a clear public generic source, local implementation, local
tests, and now-local documentation.
