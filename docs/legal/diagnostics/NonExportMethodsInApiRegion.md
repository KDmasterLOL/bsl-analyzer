# Provenance: NonExportMethodsInApiRegion

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is directly grounded in a public 1C standard.

`#std455` ("Module structure") defines the purpose of API regions such as
`ПрограммныйИнтерфейс` / `Public` and their service counterparts. These regions
are meant for the public interface of a module, so non-export methods do not
fit there.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/non_export_methods_in_api_region.rs`
is local and standards-based:

- it uses the local `RegionTree` to locate API regions by source range;
- it uses the local `ItemTree` to inspect procedures/functions and their export
  flags;
- it applies an optional local policy switch, `skipAnnotatedMethods`, for
  built-in annotations.

This strongly favors permissive treatment because the handler is a local
implementation of a published structural rule.

### Documentation

RU/EN documentation was rewritten during this audit to point directly to
`#std455` and to describe the current configurable behavior in local wording.

### Tests

Current tests are local inline fixtures covering:

- exported and non-export methods inside API regions;
- nested regions;
- methods outside API regions;
- built-in annotations with and without `skipAnnotatedMethods`;
- custom annotation behavior.

The test corpus is embedded directly in the Rust module.

## Remaining caveats

- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`NonExportMethodsInApiRegion` is a strong permissive candidate because it
directly implements a public 1C standard through local region and item-tree
analysis, with local tests and now-local documentation.
