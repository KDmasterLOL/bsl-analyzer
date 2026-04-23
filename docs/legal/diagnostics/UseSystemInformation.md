# UseSystemInformation

## Status

Strong candidate for `MIT OR Apache-2.0`, with a quality caveat.

## Why this rule is probably clean

The idea is generic and security-oriented: direct access to detailed system information can be a hotspot and may deserve review in secure codebases.

That concept is not unique to `bsl-language-server`. It follows from the platform API surface and from common secure-coding practice.

## Public basis

- Public platform availability of `СистемнаяИнформация` / `SystemInfo`.
- General security rationale around environment discovery and information disclosure.

## Implementation audit notes

Current implementation is narrow and local. It reports only direct object construction patterns for `СистемнаяИнформация` / `SystemInfo`, including both identifier and string-based constructor forms.

It does **not** try to prove data exfiltration or classify downstream usage. It is a review hotspot, not a full dataflow security analysis.

The diagnostic is disabled by default, which also matches its intended role as an optional security review rule rather than a universal coding error.

## Conclusion

`UseSystemInformation` looks like a strong permissive candidate. The rule is generic, and the current implementation is a local hotspot detector rather than copied expression.
