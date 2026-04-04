# Search Baseline Publishing

**Status**: Draft
**Date**: 2026-04-01

## Goal

Give the corporate search architecture a practical write path:

- build a searchable code snapshot from one repository state;
- publish it into centralized PostgreSQL storage;
- make the snapshot selectable later by `snapshot_id`, `branch`, or `commit`.

The current implementation uses file-level delta snapshots with immutable
parent lineage. PostgreSQL is treated as a shared published baseline that can
be rebuilt and republished cleanly when the storage model changes.

## Current CLI

`bsl-analyzer-app` now provides:

```bash
bsl-analyzer-app search baseline sync-pg \
  --source-dir . \
  --branch develop \
  --commit "$CI_COMMIT_SHA"
```

Behavior:

1. Build a temporary local FTS index from the configuration sources.
2. Export indexed `workspace-code` documents from that index.
3. Ensure PostgreSQL schema/tables/indexes exist.
4. Resolve one shared file object for each logical file.
5. Publish one immutable snapshot into:
   - `snapshots`
   - `snapshot_files` for changed or new files only
   - `snapshot_deletions` for tombstones relative to the parent snapshot
   - `file_objects`
   - `file_object_items`
   - `content_objects`
   - `semantic_embeddings` when embedding configuration is available

The published snapshot always supports immutable parent lineage metadata:

- when `--parent-snapshot-id` is passed, it is used as an explicit override;
- otherwise the CLI prefers the latest published snapshot from the same
  `corpus/branch`;
- for `workspace-code`, if the current branch has no published parent yet, the
  CLI falls back through `workspaceCode.policy` branch candidates such as
  `feature/* -> develop -> vendor`;
- for `reference`, the CLI falls back to the latest snapshot from the same
  corpus when branch is not specified.

Publish reuses a shared file-object store in the write path:

- each logical file is fingerprinted independently;
- snapshots point to shared `file_objects` instead of duplicating chunk mappings;
- unchanged files relative to the selected parent are not rewritten at all;
- deleted parent files are written into `snapshot_deletions`;
- visible snapshot state is reconstructed from the ancestry chain on read.

This is a clean cutover model. Older PostgreSQL full-materialized snapshots are
not preserved as a compatibility target. The supported migration path is:

1. recreate or clean the shared PostgreSQL schema;
2. republish `vendor`, `develop`, and `reference` snapshots with the new CLI;
3. let MCP runtimes resolve only snapshots published in the new delta format.

When `EMBEDDING_URL` is configured for the publishing process:

- indexed documents are converted into stable semantic payloads;
- missing embeddings are uploaded into shared PostgreSQL storage keyed by
  `embedding_key + model_id + dimension`;
- existing shared embeddings are not recomputed.

If `--snapshot-id` is omitted, the CLI derives it automatically:

- `workspace-code:<branch>@<commit>` when both `--branch` and `--commit` exist;
- `workspace-code:<commit>` when only `--commit` exists.

The CLI also supports environment fallback:

- `--pg-url` -> `BSL_SEARCH_BASELINE_PG_URL`
- `--pg-schema` -> `BSL_SEARCH_BASELINE_PG_SCHEMA`
- `--branch` -> `CI_COMMIT_BRANCH` or `CI_COMMIT_REF_NAME`
- `--commit` -> `CI_COMMIT_SHA` or `GITHUB_SHA`

Reference corpus can be published too:

```bash
bsl-analyzer-app search baseline sync-pg \
  --corpus reference
```

For `reference`, the default snapshot id is `reference:<package-version>`.

## Runtime read-side configuration

Runtime backend selection is now config-first.

Example `bsl-analyzer.toml`:

```toml
[search.baseline]
backend = "postgres"

[search.baseline.postgres]
schema = "bsl_search"

[search.baseline.workspaceCode.policy]
publishBranches = ["vendor", "develop"]

[[search.baseline.workspaceCode.policy.branches]]
match = "vendor"
selectBranch = "vendor"

[[search.baseline.workspaceCode.policy.branches]]
match = "develop"
selectBranch = "develop"
fallbackBranch = "vendor"

[[search.baseline.workspaceCode.policy.branches]]
match = "feature/*"
selectBranch = "develop"
fallbackBranch = "vendor"

[[search.baseline.workspaceCode.policy.branches]]
match = "fix/*"
selectBranch = "develop"
fallbackBranch = "vendor"

[[search.baseline.workspaceCode.policy.branches]]
match = "bug/*"
selectBranch = "develop"
fallbackBranch = "vendor"

[[search.baseline.workspaceCode.policy.branches]]
match = "*"
selectBranch = "develop"
fallbackBranch = "vendor"

[search.baseline.reference]
snapshotId = "reference:0.1.104"
```

Rules:

- `search.baseline.backend=sqlite` or missing config keeps the local SQLite path.
- `search.baseline.backend=postgres` enables centralized baseline mode.
- environment variables no longer choose the backend for workspace mode;
  they only provide secrets and runtime overrides.
- when explicit workspace `snapshotId/branch/commit` are absent, `workspaceCode.policy`
  resolves the published baseline from the current git branch and a configured fallback chain;
- reference profile may still use env-only PostgreSQL settings when it is started
  without a project root, because user-scope MCP installation has no
  `bsl-analyzer.toml` to read from.

Workspace MCP profile reads these PostgreSQL overrides:

- `BSL_SEARCH_BASELINE_PG_URL`
- `BSL_SEARCH_BASELINE_PG_SCHEMA`
- `BSL_SEARCH_BASELINE_SNAPSHOT_ID`
- `BSL_SEARCH_BASELINE_BRANCH`
- `BSL_SEARCH_BASELINE_COMMIT`

Reference MCP profile reads:

- `BSL_SEARCH_REFERENCE_PG_URL`
- `BSL_SEARCH_REFERENCE_PG_SCHEMA`
- `BSL_SEARCH_REFERENCE_SNAPSHOT_ID`
- `BSL_SEARCH_REFERENCE_BRANCH`
- `BSL_SEARCH_REFERENCE_COMMIT`

Reference profile also falls back to shared connection settings:

- `BSL_SEARCH_BASELINE_PG_URL`
- `BSL_SEARCH_BASELINE_PG_SCHEMA`

Resolution order for PostgreSQL mode:

1. backend is selected from `bsl-analyzer.toml`;
2. connection/schema are taken from env when present, otherwise from config;
3. snapshot/branch/commit are taken from env when present, otherwise from config.

To inspect the resolved runtime choice without starting MCP, use:

```bash
bsl-analyzer-app check-config bsl-analyzer.toml
```

The command now prints resolved `search.baseline` diagnostics for both:

- `workspace`
- `reference`

Including:

- selected backend (`sqlite` or `postgres`);
- resolved selection (`snapshot`, `branch`, `commit`, or local mode);
- configuration problems such as missing PostgreSQL connection string.

## End-to-End Workflow

The current operational path is:

1. Configure `bsl-analyzer.toml` with `search.baseline`.
2. Run `bsl-analyzer-app check-config bsl-analyzer.toml`.
3. Publish one or more snapshots with `search baseline sync-pg`.
4. Start or install MCP.
5. Verify runtime state with MCP `search(action=status)`.

Example:

```bash
# Validate runtime resolution before publishing
bsl-analyzer-app check-config bsl-analyzer.toml

# Publish develop baseline
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer-app search baseline sync-pg \
  --source-dir . \
  --branch develop \
  --commit "$CI_COMMIT_SHA"

# Publish vendor baseline after supplier update
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer-app search baseline sync-pg \
  --source-dir . \
  --branch vendor \
  --commit "$CI_COMMIT_SHA"

# Publish shared reference baseline
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer-app search baseline sync-pg \
  --corpus reference
```

`sync-pg` now prints:

- selected corpus;
- publish mode (`root` or `delta`);
- published snapshot id;
- target PostgreSQL schema;
- explicit or auto-selected parent snapshot id;
- branch and commit labels;
- reused/written/deleted file counters;
- reused/written chunk counters;
- reused/stored embedding counters when semantic publishing is enabled;
- indexed files, resolved files, and chunk counts.

Operational inspection commands:

```bash
# List recently published snapshots
bsl-analyzer-app search baseline list-pg --limit 20

# Filter by corpus / branch / commit
bsl-analyzer-app search baseline list-pg \
  --corpus workspace-code \
  --branch develop

# Inspect one specific snapshot
bsl-analyzer-app search baseline show-pg \
  --snapshot-id workspace-code:develop@abcdef

# Inspect shared file objects
bsl-analyzer-app search baseline list-file-objects-pg --limit 20

# Inspect one specific file object
bsl-analyzer-app search baseline show-file-object-pg \
  --file-object-id abcdef

# Inspect shared embedding inventories
bsl-analyzer-app search baseline list-embeddings-pg

# Inspect embedding coverage for active payloads
bsl-analyzer-app search baseline show-embedding-coverage-pg

# Safe garbage collection preview
bsl-analyzer-app search baseline gc-pg

# Apply garbage collection
bsl-analyzer-app search baseline gc-pg --execute

# Analyze retention policy without deleting snapshots
bsl-analyzer-app search baseline retention-pg --source-dir .
```

`list-pg` is intended for operators to verify:

- which snapshots are present in shared storage;
- what their parent lineage is;
- which branch/commit labels were recorded;
- whether the effective visible file/chunk counts and fingerprints exist.

`show-pg` adds effective per-collection counters for one snapshot after parent
lineage is applied.

Additional operator commands provide storage-level inspection:

- `list-file-objects-pg` lists shared file objects with snapshot reference counts;
- `show-file-object-pg` lists exact `snapshot_id -> path` references for one file object;
- `list-embeddings-pg` aggregates shared embeddings by `model_id + dimension`;
- `show-embedding-coverage-pg` reports how many active semantic payloads are covered
  by stored shared embeddings for each model/dimension pair;
- `gc-pg` is dry-run by default and reports orphan shared rows before deletion;
- `gc-pg --execute` deletes orphan `file_objects`, orphan `file_object_items`,
  and orphan rows from `semantic_embeddings`.
- `retention-pg` applies workspace branch policy to published snapshots and reports
  `active-head`, `safety-head`, `within-window`, and `expired-candidate` states,
  plus protections such as minimum preservation and ancestry descendants.

MCP `search(action=status)` is the runtime verification step:

- `workspace` should show `Configured baseline`, including support state and
  remediation when policy marks the branch as `stale` or `expired`, plus
  `Code lexical source`, `Code semantic source`, and `External baseline`;
- `reference` should show `Configured baseline`, `Docs lexical source`, `Docs semantic source`,
  `External baseline`, and `Freshness`.

## Centralized Workspace Semantic Search

`workspace-code` now supports semantic search in centralized baseline mode too.

The runtime model is also hybrid:

- lexical `find_code` resolves from the selected shared PostgreSQL snapshot;
- local workspace additions, edits, and deletions are applied as an overlay;
- semantic `search_code` uses a local SQLite cache synchronized from that same
  shared snapshot;
- shared PostgreSQL embeddings are loaded first when the local model matches;
- cache refresh is skipped when the normalized chunk fingerprint of one file is
  unchanged;
- local workspace changes are still embedded only for the overlay layer.

When workspace branch support is `expired`, MCP search tools no longer serve
workspace search results. `find_code` and `search_code` return a structured
error with reason code `expired_branch`, while `search(action=status)` remains
available to explain the remediation path.

This keeps PostgreSQL as the shared published baseline for team-visible code
while preserving immediate local relevance for the current checkout.

## Centralized Reference Semantic Search

`reference` now supports semantic search in centralized baseline mode.

The runtime model is intentionally hybrid:

- lexical `find_docs` resolves directly from the shared PostgreSQL snapshot;
- semantic `search_docs` uses a local SQLite cache in the user profile;
- shared PostgreSQL embeddings are loaded first when the local model matches;
- that cache is synchronized from the selected shared snapshot on startup;
- cache refresh is skipped when the snapshot fingerprint or snapshot id is unchanged.

This keeps the shared baseline as the canonical published source while avoiding
re-embedding the same platform help on every startup or in every project.

## Why file-level delta now

This is the current canonical storage strategy because it already enables:

- a central baseline for merged code;
- MCP runtime selection by branch or commit;
- parent-aware branch fallback for `vendor -> develop -> feature/*` workflows;
- deletion tracking without rewriting whole snapshots;
- file-object and embedding reuse across branches;
- a stable GitLab job contract;
- future slice-level optimization without replacing the read model.

## Recommended GitLab job

Example for a pipeline that updates the shared baseline after merge to `develop`:

```yaml
publish_search_baseline:
  stage: deploy
  image: rust:1.91
  rules:
    - if: '$CI_COMMIT_BRANCH == "develop"'
  script:
    - cargo build --release --bin bsl-analyzer-app
    - >
      ./target/release/bsl-analyzer-app search baseline sync-pg
      --source-dir .
      --branch "$CI_COMMIT_BRANCH"
      --commit "$CI_COMMIT_SHA"
```

Recommended variables:

- `BSL_SEARCH_BASELINE_PG_URL`
- `BSL_SEARCH_BASELINE_PG_SCHEMA`

## Architectural note

This publishing command belongs to the interface layer. It orchestrates:

- local indexing application services from `bsl-search`;
- PostgreSQL infrastructure adapters;
- CI-friendly parameter mapping.

It does not embed PostgreSQL logic directly into MCP runtime code.

## Roadmap

### Completed foundation

The current implementation already covers the first baseline-storage milestone:

1. Immutable PostgreSQL snapshot publishing for `workspace-code` and `reference`.
2. Parent snapshot lineage on publish and ancestry-aware read resolution.
3. Delta snapshot storage with `snapshot_files` + `snapshot_deletions`.
4. Shared `file_objects` and reuse of unchanged file mappings.
5. Shared semantic embedding storage and reuse by `model_id + dimension + embedding_key`.
6. Branch-policy-aware parent auto-selection for corporate flows such as `vendor -> develop -> feature/*`.
7. Runtime hybrid model:
   - shared PostgreSQL baseline for team-visible published state;
   - local SQLite semantic cache for MCP runtime;
   - local overlay for uncommitted workspace changes.
8. Operator commands for inspection and safe garbage collection.

### Next iteration

The next concrete milestones move from storage format to operations and scale:

1. Add a dedicated operations document for clean republish, inspection, and GC.
2. Add focused integration tests that exercise delta publish/read behavior
   against real PostgreSQL.
3. Introduce CI/GitLab automation for shared baseline publish after merge.
4. Add retention and branch-head maintenance policies on top of immutable
   snapshots.

### Deferred after operations hardening

These topics remain useful, but they are not the next implementation step:

1. Slice-level delta publication inside one file object.
2. Centralized vector search backend beyond the current shared embedding store.
3. More advanced retention and cleanup policies for shared storage.
