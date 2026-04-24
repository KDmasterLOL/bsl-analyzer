# Licensing

This workspace is dual-licensed per crate. The default license for new
code is `MIT OR Apache-2.0`; a smaller set of crates that still carry
historical derivative-risk from `bsl-parser` remains under
`LGPL-3.0-or-later` until a clean-room rewrite is complete.

The shipped LSP server binary (`bsl-analyzer-app`) statically links
both tiers and is therefore distributed under `LGPL-3.0-or-later`.

Provenance analysis lives in `docs/legal/`.

## Tier A — `MIT OR Apache-2.0`

SPDX: `MIT OR Apache-2.0`. Anyone may take any of these crates in
isolation and redistribute under either MIT or Apache-2.0.

| Crate | Purpose |
|---|---|
| `syntax` | Rowan-based lossless CST wrapper |
| `base-db` | Salsa foundation, VFS integration |
| `vfs`, `vfs-notify` | Virtual file system and file watching |
| `project-model` | Project configuration loader |
| `intern`, `stdx`, `profile`, `line-index`, `paths` | Utility crates |
| `cfg`, `cfg-types`, `dataflow` | Control-flow graph and dataflow analysis |
| `hir-def`, `hir-ty`, `hir` | High-level IR: ItemTree, SymbolTree, type inference |
| `ide-db`, `ide-assists`, `ide` | IDE database and high-level IDE API |
| `bsl-metadata` | Configuration XML/Config parsing |
| `bsl-platform` | Platform types and methods catalog |
| `bsl-search`, `symbol-info` | Search and symbol indexing |
| `test-fixture`, `test-utils` | Test infrastructure |
| `mcp-server` | MCP protocol server |
| `bsl-debug` | DAP protocol support |
| `bsl-launcher` | Process launcher |
| `naparnik` | AI completion integration layer |
| `onec-client` | 1C integration client |
| `xtask` | Workspace tooling |

## Tier B — `LGPL-3.0-or-later`

SPDX: `LGPL-3.0-or-later`. These crates are kept under the upstream
license because they currently contain grammar, token or test material
that traces back to the `bsl-parser` project (LGPL-3.0-or-later).

| Crate | Blocker | Tracking document |
|---|---|---|
| `parser` | SDBL grammar in `src/grammar/sdbl/`, BSL grammar in `src/grammar/*.rs` | `docs/legal/parser-bsl-grammar-audit.md`, `docs/legal/parser-sdbl-select-audit.md` |
| `lexer` | SDBL token inventory in `src/sdbl.rs` | `docs/legal/sdbl-lexer-audit.md` |
| `sdbl-hir` | Depends on the `parser` and `lexer` SDBL layers | `docs/legal/parser-sdbl-hir-audit.md` |
| `ide-diagnostics` | 17 diagnostics depend on the SDBL parser chain | `docs/legal/ide-diagnostics-licensing-summary.md` |
| `bsl-analyzer` | Top-level LSP server, statically links the crates above | — |

A Tier B crate moves to Tier A when its clean-room replacement is
complete and the corresponding provenance note is updated in
`docs/legal/`.

## Clean-room replacement policy

When working on code that is about to move from Tier B to Tier A:

1. The implementation source of truth is:
   - official 1C documentation (https://its.1c.ru/db/pubqlang,
     https://its.1c.ru/db/v8std);
   - independently authored local specifications
     (see `docs/legal/sdbl-select-mini-spec.md`);
   - observed local parser behavior only where it is explicitly
     preserved for IDE or recovery reasons.
2. The `bsl-parser` grammar files (`BSLParser.g4`, `BSLLexer.g4`,
   `SDBLParser.g4`, `SDBLLexer.g4`) must not be consulted while
   writing replacement code.
3. Commit messages for replacement work should state the primary
   source used (for example, a specific ITS page).

The full rewrite plan is in `docs/legal/sdbl-clean-room-slices.md`.

## License files

The repository ships four license texts. Which one applies to a given
file or crate follows the per-crate SPDX identifier in that crate's
`Cargo.toml`.

| File | Applies to |
|---|---|
| `LICENSE-MIT` | Tier A crates, at the recipient's option |
| `LICENSE-APACHE` | Tier A crates, at the recipient's option |
| `LICENSE-LGPL` | Tier B crates and the shipped binary |
| `LICENSE-GPL` | Accompanies `LICENSE-LGPL` as required by the LGPL |

## External fixtures and third-party content

The workspace license does **not** cover the files below. They are
bundled for build reproducibility and interoperability with the
1C:Enterprise platform. Downstream redistribution of the final binary
inherits the obligations of the original sources.

| Path | Source | Status |
|---|---|---|
| `crates/parser/tests/fixtures/Module.bsl` | ООО «1С-Софт» | CC BY 4.0 (header preserved in the file) |
| `crates/bsl-platform/data/platform_data.json` | ООО «1С-Софт» | 1C copyright, see `crates/bsl-platform/data/PROVENANCE.md` — not covered by MIT / Apache-2.0 / LGPL-3.0 |

See `NOTICE` for upstream acknowledgements.
