# Provenance: IfElseIfEndsWithElse

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic defensive-programming rule.

It is not tied to a specific normative 1C standard. The idea is general:
when an `If` chain already contains one or more `ElseIf` branches, a final
`Else` branch makes the handling of all remaining cases explicit.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/if_else_if_ends_with_else.rs` is local and
small:

- the diagnostic is emitted from local HIR lowering;
- the handler only formats the final message;
- rule behavior depends on local detection of `If` chains that contain
  `ElseIf` branches but no final `Else`.

This favors permissive treatment because the rule concept is generic and the
implementation is local.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
as a local defensive-programming check without claiming a direct normative 1C
source.

### Tests

Current tests are local inline Rust scenarios covering:

- `If` / `ElseIf` chains without `Else`;
- similar chains that do end with `Else`;
- plain `If` or `If/Else` cases that should not trigger;
- multiple `ElseIf` branches;
- nested chains;
- a FizzBuzz-style example.

The tests are local and do not depend on an external fixture file.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared HIR
  infrastructure.

## Conclusion

`IfElseIfEndsWithElse` is a strong permissive candidate because it is a generic
defensive-programming rule and the current implementation/docs/tests are local.
