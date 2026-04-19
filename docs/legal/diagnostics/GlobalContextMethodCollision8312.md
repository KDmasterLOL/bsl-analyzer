# Provenance: GlobalContextMethodCollision8312

## Status

Good candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic follows from public 1C platform migration guidance for
`8.3.12`.

Primary source:

- official migration documentation describing new global context bitwise methods
  introduced in `8.3.12`

The underlying rule is API-based: user-defined functions that reuse those names
collide with the platform's own methods.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/global_context_method_collision8312.rs`
is a local adapter over local HIR findings.

This favors permissive treatment:

- the rule comes directly from a public platform change;
- the conflicting-name list reflects platform API names rather than a creative
  project-specific taxonomy;
- the handler only formats user-facing diagnostics from local lowering results.

### Documentation

Local RU/EN documentation was rewritten during this audit to rely on the
official migration source and to describe the rule as a compatibility aid for
the `8.3.12` API changes.

### Tests

Current tests are local inline Rust scenarios that enumerate Russian and
English conflicting names and verify case-insensitive matching.

During this audit, reference-style provenance comments were removed from the
main test while keeping the same local coverage.

## Remaining caveats

- the catalog of conflicting names naturally overlaps with public `bsl-ls`
  material because both tools reflect the same platform additions;
- repository-wide relicensing still depends on the broader audit of HIR/lowering
  layers and historical wording.

## Conclusion

`GlobalContextMethodCollision8312` fits well into the permissive-candidate
bucket because it is anchored in public platform API changes and the current
implementation/docs/tests are local.
