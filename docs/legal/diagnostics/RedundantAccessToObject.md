# RedundantAccessToObject provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

This is a generic readability and style rule. The underlying idea is simple: avoid redundant self-references when direct access is already available and clearer.

That is not a unique analyzer-specific concept and does not depend on a creative standard text.

## Public basis

- `v8std.ru/diagnostics/bslls/RedundantAccessToObject/` as a secondary public reference

There is no direct `v8std` standard mapping for this rule.

## Audit result

The current implementation is local Rust code built on top of the project's own HIR diagnostics. It distinguishes several local cases:

- `ЭтотОбъект` / `ThisObject` access in object, form, and record set modules
- self-calls through the current common module name
- self-calls through the current manager module path

It also contains local exclusions, for example indexed access (`ЭтотОбъект["Поле"]`) and common modules that rely on return value reuse.

## Important caveats

- The exact supported module kinds and exclusions are implementation choices of this project, not something copied from a public standard.
- Positive detection depends on correct module metadata being available in the analysis context.

## Conclusion

`RedundantAccessToObject` looks like a strong permissive candidate. The rule is generic, and the current implementation is local and HIR-based.
