# Provenance: NonStandardRegion

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is directly grounded in a public 1C standard.

`#std455` ("Module structure") defines the expected module layout and the
standard region names for different module types. The rule therefore follows
directly from a published standard rather than from a project-specific idea.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/non_standard_region.rs`
is local and straightforward:

- it determines the current module type from metadata;
- it reads module-level regions through local cached infrastructure;
- it checks region names against the local `standard_regions` table;
- it emits a diagnostic only for names that are not standard for the current
  module type.

This strongly favors permissive treatment because the handler is a local
standards-checking implementation over a public rule.

### Documentation

RU/EN documentation was rewritten during this audit to point directly to
`#std455` and to describe the current behavior in local wording.

### Tests

Current tests are local utility-level checks that verify:

- standard and non-standard names for common modules;
- case-insensitive matching;
- module-type-specific region sets;
- special suffix-based form regions;
- behavior for unknown module types.

The test suite is embedded directly in the Rust module.

## Remaining caveats

- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`NonStandardRegion` is a strong permissive candidate because it directly
implements a public 1C standard through local metadata and region-name checks,
with local tests and now-local documentation.
