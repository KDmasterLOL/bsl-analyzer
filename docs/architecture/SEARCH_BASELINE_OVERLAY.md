# Architecture Decision Record: Search Baseline + Overlay Model

**Status**: In Progress
**Date**: 2026-04-01
**Authors**: Codex + User

## Context

`bsl-search` currently uses a local SQLite database with:

- file hashes and chunks in relational tables;
- FTS5 for lexical search;
- embeddings stored as BLOBs;
- in-memory HNSW rebuilt from persisted embeddings on startup.

This works well for local indexing, but it does not model a corporate shared
search base where:

- platform reference data is common for all users;
- most code chunks are identical across branches;
- local workspace changes must be searchable before they are pushed to GitLab;
- central storage should be updated incrementally after merge.

## Problem

If each workspace or branch keeps a fully independent search database, then the
system duplicates nearly identical chunks and embeddings. This increases:

- indexing time;
- embedding costs;
- storage footprint;
- synchronization complexity between local and shared search data.

The core problem is not only vector storage. The system needs a domain model for
combining:

- a shared baseline snapshot;
- a local or branch-specific overlay with file replacements and deletions;
- a resolved search view used by lexical and semantic search.

## Decision

Introduce a search architecture based on `baseline + overlay + resolved view`.

### Domain concepts

- `Corpus`
  Logical search corpus such as `reference` or `workspace-code`.
- `Snapshot`
  Immutable baseline state for a corpus at a specific revision.
- `BaselineRef`
  How the runtime selects the baseline snapshot for a workspace.
- `ContentObject`
  Deduplicated chunk content identified by `content_hash`.
- `Embedding`
  Vector representation for `content_hash + model_id`.
- `SnapshotItem`
  Mapping of a content object into a logical file/document path with metadata.
- `OverlayChange`
  Local replacement or deletion relative to the baseline.
- `ResolvedView`
  Final visible set of searchable documents after applying overlay changes to a
  baseline.

### Current implementation state

The current iteration already includes:

- domain types for snapshots and overlays;
- ports for storage-agnostic baseline access and publishing;
- an in-memory resolver that merges baseline documents and overlay changes;
- local SQLite baseline adapters;
- PostgreSQL baseline read adapters;
- CLI publishing of full snapshots into PostgreSQL.

The default standalone developer path still uses local SQLite. PostgreSQL is an
additional backend for shared baseline scenarios.

## Architectural boundaries

### Domain

Pure types and invariants:

- snapshot identity;
- corpus identity;
- file-level overlay changes;
- rules for resolved visibility.

### Application

Use cases:

- resolve baseline documents into a visible view;
- replace one file with overlay content;
- delete one file from the resolved view;
- provide candidate documents to lexical and semantic search layers.

### Infrastructure

Adapters:

- current SQLite store and local HNSW index;
- future PostgreSQL catalog and vector storage;
- future GitLab ingestion worker.

### Interface

- MCP runtime;
- CLI commands;
- future ingestion CLI/service.

Interface code must depend on application services, not directly on SQLite or a
future PostgreSQL adapter.

## Why file-level overlay first

The first useful version of local changes does not require chunk-level merge.
When a file changes:

1. baseline chunks for that file become hidden;
2. the file is re-chunked locally;
3. the new chunks are exposed through the overlay.

This is enough to avoid reindexing 98% of unchanged files while keeping the
model simple and correct.

## Future direction

After the first iteration stabilizes:

1. Add a central baseline catalog and content store.
2. Keep local overlays ephemeral and workspace-specific.
3. Run GitLab ingestion after merge to update shared baselines incrementally.
4. Reuse shared `ContentObject` and `Embedding` records across branches and
   snapshots.

## Consequences

Positive:

- clean separation between domain model and storage backend;
- central storage can be introduced without another large refactor;
- local changes can be searchable without full workspace reindexing;
- shared corpora such as platform reference become natural first-class citizens.

Tradeoffs:

- the search subsystem becomes more explicit and layered;
- first iteration adds abstractions before a new backend exists;
- lexical and semantic search will later need to operate on a resolved view,
  not only on one concrete database.
