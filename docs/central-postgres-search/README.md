# Central Postgres Search

This directory describes the central PostgreSQL search track: the current
storage-first implementation, the remaining serving gap, and the target
architecture where PostgreSQL becomes the canonical shared backend for
published search baselines while local developer workspaces remain overlays on
top of those baselines.

The goal of this track is to move from a SQLite-first runtime to a
Postgres-first runtime for team-wide search while preserving fast local
relevance for active feature branches and uncommitted changes.

## Scope

This documentation covers:

- centralized storage of shared baselines for `reference` and `workspace-code`
- branch selection policy for `vendor`, `develop`, and developer branches
- current runtime model and target Postgres-first serving model
- constraints that make local feature overlays necessary even in a
  Postgres-first architecture
- operational direction for future implementation

This documentation does not yet define:

- final SQL schema in full detail
- final query plans and index definitions
- production sizing numbers
- exact MCP response payloads for every degraded/runtime state

Those topics should be added later in this directory as separate documents.

## Current State Vs Target

The current codebase already provides a meaningful part of this architecture,
but it is not yet a full Postgres-first serving runtime.

### Current State

Today the implementation is best described as:

- PostgreSQL is a centralized storage backend for published snapshots.
- publication, deduplication, snapshot lineage, and garbage collection already
  work against PostgreSQL.
- MCP can resolve a published baseline from PostgreSQL and materialize a
  logical view locally.
- lexical runtime can use that resolved view for `find_code` and `find_docs`.
- semantic runtime still executes against the local `SearchEngine`, not
  directly against PostgreSQL.
- latest snapshot resolution is still timestamp-based; `snapshot_heads` is not
  implemented yet.
- PostgreSQL access currently opens a fresh connection per operation; pooling is
  still pending.

This is a valid intermediate architecture: PostgreSQL already acts as the
shared source of baseline data, but not yet as the direct search engine for all
runtime paths.

### Target State

The intended steady state is:

- PostgreSQL is the canonical shared source of truth for published search data.
- `reference` is served directly from PostgreSQL for both lexical and semantic
  search.
- `vendor` and `develop` are published as immutable shared workspace baselines.
- workspace search queries the published baseline directly in PostgreSQL and
  merges those results with a local overlay for the active checkout.
- SQLite remains only as an explicit fallback backend.

### Transition Model

The transition should be read in three layers:

- storage-first PostgreSQL: implemented
- load-all-then-search runtime over PostgreSQL snapshots: implemented
- direct PostgreSQL serving with overlay-aware merge: target, not implemented

The dedicated transition details are captured in
[Serving Transition](./11-serving-transition.md).

## Terminology

`baseline`
: A published immutable snapshot of a search corpus stored in centralized
  PostgreSQL.

`corpus`
: Logical search domain. Current planned shared corpora are:
  - `reference`
  - `workspace-code`

`snapshot`
: Immutable published version of a corpus, usually associated with branch,
  commit, parent snapshot, fingerprint, and publication timestamp.

`baseline head`
: The currently selected snapshot for a branch such as `vendor` or `develop`.

`overlay`
: Local delta between the developer's current workspace and the selected shared
  baseline. Includes new files, modified files, and deleted files.

`logical workspace view`
: Effective search view seen by MCP tools:
  `selected baseline snapshot + local overlay`.

## Document Map

- [Vision](./01-vision.md)
- [ADR: Postgres-First Runtime](./02-adr-postgres-first.md)
- [Domain Model](./03-domain-model.md)
- [Storage Schema](./04-storage-schema.md)
- [Reference Runtime](./05-runtime-reference.md)
- [Workspace Runtime](./06-runtime-workspace.md)
- [Overlay Merge Model](./07-overlay-merge.md)
- [Publishing](./08-publishing.md)
- [Operations](./09-operations.md)
- [Roadmap](./10-roadmap.md)
- [Serving Transition](./11-serving-transition.md)
- [Server Setup](./12-server-setup.md)

## Target Architecture Summary

The intended steady-state architecture is:

- PostgreSQL is the canonical shared source of truth for published search data.
- `reference` is fully centralized and does not require per-project local
  indexing.
- `vendor` and `develop` are published as immutable shared workspace baselines.
- developer branches such as `feature/*`, `fix/*`, and `bug/*` usually do not
  publish their own centralized snapshots during normal interactive work.
- MCP selects the correct shared baseline using branch policy and then applies a
  local overlay for the current checkout.
- lexical and semantic results from the shared baseline are merged with local
  overlay results in the application layer.

This means runtime search is intentionally hybrid:

- centralized baseline for shared, stable code
- local overlay for fast handling of branch-specific and uncommitted changes

## Why Local Overlay Still Exists

Even in a Postgres-first architecture, local overlay remains necessary because:

- developers work with uncommitted changes that should affect search results
  immediately
- editor-driven workflows require fast incremental updates after local file
  edits
- publishing every local branch state to PostgreSQL would add network latency,
  operational complexity, retention overhead, and noisy transient data
- feature branch runtime must remain usable when only a small subset of files
  differs from `develop`

As a result, the system is not "one database only" at runtime. It is one
centralized baseline plus one local overlay, combined into one logical search
view.
