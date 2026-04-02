# Operations

## Purpose

This document defines the operational direction for the central PostgreSQL
search architecture.

It is intentionally focused on deployment, maintenance, observability, and
safety rather than on logical schema design.

## Deployment Model

The expected production model is:

- one dedicated PostgreSQL instance or cluster for shared search data
- required extensions installed centrally
- CI-driven publication into that PostgreSQL instance
- all developers connect to the same shared backend for published corpora

## Required PostgreSQL Features

The architecture expects the following PostgreSQL capabilities:

- standard transactional storage
- full text search using `tsvector` and GIN indexes
- approximate vector search via `pgvector`
- optional fuzzy lexical matching via `pg_trgm`

The intended extension set is:

- `pgvector`
- `pg_trgm`

## Environment and Configuration

Operational configuration should provide at minimum:

- PostgreSQL connection URL
- PostgreSQL schema name when non-default schema is used
- embedding model id
- embedding dimension
- publication credentials for CI
- read credentials for developer runtime

Recommended operational split:

- CI gets write-capable credentials
- developer runtimes get read-only or narrowly scoped read credentials wherever
  practical

## Current Implementation Gaps

The following operational pieces are still pending before the system can be
treated as a production-grade Postgres-first serving runtime:

- connection pooling is not implemented yet; the current adapter opens a new
  PostgreSQL connection per operation
- deterministic branch-head resolution is not implemented yet; the current code
  still resolves latest snapshots by timestamp
- serving-oriented lexical and semantic indexes are still planned, not active

These gaps are acceptable for the current storage-first phase, but they should
be closed before broad rollout of direct PostgreSQL serving.

## Publication Operations

Operationally, publication should be treated as a managed pipeline.

Expected triggers:

- publish `vendor` when supplier baseline is updated
- publish `develop` after merge into `develop`
- publish `reference` when platform help version changes

Operational expectations:

- publication is idempotent for unchanged content
- branch head movement is atomic
- publication reports reuse statistics and timing
- failed publication does not corrupt older snapshots

## Retention and Garbage Collection

Centralized search storage must include controlled retention and cleanup.

Expected policy directions:

- keep current active branch heads always
- keep recent `vendor` heads according to policy
- keep `develop` snapshots within configured retention window
- keep latest `reference` and optional previous safety version

GC should remove only content that is no longer reachable from retained
snapshots.

That includes:

- unreferenced file objects
- unreferenced chunk payloads
- unreferenced semantic embeddings

GC must be observable and dry-run capable before destructive execution.

## Performance Operations

The production system should be tunable and inspectable.

Important operational areas:

- lexical query plans via `EXPLAIN (ANALYZE, BUFFERS)`
- vector query latency and recall behavior
- HNSW index size and maintenance cost
- memory fit for active indexes
- publication time for representative corpora

## Monitoring Signals

At minimum, operations should track:

- publish duration
- number of reused vs created file objects
- number of reused vs created embeddings
- lexical query latency
- semantic query latency
- snapshot counts by corpus and branch
- size growth of key tables and indexes
- GC candidate counts
- failed query and failed publication counts

## Maintenance Tasks

Regular maintenance should cover:

- index health checks
- vacuum / analyze cadence
- HNSW maintenance and rebuild strategy when needed
- retention review
- GC review
- backup validation

For `pgvector` specifically, index rebuild and maintenance strategy should be
planned explicitly rather than treated as an afterthought.

## Backup and Recovery

Shared search storage is a team-level service and should be recoverable.

Operational expectations:

- regular PostgreSQL backups
- tested restore procedure
- recovery plan for accidental publication mistakes
- ability to restore branch heads and immutable snapshots consistently

Because snapshots are immutable, recovery is conceptually simpler than for
mutable per-branch storage models.

## Security Considerations

Operational security should assume:

- developers need read access to shared search data
- only controlled pipelines should publish or delete shared snapshots
- secrets for publication and embeddings must be managed outside repository
- auditability is important for branch head changes and publication events

## Rollout Strategy

Recommended rollout order:

1. deploy PostgreSQL with required extensions in a controlled environment
2. validate publication and inspection commands on test corpora
3. migrate `reference` serving first
4. migrate shared workspace baseline serving next
5. tune and observe before broad rollout

## Open Operational Questions

Still to be finalized later:

- exact sizing guidance for snapshot growth
- exact HNSW tuning values for production workloads
- backup retention windows
- on-call and failure escalation path for shared search service
