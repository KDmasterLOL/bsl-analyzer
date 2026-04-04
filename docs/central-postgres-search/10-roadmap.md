# Roadmap

## Purpose

This document defines a phased implementation roadmap for the central
PostgreSQL search architecture.

The goal is to move in controlled stages from the current hybrid runtime toward
an intentionally designed Postgres-first system, while keeping the current
project usable during transition.

## Status Legend

- implemented: present in the codebase today
- partial: present in meaningful form, but not yet matching target runtime
- planned: documented target, not yet implemented

## Strategic Goal

Reach a runtime where:

- shared `reference` is served directly from PostgreSQL
- shared workspace baselines for `vendor` and `develop` are served directly from
  PostgreSQL
- developer branch relevance comes from local overlay merge
- SQLite remains only as a fallback backend

## Phase 1. Architecture Freeze

Status: partial

Deliverables:

- ADR for Postgres-first runtime
- domain model
- storage schema design
- runtime documents for `reference`, `workspace`, and overlay merge

Success criteria:

- architectural invariants are explicit
- table-per-branch is rejected formally
- merge model is accepted as the intended runtime behavior

## Phase 2. PostgreSQL Storage Model

Status: partial

Deliverables:

- schema migrations for snapshots, file objects, snapshot files, content
  payloads, snapshot deletions, and embeddings
- branch head support
- optional serving-oriented lexical and semantic structures
- basic administration and inspection commands

Success criteria:

- centralized storage schema exists and is testable in isolation
- publication can persist immutable snapshots with reuse

Current notes:

- immutable snapshot storage is implemented
- the real schema already includes `content_objects`, `file_object_items`, and
  `snapshot_deletions`
- `snapshot_heads` is still pending
- serving-oriented lexical and semantic structures are still pending

## Phase 3a. Load-All Runtime Over PostgreSQL Snapshots

Status: partial

Deliverables:

- resolve published snapshots from PostgreSQL at runtime
- materialize resolved baseline views locally
- use those views for lexical search where practical
- keep local runtime compatible while storage and publication mature

Success criteria:

- PostgreSQL acts as the shared baseline source
- lexical runtime can consume published snapshots without a new publication
  model

Current notes:

- this is the state the codebase is closest to today
- it should be treated as an explicit transition phase, not as the final
  Postgres-first runtime

## Phase 3b. Reference Publication and Direct Serving

Status: planned

Deliverables:

- publication pipeline for `reference`
- direct PostgreSQL lexical serving for `find_docs`
- direct PostgreSQL semantic serving for `search_docs`
- status reporting for centralized `reference`

Success criteria:

- reference MCP no longer requires per-project indexing
- shared platform help is served from one centralized source

## Phase 4. Workspace Baseline Publication

Status: partial

Deliverables:

- stable publication pipeline for `vendor` and `develop`
- branch head updates
- retention-safe snapshot management

Success criteria:

- shared workspace baselines are published by CI and queryable directly

Current notes:

- publication and snapshot lineage exist
- branch-head updates are still not modeled as an explicit `snapshot_heads`
  table

## Phase 5. Serving Prerequisites

Status: planned

Deliverables:

- deterministic `snapshot_heads` resolution
- connection pooling for PostgreSQL access
- explicit serving contracts for lexical and semantic overlay merge
- observability for baseline-query, overlay-query, and merge latency

Success criteria:

- branch resolution is atomic and deterministic
- PostgreSQL access pattern is suitable for repeated runtime queries
- direct serving work can proceed without reworking the storage contract

## Phase 6. Direct Lexical Workspace Serving

Status: planned

Deliverables:

- baseline lexical search directly from PostgreSQL
- local lexical overlay search
- hidden/replaced/deleted path filtering
- merged `find_code`

Success criteria:

- `find_code` no longer depends on full local baseline hydration
- local branch changes remain visible immediately

## Phase 7. Direct Semantic Workspace Serving

Status: planned

Deliverables:

- baseline semantic search directly from PostgreSQL using `pgvector`
- local semantic overlay for changed files
- merged `search_code`
- degraded runtime handling and status reporting

Success criteria:

- semantic workspace search works without full local semantic cache hydration
- merge correctness is covered by tests

## Phase 8. Tuning and Operations

Status: planned

Deliverables:

- benchmark suite
- explain-analyze-based query tuning
- HNSW tuning guidance
- retention and garbage collection jobs
- production operations documentation

Success criteria:

- query latency is acceptable on representative corpora
- maintenance flows are documented and repeatable

## Phase 9. SQLite Fallback Cleanup

Status: planned

Deliverables:

- explicit fallback policy in configuration
- reduced coupling between primary runtime and SQLite assumptions
- clear tests for fallback behavior

Success criteria:

- SQLite remains supported but no longer shapes the primary architecture

## Cross-Cutting Concerns

The following themes must be revisited in multiple phases:

- score normalization
- merge determinism
- observability
- retention safety
- semantic model versioning

## Sequencing Advice

Recommended execution order:

1. finish documentation and architecture acceptance
2. complete storage and publication gaps
3. make the transition phase explicit in code and docs
4. add serving prerequisites such as branch heads and pooling
5. migrate `reference` first
6. migrate workspace lexical path
7. migrate workspace semantic path
8. tune and operationalize

## Definition of Success

This roadmap is successful when developers can use shared search with these
properties:

- one centralized source for reference and published baselines
- fast local visibility of uncommitted workspace changes
- no need to republish feature branch state for normal development
- understandable status and failure modes
- manageable operational cost
