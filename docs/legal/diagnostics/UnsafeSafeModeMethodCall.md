# UnsafeSafeModeMethodCall provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows from public platform semantics: in 1C:Enterprise 8.3, `БезопасныйРежим()` may return not only a boolean-like value but also a string with a security profile name. Using that result implicitly as a condition is therefore unsafe.

This is an API-correctness and migration rule, not a unique analyzer-specific idea.

## Public sources

- official 1C material about the changed return behavior of `БезопасныйРежим()`

## Audit result

The current implementation is local Rust code built on top of HIR diagnostics. The handler itself is very small and only formats a fixed message for already identified unsafe usages in boolean conditions.

## Important caveats

- The rule is tied to compatibility mode `8.3.1`, which reflects the platform behavior encoded in project metadata.
- The exact detection coverage comes from local HIR logic; this file only handles the final diagnostic emission.

## Conclusion

`UnsafeSafeModeMethodCall` looks like a strong permissive candidate. The rule is grounded in public platform semantics, and the current implementation path is local and HIR-based.
