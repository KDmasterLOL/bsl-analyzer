# UsingHardcodePath provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

This is a generic portability and security rule: absolute file-system paths should not be hardcoded in source code. The idea is common secure-coding and deployment guidance, not a unique analyzer-specific invention.

## Public sources

- `v8std.ru/diagnostics/bslls/UsingHardcodePath/` as a secondary public reference

There is no direct `v8std` standard mapping for this rule.

## Audit result

The current implementation is local Rust code. It token-scans string literals and applies local heuristics for path detection:

- Windows drive-letter paths
- UNC/network paths
- Unix absolute paths under a configurable allowlist of standard root directories
- home-relative and environment-variable based path forms

It also explicitly excludes URL-looking strings.

## Important caveats

- The exact path heuristics and Unix root-word filter are local implementation policy.
- The current implementation is intentionally heuristic and optimized for low false positives rather than formal path parsing.

## Conclusion

`UsingHardcodePath` looks like a strong permissive candidate. The rule is generic, and the current implementation is clearly local heuristic logic.
