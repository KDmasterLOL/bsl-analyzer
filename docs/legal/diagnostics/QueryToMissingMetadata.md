# QueryToMissingMetadata provenance

## Status

Candidate for `MIT OR Apache-2.0`, with an SDBL implementation caveat.

## Why this rule is probably clean

The rule expresses a generic semantic requirement: a query should not reference metadata objects that do not exist. This is an obvious correctness condition and not a unique creative idea from any particular analyzer.

## Public sources

- `v8std.ru/diagnostics/bslls/QueryToMissingMetadata/` as a secondary public reference

There is no direct `v8std` standard mapping for this rule.

## Audit result

The current handler is local Rust code. It receives `SdblDiagnostic::QueryToMissingMetadata` entries produced by the SDBL lowering pipeline and turns them into user-facing diagnostics.

The interesting implementation logic therefore lives below this file, in the SDBL parser / lowering stack that resolves query table paths against metadata.

## Important caveats

- Final licensing confidence depends on the broader provenance of `parser` and `sdbl_hir`.
- The positive behavior of this rule is metadata-dependent, so the current unit tests mainly confirm wrapper behavior rather than full end-to-end detection.

## Conclusion

At the rule level, `QueryToMissingMetadata` looks like a good permissive candidate. At the implementation level, final confidence still depends on the broader SDBL parser audit.
