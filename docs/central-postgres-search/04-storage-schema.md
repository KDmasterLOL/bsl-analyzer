# Storage Schema

## Purpose

This document defines the PostgreSQL storage model for centralized search. It
distinguishes between:

- the schema that is already implemented in the codebase today
- the additional serving-oriented structures that are still target-state work

This is important because the current implementation already has a real schema,
but direct PostgreSQL serving is not implemented yet.

## Design Principles

The storage model should follow these rules:

1. snapshots are immutable
2. branches are metadata, not tables
3. file content is deduplicated
4. chunk content is reusable across snapshots
5. embeddings are reusable across snapshots for the same semantic payload
6. serving-oriented denormalization is allowed where justified by performance

## Current Implemented Schema

The current PostgreSQL adapter persists and reads the following primary tables.

### 1. `snapshots`

Purpose:

- immutable published versions of corpora

Current fields:

- `id`
- `corpus`
- `fingerprint`
- `parent_snapshot_id`
- `branch`
- `commit_sha`
- `created_at`

Notes:

- `branch` may be null for corpus versions like `reference`
- latest snapshot lookup is currently timestamp-based via `ORDER BY created_at
  DESC LIMIT 1`
- `snapshot_heads` is not implemented yet

### 2. `snapshot_files`

Purpose:

- bind logical paths in a snapshot to deduplicated file objects

Current fields:

- `snapshot_id`
- `collection`
- `path`
- `file_fingerprint`
- `document_count`
- `file_object_id`

Constraints:

- primary key on `(snapshot_id, collection, path)`

Notes:

- this reconstructs the visible file tree for a snapshot
- path removals between snapshots are tracked separately in
  `snapshot_deletions`

### 3. `file_objects`

Purpose:

- deduplicated file-level payload metadata shared across snapshots

Current fields:

- `id`
- `collection`
- `file_fingerprint`
- `document_count`

Notes:

- one canonical file object represents one normalized file content state
- reuse happens across snapshots when file content is unchanged

### 4. `file_object_items`

Purpose:

- store structured document items for one file object

Current fields:

- `file_object_id`
- `ordinal`
- `symbol_name`
- `kind`
- `line_start`
- `line_end`
- `content_hash`

Constraints:

- primary key on `(file_object_id, ordinal)`

Notes:

- this is the current equivalent of chunk storage
- textual payload is normalized into `content_objects` rather than duplicated in
  this table

### 5. `content_objects`

Purpose:

- deduplicated text payload storage for chunk or symbol content

Current fields:

- `content_hash`
- `text`

Notes:

- multiple file-object items may reference the same content payload
- this makes the implemented schema more normalized than the earlier draft

### 6. `semantic_embeddings`

Purpose:

- store reusable semantic vectors

Current fields:

- `embedding_key`
- `model_id`
- `dimension`
- `embedding`

Constraints:

- primary key on `(embedding_key, model_id, dimension)`

Notes:

- embeddings are stored once and reused across snapshots
- this is the storage layer that direct `pgvector` serving will build upon later

### 7. `snapshot_deletions`

Purpose:

- record path deletions relative to parent snapshots

Current fields:

- `snapshot_id`
- `collection`
- `path`

Constraints:

- primary key on `(snapshot_id, collection, path)`

Notes:

- this table is part of the real implementation and must be reflected in the
  documentation
- it is important for reconstructing resolved views and for future overlay-aware
  serving

### Legacy Read Path

Older publications may still be readable through a legacy `snapshot_items`
fallback path. That compatibility path is not part of the primary target schema
and should be treated as transitional.

## Current Schema Characteristics

The implemented schema intentionally optimizes for immutable publication and
reuse:

- snapshots are immutable
- branches are metadata, not tables
- file content is deduplicated at file-object level
- chunk text is deduplicated in `content_objects`
- embeddings are reusable across snapshots

This means the current schema is already suitable for centralized storage and
publication, even though it does not yet provide serving-oriented tables for
direct PostgreSQL search.

## Target Additions For Direct Serving

The following objects are still target-state structures and should not be read
as already implemented.

### 8. `snapshot_heads`

Purpose:

- fast lookup of the current published head for a branch

Suggested fields:

- `corpus`
- `branch`
- `snapshot_id`
- `updated_at`

Notes:

- one row per active branch head
- mutable, unlike `snapshots`
- this is a target improvement over the current timestamp-based branch
  resolution

### 9. Serving-Oriented Lexical Structure

Purpose:

- direct lexical serving by snapshot without reconstructing resolved documents
  in application memory on every query

Suggested fields:

- `snapshot_id`
- `path`
- `file_object_id`
- `ordinal`
- `kind`
- `symbol_name`
- `text`
- `tsv`

Notes:

- this can be a table, materialized view, or another derived serving structure
- it does not exist in the current implementation
- it exists to optimize `find_code` and `find_docs` once direct serving starts

### 10. Serving-Oriented Semantic Structure

Purpose:

- direct semantic serving by snapshot

Suggested fields:

- `snapshot_id`
- `path`
- `file_object_id`
- `ordinal`
- `embedding_key`
- `model_id`
- `dimension`
- `embedding vector`

Notes:

- may be denormalized if query latency requires it
- may also stay join-based initially if acceptable in practice
- it does not exist in the current implementation

## Why Not Table-Per-Branch

Branch-specific tables are rejected because they:

- duplicate schema and indexes
- complicate migrations
- complicate retention
- reduce deduplication reuse
- encode transient branch semantics into physical storage

Instead:

- branch identity lives in snapshot metadata
- the active branch state should eventually live in `snapshot_heads`

## Why File-Level Dedup Is the Right First Step

The initial storage model should optimize reuse at file level because it already
matches current publication semantics and gives strong benefits:

- high reuse between `vendor` and `develop`
- simpler publication logic
- simpler retention and GC
- enough structure for future chunk-level optimization later

Chunk-level dedup may be added later, but file-level dedup is the current
recommended baseline.

## Indexing Guidance

The intended indexing strategy is:

- B-tree indexes for snapshot metadata and path lookup
- GIN index for lexical `tsvector` once serving-oriented lexical storage exists
- optional `pg_trgm` support for exact-ish symbol lookup and fuzzy matching
- `pgvector` HNSW indexes for semantic search once direct serving is introduced

Detailed DDL and tuning should be documented later in operations-specific docs.

## Retention and GC Implications

Retention should operate on snapshots, not on branches-as-tables.

Expected policy shape:

- `reference`: latest published snapshot plus optional previous safety window
- `vendor`: retain at least the most recent published heads
- `develop`: retain a time window, for example 30 days
- developer-branch snapshots: retain only if they are ever introduced later as
  an explicit feature

GC must remove unreferenced:

- file objects
- file-object items and content payloads no longer reachable through any
  snapshot
- semantic embeddings no longer referenced by active chunk payloads, subject to
  reuse policy

## Open Questions

Still to be finalized later:

- exact denormalization boundary between normalized objects and serving tables
- whether lexical serving tables are fully materialized or generated incrementally
- whether semantic serving stays join-based or uses denormalized serving rows
- exact retention windows and safety rules
