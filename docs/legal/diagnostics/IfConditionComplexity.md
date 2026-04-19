# Provenance: IfConditionComplexity

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is not tied to a specific 1C coding standard.

It implements a generic maintainability heuristic: overly complex boolean
conditions in `If` / `ElseIf` branches are harder to read, reason about, and
modify safely.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/if_condition_complexity.rs` is local:

- the diagnostic is emitted from local HIR lowering;
- the handler re-checks the configurable
  `maxIfConditionComplexity` threshold against local settings;
- user-facing messages are formatted locally.

This favors permissive treatment because the rule concept is generic and the
current implementation is small, local, and configuration-driven.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
as a local maintainability check with a configurable threshold, without
claiming a direct normative 1C source.

### Tests

Current tests are local inline Rust scenarios covering:

- simple conditions below the threshold;
- conditions exactly at the threshold;
- complex `If` and `ElseIf` branches above the threshold;
- English keyword spelling;
- long multiline chains and nested conditions.

The tests are local and do not rely on an external fixture file.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared HIR
  infrastructure.

## Conclusion

`IfConditionComplexity` is a strong permissive candidate because it is a
generic local maintainability rule and the current implementation/docs/tests are
local.
