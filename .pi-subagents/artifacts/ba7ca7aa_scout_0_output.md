# Code Context

## Files Retrieved
1. `crates/mcp-server/src/graph/types.rs` (lines 1-89) - graph publish signal and status contracts.
2. `crates/mcp-server/src/graph/state.rs` (lines 20-198, 229-260, 298-320, 423-455, 589-629) - published topology, hook storage, pending retry state, and nudge single-flight.
3. `crates/mcp-server/src/graph/build.rs` (lines 19-190, 216-250) - graph build snapshots the project, publishes topology, rearms hub roots, and calls the hook.
4. `crates/mcp-server/src/graph/input.rs` (lines 1-57) - `ProjectSnapshot`, the operation-scoped validated project projection.
5. `crates/mcp-server/src/project.rs` (lines 1-92) - sole project loader and canonical `WorkspaceRoots` constructor.
6. `crates/mcp-server/src/state/bootstrap.rs` (lines 80-216, 219-443, 665-676) - initial roots, graph hook construction, engine publication, overlay retry, and sink wiring.
7. `crates/mcp-server/src/state/sync.rs` (lines 1-238) - change-hub wake loop and config-change handling.
8. `crates/mcp-server/src/state/embed.rs` (lines 1-190, 460-604) - graph publish hook, topology refresh, and re-embed integration.
9. `crates/mcp-server/src/state/types.rs` (lines 1-126) - search runtime and overlay outcome state.
10. `crates/mcp-server/src/state/overlay_retry.rs` (lines 1-277) - condvar wake/backoff loop and outcome settlement.
11. `crates/mcp-server/src/tools/search/status.rs` (lines 688-787) - user-visible overlay retry/status rendering.
12. `crates/mcp-server/src/graph/snapshot.rs` (lines 1-207) - published-topology fencing when opening graph artifacts.

## Key Code

`ProjectSnapshot` already binds the graph universe and topology to one project load (`graph/input.rs:13-51`):
```rust
pub(crate) struct ProjectSnapshot {
    pub workspace_root: PathBuf,
    pub scan_roots: Vec<PathBuf>,
    pub configs: ide::WorkspaceConfigsSnapshot,
}
```

The publish signal currently carries only the topology hash, not the project-derived search roots (`graph/types.rs:14-41`):
```rust
pub(crate) struct GraphPublishSignal {
    pub(crate) drift_pending: bool,
    pub(crate) build_start_seq: i64,
    pub(crate) topology_changed: bool,
    pub(crate) topology: u64,
}
```

The search engine root table is set only at bootstrap from the bootstrap `Project` (`state/bootstrap.rs:665-676`). On a root config edit, `apply_search_drift` marks all contexts and nudges the graph (`state/sync.rs:198-228`), and a graph topology publish refreshes contexts (`state/embed.rs:476-590`), but no path replaces `engine.workspace_roots()`. Consequently a newly added extension can be in the published graph/hub while overlay ownership and warmup still use the old boot root table.

## Architecture

### Concrete wiring changes

1. **Make root-config delivery force a graph project reload, not merely rely on fingerprint inequality.**
   - In `graph/state.rs`, add a dedicated forced-project-reload bit (e.g. `pending_project_reload: Arc<AtomicBool>`) and API such as `nudge_project_reload() -> NudgeOutcome`.
   - `state/sync.rs:198-228` must call this API for `project_model::CONFIG_FILE_NAMES`; overflow/rescan should also set it because exact config detail may be lost (`sync.rs:151-174`).
   - The claim path must bypass the ordinary `Published::wants_reload` equality check. A root config can be rewritten with an equivalent graph fingerprint yet still require republishing/reinstalling the current root table. Preserve current single-flight semantics: if loading/running, latch the forced bit and reclaim after publish; clear it only when the forced build that captured it successfully publishes, not when merely scheduling.
   - Severity **high**: using only `nudge_rebuild()` (`sync.rs:228`) permits `claim_reload_slot()` to no-op when the disk fingerprint compares equal, so search roots can remain boot-stale indefinitely.

2. **Tie roots to the exact published topology.**
   - Extend the build result / publish handoff in `graph/build.rs` so the same `ProjectSnapshot` used for scan/config projection also yields the root table. Do not reload `project::at()` inside the hook: that can observe a newer config than the graph just published.
   - Preferred shape: add a cloneable published-project payload to `GraphPublishSignal`, containing `topology` plus `bsl_search::WorkspaceRoots` (and rejected roots if logging is desired), or store it in an `Arc`-backed publish payload. Construct it from the build’s `ProjectSnapshot`; add a helper beside `project::workspace_roots` that accepts snapshot-derived roots/config identity if `ProjectSnapshot` cannot retain the validated `Project`.
   - In `state/embed.rs:476-590`, after the graph-file topology fence succeeds and while holding the engine mutex, install the signalled roots **before** `mark_workspace_context_dirty_at` / `refresh_dirty_contexts`. Thus root ownership, overlay scan, and context refresh all describe the graph topology identified by `signal.topology`.
   - Cached/adopted and fused publishes must populate the same payload; otherwise warm-cache boot remains an uncovered stale-root path. `ensure_hub_roots` already demonstrates topology-fenced publication (`graph/build.rs:48-73`).

3. **Return root installation/reconcile separately from topology-context handling.**
   - Replace the hook’s `bool` with a structured outcome, e.g. `GraphPublishOutcome { topology_refresh_handled: bool, roots: RootPublishOutcome }`, where roots distinguishes `NotRequested/Installed/Retry(String)` (names flexible).
   - Keep `pending_topology_refresh` exclusively for context rerender. Add independent pending root retry state; do not encode root failure as `false`, because today that boolean only re-raises whole-collection context work (`graph/state.rs:311-320`). Root installation can fail while context marking succeeds and vice versa.
   - Root installation must be requested on initial/cached publication too, or explicitly compared against the engine’s installed topology rather than gated solely by `topology_changed` (cold publish deliberately reports no witnessed transition in `graph/build.rs:159-169`).

4. **Integrate retry with the existing wake loop.**
   - Pass `overlay_retry` into `build_publish_hook` from `bootstrap.rs:130-137` (it is currently created later at `bootstrap.rs:159-176`; construction order must move retry creation earlier, or give the hook a late-bound weak/slot handle).
   - On successful root replacement, call `kick_fresh()`: current overlay plans clone `engine.workspace_roots()` (`embed.rs:220-300`), so a fresh pass is required to ingest/drop files under the new topology.
   - On retryable root failure, retain a distinct root obligation and wake/backoff it through `OverlayRetry`; extend `should_run`/`settle_outcome` rather than spawning another worker. The current driver owns all overlay passes and condvar wakeups (`overlay_retry.rs:72-168`). Add an externally settable root obligation/outcome that is not overwritten by `OverlayWarmupState`; a clean embedding pass must not clear a failed root-reload obligation.
   - Status must expose root retry separately in `state/types.rs` and `tools/search/status.rs`; folding it into `OverlayWarmupState::Failed` falsely says embedding failed and loses which topology/root table is serving.

5. **Deterministic tests (no sleep-only assertions).**
   - `state/sync.rs`: inject a root config change while graph fingerprint is otherwise equal; assert forced outcome claims/reclaims a reload. Add loading/running case proving the force latch survives the first publish.
   - `graph/build.rs` or `graph/state.rs`: publish topology A then edit config to topology B; hook must receive B’s root IDs and matching topology from the same snapshot. Add a race seam that edits config after build snapshot and prove payload remains A (never mixed A graph/B roots).
   - `state/embed.rs`: engine initially has root A; invoke production hook with topology B payload; assert B installed before refresh and newly-added extension is owned. Force root installation/reconcile failure and assert context-handled and root-retry outcomes differ.
   - `state/overlay_retry.rs`: use condvar/test seam and `pass_count`, not wall-clock sleeps, to prove root obligation wakes immediately, survives a clean embed outcome, backs off independently, and clears only after root success. Existing `fresh_epoch` logic (`overlay_retry.rs:249-274`) should cover a root update arriving mid-pass.
   - `tools/search/status.rs`: snapshot/string tests for root retry running/failed while overlay is otherwise synced.
   - Warm-cache/bootstrap regression: second boot or cached graph adoption must install roots tied to the adopted topology, covering the special cached path called out at `bootstrap.rs:94-103`.

## Review Findings

- **high** — `crates/mcp-server/src/state/sync.rs:198-228`: root config changes only use ordinary fingerprint-gated `nudge_rebuild`; there is no forced project reload contract.
- **high** — `crates/mcp-server/src/state/bootstrap.rs:665-676` + `state/embed.rs:476-590`: `WorkspaceRoots` is boot-derived and never replaced after topology publication.
- **high** — `crates/mcp-server/src/graph/types.rs:14-41`: publish payload provides a hash but not the project-derived roots that hash identifies; reloading project in the hook would create a TOCTOU topology/root mismatch.
- **medium** — `crates/mcp-server/src/graph/state.rs:311-320`: hook `bool` has only topology-context semantics; it cannot safely represent an independent root retry.
- **medium** — `crates/mcp-server/src/state/overlay_retry.rs:249-274`: retry obligation is derived solely from `OverlayWarmupState`, so a clean embed can currently express no outstanding root failure.

## Residual Risks

- `WorkspaceRoots` mutability/API lives in `bsl-search`; verify `set_workspace_roots` safely replaces an initialized overlay and define removal semantics for files belonging to dropped roots. A setter alone may leave stale rows.
- Cached graph metadata stores topology hash but apparently not the root table; adopted-cache publication may need to derive roots from a validated current project and fence hash equality, or persist roots with the artifact.
- Invalid mid-session config currently degrades `ProjectSnapshot::load` to workspace root/default configs (`graph/input.rs:27-47`). Decide whether that fallback may replace a previously valid search root table or should be a retryable root failure while the old table remains serving.
- Lease loss must prevent root/overlay writes just like embedding writes; retain obligation until ownership returns.

## Start Here

Open `crates/mcp-server/src/graph/types.rs` first: define the publish payload and structured outcome there, then thread them through `graph/build.rs`/`graph/state.rs` into `state/embed.rs`; finally connect the independent retry obligation in `state/overlay_retry.rs` and the config wake in `state/sync.rs`.