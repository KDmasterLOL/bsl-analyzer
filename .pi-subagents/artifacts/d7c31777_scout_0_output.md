# Code Context

## Files Retrieved
1. `crates/mcp-server/src/state/sync.rs` (lines 80-337, 1093-1210, 1395-1435) - live watcher sink, config-change handling, and existing root-table rescan seams.
2. `crates/mcp-server/src/state/bootstrap.rs` (lines 90-107, 1450-1534, 2248-2763) - startup root registration fixtures and watch-before-project-read race harness.
3. `crates/mcp-server/src/state/overlay_retry.rs` (lines 1-275, 384-461) - single-flight semantic warmup driver and fresh-epoch race seam.
4. `crates/mcp-server/src/state/types.rs` (lines 1-106) - `WorkspaceSearchMode`, `OverlayWarmupState`, and `SemanticRuntimeStatus` contracts.
5. `crates/mcp-server/src/tools/search/status.rs` (lines 95-160, 686-780, 885-935, 1196-1318) - status projection and existing root/mode/warmup tests.
6. `crates/bsl-search/src/engine.rs` (lines 1284-1304, 1344-1350, 1469-1530, 1848-1915) - mutable root-table setter and consumers.
7. `crates/mcp-server/src/change_hub.rs` (lines 803-831, 1097-1109, 1218-1252) - mutable declaration/rearm machinery and test constructors.

## Key Code

- **Current failure point (blocker):** `apply_search_drift` recognizes analyzer config changes but only marks all stored context and nudges graph (`state/sync.rs:207-225`). It does **not** reload `project::at`, rebuild/install `WorkspaceRoots`, reconcile rows, or redeclare hub targets. Therefore a live add remains unkeyable, a remove remains owned/stored, and a reassignment retains its old `root_id` until restart.
- The engine already permits replacement via `SearchEngine::set_workspace_roots`; it replaces the table and clears overlay cache (`bsl-search/src/engine.rs:1299-1303`). Tests must pin the required higher-level atomic transaction because this setter alone neither migrates/deletes stale store keys nor guarantees semantic-plan invalidation.
- Bootstrap explicitly snapshots `project.source_roots()` into the hub (`state/bootstrap.rs:90-107`). The hub has `Rearm`/`Declare` control messages and tracks desired/armed declarations (`change_hub.rs:803-831,1097-1109`), so live topology handling should exercise that existing mechanism rather than restart the hub.
- Semantic warmup is single-flight. A pass snapshots state with the retry state lock released; `kick_fresh` bumps `fresh_epoch`, and stale completion must not impose backoff (`overlay_retry.rs:95-103,163-173,269-275`). `OverlayWarmupState::Superseded` is specifically the wholesale-invalidation outcome (`types.rs:47-53`).
- Status has independent state inputs. In Postgres mode `OverlaySyncing` must force “building” even if the stored warmup outcome is stale (`tools/search/status.rs:755-780`); in SqliteLocal the overlay line must stay omitted (`status.rs:755-758`).

## Acceptance / failure-first test design

### 1. End-to-end live topology matrix — place in `crates/mcp-server/src/state/sync.rs`
Add a test module beside `search_sink_config_edit_marks_whole_collection_context_dirty` (`sync.rs:1395-1435`), reusing `workspace_with_two_extensions` ideas/helper extraction from `bootstrap.rs:2248-2288` and direct `apply_search_drift` batches.

Run every transition in both modes:

| transition | required assertions after one config event + bounded settle |
|---|---|
| add `[] -> [A]` | engine roots contain A; hub desired/armed targets contain A; A file becomes keyable and lexical hit appears; in Sqlite semantic chunks are queued/indexing, in PG A is local-overlay-visible and warmup is kicked |
| remove `[A] -> []` | A root absent from table and hub; all A lexical rows and vectors removed; old A path no longer keyable; status counts/roots exclude A |
| reassign name/root-id `[A(path ext-a)] -> [A(path ext-b)]` | old physical tree and old keys disappear; new tree is watched/keyable; same relative module path does not retain old content/vector; exactly one new owner/id |
| swap `[A=ext-a,B=ext-b] -> [A=ext-b,B=ext-a]` | catches implementations comparing only names or only root count; hits resolve under new physical ownership with no duplicates |

Use two roots with the **same relative module path** as the bootstrap collision fixture explains (`bootstrap.rs:2540-2544`), and one extension outside workspace as the rescan tests do (`sync.rs:1103-1125`) to prove watcher redeclaration rather than accidental coverage by the workspace directory.

### 2. Root-table transaction unit tests — place in `crates/bsl-search/src/engine.rs` near root/removal tests (`engine.rs:4149+`, `4464+`)
Failure-first API-level tests should demand one topology-replacement operation (not bare `set_workspace_roots`) that:
1. installs new roots,
2. invalidates overlay generation/cache,
3. removes keys no longer resolvable or whose owner changed,
4. leaves unchanged-root rows intact,
5. makes added roots eligible for FTS ingest.

Positive controls: preseed same relative path under base/A/B with distinct symbols and vectors. Assert exact `(root_id,path)` row set and semantic entries, not just search hits (lexical fallback can hide missing vectors; `bootstrap.rs:2699-2703`).

### 3. Sqlite semantic warmup race — `state/sync.rs` integration + `state/bootstrap.rs` fixture
Interleaving:
1. Start SqliteLocal with embedder seam and pause deferred embedding after plan/snapshot but before publish.
2. Edit config to add A; deliver config event; topology transaction installs A and bumps generation.
3. Release old pass.
4. Assert old pass cannot set `Ready` for pre-A topology; status remains `Indexing` until a fresh pass includes A; pending documents include base and A root IDs (pattern: `bootstrap.rs:2699-2733`).
5. Remove/reassign A while the second pass is paused; release and assert no vector for removed/old owner survives.

This needs a deterministic phase barrier analogous to the existing watch hold (`bootstrap.rs:1484-1528`), not sleeps.

### 4. Postgres overlay warmup race — place in `state/overlay_retry.rs`
Extend `a_fresh_kick_during_a_pass_overrides_the_stale_backoff` (`overlay_retry.rs:384-405`) from settlement-only to a real paused pass:

- **Add during Phase A/B:** old pass snapshots roots `[base]`; config transaction installs `[base,A]` and calls `kick_fresh`; old publish must become `Superseded` (or be generation-rejected), then immediate second pass covers A. Assert `pass_count == 2`, never concurrent, and final `Synced/NoLocalDiffs` corresponds to new topology.
- **Remove during Phase B/C:** pass plans A embeddings; remove A and kick; release HTTP/embed seam; no A embedding/fingerprint may publish, second clean pass removes stale A.
- **Reassignment:** pause plan for old `root_id/path`, swap physical owner with identical relative path, release; publication must reject the old generation rather than attach old vector to new owner.

The retry driver already guarantees fresh epoch overrides stale backoff (`overlay_retry.rs:33-45,269-275`), but tests must additionally verify the engine topology generation participates in plan publication; fresh epoch alone schedules another pass but does not prevent stale publication.

### 5. Status consistency — place in `tools/search/status.rs` near `search_status_reports_overlay_sync_for_postgres_mode` and summary matrix (`status.rs:1196-1318`)
Table-drive snapshots at transaction barriers:

- before event: old root list + Ready;
- transaction begun: root topology must be reported as updating/building, never new roots with old Ready (or old roots with “current working tree”);
- PG paused warmup: `SemanticRuntimeStatus::OverlaySyncing` and overlay “building”, regardless of stale previous `Synced` (`status.rs:755-780`);
- Sqlite paused warmup: `Indexing`, no PG overlay line;
- completion: exact new root names/count, Ready, no stale removed-root stats;
- invalid edited config: keep last-good topology and status explicitly degraded/failed; never silently claim the edited topology is current.

## Architecture
Config events flow from `WorkspaceChangeHub` to `spawn_search_sink`, which drains and calls `apply_search_drift`; today that path updates graph/context only. The required live topology transaction belongs in MCP state orchestration: reload project, derive `WorkspaceRoots` and watch targets, atomically reconcile the `SearchEngine`, redeclare/rearm hub coverage, then kick the mode-specific semantic worker. `bsl-search` should own the storage/overlay invariants of replacing a root table; status remains a thin projection of coherent state.

## Commands
Focused tests after implementation:

```bash
cargo test -p mcp-server state::sync::<new_test_name> -- --nocapture
cargo test -p mcp-server state::overlay_retry::<new_test_name> -- --nocapture
cargo test -p mcp-server tools::search::status::<new_test_name> -- --nocapture
cargo test -p bsl-search <topology_replace_test_name> -- --nocapture
```

Focused crate suites:
```bash
cargo test -p mcp-server state::sync
cargo test -p mcp-server state::overlay_retry
cargo test -p mcp-server tools::search::status
cargo test -p bsl-search
```

Full validation:
```bash
cargo test --workspace
cargo clippy --all-targets --all-features -- -D warnings
```

## Start Here
Open `crates/mcp-server/src/state/sync.rs:207-225`: this is the live config-event branch that currently stops after context invalidation and is the clearest insertion point for the failure-first matrix.

## Review findings
- **blocker:** `crates/mcp-server/src/state/sync.rs:207-225` - live config changes do not update search root ownership, watch coverage, indexed rows, or semantic topology.
- **high:** `crates/bsl-search/src/engine.rs:1299-1303` - raw root replacement only clears in-memory overlay cache; without a transactional reconcile, removed/reassigned keys and vectors can remain stored.
- **high:** `crates/mcp-server/src/state/overlay_retry.rs:269-275` - fresh epoch prevents stale backoff but does not itself prove stale semantic publication is rejected after topology replacement.
- **medium:** `crates/mcp-server/src/tools/search/status.rs:686-780` - independently locked runtime/warmup/engine inputs can expose mixed-generation status unless topology update has an explicit coherent state/barrier.

## Residual risks
- No issue text/API was available locally; acceptance semantics for invalid live config (last-good vs hard failure) need confirmation, though tests should forbid silent inconsistency either way.
- Real Postgres tests should remain hermetic by using baseline/embedder fakes; a network-backed suite would be flaky and not failure-first.
- Watcher delivery timing is platform-sensitive; use `apply_search_drift`, hub acknowledgements, phase barriers, and bounded predicates rather than fixed sleeps.