# Code Context

## Files Retrieved
1. `crates/bsl-search/src/workspace_roots.rs` (lines 1-104, 104-145) - defines the composite `FileKey` identity and the root table that alone maps paths to keys.
2. `crates/bsl-search/src/engine.rs` (lines 151-175, 1270-1345) - owns roots, store, live vector index, and overlay cache; contains the current destructive setter.
3. `crates/bsl-search/src/workspace_overlay.rs` (lines 43-212, 334-361) - enumerates all in-memory overlay carriers and shows exactly what `clear()` destroys/preserves.
4. `crates/bsl-search/src/store.rs` (lines 54-141, 640-658, 683-763, 1753-1944, 2169-2326) - root-keyed persistent carriers and existing per-key/table-wide APIs.
5. `crates/mcp-server/src/state/sync.rs` (lines 1170-1228) - existing root-addition test/call flow demonstrating setter + rewalk behavior.

## Key Code

`FileKey` is the indivisible identity (`crates/bsl-search/src/workspace_roots.rs:36-55`):

```rust
pub struct FileKey {
    pub root_id: String,
    pub path: String,
}
```

The documentation explicitly says `(root_id, path)` is required because extensions repeat relative layouts, and `WorkspaceRoots` is the only absolute-path-to-key seam (`workspace_roots.rs:1-10`). The configuration root reserves `root_id == ""` (`workspace_roots.rs:12-17`).

The engine co-locates the state that must move together (`engine.rs:151-175`): `Store`, live `VectorIndex`, `workspace_roots`, and `Mutex<WorkspaceOverlayCache>`.

The overlay result/cache carriers are (`workspace_overlay.rs:43-212`):

- published `WorkspaceOverlayIndex`: `overlay.changes`, `hidden_paths`, `lexical_documents`, `vector_documents` (`43-55`);
- cache: `entries`, `hidden_paths`, `embedding_cache`, `dirty_paths`, `dirty_failures`, `unread_keys`, `settled_seq`, sequencing/fence state, watcher/initialization/full-rescan flags, and provider/counters (`154-212`).

Persistent key carriers are declared together in `ROOT_KEYED_TABLES` (`store.rs:54-141`):

- `files` (and its chunks/FTS/vector IDs through file ownership);
- `baseline_manifest_files` fingerprints;
- `overlay_tombstones` (persistent hidden paths);
- `overlay_files`;
- `overlay_fingerprint_cache`;
- `context_dirty`.

Additionally, overlay embedding cache is keyed by semantic embedding key rather than `FileKey` and can generally be retained/reused (`store.rs:2276-2326`), matching the in-memory `embedding_cache` carrier. Live vectors are keyed by chunk IDs: callers obtain IDs before `remove_file` specifically to evict those vectors (`store.rs:716-763`). Embedding-generation triggers cover chunk/file deletion and protect persisted vector sidecars from stale loads (`store.rs:640-658`).

### Destructive setter behavior — high severity

`SearchEngine::set_workspace_roots` first replaces the root table and then calls `cache.clear()` (`engine.rs:1299-1303`). `clear()` destroys entries, hidden paths, dirty marks/failures, unread obligations, initialized/full-rescan state and resident counter (`workspace_overlay.rs:349-360`). It does **not** clear `embedding_cache`, `settled_seq`, watcher mode, or graph provider. It bumps the wholesale fence. The setter does not touch any persistent root-keyed row, chunk, FTS row, live vector, or persisted vector artifact.

This creates a split transition: new attribution is immediately active while old-root store rows/vectors remain, and positive obligations (`dirty_paths`, `unread_keys`) are discarded. Lock poisoning is silently ignored (`if let Ok`), so roots can change while the old overlay cache survives unchanged (`engine.rs:1300-1303`). **Severity: blocker for an atomic WorkspaceRoots transition.**

The MCP test `crates/mcp-server/src/state/sync.rs:1181-1228` only covers adding a root by calling the setter, re-enabling watcher mode, then performing a separate rewalk. It proves eventual discovery of the new root, not atomicity; between setter and rewalk the new root is configured but absent, and pre-existing dirty/unread evidence was cleared.

## Architecture

Path attribution flows `WorkspaceRoots::{root_of,key_of_path}` -> `FileKey` -> every lexical/persistent/overlay carrier. Consequently a topology change has three classes:

1. **Removed roots:** every old key under those root IDs becomes invalid and must be removed from files/chunks/FTS, tombstones, overlay files, fingerprint rows, context-dirty rows, in-memory entries/hidden/dirty/unread/fences, and live vectors.
2. **Reassigned roots:** same physical file now maps to a different `(root_id,path)` (for example longest-prefix ownership changes when a nested root is added/removed). This is not merely removal: all file-key carriers must migrate or be invalidated and rebuilt under the new key. Chunk IDs/vectors must not remain reachable under both identities.
3. **Added roots:** files newly in scope must be represented as pending dirty/unread work (or fully indexed) before the new table becomes observable. Adding a nested root can also reassign files formerly owned by an enclosing root.

The store has useful pieces but no aggregate root-transition API: atomic `remove_file` only covers files/chunks/FTS (`store.rs:716-742`); tombstone removal is separate (`1920-1925`); fingerprint per-key deletion exists (`2266-2274`); context-dirty deletion is separate (`982-1009`). Table-wide clears exist but are overly destructive. No existing API coordinates these with the live vector index and overlay cache.

### Narrowest lower-layer transition contract

Put the operation on `SearchEngine`, the lowest owner that simultaneously holds roots, store, vector index, and overlay cache:

```rust
pub fn transition_workspace_roots(
    &mut self,
    next: WorkspaceRoots,
    delta: WorkspaceRootDelta,
) -> Result<WorkspaceRootTransition, SearchError>
```

`WorkspaceRootDelta` should carry explicit **old-key -> optional new-key** reassignments/removals plus **added/new keys requiring refresh** (derived from one project-model scan by the caller or a bsl-search planning helper). Do not accept only root IDs: nested/alias topology makes ownership a longest-prefix, canonical-spelling decision, so root-set difference alone cannot identify reassigned files (`workspace_roots.rs:93-145`). The operation must validate every old key against the currently installed roots and every new key against `next` before mutation.

Commit semantics: either the old roots and all carriers remain observable, or `next` and the complete delta do. Within the engine lock, stage embeddings/documents first; use one SQLite transaction for all root-keyed persistent carriers; evict/rebuild live vectors with rollback/rebuild-on-error; transform cache keys while preserving unrelated entries, hidden paths, dirty/unread obligations and reusable embedding vectors; only assign `self.workspace_roots = Some(next)` last. Return counts for removed/reassigned/added keys and whether a full refresh remains pending. If a truly atomic live-vector rollback is impractical, rebuild the live index from the committed store before publishing `next`; never expose the new table with an old index.

This is narrower and safer than exposing a `Store` migration: `Store` cannot update the live vectors/cache/root pointer. It is also narrower than teaching MCP lifecycle code every carrier: that would duplicate bsl-search invariants above its owning layer.

## Existing Tests / Missing Coverage

Existing evidence includes root identity/overlap/alias/longest-prefix tests in `workspace_roots.rs` (notably lines 386-484 and 718-747), removal/vector failure seams documented in `engine.rs:140-150`, and the add-root eventual-rewalk test in `sync.rs:1181-1228`.

Required transition tests should assert, in one engine-level fixture:

- removed root leaves no hits, file/chunk rows, tombstones, overlay/fingerprint/context-dirty keys, or live vectors;
- adding a nested root reassigns existing files without duplicate old/new hits;
- dirty and unread keys are remapped/preserved rather than lost;
- unrelated-root overlay entries/hidden/fingerprints/vectors and reusable embedding cache survive;
- injected SQLite/vector/cache-lock failure leaves old roots and old observable search state intact;
- poisoned cache lock returns an error rather than silently splitting state;
- new-root files are not observable as configured-but-missing between transition and indexing.

## Residual Risks

- Canonical aliases and nested roots mean physical-file equivalence cannot be inferred from textual root IDs; delta planning must use the same `WorkspaceRoots` spellings/attribution rules.
- SQLite commit and in-memory HNSW mutation are not intrinsically one transaction. The implementation needs staging or deterministic rebuild/rollback before publishing roots.
- Persisted baseline manifest semantics may intentionally describe an external snapshot; reassignment policy must distinguish “migrate identity” from “invalidate and refetch.”
- The current public setter permits callers to bypass any new transition API unless it is made private/deprecated or constrained to initial setup.

## Start Here

Open `crates/bsl-search/src/engine.rs:1279-1303` first: it is the current public transition point and directly reveals the split/destructive behavior. Then inventory `WorkspaceOverlayCache` at `workspace_overlay.rs:154-212` and `ROOT_KEYED_TABLES` at `store.rs:54-141` before designing the delta.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Concrete blocker and transition findings cite engine.rs:1299-1303, workspace_overlay.rs:349-360, store.rs:54-141, and sync.rs:1181-1228; residual risks are enumerated."
    }
  ],
  "changedFiles": [],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "targeted repository find/grep/read inspection",
      "result": "passed",
      "summary": "Mapped WorkspaceRoots, engine setter, overlay carriers, persistent root-keyed tables, APIs, and relevant tests."
    }
  ],
  "validationOutput": [
    "Read-only reconnaissance; no source modifications or tests run."
  ],
  "residualRisks": [
    "Alias/nested-root reassignment requires shared canonical attribution rules.",
    "SQLite and live vector mutation need staging or rollback/rebuild to provide atomic observability.",
    "Public destructive setter remains a bypass unless restricted."
  ],
  "noStagedFiles": true,
  "diffSummary": "No code diff; wrote only the requested reconnaissance artifact.",
  "reviewFindings": [
    "blocker: crates/bsl-search/src/engine.rs:1299-1303 - set_workspace_roots publishes new roots, destructively clears only in-memory overlay state, and leaves persistent rows/live vectors under old keys.",
    "high: crates/bsl-search/src/workspace_overlay.rs:349-360 - clear drops dirty and unread obligations rather than transitioning them.",
    "high: crates/bsl-search/src/engine.rs:1301-1303 - cache lock poisoning is ignored, allowing roots and overlay cache to diverge.",
    "medium: crates/mcp-server/src/state/sync.rs:1181-1228 - existing add-root test validates eventual rewalk only, not atomic transition or preservation."
  ],
  "manualNotes": "Recommended ownership is an engine-level transactional transition accepting an explicit per-file old/new key delta; a Store-only or MCP-only contract cannot coordinate every carrier."
}
```
