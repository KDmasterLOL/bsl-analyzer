# Workspace Runtime

## Purpose

This document defines the runtime flow for workspace search in the central
PostgreSQL architecture.

The core rule is:

- shared published code is served from PostgreSQL
- active local branch changes are served from a local overlay
- MCP merges both into one logical workspace view

## Current Implementation Status

The current codebase is in an intermediate state:

- workspace baseline selection and branch support evaluation already exist
- MCP can load snapshot documents from PostgreSQL and construct a resolved local
  view over the selected baseline
- `find_code` can search that resolved view lexically, so overlay-aware lexical
  behavior is partially available today
- `search_code` still executes semantic search only against the local
  `SearchEngine`, and the workspace baseline is not passed into that path at all
- direct PostgreSQL lexical serving, direct PostgreSQL semantic serving, and the
  final score-normalized merge path are still target-state work

This means the current runtime is "centralized storage plus local resolved-view
search", not yet "direct PostgreSQL baseline search plus overlay merge".

## Runtime Inputs

Workspace runtime needs the following inputs.

### 1. Project configuration

Used for:

- backend selection
- branch policy
- centralized connection settings
- stale/expired policy

### 2. Current local branch

Used for:

- selecting the shared baseline snapshot
- evaluating support status and remediation guidance

### 3. Current local workspace files

Used for:

- detecting added files
- detecting modified files
- detecting deleted files
- building local lexical and semantic overlay state

## Baseline Selection Flow

At startup, the runtime resolves the shared baseline snapshot.

Typical policy:

- `vendor` branch selects shared `vendor`
- `develop` branch selects shared `develop`
- `feature/*`, `fix/*`, `bug/*` select shared `develop`

The selection step returns:

- selected branch head or explicit snapshot
- selected `snapshot_id`
- support state such as `ready`, `stale`, or `expired`

If support state is `expired`:

- workspace search tools should not serve workspace results
- status tools should still explain the remediation path

## Overlay Build Flow

After baseline selection, the runtime builds the local overlay delta.

The overlay build must detect:

- paths added locally
- paths changed locally relative to the selected baseline
- paths deleted locally relative to the selected baseline

This delta becomes the local source for:

- lexical overlay search
- semantic overlay search when semantic runtime is available
- path hiding rules for baseline results

## Target Runtime Search Flows

### `find_code`

Target flow:

1. resolve selected shared baseline snapshot
2. evaluate support state
3. refresh local overlay state
4. query centralized lexical index in PostgreSQL for baseline hits
5. query local overlay lexical index for changed/new file hits
6. remove baseline hits hidden by overlay
7. merge and sort results
8. return final result set

### `search_code`

Target flow:

1. resolve selected shared baseline snapshot
2. evaluate support state
3. refresh local overlay state
4. query centralized semantic index in PostgreSQL for baseline hits
5. query local overlay semantic index for changed/new file hits
6. remove baseline hits hidden by overlay
7. merge and sort results
8. return final result set

### `search(status)`

Target flow:

1. report selected baseline backend and snapshot
2. report branch support state
3. report overlay state
4. report semantic readiness and degraded modes
5. provide remediation guidance when the branch is stale or expired

## Degraded Modes

The workspace runtime must support degraded but usable behavior.

### Baseline available, semantic unavailable

Expected behavior:

- `find_code` remains available
- `search_code` reports semantic unavailability or warmup status
- `search(status)` explains current runtime mode

### Baseline available, overlay semantic unavailable

Expected behavior:

- lexical merge still works
- semantic results may come only from baseline
- status must say that local semantic overlay is not ready

### Baseline unavailable

Expected behavior depends on policy:

- if PostgreSQL is required for this workspace mode, runtime should fail clearly
- if fallback is explicitly configured, SQLite fallback may be used

### Branch expired

Expected behavior:

- `find_code` and `search_code` must refuse workspace search
- `search(status)` must remain available
- response must explain that the branch should be updated from `develop`

## Caching Strategy

The target Postgres-first runtime should minimize local persistent baseline
storage.

Local persistent state may still exist for:

- overlay embedding cache
- temporary overlay lexical cache
- optional query cache

But local runtime should not require full baseline hydration into SQLite before
workspace search becomes usable.

That is one of the major architectural goals of this track. The current
implementation has not reached that point yet because it still materializes a
resolved baseline view locally before search.

## Merge Responsibility

Workspace runtime is responsible for producing one logical result set from two
physical sources.

The runtime layer must therefore own:

- baseline query orchestration
- overlay query orchestration
- hidden/replaced path filtering
- score normalization
- final ranking and truncation

This responsibility should remain explicit in implementation and in tests.

## Observability Requirements

Workspace runtime should expose enough state to diagnose performance and
correctness issues.

Useful runtime signals include:

- selected snapshot id
- selected branch and fallback branch
- support state
- count of added/replaced/deleted overlay files
- baseline query latency
- overlay query latency
- merge latency
- semantic availability state

## Expected Advantages

Compared to the current SQLite-first baseline hydration model, the target
workspace runtime should eventually provide:

- no full cold copy of shared baseline into local SQLite before first use
- one canonical shared source for published baseline data
- immediate visibility of local uncommitted changes
- lower duplication across projects and developers

## Expected Hard Parts

The hardest engineering points are expected to be:

- score normalization across baseline and overlay sources
- hidden/replaced path filtering correctness
- ensuring semantic search respects the same overlay contract as lexical search
- keeping local overlay updates fast under active editing
- choosing the right serving representation in PostgreSQL for low query latency

## Implementation Order

A pragmatic implementation order is:

1. direct centralized lexical baseline serving
2. local lexical overlay merge
3. direct centralized semantic baseline serving
4. local semantic overlay merge
5. iterative score normalization improvements
