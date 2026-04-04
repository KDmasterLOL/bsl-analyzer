# ADR: Postgres-First Shared Search Runtime

## Status

Proposed

## Context

The project already supports:

- local SQLite-backed search runtime
- centralized publication of shared baselines in PostgreSQL
- branch policy selection for `vendor`, `develop`, and developer branches
- local workspace overlays for changed and deleted files

The next architectural decision is how the long-term runtime should work for
corporate/team usage.

The main question is whether the system should continue to use PostgreSQL only
as a publication source that is synchronized into local SQLite, or whether
PostgreSQL should become the canonical shared runtime backend.

This decision is especially important for:

- one shared platform reference corpus
- one shared `vendor` baseline
- one shared `develop` baseline
- many developer branches that differ from `develop` only by a relatively small
  local delta

## Decision

Adopt a Postgres-first runtime architecture for shared search.

The decision includes the following rules:

1. PostgreSQL is the canonical shared backend for published baselines.
2. `reference` is treated as a centralized shared corpus.
3. `workspace-code` baselines for `vendor` and `develop` are published as
   immutable shared snapshots.
4. Developer branches such as `feature/*`, `fix/*`, and `bug/*` are not
   published to PostgreSQL during normal interactive MCP usage.
5. Developer branch search is represented as:
   `selected shared baseline + local overlay`.
6. Search result merge happens in the application layer, not in SQL across two
   databases.
7. SQLite remains supported only as a fallback backend for single-user or
   offline scenarios.
8. Branches are metadata on snapshots, not separate tables or schemas.

## Rationale

### Why PostgreSQL should be the shared runtime backend

- Shared corpora such as platform reference are inherently global and should not
  be reindexed separately per project.
- `vendor` and `develop` represent stable team-level baselines.
- Centralized storage enables one publication flow, one retention policy, and
  one operations surface.
- Semantic search is a natural fit for centralized vector storage using
  `pgvector`.
- Centralized baselines make CI publication straightforward and reproducible.

### Why developer branches should stay local at runtime

- Developers need immediate search visibility for uncommitted local changes.
- Re-publishing feature branch state on every MCP startup would add network
  latency and unnecessary write pressure.
- Publishing transient local branch states would complicate retention and GC.
- Most developer branches differ from `develop` by a small subset of files, so
  local overlay is cheaper and conceptually correct.

### Why not table-per-branch

- It couples branch management to physical storage layout.
- It complicates migrations, retention, indexing, and maintenance.
- It reduces reuse of shared file objects and embeddings.
- It makes branch lifecycle management operationally expensive.

Instead, branches are represented as snapshot metadata and branch heads.

## Consequences

### Positive

- Shared search data becomes centrally managed and reproducible.
- Platform reference can be stored and searched once for all users.
- `vendor` and `develop` become first-class shared corpora.
- Future CI-driven updates fit naturally into the architecture.
- Local developer runtime remains responsive for active edits.

### Negative

- Workspace search runtime becomes a hybrid model rather than a single index.
- Baseline hits and local overlay hits must be merged in application code.
- Result ranking normalization becomes an explicit design concern.
- Runtime observability must cover both remote baseline queries and local merge
  behavior.

### Neutral but important

- The system is not purely centralized at runtime.
- The true runtime object is a logical workspace view, not a single physical
  database.

## Architectural Invariants

The following invariants should hold across implementation phases:

1. Published baselines are immutable.
2. `vendor`, `develop`, and `reference` are shared corpora managed centrally.
3. Local uncommitted changes must influence search results immediately.
4. Local overlay must be able to hide or replace baseline results for the same
   path.
5. Runtime merge must be deterministic and explainable.
6. PostgreSQL schema should remain snapshot-oriented, not branch-table-oriented.

## Rejected Alternatives

### Continue with SQLite-first runtime only

Rejected because:

- shared corpora are duplicated per user and per project
- centralized publication is underutilized
- startup cost remains dominated by local cache hydration
- platform reference remains wastefully reindexed

### Publish every developer branch state to PostgreSQL at MCP startup

Rejected because:

- developers also have uncommitted changes that would still require local
  overlay
- runtime startup would add remote writes and branch churn
- retention and garbage collection would become significantly noisier

### Table-per-branch or schema-per-branch

Rejected because:

- it scales poorly operationally
- it weakens reuse and deduplication
- it encodes transient branch semantics into physical storage

## Next Steps

Implementation should proceed in stages:

1. document the overlay merge model explicitly
2. design PostgreSQL storage schema around immutable snapshots and deduplicated
   file objects
3. move `reference` runtime to direct centralized serving first
4. move workspace baseline runtime to direct centralized serving with local
   overlay merge
5. keep SQLite as a supported fallback, but not as the primary target
