# UnaryPlusInConcatenation provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule is based on public language semantics: in a string concatenation, an accidental second `+` turns into a unary operator and changes the meaning of the expression. This is a generic parsing and runtime-correctness concern, not a unique analyzer-specific idea.

## Public sources

- Public BSL language semantics for concatenation and unary operators
- `v8std` language reference pages about expression semantics and string concatenation as general background

## Audit result

The current implementation is local Rust code that reports HIR diagnostics for a narrow pattern: accidental unary plus inside a string concatenation.

## Important caveats

- The implementation is intentionally narrow.
- It does not try to validate every suspicious arithmetic or concatenation pattern.
- Unary plus on numeric literals is intentionally allowed by the current detector.

## Conclusion

`UnaryPlusInConcatenation` looks like a strong permissive candidate. The rule follows from public language behavior, and the current implementation is a local narrow-pattern detector with clearly documented scope.
