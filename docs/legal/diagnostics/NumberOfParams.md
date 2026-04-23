# Provenance: NumberOfParams

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic has a public standards-based rationale.

`#std640` ("Parameters of procedures and functions") supports the general
recommendation to keep parameter lists readable and manageable. Limiting the
total number of parameters is a concrete local heuristic derived from that
guidance.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/number_of_params.rs`
is local and simple:

- HIR lowering computes the total number of parameters for a method;
- the handler compares that count with the configurable `maxParamsCount`;
- the diagnostic message and threshold handling are produced locally.

This strongly favors permissive treatment because the implementation is a local
configurable check built on top of a public design recommendation.

### Documentation

RU/EN documentation was rewritten during this audit to separate the public
rationale from the local configurable threshold.

### Tests

Current tests are local inline fixtures covering:

- the default threshold;
- a custom threshold;
- boundary behavior at the threshold;
- methods with no parameters;
- methods that exceed the limit.

The test suite is embedded directly in the Rust module.

## Remaining caveats

- the exact numeric threshold is a local policy choice rather than a literal
  value mandated by the standard;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`NumberOfParams` is a strong permissive candidate because it is a local
configurable implementation of a public 1C design recommendation, with local
tests and now-local documentation.
