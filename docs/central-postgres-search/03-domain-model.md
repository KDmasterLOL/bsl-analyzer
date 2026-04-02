# Domain Model

## Purpose

This document defines the core domain entities for the central PostgreSQL search
architecture. The goal is to separate stable architectural concepts from any one
runtime implementation or SQL schema revision.

## Core Concepts

### Corpus

A corpus is a logical search domain.

Planned shared corpora:

- `reference`
- `workspace-code`

Properties:

- each corpus has its own publication lifecycle
- each corpus may have its own retention policy
- branch policy is relevant mainly for `workspace-code`

### Snapshot

A snapshot is an immutable published state of one corpus.

Properties:

- identified by `snapshot_id`
- belongs to one corpus
- may reference `branch`, `commit`, and `parent_snapshot_id`
- has one stable `fingerprint`
- is created once and never mutated in place

Invariants:

- snapshot identity is immutable
- snapshot contents are immutable
- replacing a branch head creates a new snapshot, not an in-place update

### Branch Head

A branch head is the currently selected snapshot for a named branch.

Examples:

- `vendor -> snapshot X`
- `develop -> snapshot Y`

Properties:

- mutable pointer
- branch-specific
- used for fast runtime resolution

Important distinction:

- snapshot is immutable content
- branch head is mutable selection metadata

### File Object

A file object is a deduplicated published file payload.

Properties:

- represents one canonical file content state
- may be referenced by many snapshots
- is identified by stable content fingerprint/hash

Why it exists:

- avoids rewriting identical file payloads across snapshots
- allows efficient reuse across `vendor`, `develop`, and future branches

### Snapshot File

A snapshot file binds a logical path in one snapshot to one file object.

Properties:

- belongs to one snapshot
- has one visible path
- points to one file object

Meaning:

- snapshot-level file tree is reconstructed from snapshot files
- file reuse happens below this layer through file objects

### Chunk

A chunk is a searchable unit derived from a file object.

For `workspace-code`, a chunk is typically:

- module header
- procedure
- function

For `reference`, a chunk is typically:

- type
- method
- global function
- documentation entry

Properties:

- chunk belongs to one file object
- chunk has text payload for lexical search
- chunk has semantic payload for embeddings
- chunk has stable chunk-local identity inside its file object

### Embedding Payload

An embedding payload is the semantic representation of a chunk.

Properties:

- bound to stable `embedding_key`
- bound to one `(model_id, dimension)` pair
- reusable across snapshots if semantic payload is unchanged

This layer is intentionally separate from snapshots and file objects because:

- one chunk payload may be reused by many snapshots
- embeddings may need regeneration when model or dimension changes

### Baseline Selection

Baseline selection is the runtime decision that maps a workspace branch to one
shared snapshot.

Examples:

- `vendor` selects `vendor`
- `develop` selects `develop`
- `feature/*` selects `develop`

Properties:

- policy-driven
- evaluated at runtime
- may produce warnings like `stale`
- may produce hard denial like `expired`

### Local Overlay Delta

Local overlay delta is the set of local workspace changes relative to the
selected baseline snapshot.

It contains:

- added files
- replaced files
- deleted files

Properties:

- private to one local runtime
- can include uncommitted changes
- changes frequently
- must not require centralized publication during normal work

### Logical Workspace View

Logical workspace view is the effective runtime view searched by MCP.

Definition:

- `logical workspace view = selected baseline snapshot + local overlay delta`

This is the central runtime abstraction for workspace search.

## Entity Relationships

The intended relationships are:

- one corpus has many snapshots
- one branch head points to one current snapshot
- one snapshot has many snapshot files
- one snapshot file points to one file object
- one file object has many chunks
- one chunk may have many embeddings across different models
- one runtime selects one snapshot and applies one local overlay delta

## Runtime Search Sources

Workspace runtime uses two sources.

### Shared baseline source

Contains:

- centralized lexical data
- centralized semantic data
- branch and snapshot metadata

Scope:

- stable published content only

### Local overlay source

Contains:

- local lexical data for changed files
- local semantic data for changed files when available
- hidden/deleted path information

Scope:

- current local developer delta only

## Domain Invariants

The following invariants should remain true regardless of implementation stage.

1. Shared baselines are immutable.
2. Branch heads are mutable pointers to immutable snapshots.
3. File objects are reusable across snapshots.
4. Chunk payload identity is stable enough to support embedding reuse.
5. Local overlay never mutates centralized shared state during normal search.
6. The final runtime result is computed from a logical view, not from one
   storage backend alone.
7. Replaced and deleted local paths always dominate baseline visibility.

## Non-Goals of This Model

This document does not define:

- physical SQL column names
- exact indexing strategy
- exact rank formula
- exact MCP tool contracts

Those belong to storage and runtime documents.
