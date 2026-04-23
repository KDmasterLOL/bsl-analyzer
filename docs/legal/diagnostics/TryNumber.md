# TryNumber provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows from public 1C guidance about exception handling: `Попытка...Исключение` should not be used as a normal control-flow mechanism for routine conversions. This is a public coding-style and correctness concern, not a unique analyzer-specific idea.

## Public sources

- `#std499` "Перехват исключений в коде"

## Audit result

The current implementation is local Rust code based on HIR diagnostics emitted during lowering.

It reports only calls to `Число()` / `Number()` that occur inside the `try` part of a `Попытка...Исключение` block.

## Important caveats

- The implementation is intentionally narrow.
- It does not try to detect all cases of exception-driven casting.
- Calls in the `except` block are ignored.
- Calls outside `try` blocks are ignored.

## Conclusion

`TryNumber` looks like a strong permissive candidate. The rule is grounded in public exception-handling guidance, and the current implementation is a local narrow-pattern detector with clearly documented limits.
