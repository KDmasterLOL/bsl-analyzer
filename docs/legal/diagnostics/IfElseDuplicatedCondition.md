# Provenance: IfElseDuplicatedCondition

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic duplicate-condition rule.

It is not tied to a specific normative 1C standard. The core idea is general:
repeating the same condition later in an `If` / `ElseIf` chain usually makes
that later branch unreachable.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/if_else_duplicated_condition.rs` is local
and small:

- the diagnostic is emitted from local HIR lowering;
- the handler only formats the final user-facing message;
- rule behavior depends on local normalization and duplicate-condition
  detection.

This favors permissive treatment because the rule concept is generic and the
implementation is local.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
as a generic suspicious-pattern check without claiming a direct normative 1C
source.

### Tests

Current tests are local inline Rust scenarios covering:

- simple duplicated conditions;
- distinct conditions that should not trigger;
- case-insensitive identifier matching;
- whitespace normalization;
- case-sensitive string literal handling;
- nested `If` chains and repeated-condition groups.

The tests are local and do not depend on an external fixture file.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared HIR
  infrastructure.

## Conclusion

`IfElseDuplicatedCondition` is a strong permissive candidate because it is a
generic duplicate-condition rule and the current implementation/docs/tests are
local.
