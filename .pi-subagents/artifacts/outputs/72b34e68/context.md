# Code Context

## Files Retrieved
1. `crates/bsl-search/src/engine.rs` (lines 145-169, 1280-1304, 1334-1367, 1470-1775) - owns the live roots, overlay cache, carrier snapshot, reconcile/removal machinery, and the currently non-fallible/non-atomic root setter.
2. `crates/bsl-search/src/workspace_roots.rs` (lines 44-344) - `FileKey`, private `Root`, root construction/identity, attribution, resolution, and public root enumeration.
3. `crates/bsl-search/src/key_carriers.rs` (lines 1-113) - exhaustive positive-carrier model (`StoreRow`, in-memory overlay, unread obligation, fingerprint, external manifest).
4. `crates/bsl-search/src/workspace_overlay.rs` (lines 145-225, 340-426, 1868-1883) - all in-memory key-bearing sets, wholesale fence, clear semantics, and carrier snapshots.
5. `crates/bsl-search/src/store.rs` (lines 60-143, 715-748, 1870-1947, 1949-2037, 2169-2275, 2325-2350) - persistent root-keyed tables and existing per-table transaction seams.
6. `crates/mcp-server/src/state/sync.rs` (lines 1160-1225) - existing live root-change test/call site currently calls `set_workspace_roots`, re-enables watcher mode, then rewalks.
7. `crates/bsl-search/src/lib.rs` (lines 70-86) - public exports affected if a transition report/error-facing API is added.

## Key Code

Current unsafe seam:
```rust
// engine.rs:1299-1304
pub fn set_workspace_roots(&mut self, roots: WorkspaceRoots) {
    self.workspace_roots = Some(roots);
    if let Ok(mut cache) = self.workspace_overlay_cache.lock() {
        cache.clear();
    }
}
```
It publishes the new topology before checking the cache lock, silently ignores poisoning, and retracts none of the persistent carriers. A removed root's rows therefore remain searchable; worse, if an ID is reused for a different declared/canonical root, old rows resolve into the new directory.

`WorkspaceRoots` has enough internal data to compare bindings, but does not expose it (`workspace_roots.rs:88-105`). `ids()` alone (`:286-288`) is insufficient: the reserved configuration ID stays `""` even when its directory changes, and an extension can retain the same relative ID while its canonical/declared target changes.

Positive carriers are exhaustively enumerated in `key_carriers.rs:27-51`; `CarrierKeys::all_keys` (`:91-98`) is the correct candidate universe. Transition behavior by carrier:

- `StoreRow`: delete retired/rebound keys and cascaded chunks/FTS.
- `OverlayEntry`, `UnreadObligation`: remove through one cache operation, not just `clear` after publishing roots.
- `FingerprintRow`: delete retired/rebound keys; these assert verification of a physical file.
- `BaselineManifest`: never delete (remote snapshot ownership); retired keys must cease being served by filtering/hiding, but a blanket cache clear loses hidings. Since the new topology cannot resolve them, query-side manifest results must be hidden by the transition snapshot or topology filtering.
- Negative/non-carriers still requiring cleanup: `overlay_tombstones` and `context_dirty` for retired/rebound key spaces, otherwise a future root reusing the ID inherits stale absence/freshness state. Persisted `overlay_files`/chunks are also root-keyed (`store.rs:94-117`) even though `KeyCarrier` currently models the live in-memory overlay, so transition cleanup must include them or explicitly prove they are unused in this mode.

## Architecture

### Minimal implementation map

1. **Describe changed key spaces in `workspace_roots.rs`.**
   - Add a crate-visible binding value, e.g. `RootBinding { id, declared, canonical }`, or a method `WorkspaceRoots::changed_or_removed_ids(&self, next: &Self) -> HashSet<String>`.
   - Treat an ID as retired when absent in `next`, or rebound when either declared or canonical path differs. Added IDs need no cleanup. Comparing canonical only is risky when canonicalization falls back for a temporarily absent path; compare the complete binding.
   - Add focused unit tests beside current root identity tests: removed extension; same extension retained; same ID/different target; configuration path change with stable empty ID; workspace move that changes extension IDs.

2. **Add a single persistent transaction seam in `store.rs`.**
   - Add something like `Store::retract_workspace_keys(&self, collection, keys) -> Result<Vec<i64>, SearchError>` (or by retired root IDs plus explicit keys).
   - In one `unchecked_transaction`, collect affected chunk IDs, delete main `chunks_fts`, `files` (chunks cascade), persisted overlay FTS/chunks/files, fingerprint rows, tombstones, and `context_dirty`; commit once. Do not mutate `baseline_manifest_files` or shared content-addressed `overlay_embedding_cache`.
   - Reuse SQL intent from `remove_file` (`store.rs:715-748`), `remove_overlay_file` (`:2029-2037`), and fingerprint deletion (`:2262-2275`), but do not compose those public methods: each opens/executes its own transaction and cannot provide all-or-nothing batch behavior.
   - Prefer a temp table/parameterized loop within one transaction. Root-ID-only deletion is sufficient only after bindings are classified correctly; explicit keys are needed if transition policy retains some key space.
   - Add store fault-injection/trigger test modeled on `remove_file_is_atomic_all_or_nothing` (`store.rs:3209+`) proving every persistent table rolls back together.

3. **Add one cache transition operation in `workspace_overlay.rs`.**
   - Add `WorkspaceOverlayCache::transition_roots(retired_keys_or_ids, manifest_keys)`, executed while its mutex is held. It must remove retired entries, unread/dirty/failure/settled state, and install hidings for retired manifest keys (or retain an explicit topology-hidden set). It must bump the wholesale fence so every Phase-A `RefreshPlan` built against old roots returns `PublishOutcome::Superseded` (`wholesale_seq`, lines 189-193).
   - Do not implement as current `clear()`: it clears `hidden_paths`, which can immediately resurrect an external-baseline hit belonging to a removed root. Decide whether watcher mode survives; current `clear` does preserve `watcher_mode`, and live transition should too.
   - Test stale-plan publication, manifest-only removed key remains hidden, unread-only/overlay-only key disappears, and watcher mode remains enabled.

4. **Replace the engine setter with a fallible atomic transition in `engine.rs`.**
   - Suggested API: `pub fn transition_workspace_roots(&mut self, next: WorkspaceRoots) -> Result<WorkspaceRootsTransition, SearchError>`. Keep `set_workspace_roots` only for initial `None -> Some` setup (or make it return `Result` and route both cases); update production call sites, especially `mcp-server/src/state/bootstrap.rs:667-672` and live sync code.
   - While the outer `SearchEngine` mutex already excludes public searches, acquire the inner overlay lock first and fail on poison. Snapshot carriers once (`carrier_keys`, `engine.rs:1614-1645`) before changing roots. Compute retired/rebound keys from `all_keys`; include manifest-only keys.
   - Run persistent batch retraction, update cache/hidings and invalidate stale plans, evict affected live vector IDs, then assign `self.workspace_roots = Some(next)` last. Return counts by carrier/root for observability.
   - Avoid calling `remove_workspace_key_with` in a loop: it performs several independent commits, mutates cache between failures, and intentionally leaves a vector/SQLite failure window (`engine.rs:1701+`). It is correct for best-effort deletion reconciliation, not an atomic topology transaction.

5. **Update the live owner/call site.**
   - `mcp-server/src/state/sync.rs:1200-1209` is the demonstrated live transition. It must handle `Result` and only invoke the subsequent rewalk after commit. Do not separately re-enable watcher mode if transition preserves it.
   - Bootstrap/CLI callers can use an explicit initial-install method to avoid unnecessary carrier scans.

### Transaction and ordering seam

SQLite and the in-memory vector index cannot share a transaction. The smallest defensible contract is atomic *visibility under the engine's outer mutex*, plus deterministic repair:

1. lock cache and compute/snapshot plan;
2. begin SQLite transaction and collect vector IDs;
3. commit all persistent retractions;
4. evict vectors, mutate cache, publish roots before releasing engine lock.

If vector eviction can fail, step 4 needs a repair path that rebuilds/reloads the vector index from the now-committed store before returning. Assigning old roots on error does not restore deleted DB state. Conversely, evicting vectors before SQLite commit requires rebuilding vectors if the DB transaction rolls back. A transition API must document which repair is used and tests must force `FORCE_VECTOR_REMOVE_ERROR` (`engine.rs:145-151`). For FTS-only engines this complication is absent.

### Focused end-to-end tests

In `engine.rs` tests:

- remove a live extension with duplicate relative path: extension hit disappears, configuration twin remains, all local carriers for extension are gone;
- rebind same root ID to another directory: old content never resolves/serves under new directory; new file appears only after rewalk;
- external manifest-only removed-root key is hidden immediately and repeated transition is idempotent;
- keys existing solely in each carrier (store, overlay, unread, fingerprint, manifest) are handled, extending the carrier exhaustiveness style at `key_carriers.rs:115+`;
- forced SQL failure leaves old roots and every persistent/in-memory carrier unchanged;
- forced vector removal failure yields repaired searchable state and does not publish a half-transition;
- old off-lock refresh plan cannot publish after transition (wholesale fence);
- configuration root change (`root_id == ""`) retracts old configuration rows;
- retained root keys and embeddings remain untouched (minimality/performance).

In `mcp-server/src/state/sync.rs`, strengthen `the_rescan_walk_follows_the_table...` (`:1168+`) to assert removed/rebound-root stale hits are absent immediately at transition, not merely that the next walk marks the added root.

## Review Findings

- **blocker:** `crates/bsl-search/src/engine.rs:1299-1304` - live `WorkspaceRoots` replacement publishes topology and clears only in-memory state; persistent rows/fingerprints/tombstones and remote manifest visibility are not transitioned atomically.
- **high:** `crates/bsl-search/src/workspace_overlay.rs:349-363` - `clear()` removes `hidden_paths`; using it during external-baseline root removal can resurrect manifest-only hits from a removed root.
- **high:** `crates/bsl-search/src/engine.rs:1701+` - existing per-key removal is deliberately non-atomic across SQLite/vector/cache and is unsuitable as the transition primitive.
- **medium:** `crates/bsl-search/src/workspace_roots.rs:286-296` - public ID/entry access cannot distinguish retained binding from same-ID rebinding using canonical and declared spellings; a dedicated diff belongs inside this module.
- **medium:** `crates/bsl-search/src/store.rs:1901-1947, 2029-2037, 2262-2275` - carrier cleanup APIs are separate commits/loops, so there is no existing rollback boundary for a topology transition.

## Residual Risks

- Concurrent Phase-A overlay warmup uses cloned old roots off the engine lock; only wholesale sequence invalidation prevents its later publication. Ensure the fence is captured/published on every planning route.
- Filesystem changes between root construction and transition can alter canonicalization, making an apparently retained binding look rebound (safe but expensive) or vice versa after fallback. Complete binding comparison plus conservative retraction is safest.
- Another process can write the same SQLite DB despite the in-process engine mutex. WAL serializes commits, but a stale daemon may repopulate retired IDs after transition; workspace lease ownership must be the cross-process precondition.
- Manifest rows cannot be deleted because they belong to the baseline snapshot. Filtering/hiding must therefore be applied consistently on lexical and semantic merge paths, not only overlay stats.
- `context_dirty` is deliberately excluded from positive carriers, but stale marks under a rebound ID are semantically dangerous and must be transactionally removed.

## Start Here

Open `crates/bsl-search/src/engine.rs` at lines 1290-1304 first: replace the setter contract and define its ordering. Then implement the lower-level root diff in `workspace_roots.rs` and the single SQLite retraction transaction in `store.rs`.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Concrete severity-tagged findings and exact implementation/test map cite bsl-search engine, store, workspace_roots, workspace_overlay, key_carriers, and the live mcp-server call site."
    }
  ],
  "changedFiles": [],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "targeted repository find/grep/read inspection",
      "result": "passed",
      "summary": "Mapped root installation, all key carriers, persistent tables/transactions, cache fencing, and live transition call sites."
    }
  ],
  "validationOutput": [
    "Read-only reconnaissance; no source files modified and no tests executed."
  ],
  "residualRisks": [
    "SQLite and live vector index require an explicit repair strategy because they cannot commit atomically.",
    "Off-lock refresh plans and cross-process stale writers must be fenced.",
    "Manifest-only keys require persistent topology filtering/hiding rather than manifest deletion."
  ],
  "noStagedFiles": true,
  "diffSummary": "No source diff; wrote only the requested reconnaissance artifact.",
  "reviewFindings": [
    "blocker: crates/bsl-search/src/engine.rs:1299-1304 - live root replacement is non-fallible and leaves persistent carriers under obsolete/rebound root identities.",
    "high: crates/bsl-search/src/workspace_overlay.rs:349-363 - clear removes baseline hidings and can resurrect removed-root manifest hits.",
    "high: crates/bsl-search/src/engine.rs:1701+ - per-key removal has a documented SQLite/vector atomicity gap and cannot serve as the transition transaction."
  ],
  "manualNotes": "Implementation should preserve initial-install convenience but make live replacement explicitly fallible and observable."
}
```
