# TypeMismatch provenance

## Status

Promising future candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The idea of reporting incompatible types is generic static-analysis functionality, not a unique analyzer-specific concept. Type mismatch diagnostics are a standard part of language tooling.

## Public sources

- No single 1C public standard is required here; this is a generic type-system and static-analysis concept.

## Audit result

The current implementation is only a local handler stub plus metadata and message formatting. It accepts `expected` and `actual` types from `hir-ty::infer` diagnostics and formats a human-readable message.

## Important caveats

- There is currently no live emitter. The handler is wired, but the inference-side emission is still disabled.
- Because the rule is not live yet, there are no meaningful production semantics to audit beyond the handler skeleton.
- Final provenance and licensing confidence should be rechecked once the emitter logic is implemented, because the real behavior will then depend on the concrete inference and assignability algorithm.

## Conclusion

`TypeMismatch` looks like a good future permissive candidate, but today it should be treated as an inactive placeholder diagnostic rather than a fully audited live rule.
