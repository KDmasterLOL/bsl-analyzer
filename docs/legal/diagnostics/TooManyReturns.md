# TooManyReturns provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

This is a generic maintainability rule. The idea that too many exit points make a method harder to understand is common static-analysis guidance and not a unique analyzer-specific invention.

## Public basis

- Sonar rule `S1142` as a public secondary reference for the general idea

There is no direct `v8std` mapping for this rule.

## Audit result

The current implementation is local Rust code. Return statements are collected by the project's own HIR lowering, and this handler simply applies the configured threshold and emits a diagnostic on the method name.

The default threshold is also local project policy:

- `maxReturnsCount = 3`

## Important caveats

- The exact threshold is a configuration choice of this project, not a public standard requirement.
- The rule is disabled by default, which further supports the interpretation that this is local style policy rather than a mandatory platform rule.

## Conclusion

`TooManyReturns` looks like a strong permissive candidate. The rule is generic, and the current implementation is local and HIR-based.
