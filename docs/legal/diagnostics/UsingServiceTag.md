# UsingServiceTag provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

This is a generic code-hygiene rule: service tags, debug markers, merge leftovers, and placeholder comments should not remain in production code. The idea is standard maintenance guidance and not a unique analyzer-specific invention.

## Public sources

- `v8std.ru/diagnostics/bslls/UsingServiceTag/` as a secondary public reference

There is no direct `v8std` standard mapping for this rule.

## Audit result

The current implementation is local Rust code. It scans comment tokens and checks them against:

- a built-in default set of service tags
- several local phrase patterns for generated handler placeholders and constructor markers
- an optional custom regex pattern from configuration

## Important caveats

- The exact default tag set and phrase patterns are local implementation policy of this project.
- The diagnostic is intentionally heuristic and comment-based, not semantic.

## Conclusion

`UsingServiceTag` looks like a strong permissive candidate. The rule is generic, and the current implementation is clearly local comment-scanning logic.
