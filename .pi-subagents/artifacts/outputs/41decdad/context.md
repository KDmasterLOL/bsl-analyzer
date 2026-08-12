# Code Context

## Files Retrieved
1. `crates/mcp-server/src/state/bootstrap.rs` (lines 110-205, 720-794) - production composition, search-init thread, initial fresh `Project` and root installation.
2. `crates/mcp-server/src/state/sync.rs` (lines 90-244) - dedicated search hub sink and config-change behavior.
3. `crates/mcp-server/src/diagnostics_state/drift.rs` (lines 225-414, 730-855) - resident drain/scan classification, locks, and full-reload trigger.
4. `crates/mcp-server/src/diagnostics_state/lifecycle.rs` (lines 650-692) - post-resident-build hub re-arm and fresh topology guard.
5. `crates/mcp-server/src/graph/build.rs` (lines 1-194, 689-735) - graph reload publication, hub re-arm, topology transition signal, fused writer root snapshot.
6. `crates/mcp-server/src/graph/state.rs` (lines 110-164, 220-354) - publish-hook seam and explicit no-graph-lock invariant.
7. `crates/mcp-server/src/graph/input.rs` (lines 1-55) - fresh per-operation `ProjectSnapshot` containing scan roots/config topology.
8. `crates/mcp-server/src/state/embed.rs` (lines 160-205) - production graph publish-hook composition.
9. `crates/mcp-server/src/project.rs` (lines 35-84) - authoritative process-override-aware `Project` and `WorkspaceRoots` constructors.
10. `crates/bsl-search/src/engine.rs` (lines 1270-1311) - search root getter/setter and cache invalidation.
11. `crates/mcp-server/src/state/mod.rs` (lines 20-68) - shared ownership and mutex topology.
12. `crates/mcp-server/src/drift_classify.rs` (lines 59-90) - exact-root config classification contract (located by targeted search).

## Key Code

### Exact production trigger and fanout

1. Bootstrap creates one `WorkspaceChangeHub`, then composes graph, resident, and search around clones of it. Search gets its own cursor before its init thread starts; graph and resident receive the hub directly (`state/bootstrap.rs:110-205`).
2. The search consumer is the named OS thread `bsl-search-overlay-watch`; it blocks in `hub.wait_for_change`, drains its cursor, and calls `apply_search_drift` (`state/sync.rs:90-143`). A config basename in either canonical or raw path causes whole-context dirty marking plus `graph.nudge_rebuild()` (`state/sync.rs:201-239`). Unlike resident classification, this is basename-based, not restricted to workspace-root config paths.
3. Resident reads/reconcile ticks drain their own cursor. `apply_drained_entries` computes exact canonical workspace-root config paths and classifies against the resident baseline; config drift calls `kick_full_reload()` and returns (`diagnostics_state/drift.rs:367-398`, with path construction at `drift.rs:754-766`). Scan fallback fingerprints config independently; on a clean scan it also kicks full reload (`drift.rs:250-287`).
4. Graph hub handling is reached indirectly from search's config nudge (and graph's own freshness machinery). A full graph build loads a fresh `ProjectSnapshot`, publishes under `inner`, computes `topology_changed` by comparing old/new topology, releases `inner`, re-arms hub roots, then calls `notify_published(build_start_seq, topology_changed)` (`graph/build.rs:142-188`).
5. Resident full rebuild likewise loads fresh project-derived state and, after successful publication, calls `ensure_hub_roots`; it reloads `ProjectSnapshot` again to prevent a slow stale build rolling the shared hub backward (`diagnostics_state/lifecycle.rs:650-687`).
6. Graph publication invokes the production hook composed in bootstrap (`state/bootstrap.rs:126-145`; `state/embed.rs:171-198`). `notify_published` explicitly runs it with no graph `inner` lock held (`graph/state.rs:294-354`). Currently this hook refreshes dirty search contexts; this is the strongest existing composition seam for installing newly-derived search roots too.

### Fresh project/root values

- `crate::project::at(root)` reloads config, applies the process-wide `SourceSetOverride`, and returns `Project::with_config`; all production reload code should use this rather than raw `Project::new` (`project.rs:35-48`).
- `crate::project::workspace_roots(&project)` is explicitly the shared constructor intended to keep search and resident root identities equal (`project.rs:50-84`).
- Search bootstrap already has a fresh `Project` and installs `roots_of(&project)` before publishing the engine (`state/bootstrap.rs:720-794`).
- Every graph/resident build has fresh `ProjectSnapshot { scan_roots, configs }`, but it does **not** carry `Project` or `bsl_search::WorkspaceRoots` (`graph/input.rs:10-51`).
- A graph publish hook receives topology hash/change state, not `Project` or roots (`graph/state.rs:304-354`). Thus roots can be freshly reloaded in the hook from its captured `workspace_root`, or production build output/signal can be extended to carry them.

### Locks and threads

- `SharedSearchEngine` is `Arc<Mutex<Option<SearchEngine>>>`; the search sink and publish hook share it (`state/mod.rs:31-53`, `state/bootstrap.rs:126-184`).
- Search sink is a perpetual dedicated thread; it takes the engine mutex briefly for watcher-mode setup and per-drift mutation (`state/sync.rs:101-143`).
- Resident mutable lifecycle is guarded by one mutex; classification takes short locks and full builds happen off-thread, swapping under lock (`diagnostics_state/lifecycle.rs:41-46`, `diagnostics_state/drift.rs:235-244`).
- Graph build runs off-thread/single-flight. Publication mutates graph `inner`, releases it, then calls hub re-arm and hook (`graph/build.rs:158-183`). The hook is deliberately lock-order-safe because no graph lock is held (`graph/state.rs:294-304`).
- `SearchEngine::set_workspace_roots` only replaces the table and clears `workspace_overlay_cache`; it does not migrate/purge stored documents (`bsl-search/src/engine.rs:1299-1305`).

## Architecture

The hub is the event transport, not a central topology owner. Each consumer has an independent cursor. Resident independently recognizes exact config paths and rebuilds its entire resident from a fresh project. Search recognizes config basenames, marks context dirty, and nudges graph. Graph rebuild derives the fresh semantic topology, publishes atomically, re-arms the shared hub, and signals search through the existing publish hook. Both graph and resident may call `hub.ensure_roots`; live-topology guards prevent an older build from reverting newer watch targets.

The best composition seam is `SharedState::build_publish_hook` / `refresh_search_contexts_after_graph` (`state/embed.rs:171-198`), immediately after graph publication and before context refresh. It already owns `SharedSearchEngine` and `workspace_root`, executes off query paths with no graph lock, and receives a witnessed `topology_changed`. On that signal it can call `crate::project::at(workspace_root)`, derive `crate::project::workspace_roots`, lock the engine, and call `set_workspace_roots` before resolving/re-rendering dirty keys. This keeps project-model knowledge in MCP composition rather than graph or `bsl-search`, and aligns the search table with the topology the graph just published.

A stricter atomic alternative is to extend `PublishedBuild`/`GraphPublishSignal` with roots derived from the exact build snapshot. That avoids a post-publish config reread race, but requires plumbing across graph boundaries and either adding `WorkspaceRoots` to the snapshot/build or retaining a fresh `Project`. Simply rereading in the hook is smaller but must fence against `signal.topology`: derive current topology and refuse/defer if it differs, analogous to both `ensure_hub_roots` guards.

## Findings

- **high:** `crates/mcp-server/src/state/sync.rs:201-239` updates graph/context on config topology drift but never replaces `SearchEngine.workspace_roots`. Added extensions therefore remain unattributable to search incremental dirty/remove/render paths until daemon restart. This is the apparent #209 production gap.
- **medium:** `crates/bsl-search/src/engine.rs:1299-1305` root replacement clears only the overlay cache. A design must decide how rows belonging to removed/renamed roots are reconciled; changing the table alone can leave persisted orphan hits.
- **medium:** search config detection is basename-anywhere (`state/sync.rs:201-217`), whereas resident only accepts exact workspace-root config paths (`diagnostics_state/drift.rs:367-390`, `drift_classify.rs:69-90`). This can cause harmless extra graph nudges but should not be copied as the authority for deriving roots.
- **low:** graph and resident independently re-arm one shared hub (`graph/build.rs:62-82`; `diagnostics_state/lifecycle.rs:669-687`). Their topology guards are essential and must remain if search root updating is added.

## Invariants

- Root identity is relative to the project directory and must be constructed by `crate::project::workspace_roots`; search and resident must agree (`project.rs:50-84`).
- Process `SourceSetOverride` must be honored through `crate::project::at` (`project.rs:35-48`).
- Do not hold graph `inner` while locking search; publication hook currently guarantees this (`graph/state.rs:294-304`).
- Do not publish roots from a superseded build; compare fresh/current topology with the build/signal topology, matching hub re-arm guards.
- Config topology change dirties all graph contexts, not merely new-extension files (`state/sync.rs:201-239`).
- Search watcher mode/cursor remains continuously active; root-table replacement must happen under the same engine mutex as drift application to serialize attribution changes.

## Open Design Choices / Residual Risks

1. **Root payload vs guarded reread:** carry exact `WorkspaceRoots` through graph publication (strong snapshot coherence, broader change) or reload in publish hook and verify topology (small seam, extra parse/discovery and race handling).
2. **Removed roots:** purge their stored code/overlay rows immediately, mark/reconcile via a clean rewalk, or retain them intentionally until another indexing pass. `set_workspace_roots` alone does not answer this.
3. **Invalid mid-session config:** graph falls back to workspace-only/default configs (`graph/input.rs:24-43`), while search bootstrap fails offline on invalid config (`state/bootstrap.rs:735-745`). Decide whether live search keeps last-known-good roots or temporarily adopts fallback roots.
4. **Non-topology config edits:** graph only signals `topology_changed` when topology hash differs. This is correct for root replacement; context dirty marking still handles broader config changes.
5. **Publish deferral:** topology refresh can be deferred when engine is unpublished or newer drift is pending (`graph/state.rs:314-330`). Root update must participate in the same retry semantics, not report the topology refresh handled before roots are installed.

## Start Here

Open `crates/mcp-server/src/state/embed.rs:171-198` first, then follow `refresh_search_contexts_after_graph`. It is the existing production graph-to-search composition boundary with the correct thread and lock ordering; add/assess topology-root refresh there, using `crates/mcp-server/src/project.rs:35-84` as the sole constructor.

## Validation

Read-only reconnaissance at `develop` commit `4b703bd1`. No source files edited and no tests run. Git reported only the runtime artifact directory as untracked.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Concrete severity-tagged findings and residual risks cite exact Rust file:line ranges and map hub -> graph/resident/search production flow."
    }
  ],
  "changedFiles": [],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "git branch --show-current && git rev-parse --short HEAD && git status --porcelain=v1",
      "result": "passed",
      "summary": "Confirmed develop at 4b703bd1; only .pi-subagents/ is untracked."
    }
  ],
  "validationOutput": [
    "Read-only targeted grep/read inspection completed.",
    "Production trigger, locks/threads, fresh Project availability, and composition seam attested with file:line evidence."
  ],
  "residualRisks": [
    "Root-table replacement alone does not purge persisted rows from removed roots.",
    "Reloading Project in the publish hook needs a topology fence to avoid mixing a newer config with the just-published graph.",
    "Invalid mid-session config requires an explicit last-known-good versus fallback-root policy."
  ],
  "noStagedFiles": true,
  "diffSummary": "No source diff; wrote only the requested reconnaissance artifact.",
  "reviewFindings": [
    "high: crates/mcp-server/src/state/sync.rs:201-239 - config topology drift nudges graph and dirties context but never updates SearchEngine workspace roots.",
    "medium: crates/bsl-search/src/engine.rs:1299-1305 - set_workspace_roots clears overlay cache but does not reconcile persisted documents from removed roots.",
    "medium: crates/mcp-server/src/state/sync.rs:201-217 - search treats matching config basenames anywhere as config drift, unlike resident exact-root matching."
  ],
  "manualNotes": "Recommended seam: graph publish hook in state/embed.rs, with project::at + project::workspace_roots and a signal-topology guard."
}
```
