# Provenance: NestedConstructorsInStructureDeclaration

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic readability and maintainability rule.

The underlying idea is simple: deeply nested constructor expressions make a
structure declaration harder to scan and maintain. Extracting nested values into
separate variables usually improves clarity.

There is no direct normative `v8std` source for this exact rule. `v8std.ru`
exposes it as a `bslls` diagnostic without a standard mapping.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/nested_constructors_in_structure_declaration.rs`
is local:

- it iterates over local HIR expressions;
- it checks only `New` expressions for `Structure` and `FixedStructure`;
- it reports a diagnostic only when one of the constructor arguments is itself a
  nested constructor with parameters.

This favors permissive treatment because the behavior is expressed through a
small local semantic walk over the current HIR representation.

### Documentation

RU/EN documentation was rewritten during this audit to describe the rule in
neutral local wording.

### Tests

Current tests are local inline fixtures covering:

- empty and single-parameter constructors that should not trigger;
- nested constructors without parameters;
- nested `Structure` / `FixedStructure` constructors with parameters;
- RU and EN keyword variants.

The test corpus is embedded directly in the Rust module.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`NestedConstructorsInStructureDeclaration` is a strong permissive candidate
because it is a generic readability rule with local HIR-based implementation,
local tests, and now-local documentation.
