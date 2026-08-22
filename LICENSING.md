# Licensing

This workspace is dual-licensed per crate. The default license for new
code is `MIT OR Apache-2.0`; a smaller set of crates that still carry
historical derivative-risk from `bsl-parser` remains under
`LGPL-3.0-or-later` until a clean-room rewrite is complete.

The shipped LSP server binary (`bsl-analyzer-app`) statically links
both tiers and is therefore distributed under `LGPL-3.0-or-later`.

Provenance analysis lives in `docs/legal/`. Start with
`docs/legal/sdbl-provenance-2026-07-audit.md`: it is the current position, it
supersedes the April 2026 estimates on the state of the code, and it carries the
exit criteria for moving a crate from Tier B to Tier A.

In short: the SDBL and BSL grammar layers were originally written with the
upstream `bsl-parser` grammar files open — this is established by this
repository's own history, not inferred. The SDBL layer has since been largely
rewritten slice by slice and now diverges from upstream in structure; the
rewrite is incomplete in identifiable places, listed in the audit.

## Tier A — `MIT OR Apache-2.0`

SPDX: `MIT OR Apache-2.0`. Anyone may take any of these crates and
redistribute it under either MIT or Apache-2.0.

Two caveats apply to the phrase "in isolation", which earlier versions
of this document used without qualification:

- **A crate is only as reusable as its dependencies.** `hir-ty`, `hir`
  and `dataflow` depend on `cfg`; fourteen crates depend on
  `bsl-metadata`; several depend on Tier B crates. Taking such a crate
  means taking its dependency tree under whatever those crates are
  licensed. The per-crate SPDX describes that crate's own code, not the
  terms on which the resulting build can be redistributed.
- **`cfg` and `bsl-metadata` are under review.** Both were written with
  a copyleft-licensed project open as the working reference; see the
  notice in `NOTICE`. Their tier may change.

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
| `parser` | BSL grammar in `src/grammar/*.rs`; SDBL rule naming (exit-criteria item 7). The SDBL recovery layer was attested in July 2026 (Slice 12) | `docs/legal/parser-bsl-grammar-audit.md`, `docs/legal/sdbl-provenance-2026-07-audit.md` |
| `lexer` | Shares the crate with the BSL lexer. The SDBL side is complete: every `SdblTokenKind` variant is covered by an attestation (Slices 1–5) | `docs/legal/sdbl-provenance-2026-07-audit.md` |
| `sdbl-hir` | Assessed as medium risk in April 2026, never compared line by line; parts of the lowering were written against `bsl-language-server` | `docs/legal/parser-sdbl-hir-audit.md`, `docs/legal/sdbl-provenance-2026-07-audit.md` |
| `ide-diagnostics` | 17 diagnostics depend on the SDBL parser chain | `docs/legal/ide-diagnostics-licensing-summary.md` |
| `bsl-analyzer` | Top-level LSP server, statically links the crates above | — |

A Tier B crate moves to Tier A when its clean-room replacement is
complete and the corresponding provenance note is updated in
`docs/legal/`. The concrete checklist is in
`docs/legal/sdbl-provenance-2026-07-audit.md`, section “Exit criteria”.

Note that finishing the SDBL work is not by itself sufficient for `parser`
and `lexer`: both crates also host the BSL layer, which has the same
provenance and no rewrite plan.

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
   writing replacement code. Consulting them to audit what was or was
   not derived is a separate activity: it is permitted, it must be
   recorded in `docs/legal/`, and whoever performs it must not also
   write replacement code afterwards without a clean context.
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
| `crates/parser/tests/fixtures/Module.bsl` | ООО «1С-Софт» | CC BY 4.0. Licence header preserved in the file and gated by a test; body modified — see below |
| `crates/bsl-metadata/fixtures/designer/Catalogs/Справочник1/Commands/Команда1/Ext/CommandModule.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Catalogs/Справочник1/Ext/ManagerModule.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Catalogs/Справочник1/Ext/ObjectModule.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Catalogs/Справочник1/Forms/ФормаВыбора/Ext/Form/Module.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Catalogs/Справочник1/Forms/ФормаСписка/Ext/Form/Module.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Catalogs/Справочник1/Forms/ФормаЭлемента/Ext/Form/Module.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Catalogs/СправочникСМенеджером/Ext/ManagerModule.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Catalogs/СправочникСМенеджером/Ext/ObjectModule.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/CommonModules/ГлобальныйСерверныйМодуль/Ext/Module.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/CommonModules/КлиентскийОбщийМодуль/Ext/Module.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Documents/Документ1/Forms/ФормаВыбора/Ext/Form/Module.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Documents/Документ1/Forms/ФормаДокумента/Ext/Form/Module.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Documents/Документ1/Forms/ФормаСписка/Ext/Form/Module.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Ext/ExternalConnectionModule.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Ext/ManagedApplicationModule.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/Ext/SessionModule.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/HTTPServices/HTTPСервис1/Ext/Module.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/InformationRegisters/РегистрСведений1/Ext/ManagerModule.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/InformationRegisters/РегистрСведений1/Ext/RecordSetModule.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-metadata/fixtures/designer/WebServices/WebСервис1/Ext/Module.bsl` | bsl-language-server-rust | test fixture, added by `4601ffcb` |
| `crates/bsl-platform/data/platform_data.json` | ООО «1С-Софт» | 1C copyright, see `crates/bsl-platform/data/PROVENANCE.md` — not covered by MIT / Apache-2.0 / LGPL-3.0 |

Test material that came from `bsl-language-server` — the fixture of the
unknown-preprocessor-symbol diagnostic — is **no longer present**. It was
replaced on 2026-08-21 with material derived from section 4.8.1.2 of the 1C
Developer's Guide, so it is not an entry in the table above: the table lists
third-party content that is still bundled. See
`docs/legal/bsl-clean-room-slice-b2.md`;
`crates/ide-diagnostics/tests/retired_material.rs` fails if the retired
material reappears verbatim in any Git-tracked file of that crate, whatever
its extension.

### The body of `Module.bsl` was modified here

CC BY 4.0 asks that modifications be indicated. The file's own header records
the adaptation made before we received it. This repository changed two further
lines in `f1fc00ff` (2026-05-13), correcting an event-handler statement to the
syntax section 4.6.11.1 gives. The licence header itself has never been touched:
`crates/parser/tests/fixture_licence.rs` fails if it is removed or altered.

### Test material embedded in Rust sources

The rows above name files, not directories. An earlier revision of this table
named the seven directories those files sit in, on the reasoning that each
directory held only files from `4601ffcb`. That reasoning was wrong and the check
behind it was narrower than the claim it licensed: it counted only `.bsl` files.
`designer/Catalogs/` alone holds eighteen files — eight BSL and ten XML — and
several of them were introduced by other commits. A directory row would have
taken those out of the workspace licence and attributed them to a project that
never supplied them, which is an error in the direction of someone else's rights.

The other 15 BSL files under `crates/bsl-metadata/fixtures/` — in
`cfe_dependencies/`, `extension_common_module/` and seven `designer/`
subdirectories — came from different commits and are **not** covered by these
rows.

A second body of test material has no path of its own. Diagnostic fixtures that
were once `.bsl` files now live inside Rust test sources, and the Rust code
around them is ours. Listing those Rust files here would say the wrong thing
twice: it would put our own code outside the workspace licence, and it would
still not say which part of the file is not ours.

**What this document states about that material is a class, not a map.** The
commits that introduced those fixtures record, in their own words, that test
files were copied from `bsl-language-server` and from `bsl-language-server-rust`.
`07d2b977` deleted 187 such fixtures when the tests moved inline, and the
material of a large part of them is still present in `crates/ide-diagnostics`.

A per-file map is deliberately not published. Four independent methods of
building one were tried and each failed in its own direction — attributing a
fixture to the commit that merely moved it, crediting shared boilerplate as
surviving material, matching a line that any test could contain, and following a
rename into a sibling fixture. A textual match also cannot see material a test
builds programmatically. A map that looks precise and is repeatedly wrong is a
worse record than a class statement that is right, and the remedy for the
uncertainty is not a better map but replacement of the material, tracked in
`https://github.com/itrous/bsl-analyzer/issues/52`.

The evidence for the class statement — the commit messages, the migration commit,
and the four failure modes in full — is in
`docs/legal/bsl-clean-room-slice-b4.md`.

See `NOTICE` for upstream acknowledgements.
