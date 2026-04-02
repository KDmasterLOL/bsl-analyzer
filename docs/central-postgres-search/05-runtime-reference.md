# Reference Runtime

## Purpose

This document defines the runtime model for the shared `reference` corpus in
the central PostgreSQL architecture.

Unlike workspace search, reference search does not need a per-project overlay.
This makes it the simplest and safest first candidate for direct Postgres-first
serving.

## Current Implementation Status

The current codebase only partially matches the target runtime:

- `reference` snapshots can already be published to and loaded from PostgreSQL
- `find_docs` can resolve a reference snapshot from PostgreSQL, materialize a
  resolved view locally, and run lexical search against that local view
- `search_docs` still executes against the local `SearchEngine`; it
  receives an external baseline parameter but intentionally ignores it
- direct PostgreSQL lexical and semantic serving for `reference` is still
  target-state work

So today `reference` uses PostgreSQL as a shared snapshot source, but not yet
as the direct search backend.

## Why `reference` Is Simpler

The `reference` corpus differs from `workspace-code` in several important ways:

- it is shared by all users
- it is not tied to one developer branch
- it does not depend on uncommitted local code changes
- it does not require hidden/replaced/deleted path semantics

Because of that, `reference` runtime should be fully centralized.

## Runtime Inputs

Reference runtime needs:

- centralized PostgreSQL connection settings
- corpus selection for `reference`
- optional snapshot selection when a non-head snapshot is requested
- semantic model selection for vector queries

It does not require:

- workspace root
- local overlay state
- branch policy for feature branches

## Target Runtime Flow

### `find_docs`

Target flow:

1. resolve the active `reference` snapshot
2. run lexical search directly in PostgreSQL
3. return results without local merge

### `search_docs`

Target flow:

1. resolve the active `reference` snapshot
2. compute query embedding locally
3. run semantic nearest-neighbor search directly in PostgreSQL via `pgvector`
4. return results without local merge

### `search(status)`

Target flow:

1. report backend as PostgreSQL
2. report selected reference snapshot
3. report snapshot freshness and metadata
4. report semantic availability state

## Expected Properties

Once direct serving is implemented, reference runtime should have these
properties:

- no per-project reindexing
- no local baseline hydration before search is usable
- one canonical shared help corpus for all developers
- one publication pipeline for platform help updates

## Failure Modes

### PostgreSQL unavailable

Expected behavior:

- reference search fails clearly
- status response explains connection issue
- fallback is used only if explicitly configured

### Semantic unavailable

Expected behavior:

- `find_docs` remains available
- `search_docs` reports semantic unavailability
- status explains current degraded mode

## Why This Is the First Migration Target

Reference runtime is the best first Postgres-first serving target because:

- it exercises centralized lexical serving
- it exercises centralized vector serving
- it does not require overlay merge
- it is shared across all projects and therefore has immediate value

A successful direct reference runtime validates the serving architecture before
workspace merge complexity is introduced.
