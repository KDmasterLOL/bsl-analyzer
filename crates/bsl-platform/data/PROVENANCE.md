# Platform data provenance

## Files

- `platform_data.json` — rich API/help corpus extracted from HBK archives;
- `global_catalog.json` — exact global-name surface attested from the versioned
  1C:EDT platform model.

## Origin

This file is a structured extract of the 1C:Enterprise platform
context-help archives, primarily:

  * `shcntx_ru.hbk` — platform type catalogue (types, methods,
    properties, constructors, global functions);
  * `shlang_ru.hbk` — BSL language keyword documentation.

Both archives ship with the 1C:Enterprise platform. The extraction is
performed by `crates/bsl-platform/tools/html-parser/` and the result is
committed here for reproducible builds without requiring every
contributor to install 1C:Enterprise locally.

## Ownership

Content in these files originates from 1C products. In particular, the
`description`, `param_descriptions`, and `examples` fields — is the
copyright of **ООО «1С-Софт»** («1C-Soft» LLC). It is reproduced here
under the practical assumption that it is necessary for interoperability
with the 1C:Enterprise platform and that it is not being redistributed
as a standalone documentation product.

Structured interface facts (type and method names, parameter lists,
return types, availability context, minimum platform versions) describe
the platform's application-programming interface and are treated as
factual information about the platform rather than protectable
expression. Descriptive text, example code and parameter descriptions
are clearly 1C's copyrighted expression.

## Licensing

This file is **not** covered by the workspace's MIT / Apache-2.0 /
LGPL-3.0 licensing. The Rust code that parses and consumes it (in
`crates/bsl-platform/src/` and
`crates/bsl-platform/tools/html-parser/`) is the project's own work and
remains under the crate license declared in `Cargo.toml`.

Downstream consumers who redistribute the final binary of `bsl-analyzer`
inherit an obligation to respect 1C's rights over this content.
Stripping this file (`BSL_PLATFORM_PATH` unset and no
`platform_data.json` present) causes the build to fall back to empty
structures; see `crates/bsl-platform/build.rs`.

## Machine-readable global catalog

`global_catalog.json` schema version 1 was structurally extracted from the
platform-versioned resources bundled with 1C:EDT 2026.1.2.2:

- `resources/v8.3.27/GlobalContext.type`, SHA-256
  `1e1aeb9c893ade97c45a77aa74ad8b3c82847f9477a3639e51f2ad704e416349`;
- `resources/v8.3.27/SystemEnums.type`, SHA-256
  `6291d183e5ba4bf2cc8fa1525627d8fa1f3bcbf658ee197fc6cc9b3884baa036`.

The manifest attests complete **name/kind/availability** coverage of the global
context and system-enumeration surface for the 8.3.27 release line. It contains
507 function entries, 100 properties and 628 system-enumeration entries. A few
compatibility aliases intentionally collide; deterministic platform precedence
is retained and a collision can never turn a known spelling into an absence.

Capabilities are derived from the model plus the EDT compile probe documented in
`docs/architecture/UNRESOLVED_NAME_SEMANTICS.md`: functions are callable but not
values, global properties and system enumerations are readable values, and only
EDT properties marked writable are assignable.

`PlatformGlobalCatalog::status_for_target()` reports `Complete` only for 8.3.27
(including an optional fourth build component). A different or malformed target
reports `UnsupportedTarget`; absence-based diagnostics remain suppressed. When
the manifest is unavailable the build emits an empty catalog with status
`Missing`.

## How to regenerate

### Rich HBK corpus

1. Install 1C:Enterprise and ensure `shcntx_ru.hbk` and
   `shlang_ru.hbk` are available in the platform directory.
2. Point `BSL_PLATFORM_PATH` at that directory, or let the build script
   discover it automatically on Linux / Windows / macOS.
3. Running `cargo build` will invoke `html-parser` to re-extract the data.

### Exact EDT catalog

Extract `GlobalContext.type` and `SystemEnums.type` from the matching
`com._1c.g5.v8.dt.platform_v<version>` EDT plug-in, then run:

```bash
python3 scripts/extract-edt-platform-globals.py \
  --global-context /path/GlobalContext.type \
  --system-enums /path/SystemEnums.type \
  --platform-version 8.3.27 \
  --edt-version 2026.1.2.2 \
  --extracted-at YYYY-MM-DD \
  --expected-functions 507 \
  --expected-properties 100 \
  --expected-system-enums 628 \
  --output crates/bsl-platform/data/global_catalog.json
```

The extractor parses XML structurally, unions duplicate compatibility entries,
sorts deterministically and records both source hashes. The expected counts are
an independent attestation gate: extraction fails instead of marking a truncated
or drifted EDT resource complete, and `build.rs` rechecks the committed manifest
against those counts.

## References

  * Upstream archives (proprietary): part of the 1C:Enterprise platform
    distribution; license terms apply to each 1C installation.
  * Extraction tool: `crates/bsl-platform/tools/html-parser/`.
  * Build integration: `crates/bsl-platform/build.rs`.
