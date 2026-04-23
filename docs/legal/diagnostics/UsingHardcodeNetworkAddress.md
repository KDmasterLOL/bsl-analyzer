# UsingHardcodeNetworkAddress provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

This is a generic security and maintainability rule: infrastructure addresses should not be hardcoded in source code. The idea is common secure-coding guidance and not a unique analyzer-specific invention.

## Public sources

- `v8std.ru/diagnostics/bslls/UsingHardcodeNetworkAddress/` as a secondary public reference

There is no direct `v8std` standard mapping for this rule.

## Audit result

The current implementation is local Rust code. It scans string literals, applies IPv4/IPv6 regex matching, and then filters out several categories of false positives with local heuristics.

Examples of local implementation details:

- localhost exclusion
- URL exclusion
- version-pattern exclusion
- word-based context exclusions such as `Version`, `Namespace`, `Driver`
- AST-based context checks for statements, parameters, and returns

## Important caveats

- The exact regexes and exclusion patterns are local implementation policy of this project.
- Because the rule is heuristic, its legal cleanliness is strong, but its behavioral scope is wider than a simple “literal IP” statement.

## Conclusion

`UsingHardcodeNetworkAddress` looks like a strong permissive candidate. The rule is generic, and the current implementation is clearly local heuristic logic rather than a direct port of a specific foreign expression.
