# Provenance: MismatchedArgCount

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic semantic correctness rule.

Checking that a call passes the expected number of arguments is a basic
language- and API-analysis task. The idea is generic and not tied to a specific
1C standard.

There is no direct normative 1C standard source for this exact rule.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/mismatched_arg_count.rs` is local and very
small:

- it receives already computed argument-count mismatch information from HIR type
  inference;
- it formats a local diagnostic message using the expected and actual argument
  counts;
- it relies on the shared helper for standard severity/tags/disabled handling.

This strongly favors permissive treatment because the implementation is clearly
local and semantic-analysis based.

### Documentation

Local RU/EN documentation was added during this audit to describe the rule and
its current semantic scope.

### Tests

Current tests are local fixture-style Rust scenarios that resolve a common
module procedure and verify that a call with too few arguments produces exactly
one `MismatchedArgCount` diagnostic.

The fixture is small, local, and embedded directly in the test module.

## Important caveat

There is no strong public normative source here. The legal basis comes from
independent local implementation of a generic semantic-analysis idea.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`MismatchedArgCount` is a strong permissive candidate because it is a generic
semantic correctness rule with clearly local implementation, local tests, and
now-local documentation.
