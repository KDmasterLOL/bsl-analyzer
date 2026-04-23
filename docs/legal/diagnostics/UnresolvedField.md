# UnresolvedField

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The idea behind this diagnostic is generic static analysis: if code accesses a field that does not exist on a known type, report it as an error.

That idea is not specific to `bsl-language-server`. It follows naturally from typed semantic analysis and from the platform metadata model.

## Public basis

- General BSL/platform semantics of field access on typed values.
- Public metadata model that defines which fields are available for metadata reference types.

## Implementation audit notes

Current implementation is clearly local and HIR-based:

- the diagnostic is emitted from type inference,
- it is created only for sufficiently known receiver types,
- `Ty::MetadataRef { .. }` is used as the high-confidence emit guard,
- `Ty::Unknown`, unions, and weaker cases stay silent.

This means the Rust implementation is not just a superficial syntax check. It relies on your own type inference and field lookup pipeline.

## Conclusion

`UnresolvedField` looks like a strong permissive candidate. The rule is generic, and the implementation is a distinctly local semantic-analysis asset rather than copied expression.
