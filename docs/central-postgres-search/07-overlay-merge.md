# Overlay Merge Model

## Purpose

This document defines how runtime search results are produced when shared
baseline data lives in centralized PostgreSQL and developer-specific changes
live locally.

The key design point is:

- shared baseline results come from PostgreSQL
- local branch changes come from the current workspace
- MCP merges both into one logical result set

This is an application-layer merge, not a SQL join across two databases.

## Logical Model

For workspace search, the effective runtime object is:

`LogicalWorkspaceView = SelectedBaselineSnapshot + LocalOverlayDelta`

Where:

- `SelectedBaselineSnapshot` is usually:
  - `vendor` for branch `vendor`
  - `develop` for branch `develop`
  - `develop` for branches like `feature/*`, `fix/*`, `bug/*`
- `LocalOverlayDelta` contains:
  - newly added local files
  - locally modified files
  - locally deleted files

The overlay never changes the shared baseline. It only changes the effective
view seen by the runtime.

## Overlay Semantics

The overlay must support three operations.

### 1. Add

If a file exists locally but does not exist in the selected baseline:

- it is included only from the local workspace
- no baseline result exists for that path

### 2. Replace

If a file exists both locally and in the selected baseline, but local content is
different:

- the local file replaces the baseline version logically
- baseline hits for that path must not appear in the final result set
- local hits for that path are eligible for the final result set

### 3. Delete

If a file exists in the selected baseline but does not exist locally:

- the file is considered hidden by the overlay
- baseline hits for that path must not appear in the final result set

## Runtime Inputs

For workspace search, MCP consumes two search sources.

### Source A: Shared baseline search

Data source:

- centralized PostgreSQL

Search types:

- lexical baseline search
- semantic baseline search

Scope:

- selected snapshot for `workspace-code`

### Source B: Local overlay search

Data source:

- current local workspace

Search types:

- lexical search over locally changed/new files
- semantic search over locally changed/new files, when semantic runtime is
  available

Scope:

- only files represented by the overlay delta

## Lexical Merge Algorithm

### Baseline phase

1. Select the correct shared baseline snapshot using branch policy.
2. Query PostgreSQL lexical index for the requested terms.
3. Request more than the final limit to allow later filtering and merging.

### Overlay phase

1. Build or refresh local overlay state.
2. Run lexical search only on local overlay documents.

### Filtering phase

Before combining results, remove from baseline result set:

- all hits whose path is in `deleted_paths`
- all hits whose path is in `replaced_paths`

This ensures that once a local file replaces or deletes a baseline file, the
baseline version becomes invisible.

### Merge phase

1. Combine filtered baseline hits and local overlay hits.
2. Normalize scores into one comparable ranking scale.
3. Sort descending by merged score.
4. Deduplicate by `(path, symbol, line range)` if necessary.
5. Truncate to requested limit.

## Semantic Merge Algorithm

Semantic merge follows the same conceptual steps, but source data differs.

### Baseline semantic source

- query embeddings and nearest-neighbor results come from PostgreSQL via
  `pgvector`

### Overlay semantic source

- local changed files are chunked and embedded locally
- local vector hits are computed locally

### Filtering and merge

Use the same path-hiding semantics as lexical merge:

- baseline semantic hits from replaced paths are removed
- baseline semantic hits from deleted paths are removed
- local semantic hits are merged into the final ranked set

## Score Normalization

Because results come from two different search engines, raw scores must not be
trusted as directly comparable.

The merge layer must therefore define a stable normalization strategy.

The first acceptable implementation can be simple:

- keep lexical merge separate from semantic merge
- normalize each source into a bounded score range
- bias local overlay results slightly when scores are effectively tied

Later refinements may include:

- source-specific score calibration
- exact symbol match boosting
- reciprocal rank fusion
- reranking after merge

The key invariant is:

- local replacement must win over hidden baseline results by path semantics
- not by accidental score advantage

## Why Merge in Application Layer

The runtime intentionally does not attempt a cross-database search join because:

- PostgreSQL stores the shared baseline
- local overlay is derived from current uncommitted workspace state
- uncommitted local state should not be pushed to the server on every change
- application-layer merge keeps developer feedback fast and local

This model also reflects product reality:

- baseline is shared, stable, and published
- overlay is private, fast-changing, and local

## Result Guarantees

The merge layer should preserve these guarantees.

1. If a baseline file is deleted locally, it must not appear in final results.
2. If a baseline file is modified locally, only the local version may appear in
   final results.
3. New local files must appear in final results even though they are absent from
   the baseline.
4. Shared baseline relevance should still dominate for all unaffected files.
5. Local changes should become visible without remote publication.

## Operational Advantages

This merge model deliberately preserves:

- centralized storage for shared corpora
- fast local response for active development
- support for uncommitted workspace changes
- low write pressure on centralized PostgreSQL during normal development

## Known Tradeoffs

This model introduces explicit complexity in:

- score normalization
- merge determinism
- testing of hidden/replaced path semantics
- observability for baseline query vs overlay query vs final merge

That complexity is accepted because it avoids a worse tradeoff:

- pushing every transient developer branch state into centralized storage

## Testing Implications

The merge layer should be validated with dedicated tests for:

- unchanged baseline with empty overlay
- added file overlay
- replaced file overlay
- deleted file overlay
- lexical baseline hits filtered by local replacement
- semantic baseline hits filtered by local replacement
- score ordering when both baseline and overlay produce hits
- degraded semantic mode where lexical remains available

## Implementation Guidance

The first implementation should prioritize correctness over ranking perfection.

Recommended order:

1. implement deterministic path hiding and replacement semantics
2. implement simple lexical merge
3. implement semantic merge using the same path semantics
4. improve score normalization only after correctness is proven by tests
