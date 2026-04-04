# Publishing

## Purpose

This document defines the target publication model for centralized search
snapshots in PostgreSQL.

Publication is the process that transforms source corpus content into immutable
shared snapshots and updates branch heads.

## Publication Principles

The publication pipeline should satisfy these rules:

1. publication is deterministic
2. snapshots are immutable
3. branch heads are updated atomically after successful publication
4. publication reuses existing file objects and embeddings whenever possible
5. publication does not mutate older snapshots in place

## Shared Publication Targets

The initial expected publication targets are:

- `reference`
- `workspace-code` for `vendor`
- `workspace-code` for `develop`

Normal developer feature branches are not publication targets during interactive
MCP runtime.

## Publication Inputs

A publication job needs:

- source corpus content
- target corpus id
- branch name when applicable
- commit sha when applicable
- parent snapshot when applicable
- semantic model metadata when embeddings are generated or reused

## Publication Outputs

A successful publish produces:

- one immutable snapshot row
- snapshot-to-file bindings
- reused or newly created file objects
- reused or newly created chunk payloads
- reused or newly created embeddings
- updated branch head when publication targets a branch head

## Publication Flow

Recommended logical flow:

1. collect source documents for the target corpus
2. compute stable fingerprint for the snapshot
3. resolve parent snapshot when the branch policy expects one
4. identify reusable file objects by content fingerprint
5. create missing file objects
6. create missing chunk payloads
7. create or reuse embeddings
8. insert immutable snapshot metadata
9. bind snapshot paths to file objects
10. update branch head atomically

## Parent Snapshot Semantics

Parent snapshots are metadata about lineage, not mutable dependencies.

Examples:

- `develop` snapshot may record current `vendor` snapshot as parent lineage
- future optional feature snapshot may record `develop` snapshot as parent

Parent linkage helps with:

- auditability
- publication provenance
- future retention logic
- future delta-aware optimizations

## Reuse Rules

### File reuse

If a file content fingerprint already exists:

- reuse existing file object
- do not create duplicate content rows

### Chunk reuse

If file object is reused:

- its chunks are reused automatically

### Embedding reuse

If the same semantic payload exists for the same `(model_id, dimension)`:

- reuse existing embedding row
- do not regenerate vectors unnecessarily

## Branch Head Updates

Branch heads should be updated only after successful publication of the snapshot.

This implies:

- no partial branch head movement
- old head remains valid until the new snapshot is fully committed

## Retention Direction

Retention should be snapshot-based.

Initial intended policy:

- `reference`: retain latest, maybe previous one for safety
- `vendor`: retain a bounded number of recent published heads
- `develop`: retain a time-based window, for example 30 days

Retention should never break currently referenced branch heads.

## CI Integration

Centralized publication is designed for CI-driven operation.

Typical model:

- when `vendor` is updated, publish new `vendor`
- when changes are merged into `develop`, publish new `develop`
- when platform help version changes, publish new `reference`

This makes shared search state reproducible and team-visible.

## Operational Guarantees

The publication pipeline should provide:

- idempotent publish behavior for unchanged content
- observable statistics for reused vs created objects
- clear failure boundaries
- no mutation of already published content

## Non-Goals

This document does not yet define:

- exact SQL transaction boundaries
- exact CI YAML snippets
- exact GC implementation
- exact retry policy

Those should be documented later in roadmap and operations documents.
