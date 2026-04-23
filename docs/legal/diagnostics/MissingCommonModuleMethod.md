# Provenance: MissingCommonModuleMethod

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic semantic correctness rule.

The underlying idea is straightforward: a qualified call of the form
`CommonModule.Method()` is valid only when the referenced common module exposes
such a method as part of its public API. Calling a missing or non-export method
leads to runtime failure.

There is no strong direct normative `v8std` source for this exact rule.
`v8std.ru` itself classifies `MissingCommonModuleMethod` as a diagnostic without
direct standard mapping.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/missing_common_module_method.rs`
is local and semantic-analysis based:

- HIR lowering emits a body diagnostic for a qualified call that looks like a
  common-module access;
- the handler resolves the pair `(module, method)` through local metadata-aware
  path resolution;
- the diagnostic is reported only when resolution does not produce an exported
  method.

This strongly favors permissive treatment because the current behavior is based
on local HIR/path-resolution infrastructure rather than on copied parser or doc
logic.

### Documentation

RU/EN documentation was rewritten during this audit to describe the current
behavior directly instead of reusing placeholder or upstream wording.

### Tests

Current tests are local inline fixtures focused on:

- unresolved qualified calls;
- local-variable and parameter shadowing;
- mixed scenarios where only the real common-module access should be reported.

One ignored fixture still documents future metadata-backed resolution through
`Configuration.xml`, but it does not introduce a licensing concern by itself.

## Remaining caveats

- there is no clean official standard page to cite as a primary normative
  source;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`MissingCommonModuleMethod` is a strong permissive candidate because it is a
generic semantic correctness rule with local HIR-based implementation, local
tests, and now-local documentation.
