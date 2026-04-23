# Provenance: MissedRequiredParameter

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is directly grounded in public 1C guidance.

The closest public source is `#std640`, which describes parameters of
procedures and functions and distinguishes required parameters from optional
ones with default values.

So the rule idea is public: required parameters should not be silently skipped.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/missed_required_parameter.rs` is local and
semantic:

- it receives call information from HIR lowering;
- it resolves local, qualified, and manager-module calls using local symbol
  infrastructure;
- it checks which parameters are required and which arguments were actually
  supplied;
- it reports only the names of truly missing required parameters.

This strongly favors permissive treatment because the implementation is local
and more specific than the generic public rule statement.

### Documentation

Local RU/EN documentation was normalized during this audit to describe the rule
in terms of required parameters, explicit `Undefined`, and semantically resolved
calls.

### Tests

Current tests are local fixture-style Rust scenarios covering resolved common
module calls with missing required arguments.

The fixture is embedded directly in the test module and is small.

## Important caveat

The rule idea is public and strong. The implementation itself is local and does
not depend on parser-layer provenance in the way SDBL diagnostics do.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`MissedRequiredParameter` is a strong permissive candidate because it is a
direct implementation of a public parameter-contract rule with local semantic
resolution, local tests, and rewritten documentation.
