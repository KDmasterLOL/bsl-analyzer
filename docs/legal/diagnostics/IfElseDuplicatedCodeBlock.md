# Provenance: IfElseDuplicatedCodeBlock

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic duplicate-branch rule.

It is not tied to a specific normative 1C standard. The closest public 1C
context is `#std440`, which generally recommends avoiding duplicate code, but
that standard does not define this diagnostic one-to-one.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/if_else_duplicated_code_block.rs` is local
and small:

- the diagnostic is emitted from local HIR lowering;
- the handler only formats the final message;
- rule behavior is determined by local duplicate-branch detection.

This favors permissive treatment because the rule concept is generic and the
current implementation is local.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
as a generic suspicious-pattern check, with `#std440` used only as related
public context about duplicate code.

### Tests

Current tests are local inline Rust scenarios covering:

- identical `If` / `Else` branches;
- identical `If` / `ElseIf` branches;
- empty branches that should be ignored;
- branches with different statement counts;
- nested duplicated blocks.

The tests are local and do not depend on an external fixture file.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared HIR
  infrastructure.

## Conclusion

`IfElseDuplicatedCodeBlock` is a strong permissive candidate because it is a
generic duplicate-code rule and the current implementation/docs/tests are
local.
