# Provenance: IdenticalExpressions

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is not tied to a specific 1C coding standard.

It implements a generic static-analysis idea: identical expressions on both
sides of an operator, or repeated conditions in a logical chain, often indicate
copy-paste mistakes or logic bugs.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/identical_expressions.rs` is local and
substantial:

- main checking is performed over the local HIR model;
- semantic equality is computed through local recursive expression comparison;
- a local AST fallback handles preprocessor-split edge cases that are not fully
  represented in HIR;
- operator exceptions and divisor allowlists are implemented through local
  policy.

This strongly favors permissive treatment because both the rule concept and the
implementation strategy are generic rather than inherited from a 1C-specific
standard.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
as a generic suspicious-pattern check without claiming a direct 1C standard
basis.

### Tests

Current tests are local inline Rust scenarios covering:

- identical operands for comparison, arithmetic, and logical operators;
- statement-level exceptions such as self-assignment;
- logical chains with repeated operands;
- module-level expressions;
- preprocessor-split edge cases.

The tests are local and do not depend on an external fixture file.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure layers.

## Conclusion

`IdenticalExpressions` is one of the clearest permissive candidates because it
is a generic bug-pattern rule and the current implementation/docs/tests are
local.
