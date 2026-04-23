# UnresolvedMethodCall

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule is generic semantic analysis: if a qualified method call cannot be resolved against the known module and symbol model, report it.

That idea is not specific to `bsl-language-server`. It follows naturally from workspace-aware symbol resolution and export visibility checks.

## Public basis

- General BSL semantics of calling exported common-module methods.
- Public platform/module model that distinguishes existing, exported, and unavailable methods.

## Implementation audit notes

Current implementation is clearly local and semantic:

- it is emitted from type inference / semantic resolution rather than syntax-only matching;
- it distinguishes several unresolved-call situations through `hir::UnresolvedMethodKind`;
- it uses the workspace module index and symbol tree to decide whether the target module exists, whether the method exists, and whether it is exported.

This is a local HIR/symbol-resolution asset, not a copied prose-style rule.

## Conclusion

`UnresolvedMethodCall` looks like a strong permissive candidate. The rule is generic, and the implementation is built on your own module/symbol resolution pipeline.
