# Provenance: MethodSize

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic maintainability and refactoring rule.

Very large methods are a common static-analysis smell because they tend to mix
responsibilities, complicate testing, and reduce readability. This idea is
generic and not tied to a specific 1C standard.

There is no direct normative 1C standard source for this exact rule.

## Audit result

### Production code

Current implementation in `crates/ide-diagnostics/src/handlers/method_size.rs`
is local and HIR-based:

- it receives method size information from HIR diagnostics;
- it applies a configurable `maxMethodSize` threshold;
- it reports only methods whose computed size exceeds that threshold.

This strongly favors permissive treatment because the implementation is local
and straightforward.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
as a generic refactoring diagnostic and to match the actual configurable
behavior.

### Tests

Current tests are local Rust scenarios covering:

- empty methods;
- one-line methods;
- default threshold behavior;
- custom threshold behavior.

There is also a larger inline regression builder that reproduces several method
shapes and exact threshold boundaries. It is embedded directly in the Rust test
module.

## Important caveat

There is no strong public normative source here. The legal basis comes from
independent local implementation of a generic static-analysis idea.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`MethodSize` is a strong permissive candidate because it is a generic
maintainability rule with a clearly local HIR-based implementation, local test
coverage, and rewritten documentation.
