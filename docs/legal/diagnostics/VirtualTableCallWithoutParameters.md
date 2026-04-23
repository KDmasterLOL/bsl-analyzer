# VirtualTableCallWithoutParameters provenance

## Status

Candidate for `MIT OR Apache-2.0`, with an SDBL implementation caveat.

## Why this rule is probably clean

The rule is based on a public performance concern: virtual tables should be called with appropriate parameters so filtering happens as early as possible. That idea is reflected in public 1C guidance and is not a unique analyzer-specific invention.

## Public sources

- `#std657` on virtual table usage
- `#std733` on effective use of the `Turnovers` virtual table
- 1C recommendation about using the `Condition` parameter when accessing a virtual table

## Audit result

The current handler is local Rust code, but it is only a thin dispatch layer over `sdbl_hir::SdblDiagnostic::VirtualTableCallWithoutParameters`.

The important behavioral point is that the current implementation is narrower and more concrete than some prose descriptions: it reports virtual table calls with missing or empty parameter lists, not every possible case where a filter was placed outside the parameter list.

## Important caveats

- Final licensing confidence depends on the broader provenance of `parser` and `sdbl_hir`.
- The current implementation scope is determined by local SDBL lowering logic, not by this handler file.

## Conclusion

At the rule and docs level, `VirtualTableCallWithoutParameters` looks like a good permissive candidate. At the implementation level, final confidence still depends on the broader SDBL parser audit.
