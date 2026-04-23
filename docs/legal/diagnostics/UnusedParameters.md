# UnusedParameters provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

This is a generic maintainability rule: parameters that are never used inside a method should usually be removed. The idea is standard static-analysis guidance and not a unique analyzer-specific invention.

## Public sources

- `v8std.ru/diagnostics/bslls/UnusedParameters/` as a secondary public reference

There is no direct `v8std` standard mapping for this rule.

## Audit result

The current implementation is local Rust code. It walks HIR bodies, tracks used identifiers, and applies several local exclusions for methods that are expected to have fixed signatures:

- platform event handlers
- form and HTTP handlers from metadata
- attachable methods with configured prefixes
- same-module callbacks registered through `NotifyDescription`
- empty methods

## Important caveats

- The exact exclusion list and `attachableMethodPrefixes` option are local implementation policy.
- The rule currently works at identifier-usage level inside HIR bodies; that is a local implementation choice, not a standard requirement.

## Conclusion

`UnusedParameters` looks like a strong permissive candidate. The rule is generic, and the current implementation is local HIR-based logic with project-specific exclusions.
