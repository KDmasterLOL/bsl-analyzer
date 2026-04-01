# Search Baseline Publishing

**Status**: Draft
**Date**: 2026-04-01

## Goal

Give the corporate search architecture a practical write path:

- build a searchable code snapshot from one repository state;
- publish it into centralized PostgreSQL storage;
- make the snapshot selectable later by `snapshot_id`, `branch`, or `commit`.

This is intentionally simpler than the future branch-delta model. The first
useful step is a full immutable snapshot publish that CI can run after merge.

## Current CLI

`bsl-analyzer-app` now provides:

```bash
bsl-analyzer-app search baseline sync-pg \
  --source-dir . \
  --branch main \
  --commit "$CI_COMMIT_SHA"
```

Behavior:

1. Build a temporary local FTS index from the configuration sources.
2. Export indexed `workspace-code` documents from that index.
3. Ensure PostgreSQL schema/tables/indexes exist.
4. Publish one immutable snapshot into:
   - `snapshots`
   - `snapshot_items`
   - `content_objects`

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

Example `.bsl-analyzer.json`:

```json
{
  "search": {
    "baseline": {
      "backend": "postgres",
      "postgres": {
        "schema": "bsl_search"
      },
      "workspaceCode": {
        "branch": "main"
      },
      "reference": {
        "snapshotId": "reference:0.1.104"
      }
    }
  }
}
```

Rules:

- `search.baseline.backend=sqlite` or missing config keeps the local SQLite path.
- `search.baseline.backend=postgres` enables centralized baseline mode.
- environment variables no longer choose the backend for workspace mode;
  they only provide secrets and runtime overrides.
- reference profile may still use env-only PostgreSQL settings when it is started
  without a project root, because user-scope MCP installation has no
  `.bsl-analyzer.json` to read from.

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

1. backend is selected from `.bsl-analyzer.json`;
2. connection/schema are taken from env when present, otherwise from config;
3. snapshot/branch/commit are taken from env when present, otherwise from config.

## Why full snapshot first

This is not the final storage strategy. It is the smallest useful write-side
integration because it already enables:

- a central baseline for merged code;
- MCP runtime selection by branch or commit;
- a stable GitLab job contract;
- later migration to delta/slice publishing without replacing the read model.

## Recommended GitLab job

Example for a pipeline that updates the shared baseline after merge to `main`:

```yaml
publish_search_baseline:
  stage: deploy
  image: rust:1.91
  rules:
    - if: '$CI_COMMIT_BRANCH == "main"'
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

## Next steps

1. Add reference-corpus publishing for shared platform help.
2. Add branch policy helpers such as `main`, `release/*`, feature snapshots.
3. Introduce delta publishing so snapshots can reuse unchanged file mappings
   instead of rewriting the full branch state each time.
4. Add shared embedding storage keyed by `content_hash + model_id`.
