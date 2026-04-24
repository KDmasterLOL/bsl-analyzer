# Platform data provenance

## File

`platform_data.json`

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

All content in `platform_data.json` — in particular the
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

## How to regenerate

1. Install 1C:Enterprise and ensure `shcntx_ru.hbk` and
   `shlang_ru.hbk` are available in the platform directory.
2. Point `BSL_PLATFORM_PATH` at that directory, or let the build script
   discover it automatically on Linux / Windows / macOS.
3. Running `cargo build` will invoke `html-parser` to re-extract the
   data into this file.

## References

  * Upstream archives (proprietary): part of the 1C:Enterprise platform
    distribution; license terms apply to each 1C installation.
  * Extraction tool: `crates/bsl-platform/tools/html-parser/`.
  * Build integration: `crates/bsl-platform/build.rs`.
