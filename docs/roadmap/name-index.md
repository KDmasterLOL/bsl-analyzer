# Persistent workspace name-index (plan v3)

**Status:** superseded by `name-index-v6.md`. Kept for historical context.
**Owner:** TBD.
**Drives:** lazy-text-loading (separate plan, blocked on this).

## 1. Goal

Replace `crates/hir-def/src/name_usage_index.rs` (the pair
`source_root_name_usage_query` + `file_name_usage_query`) with a non-Salsa
workspace-wide lexical identifier index. The existing pair iterates every
BSL `FileId` in the source root and demands each file's text from Salsa,
forcing all ~25k texts (~1.66 GB) resident on the first workspace-wide
`find_references`. The persistent index unblocks lazy-text by serving
the same lookup without ever needing every file's text in Salsa.

## 2. Non-goals

- Lazy-text itself. Index is a precondition, not a part.
- Disk persistence of the index.
- Workspace symbols beyond plain identifier occurrences.
- Indexing of SDBL contents inside string literals.

## 3. Measurements that justify the design

On the target workspace (ERP, 25 617 .bsl, 1.44 GB on disk):

| metric | value |
|---|---:|
| current eager workspace load (Salsa) | ~47 s |
| lexer-only parallel build (12 cores) | 2.4 s |
| lexer-only single-thread build | 12.3 s |
| per-file refresh (extrapolated) | ~68 µs |
| final HashMap size (steady-state) | 67.5 MB |
| unique names | 532 391 |
| (name, file) pairs | 4 189 604 |

3 seconds of cold-start build replaces 47 seconds of eager text load and
removes 1.66 GB of resident Salsa input. The build is cheap enough to do
eagerly on every LSP startup.

**Measurement caveat.** The `--bench-index` tool in commit `7e0427ce`
filters `TokenKind::Ident` only, but the production index (§8) must
include keyword tokens too — `SyntaxKind::is_name_token` accepts both
because BSL allows `obj.Если()` where `Если` is a keyword used as a
method name. Rough adjustment: ~50–80 keywords × 25 617 files
≈ 1.25 M extra (name, file) pairs, adding ~5 MB to the steady-state
index (~7 %) and ~5–10 % to the lex phase. Architectural conclusions
do not change; the bench will be brought into parity when the
implementation-side `is_name_token` helper (§8) lands in Landing 2.

## 4. Where the index lives

**New crate `crates/bsl-name-index/`.** Direct deps: `lexer`, `vfs`,
`paths`, `parking_lot`, `rustc-hash`, `rayon`. No deps on `hir-def`,
`hir-ty`, `hir`, `ide`, `parser`, `syntax`.

**Held as an adjunct field on `RootDatabaseImpl`** (in `ide-db`), NOT
as a Salsa input. The structure is `Arc<WorkspaceNameIndex>` cloned via
the existing Salsa snapshot mechanism (Arc clone is cheap; the inner
`RwLock<Inner>` is shared across snapshots).

```
crates/ide-db/src/database.rs:
    pub struct RootDatabaseImpl {
        storage: salsa::Storage<Self>,
        name_index: Arc<bsl_name_index::WorkspaceNameIndex>,
        // …
    }
```

This placement is the key architectural decision in v3:

- It avoids leaking a new accessor onto `ide::Analysis` and reworking
  `LatencyRequestContext` (a problem Codex flagged in v2).
- It keeps the existing `ide::references::find_references` call shape:
  it already reaches through the DB trait; we just point it at
  `db.name_index().lookup(name, scope)` instead of
  `db.source_root_name_usage_query(scope)`.
- Dependency edges form a clean DAG:
  `bsl-analyzer → ide-db → bsl-name-index`; `bsl-name-index` depends
  only on lexer/vfs/paths. No cycles.

### 4.1. Layer placement

`bsl-name-index` is **analysis/lexical infrastructure** alongside
`base-db` and `vfs` — below the semantic layer (`hir-def`/`hir-ty`/`hir`),
not in it. Consumers (`ide::references`) reach it through the DB trait,
same as today. `bsl-name-index` itself has no knowledge of HIR or
references; it's a pure occurrence index parameterized only by
`vfs::FileId` and `base_db::SourceRootId`.

## 5. Public API

```rust
// crates/bsl-name-index/src/lib.rs

pub struct WorkspaceNameIndex { /* parking_lot::RwLock<Inner> + AtomicU64 gen */ }

pub enum IndexState {
    Empty,
    Building { started_at: Instant, gen: u64 },
    Ready { last_rebuild_ms: u64, indexed_files: usize, gen: u64 },
    Failed { error: String, gen: u64 },
}

pub enum LookupResult {
    Ready(Vec<FileId>),
    Pending,  // index in Building / Empty state
    Failed,   // last rebuild errored
}

pub struct NameIndexStats {
    pub state: IndexState,
    pub unique_names: usize,
    pub total_pairs: usize,
    pub indexed_files: usize,
    pub est_size_bytes: usize,
    pub failed_files: Vec<FileId>,
}

impl WorkspaceNameIndex {
    pub fn new() -> Self;

    /// Submit a full rebuild. `mem_docs` is the editor-text overlay
    /// snapshot at submit time; any didChange/refresh that arrives after
    /// submit is reapplied to the new `Inner` before swap via the
    /// replay log (§7). Returns a handle for waiting/cancellation.
    pub fn rebuild_async(
        &self,
        files: Vec<(FileId, SourceRootId, PathBuf)>,
        mem_docs: HashMap<FileId, Arc<str>>,
    ) -> RebuildHandle;

    /// One-shot synchronous rebuild for non-LSP contexts (CLI
    /// `analyze --salsa`, integration tests, fixture setup). Bypasses
    /// the replay log / state machine entirely: builds in the calling
    /// thread, swaps once, returns when Ready. Not safe to call
    /// concurrently with `refresh` / `rebuild_async`. Used wherever
    /// `RootDatabaseImpl::default()` + manual `set_file_text` is the
    /// loading pattern (Codex pass-3 finding NEW 1).
    pub fn rebuild_sync(
        &self,
        files: &[(FileId, SourceRootId, PathBuf)],
        mem_docs: &HashMap<FileId, Arc<str>>,
    );

    /// Synchronous per-file update. Bumps generation. Called from
    /// `GlobalState::set_bsl_file_text` (§6) — never from a background
    /// task, never with a borrowed reference outliving the call.
    pub fn refresh(&self, file_id: FileId, source_root: SourceRootId, content: &str);

    /// Synchronous batch update for `handle_vfs_msg` storms.
    pub fn refresh_batch(&self, entries: &[(FileId, SourceRootId, String)]);

    pub fn remove(&self, file_id: FileId);
    pub fn remove_batch(&self, files: &[FileId]);

    /// Cancel any in-flight rebuild and clear the index. Called from
    /// `reload_project_config` / `prune_stale_workspace_files`.
    pub fn reset(&self);

    /// O(1) source-root-scoped lookup. Result enum forces callers to
    /// handle Pending state explicitly — no silent false-negatives.
    pub fn lookup(&self, name: &str, scope: SourceRootId) -> LookupResult;

    pub fn state(&self) -> IndexState;

    /// Block up to `timeout` waiting for the index to leave Building
    /// state. Returns the resolved state (`Ready`/`Failed`/`Empty`) or
    /// the current state if `timeout` expires. Used by `find_references`
    /// in Landing 3 (§6.4) — the references handler does not hold the
    /// `RebuildHandle`, so this method on `WorkspaceNameIndex` is the
    /// canonical wait entry. Implementation: internal `Condvar` inside
    /// `Inner` signalled on every state transition; `state_wait_with_timeout`
    /// acquires the inner lock briefly to check state, then `wait_timeout`s
    /// on the condvar. Distinct from `RebuildHandle::wait_with_timeout`,
    /// which is for the caller that submitted the build.
    pub fn state_wait_with_timeout(&self, timeout: Duration) -> IndexState;

    pub fn stats(&self) -> NameIndexStats;
}

pub struct RebuildHandle { /* cancellation flag + join state */ }
impl RebuildHandle {
    pub fn cancel(&self);
    pub fn wait_with_timeout(&self, timeout: Duration) -> IndexState;
}
```

### 5.1. Internal representation

```rust
struct Inner {
    by_name: FxHashMap<NameId, Vec<FileId>>,
    by_file: FxHashMap<FileId, FxHashSet<NameId>>,
    file_to_root: FxHashMap<FileId, SourceRootId>,
    interner: NameInterner,  // 50-line custom: HashMap<String,NameId> + Vec<String>
}
```

`file_to_root` is maintained in lockstep with `by_file` so `lookup` can
filter to scope without taking the workspace borrow. SourceRoot is
required in refresh signatures (Codex v2 finding D).

## 6. Integration

### 6.1. GlobalState

```rust
// crates/bsl-analyzer/src/global_state.rs
pub struct GlobalState {
    // … existing fields …
    // name_index is held inside RootDatabaseImpl; GlobalState gets
    // helper accessor for convenience but does not own a separate copy.
}

impl GlobalState {
    /// Single point of truth: any code that writes BSL text into the
    /// Salsa input MUST go through this helper. Direct calls to
    /// `db.set_file_text` are forbidden for BSL files outside this
    /// method. Enforced by a `#[deprecated]` shim that forwards to the
    /// helper when feature `name-index-strict` is on (CI).
    fn set_bsl_file_text(&mut self, file_id: FileId, source_root: SourceRootId, text: &str) {
        self.analysis_host.raw_database_mut().set_file_text(file_id, text);
        self.analysis_host
            .raw_database()
            .name_index()
            .refresh(file_id, source_root, text);
    }
}
```

### 6.2. process_changes

The existing function holds `let db = self.analysis_host.raw_database_mut()`
across the full loop, blocking access to `self.name_index`. Codex v2
finding E.

**Fix**: split into two passes.

```rust
pub fn process_changes(&mut self, suppress_metadata_bump: bool) -> (bool, bool) {
    // Pass 1: Salsa-only mutations. Holds db mut borrow.
    let mut name_refresh_batch: Vec<(FileId, SourceRootId, String)> = Vec::new();
    let mut name_remove_batch: Vec<FileId> = Vec::new();
    {
        let db = self.analysis_host.raw_database_mut();
        for change in changed_files {
            match change {
                Change::Create(text, _) | Change::Modify(text, _) => {
                    if is_bsl_source(&file_set, file_id) {
                        db.set_file_text(file_id, &text);
                        name_refresh_batch.push((file_id, source_root_id, text));
                    }
                }
                Change::Delete => {
                    if is_bsl_source(&file_set, file_id) {
                        name_remove_batch.push(file_id);
                    }
                }
            }
            // … existing SourceRoot membership / metadata XML handling …
        }
    } // db borrow dropped

    // Pass 2: index updates. Synchronous.
    if !name_refresh_batch.is_empty() {
        let entries: Vec<_> = name_refresh_batch
            .iter()
            .map(|(fid, sr, t)| (*fid, *sr, t.clone()))
            .collect();
        self.analysis_host.raw_database().name_index().refresh_batch(&entries);
    }
    if !name_remove_batch.is_empty() {
        self.analysis_host.raw_database().name_index().remove_batch(&name_remove_batch);
    }

    (true, config_file_changed)
}
```

The `text.clone()` for batch is one-time per didChange — acceptable
overhead given refresh is ~68 µs/file.

### 6.3. Rebuild on cold start

```rust
// crates/bsl-analyzer/src/server.rs near LoadingProgress::Finished
state.process_changes(true);
state.init_source_root();
state.warm_metadata_cache();

// NEW: collect files + open-doc overlay; dispatch rebuild.
let (files, mem_docs) = state.snapshot_for_rebuild();
let handle = state.analysis_host
    .raw_database()
    .name_index()
    .rebuild_async(files, mem_docs);
state.name_index_rebuild_handle = Some(handle);

state.vfs_done = true;
state.report_progress("Loading", Progress::End, …);
```

`vfs_done = true` is **not** gated on rebuild completion (per Codex
v2: cold-start UX). The index runs in its own dedicated rayon scope —
not on `task_pool` (which is bounded for latency-sensitive requests).

### 6.4. find_references

Two distinct LSP mechanisms could apply here; this plan deliberately uses
only one (Codex pass-3 NIT 1):

- **`workDoneToken`** is for *workspace-level* status notifications
  ("indexing 35% complete"). Emitted by `rebuild_async` via the existing
  `GlobalState::report_progress` plumbing — unrelated to references.
- **`partialResultToken`** is for *streaming chunks* of a result back to
  the client mid-request. Not used by this plan. No existing handler in
  the codebase streams partial results, and a workspace-wide identifier
  lookup is fast once the index is Ready, so streaming has no payoff.

Behavior on Pending therefore depends on which Landing is active:

**Landings 1 and 2 (fallback alive):** transparent fallback. Caller never
sees Pending or partial results.

```rust
// crates/ide/src/references.rs:99-103
let scope = self.db.file_source_root_input(file_id).source_root_id(&*self.db);
let normalized = name.to_lowercase();
let files = match self.db.name_index().lookup(&normalized, scope) {
    LookupResult::Ready(files) => files,
    LookupResult::Pending | LookupResult::Failed => {
        // Transitional fallback retained until Landing 3.
        // No user-facing partial-result UX.
        self.db.source_root_name_usage_query(scope).get(&normalized)
    }
};
```

**Landing 3 (fallback removed):** synchronous bounded wait. The index
build budget is ~3 s on the largest tested workspace; a 500 ms wait
covers ~95 % of post-startup interactions, and the warning path is
explicit for the rest.

```rust
let files = match self.db.name_index().lookup(&normalized, scope) {
    LookupResult::Ready(files) => files,
    LookupResult::Pending => {
        match self.db.name_index().state_wait_with_timeout(Duration::from_millis(500)) {
            IndexState::Ready { .. } => match self.db.name_index().lookup(&normalized, scope) {
                LookupResult::Ready(files) => files,
                _ => return ReferencesResult::Incomplete("index unexpectedly not ready after wait"),
            },
            _ => return ReferencesResult::Incomplete("name index not ready after 500ms"),
        }
    }
    LookupResult::Failed => {
        tracing::warn!("name index in Failed state; returning incomplete references");
        return ReferencesResult::Incomplete("name index failed");
    }
};
```

`ReferencesResult::Incomplete` is a new variant or wrapper that
preserves the existing `Vec<Location>` return shape and adds a
`tracing::warn!` log line; the LSP wire response is still a normal
references reply with whatever locations were found (possibly empty),
because `textDocument/references` has no partial-result mechanism we'd
want to engage here. Clients see a normal response with a warn in
server logs — same UX as today for "no results".

### 6.5. Reload / prune / shutdown

```rust
fn reload_project_config(&mut self) -> bool {
    // Cancel any in-flight rebuild before reconfiguring SourceRoot.
    if let Some(h) = self.name_index_rebuild_handle.take() {
        h.cancel();
    }
    self.analysis_host.raw_database().name_index().reset();
    // … existing reload …
}

fn prune_stale_workspace_files(&mut self) {
    let dropped: Vec<FileId> = /* existing collection logic, returned */;
    if !dropped.is_empty() {
        self.analysis_host.raw_database().name_index().remove_batch(&dropped);
    }
}

// crates/bsl-analyzer/src/server.rs — clean shutdown path.
fn handle_shutdown(state: &mut GlobalState) -> Result<()> {
    // Cancel any in-flight rebuild so the rayon scope releases promptly;
    // `reset` is not necessary on shutdown (process exits anyway) but
    // explicit cancel avoids tearing through a partially-built `Inner`
    // and surfaces panics for diagnostics. (Codex pass-3 STILL-OPEN 2.)
    if let Some(h) = state.name_index_rebuild_handle.take() {
        h.cancel();
    }
    // … existing shutdown …
    Ok(())
}
```

## 7. Concurrency model (the load-bearing detail)

### 7.1. Generation protocol

```rust
struct Inner {
    /* … */
    gen: AtomicU64,
}
```

- **Every mutation** (`rebuild swap`, `refresh`, `refresh_batch`,
  `remove`, `remove_batch`, `reset`) bumps `gen` by 1.
- `rebuild_async`:
  1. Captures `start_gen = gen.load()`.
  2. Builds new `Inner` off-lock (no shared state held during lex/merge).
  3. Acquires brief write-lock for swap.
  4. Under lock: applies replay log (§7.2) to the new `Inner`,
     then swaps `*inner = new_inner`. Bumps `gen` once.
  5. Releases lock.

Rebuild **always** swaps (assuming no panic / cancel), because any
in-flight refresh during build was either (a) seen by replay log or
(b) about to be re-applied on top of the swapped `Inner`. The single
atomic bump on swap is enough to invalidate any stale read.

### 7.2. Replay log

```rust
struct Inner {
    /* … */
    is_building: AtomicBool,
    replay_log: parking_lot::Mutex<Vec<ReplayOp>>,
}

enum ReplayOp {
    Refresh { file_id: FileId, source_root: SourceRootId, content: Arc<str> },
    Remove { file_id: FileId },
}
```

- `rebuild_async` sets `is_building = true` before reading any input.
- `refresh` / `remove` (called synchronously from `process_changes`)
  observe `is_building`:
  - If `false`: apply directly to current Inner.
  - If `true`: apply directly to current Inner AND push to replay_log.
- Pre-swap step (§7.1.4): drain replay_log, apply each op to the
  freshly-built Inner. Clears `is_building = false` post-swap.

This guarantees: the post-swap Inner contains both the lex'd state
**and** every refresh/remove that arrived during the build window.
Replay overhead is bounded by edit rate × build duration — typically
a handful of operations.

### 7.3. Cancellation

`RebuildHandle.cancel()` sets a flag observed inside the rebuild
loop (between lex batches). Cancelled rebuild does NOT swap; replay
log is also cleared. Used by `reset()` (config reload) and on shutdown.

### 7.3a. Test hooks

The `#[cfg(test)] pause_before_swap` hook (used by §10 test 9b) MUST
fire AFTER the off-lock lex/merge phase finishes but BEFORE the
rebuild thread acquires the swap write-lock. While parked it holds
no lock; the test driver can call `refresh` (which takes its own
brief write-lock) without contention. Implementation: a
`#[cfg(test)] Option<Arc<Barrier>>` field on `Inner` (or on the
rebuild closure environment) checked between merge-complete and
`inner.write()`. No production code path observes this field.

### 7.4. Lock disciplines

- `lookup` takes `read()` lock — fast (microseconds). Multiple
  concurrent lookups allowed.
- `refresh` / `remove` take `write()` lock briefly (per-call). No
  blocking on rebuild's off-lock work.
- Rebuild's swap takes `write()` lock for the duration of replay +
  swap (milliseconds).
- The `is_building` `AtomicBool` is set/cleared OUTSIDE the lock; no
  ordering issue because the replay log is drained under the lock,
  so any refresh that races sees either `is_building=false` (applies
  directly) or `is_building=true` (also queues to replay).

### 7.5. Failure handling

- Rebuild thread catches `std::panic::catch_unwind`. On panic,
  transitions state to `Failed { error, gen }`. Replay log is
  drained against the OLD Inner (no swap). LSP layer logs warn.
- Subsequent `lookup` returns `LookupResult::Failed` so callers can
  choose between partial results and explicit warning.
- A new `rebuild_async` is allowed from `Failed` state and starts fresh.

## 8. Token predicate

**Match `SyntaxKind::is_name_token` parity** (Codex v2 finding 6):
include `Ident` AND all keyword tokens. BSL allows `obj.Если()` where
`Если` is a method name despite being a keyword token, and the existing
`file_name_usage_query` uses `is_name_token` which permits this.

Implementation: a shared helper crate (or single function exported from
`lexer`) `is_name_token(TokenKind) -> bool`. Both the index build and
`references.rs` use the same predicate — parity tested by a property
test that lex'd identifiers equal parsed name tokens on a fixture set.

Normalization: `to_lowercase()` (BSL case-insensitive).

## 9. Phased delivery (3 PRs)

Splitting into three landings rather than two is a Codex pass-3
recommendation (STILL-OPEN 3): a latent bug in the async build
would surface as silent false-negative references the moment the
fallback is removed. Keeping the fallback alive through one extra
PR of in-tree bake gives us time to detect and fix such bugs before
they affect users.

### Landing 1: provider plumbing + fallback (~3 days)

1. Create empty `crates/bsl-name-index/` with skeleton
   `WorkspaceNameIndex` that **always returns `LookupResult::Pending`**.
   State machine present but no real build logic.
2. Wire `RootDatabaseImpl` to hold `Arc<WorkspaceNameIndex>`.
3. Add `GlobalState::set_bsl_file_text` helper. Migrate
   `process_changes` to two-pass. Refresh/remove calls go to the
   skeleton.
4. Add `db.name_index()` accessor.
5. In `find_references`, call `db.name_index().lookup(...)`; on
   `Pending`/`Failed` route to the existing
   `source_root_name_usage_query` per §6.4 Landings 1+2 branch.
6. Wire `rebuild_sync` for tests/CLI but leave its body unimplemented
   (`todo!()`); CLI `analyze --salsa` still uses the old query.
7. Tests: existing references tests stay green. One new test covers
   the explicit-Pending → fallback path.

Outcome: skeleton in place, zero behavior change. Reviewable in
isolation. Can sit on `develop` indefinitely without risk.

### Landing 2: async persistent index, fallback retained (~5 days)

1. Implement `rebuild_async`, `refresh`, `remove`, generation, replay
   log per §7.
2. Implement `rebuild_sync` for CLI/test use.
3. Wire rebuild dispatch in `server.rs` after `vfs_done`.
4. Wire reset/cancel in `reload_project_config`,
   `prune_stale_workspace_files`, and `handle_shutdown` per §6.5.
5. Implement keyword-aware token predicate, shared helper, parity
   test (§8).
6. Migrate CLI `analyze --salsa` to call `rebuild_sync` after loading
   texts. Existing direct-`set_file_text` test fixtures get the
   same treatment via a shared test helper (`crates/ide-db/src/test_utils.rs`
   or local module). No fixture left building a default DB without
   syncing the index. (Codex pass-3 NEW 1.)
7. Add full test matrix (§10), including barrier-based concurrency
   test (#9b).
8. **Fallback path remains** in `find_references` — `lookup` Ready
   becomes the live path, but Pending/Failed still routes to the
   old `source_root_name_usage_query`. This is the bake period.

Outcome: real index is live, exercised by every reference query
on Ready state; if a build bug returns Pending in steady state, the
old query covers it transparently with a log line. We have time to
notice and fix.

### Landing 3: fallback removal + cleanup (~2 days)

Lands after ≥ 1 week of Landing 2 in production-equivalent use
(internal dogfooding or CI corpus runs) with no fallback-triggered
log lines.

1. Replace the §6.4 Landings 1+2 branch with the Landing 3 branch
   (`state_wait_with_timeout` + `ReferencesResult::Incomplete`).
2. Delete `crates/hir-def/src/name_usage_index.rs`.
3. Remove re-exports from `crates/hir-def/src/lib.rs:72-75`,
   `crates/hir/src/lib.rs:126-139`,
   `crates/ide-db/src/database.rs:348-353`.
4. Remove the hir test referencing the old query
   (`crates/hir/src/lib.rs:1173-1204` per Codex grep).
5. Update CLI bench comments in `crates/bsl-analyzer/src/bin/main.rs`.

Outcome: single code path for workspace identifier lookups; the
old query is gone; lazy-text-loading is unblocked.

### Effort

- Landing 1: 3 days
- Landing 2: 5 days
- Landing 3: 2 days
- **Total: ~10 working days** (one engineer familiar with the
  codebase). Optimistic 8 d, pessimistic 14 d.

Bake interval between Landing 2 and Landing 3 is ≥ 1 week of
production-equivalent use, not counted in the 10 d.

## 10. Test matrix

Unit (`crates/bsl-name-index/tests/`):

1. `lookup_returns_files_containing_name`
2. `lookup_is_case_insensitive`
3. `lookup_filters_by_source_root`
4. `refresh_updates_existing_file_deltas` (add/remove identifiers)
5. `refresh_for_new_file_adds_pairs`
6. `remove_drops_all_pairs_for_file` — and `file_to_root` entry too
   (Codex pass-3 NIT 3)
7. `rebuild_idempotent` (two consecutive rebuilds → identical stats)
8. `keyword_shaped_identifier_indexed` — `obj.Если()` → `lookup("если")`
9. `concurrent_refresh_during_rebuild_consistent` — submit rebuild,
   inject refresh, wait for swap, lookup observes refresh
   (serial harness, exercises the API contract end-to-end)
9b. `barrier_replay_drains_before_swap` — explicit deterministic
    race test using a `#[cfg(test)] pause_before_swap` hook in the
    rebuild thread.

    **Hook placement is load-bearing.** The hook fires AFTER the
    off-lock lex/merge phase completes but BEFORE the rebuild thread
    attempts to acquire the swap write-lock. The hook does not hold
    any lock while parked — the test thread must be free to call
    `refresh(...)` (which takes its own brief write-lock internally)
    without deadlocking against the rebuild. If we placed the hook
    after `lock.write()` acquisition or after `replay_log.drain()`,
    the injected refresh would either deadlock waiting for the lock
    or land too late to be swept up by drain — both wrong.

    Sequence:
    (a) main thread submits `rebuild_async`,
    (b) rebuild thread completes lex/merge off-lock, sets
        `is_building=true` (already true since start), reaches the
        pre-lock barrier, parks, signals main,
    (c) main thread calls `refresh(file_id_X, source_root, content)`,
        which independently takes the write-lock briefly, observes
        `is_building=true`, applies to live `Inner` AND appends to
        replay_log; refresh returns,
    (d) main releases barrier,
    (e) rebuild thread acquires write-lock, drains replay_log
        (containing the injected op), applies to the new `Inner`,
        swaps, releases lock,
    (f) main thread calls `lookup` on file_id_X, asserts the
        post-swap `Inner` contains the injected content.

    Without this test the §7.2 replay protocol is unobservable from
    integration tests. (Codex pass-3 NEW 2 / pass-4 STILL-OPEN 1.)
10. `rebuild_panic_transitions_to_failed`
11. `reset_cancels_in_flight_rebuild` — exercises §7.3 cancellation
    path explicitly (Codex pass-3 NIT 3)
12. `open_doc_overlay_wins_over_disk`
13. `delayed_rebuild_start_picks_up_mem_doc_edits`
14. `failed_state_lookup_returns_failed_then_fallback_path_runs` —
    integration with Landings 1+2 §6.4 fallback (Codex pass-3 NIT 3)
15. `rebuild_sync_after_default_db_construction_yields_ready_state` —
    covers the test-fixture path (`RootDatabaseImpl::default()` +
    `set_file_text` + `rebuild_sync`). Validates Landing 2 step 6.
    (Codex pass-3 NEW 1 / NIT 3.)

Integration (`crates/bsl-analyzer/tests/name_index_lsp.rs`):

16. End-to-end LSP `find_references` workflow on 3-file workspace.
17. `didChange` adds a new identifier → next references picks it up.
18. New BSL file created → indexed → references finds it.
19. `bsl-analyzer.toml` reload (extension added) → old rebuild
    cancelled, new rebuild covers new source roots.
20. `handle_shutdown` after `rebuild_async` started → rebuild
    cancelled cleanly, no orphaned thread (§6.5 / Codex pass-3
    STILL-OPEN 2).

Regression: every existing `references` test passes after Landing 2
and again after Landing 3.

## 11. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Sync invariant drift (`file_text` ≠ index) | Single helper `set_bsl_file_text`; CI feature `name-index-strict` blocks direct `db.set_file_text` for BSL files. |
| Build memory spike (peak 640 MB observed) | Dedicated rayon scope, not concurrent with other workspace tasks. Drop intermediate `per_file` Vec immediately on merge — production code does not materialize it as our bench did. |
| Cold-start UX (Pending state for ~3 s) | §6.4 Landings 1+2 absorb it via transparent fallback. Landing 3 blocks `find_references` up to 500 ms via `state_wait_with_timeout`; on timeout returns `ReferencesResult::Incomplete` + server log. Optional `bsl/nameIndexState` notification (§14) for clients that want to surface indexing status separately. Distinct from streaming partial-result tokens, which are explicitly NOT used (§6.4). |
| Replay log unbounded under edit storm | Replay log applied at swap time; if storm continues past swap, the next rebuild absorbs it. Bound replay_log size at 10 000 ops with `tracing::warn!` if exceeded — practically unreachable. |
| Path normalization differences between lexer and parser | Shared helper `is_name_token`/normalization in one crate; parity test on a corpus of representative BSL files. |
| Future contributor adds a workspace-wide query that needs file_text | Architecture doc + `name_index.rs` doc-comment cite the invariant. Persist as `CONTRIBUTING.md` entry. |

## 12. Out of scope follow-ups

- **Disk persistence** of the index across sessions (~50 MB serialized).
  Would cut cold-start build to ~200 ms mmap or ~500 ms deserialize.
- **Workspace symbols** provider built on the same index (currently no
  LSP handler is wired).
- **Lazy-text** itself — separate plan, runs to completion after Landing 2
  ships and gets a regression sweep.
- **SDBL identifier indexing** (only useful if cross-language references
  ever become a goal).

## 13. References to existing code touched

| File | Touch |
|---|---|
| `crates/bsl-name-index/**` (new) | full implementation |
| `crates/ide-db/src/database.rs:225` | hold `Arc<WorkspaceNameIndex>` on `RootDatabaseImpl` + accessor |
| `crates/bsl-analyzer/src/workspace.rs:153` | two-pass `process_changes` |
| `crates/bsl-analyzer/src/workspace.rs:294` | `prune_stale_workspace_files` returns dropped IDs |
| `crates/bsl-analyzer/src/global_state.rs` | `set_bsl_file_text` helper |
| `crates/bsl-analyzer/src/server.rs:200` | dispatch `rebuild_async` after `vfs_done` |
| `crates/ide/src/references.rs:99` | route through `db.name_index().lookup` |
| `crates/hir-def/src/name_usage_index.rs` | delete after Landing 2 |
| `crates/hir-def/src/lib.rs:72-75` | remove re-exports |
| `crates/hir/src/lib.rs:1173-1204` | remove test |
| `crates/ide-db/src/database.rs:348-353` | remove `DefDatabase` query impl |
| `Cargo.toml` | add workspace member `crates/bsl-name-index` + workspace dep |

## 14. Decisions deferred to implementation

These are not architectural blockers; they get resolved during coding.

- Exact `Inner` representation: `FxHashMap` vs `IndexMap` vs custom.
  Benchmark on real workload.
- Whether `NameInterner` uses 50-line custom or pulls `string-interner`
  crate (would add a dep).
- Whether to expose `state()` over LSP via custom `bsl/nameIndexState`
  notification for client debugging.

Note: the `state_wait_with_timeout` duration in §6.4 Landing 3 is
fixed at 500 ms in the plan, not deferred. Tunable via a const but
the policy is final.

---

**Awaiting Codex pass 5 sign-off.**
