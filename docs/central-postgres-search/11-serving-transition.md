# Serving Transition

## Purpose

This document explains the gap between the current implementation and the
target Postgres-first runtime.

That gap is not a documentation bug only. It is a real engineering transition:
the codebase already uses PostgreSQL as centralized snapshot storage, but it
does not yet use PostgreSQL as the direct runtime search engine.

## Current Runtime Reality

The current implementation is best described as:

- PostgreSQL stores published immutable snapshots
- MCP resolves a snapshot from PostgreSQL
- MCP loads snapshot documents into application memory
- runtime search is then executed locally over that resolved view or over the
  local `SearchEngine`

This has two important consequences:

- PostgreSQL is already the source of baseline data
- PostgreSQL is not yet the direct lexical or semantic serving layer

## Current Behavior By Tool

### `find_docs`

Current behavior:

1. resolve `reference` snapshot from PostgreSQL
2. load snapshot documents into a resolved local view
3. run lexical search locally against that view

This already uses PostgreSQL as the source of truth, but not as the direct
search engine.

### `search_docs`

Current behavior:

1. use the local `SearchEngine`
2. receive an `external_baseline` parameter but ignore it at semantic query time

This means the `reference` semantic path is at least wired for a future
baseline-aware implementation, but it still does not use PostgreSQL as the
direct search backend. It is the largest gap for the `reference` corpus because
the documentation describes direct centralized semantic serving as the target.

### `find_code`

Current behavior:

1. resolve selected workspace baseline from PostgreSQL
2. materialize a resolved view with overlay semantics
3. run lexical search locally against that resolved view

This gives partial overlay correctness today, but it still depends on loading
baseline content into local runtime state.

### `search_code`

Current behavior:

1. use the local `SearchEngine`
2. do not receive or query a workspace `external_baseline` at all

This means the semantic runtime is not merely ignoring baseline data. It is not
architecturally connected to the baseline path yet, so Phase E requires a
signature-level integration step in addition to the serving implementation. The
semantic runtime has therefore not yet reached the same overlay-aware contract
that the lexical runtime is moving toward.

## Why This Intermediate Step Exists

The storage-first transition is still valuable because it already delivers:

- centralized publication
- immutable snapshots
- file-object deduplication
- reusable embeddings
- branch-oriented snapshot lineage
- garbage collection over shared storage

That is enough to validate the publication and retention model before the more
complex serving layer is introduced.

## Target Serving Model

The target runtime is different:

- lexical baseline hits come directly from PostgreSQL
- semantic baseline hits come directly from PostgreSQL
- local overlay hits come from fast local runtime state
- the application layer merges the two sources into one logical result set

In other words, the target system is not:

- "copy everything locally, then search"

It is:

- "query the centralized baseline directly, then merge with local overlay"

## Transition Phases

### Phase A. Storage-First PostgreSQL

What it means:

- centralized schema exists
- snapshots are publishable
- baseline documents are loadable

Status:

- implemented in meaningful form

### Phase B. Load-All-Then-Search Runtime

What it means:

- runtime loads snapshot content from PostgreSQL
- runtime searches that content locally

Status:

- implemented in meaningful form

### Phase C. Direct `reference` Serving

What it means:

- `find_docs` executes lexical queries directly in PostgreSQL
- `search_docs` executes vector search directly in PostgreSQL

Status:

- planned

### Phase D. Direct Workspace Lexical Serving

What it means:

- `find_code` queries baseline lexical data directly in PostgreSQL
- overlay lexical hits are queried locally
- merge enforces hide/replace/delete semantics

Status:

- planned

### Phase E. Direct Workspace Semantic Serving

What it means:

- `search_code` queries baseline vectors directly in PostgreSQL
- overlay semantic hits are queried locally
- merge enforces the same overlay contract as lexical search

Status:

- planned

## Hard Problems In The Transition

The transition is not only a matter of changing the database adapter.

### 1. Overlay Contract

When a file is replaced or deleted locally, baseline hits for that path must not
leak into final results.

This is straightforward for lexical search over a resolved view. It becomes
harder when results come from two different query engines.

### 2. Score Normalization

Baseline PostgreSQL scores and local overlay scores will not be directly
comparable.

The runtime needs:

- deterministic merge rules
- normalization or banding strategy
- stable ranking under mixed-source results

### 3. Query Fan-Out

The target runtime performs two physical queries:

- one against PostgreSQL
- one against local overlay state

The application layer must then merge, trim, and explain the final result set.

### 4. Operational Readiness

Before frequent runtime queries hit PostgreSQL directly, the platform needs:

- connection pooling
- deterministic branch-head resolution
- serving-oriented indexes and query plans

## Required Prerequisites

Before direct PostgreSQL serving becomes the default runtime, the following
should exist:

- `snapshot_heads` or an equivalent deterministic branch-head mechanism
- PostgreSQL connection pooling
- serving-specific lexical and semantic query strategy
- tests that prove overlay filtering works for both lexical and semantic search

## Documentation Rule

Until direct serving is implemented, all documents in this directory should be
read with this rule:

- PostgreSQL-as-storage is current
- PostgreSQL-as-direct-search-runtime is target
