# Persistent workspace name-index (plan v6)

**Status**: READY-TO-IMPLEMENT, spike-validated, awaiting Codex pass.
**Supersedes**: `docs/roadmap/name-index.md` (v5, non-Salsa adjunct).
**Reason for new revision**: v5 shared `Arc<WorkspaceNameIndex>` across cloned
Salsa snapshots without binding a lookup to the request's Salsa revision —
an architectural footgun confirmed during Codex review. v6 partitions the
index across `#[salsa::tracked]` queries keyed by `ModuleLikeId`, so all
invalidation, cancellation, and snapshot coherence are inherited from Salsa.

## 1. Goal

Replace the eager `source_root_name_usage_query` (which forces `db.file_text`
for every BSL file in the source root, blocking lazy-text — currently
~1.66 GB resident on ERP) with a partitioned Salsa-tracked workspace
name-index keyed by `ModuleLikeId`. The index serves `find_references` and
forms the foundation for lazy-text loading.

## 2. Non-goals

- Workspace symbols provider (`textDocument/workspaceSymbol`). Same data
  shape works, but the LSP handler is out of scope.
- Disk persistence of the index across LSP sessions.
- SDBL identifier indexing.
- CommandModule / HTTPService / WebService coverage (their UUIDs sit in
  specialised `Configuration` tables — separate slice).
- Removal of `MetadataObject::uuid()` accessor or the spike branches.

## 3. Spike findings (empirical anchor)

Spike branch `spike/name-index-salsa-partitioned` (commits 8abbd545,
185a865d, bb5c9630, 3e52387d). On a real ERP workspace
(25 617 .bsl files, 6 676 MDOs, 21 834 module-likes, 532 391 unique names):

| Question | Result | Bar | Verdict |
|---|---|---|---|
| Q1 ModuleLikeId coverage | **94.4%** | ≥ 90% | GO |
| Q2 `db.file_text` reads on lookup | **0 by construction** | = 0 | GO |
| Q2 incremental edit cost | set: 16 μs · re-lookup: 2.6 ms (1 of 21 834 memos re-runs) | works | GO |
| Q3 warm lookup, hottest name (10 187 hits) | 5.6 ms | ≤ 5 ms | borderline |
| Q3 warm lookup, typical (≤ 5 k hits) | 1–3.5 ms | ≤ 5 ms | GO |

**Q2 is enforced by the type system, not by discipline**: the spike
`SpikeDatabase` has no `FileTextInput`, so `db.file_text` is literally
unavailable. v6 carries this property forward — `FileLexDigest` is a
`#[salsa::input]` populated directly from VFS, not a derived query.

**Q3 5.6 ms on the worst name** is acceptable without an aggregator —
`find_references` is a user-triggered click, 5 ms is imperceptible. If
profiling later shows a hot path that runs more frequently, an aggregator
follow-up is straightforward (§12).

## 4. Architecture

### 4.1. Identity — `ModuleLikeId`

```rust
// crates/hir-def/src/name_index_v6.rs (or sibling)
#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub enum ModuleLikeId {
    /// Stable: a `CommonModule` UUID.
    CommonModule(Uuid),
    /// Stable: a root MDO UUID (Catalog/Document/Report/Registers/Enum/…).
    /// `kind` is `bsl_metadata::MdoType` — this is the one place `hir-def`
    /// gains a dep edge on `bsl-metadata`. Justified: `MdoType` is already
    /// used pervasively in `hir-ty` for type resolution and would be
    /// pulled in eventually; better to take the dep deliberately here.
    Root { kind: MdoType, uuid: Uuid },
    /// Stable composite: form name attached to a parent MDO UUID.
    Form { owner: Uuid, form_name: Name },
    /// Stable composite: command name attached to a parent MDO UUID.
    Command { owner: Uuid, command_name: Name },
    /// Files without a stable owner (HTTPService, WebService, CommonForm /
    /// CommonCommand, ManagedApplicationModule, …). Indexed individually
    /// so they still participate in workspace-wide lookup.
    OrphanFile(FileId),
}
```

`Uuid` is sourced from the `MetadataObject::uuid()` / `CommonModule::uuid()`
/ `Register::uuid()` accessors (commit 8abbd545). The classifier is the
production version of `bsl_analyzer::spike_name_index::derive_module_like_kind`
— see §4.2 for placement.

### 4.2. Layer placement

```
crates/
  hir-def/src/name_index_v6/
    mod.rs        — Salsa types + tracked queries (no Configuration here)
    populate.rs   — populate API: take (FileId, &[Name]) pairs

  ide-db/src/
    name_index_classifier.rs
                  — pub fn derive_module_like_id(
                        config: &bsl_metadata::Configuration,
                        file_path: &str,
                    ) -> Option<ModuleLikeId>;
                    moved from spike, uses bsl-metadata accessors

  bsl-analyzer/src/
    workspace.rs  — process_changes calls populate API after set_file_text
    server.rs     — boot: parallel_prime_caches_v6 after vfs_done
```

The Salsa machinery sits in `hir-def` (the existing semantic-incremental
home). The Configuration-dependent classifier sits in `ide-db` (one layer
above). Integration sits in `bsl-analyzer`. No new crate.

### 4.3. Salsa surface

```rust
// crates/hir-def/src/name_index_v6/mod.rs

/// Per-file lexical digest. INPUT — populated by the lexer/walker, not
/// derived from `FileTextInput`. Mutating one digest invalidates exactly
/// the `module_like_name_index` memos that reference it.
///
/// `Arc<FxHashSet<Name>>` field type matches the spike (lifetime-free
/// shape that compiles cleanly with salsa 0.26 — `#[salsa::input]` does
/// NOT take a `'db` lifetime parameter, unlike `#[salsa::tracked]`).
#[salsa::input(debug)]
pub struct FileLexDigest {
    pub file_id: FileId,
    pub names: Arc<FxHashSet<Name>>,
}

/// Per-ModuleLikeId membership list. INPUT — populated once per module.
/// On membership change (file added/removed within a Form, MDO renamed,
/// …) replace the whole input; Salsa invalidates dependents.
#[salsa::input(debug)]
pub struct ModuleLikeMembership {
    pub id: ModuleLikeId,
    pub files: Arc<Vec<FileLexDigest>>,
}

/// Workspace registry — list of all known module-likes. INPUT — updated on
/// MDO add/remove events from `process_changes`.
#[salsa::input(debug)]
pub struct WorkspaceMembers {
    pub members: Arc<Vec<ModuleLikeMembership>>,
}

/// Union of digests for one module-like. TRACKED — re-runs only when the
/// membership's digest list changes or one of its referenced
/// `FileLexDigest.names` changes.
#[salsa::tracked]
pub fn module_like_name_index(
    db: &dyn DefDatabase,
    membership: ModuleLikeMembership,
) -> Arc<ModuleNameIndex>;

/// Workspace-wide lookup. Plain function, NOT tracked: deliberate, so that
/// editing one file does not invalidate the global aggregator (which would
/// re-merge all ~10–20 k indices on every keystroke). Cost is dominated by
/// hash lookups across cached per-module memos — 1–6 ms on ERP (spike Q3).
pub fn lookup_workspace(db: &dyn DefDatabase, name: &Name) -> Vec<FileId>;
```

`Name` is the existing `hir_def::Name` (already used in
`name_usage_index.rs`). `Arc<FxHashSet<Name>>` is the digest-storage shape
verified in the spike (`crates/bsl-analyzer/src/spike_name_index_salsa.rs`):
cheap `Arc::clone()` on read, full replace on `set_names(db).to(new_arc)`
when the file is re-lexed. `Arc<Vec<…>>` for membership lists is the same
pattern.

### 4.4. Input handle storage — `NameIndexHandles`

`WorkspaceNameIndex::refresh(db, file_id, names)` mutates the **existing**
`FileLexDigest` cell for `file_id`. Same for `rebind` on
`ModuleLikeMembership`. To find those handles after populate, v6 needs a
side map from key → `Copy` Salsa input handle. This is the **same pattern
base-db already uses for `FileTextInput`** (`crates/base-db/src/lib.rs:176`,
the `Files` struct), not a new architectural exception.

```rust
// crates/hir-def/src/name_index_v6/handles.rs
use std::hash::BuildHasherDefault;
use std::sync::atomic::AtomicBool;
use parking_lot::{Condvar, Mutex};
use rustc_hash::FxHasher;

#[derive(Debug, Default, Clone)]
pub struct NameIndexHandles {
    digests: Arc<DashMap<FileId, FileLexDigest, BuildHasherDefault<FxHasher>>>,
    memberships: Arc<DashMap<ModuleLikeId, ModuleLikeMembership, BuildHasherDefault<FxHasher>>>,
    workspace: Arc<RwLock<Option<WorkspaceMembers>>>,
    /// `false → true` once, when `prime_workspace_name_index` completes.
    /// Never flips back; subsequent edits are deltas through `refresh`.
    /// **Read-only check from `lookup`** — `lookup` returns `Pending`
    /// instantly if unprimed, never blocks. Blocking inside `lookup`
    /// (which runs under a Salsa snapshot) would deadlock prime: prime
    /// needs `raw_database_mut()` to commit `FileLexDigest` writes, and
    /// Salsa blocks `mut` access until all live snapshots are dropped
    /// (see `Files` doc-comment, `base-db/src/lib.rs:155-175`, ABBA).
    primed: Arc<AtomicBool>,
    /// Signal channel for `wait_until_primed` (§5). Only callers that
    /// have already dropped their snapshot may park here — typically the
    /// `find_references` handler after observing `LookupResult::Pending`
    /// (§6.3). `lookup` itself NEVER touches this condvar.
    prime_signal: Arc<(Mutex<()>, Condvar)>,
}
```

Salsa input handles (`FileLexDigest`, `ModuleLikeMembership`, `WorkspaceMembers`)
have NO lifetime parameter — `#[salsa::input]` produces a `Copy` newtype
around an `Id`. The `BuildHasherDefault<FxHasher>` hasher type matches
the canonical pattern in `crates/base-db/src/lib.rs:178`.

Held as a field on `RootDatabaseImpl` (in `ide-db`) alongside the existing
`files: Files`:

```rust
// crates/ide-db/src/database.rs
pub struct RootDatabaseImpl {
    storage: salsa::Storage<Self>,
    files: Files,                      // existing
    name_index: NameIndexHandles,      // new — handle storage only
    // …
}
```

**This is NOT a v5 regression.** v5's footgun: the WHOLE INDEX (query
results) lived outside Salsa, snapshot-incoherent. v6's `NameIndexHandles`
holds ONLY input-cell HANDLES (8-byte `Copy` values). The actual digest
data lives inside the Salsa `FileLexDigest` cells; query results live in
`module_like_name_index` memos. Both are revisioned by Salsa. The handle
map's role is purely "key → handle" lookup, identical to `Files::file_texts`
for `FileTextInput`.

**Locking invariant** (carried verbatim from `Files` doc, `lib.rs:155-175`):

> Setters MUST NOT hold a DashMap shard guard across a Salsa setter call.
> The generated `input.set_<field>(db)` acquires `zalsa_mut()` internally,
> which blocks until every live database handle is dropped.
> Fix: look up the existing handle under a short `get()` guard,
> copy the handle (it is `Copy`), drop the guard, and only then invoke
> the Salsa setter.

Single-mutator invariant (only the LSP main loop writes) is enforced by
`set_bsl_file_text` discipline + `name-index-strict` CI feature, matching
the existing pattern for `set_file_text`. Pair with
`AnalysisHost::request_cancellation()` at `process_changes` entry, also
matching the existing pattern.

`WorkspaceNameIndex::refresh` flow (mirrors `Files::set_file_text` from
`crates/base-db/src/lib.rs:213-233`):

```rust
fn refresh(db: &mut dyn DefDatabase, file_id: FileId, names: Arc<FxHashSet<Name>>) {
    use salsa::Setter;
    // Arc-clone the handle struct so we don't keep `db` borrowed while
    // the Salsa setter takes `&mut self`. Same trick as `Files::set_file_text`:
    //   `let files = self.files.clone(); files.set_file_text(self, …);`
    let handles = db.name_index().clone();
    // Short-lock: get under brief shard guard, copy the Copy handle, drop guard.
    let existing = handles.digests.get(&file_id).map(|e| *e.value());
    match existing {
        Some(digest) => digest.set_names(db).to(names),
        None => {
            let digest = FileLexDigest::new(db, file_id, names);
            let prev = handles.digests.insert(file_id, digest);
            debug_assert!(prev.is_none(), "single-mutator violated");
        }
    }
}
```

`remove` deletes from `digests`; the removed `FileLexDigest` becomes
unreachable from any `ModuleLikeMembership` once `rebind` updates the
membership lists. (Salsa GC reclaims it on the next revision bump.)

`rebind` follows the same Arc-clone-then-get-copy-drop-set shape on
`memberships`. `lookup` is **non-blocking**: it checks `primed` and
either returns `Ready` (workspace iteration) or `Pending` (caller's
responsibility to surface). The caller (find_references handler) holds
a Salsa snapshot; blocking it on a condvar would prevent prime from
acquiring `raw_database_mut()` to commit input writes — ABBA deadlock
(see `Files` doc-comment, `base-db/src/lib.rs:155-175`).

```rust
fn lookup(db: &dyn DefDatabase, name: &Name) -> LookupResult {
    use std::sync::atomic::Ordering;

    let handles = db.name_index().clone();

    if !handles.primed.load(Ordering::Acquire) {
        // Cold-start window. Returning Pending is the only correct
        // option — caller holds a snapshot; blocking here would deadlock
        // prime. find_references handler translates Pending to a
        // window/showMessage "Indexing, retry" notification (§6.3).
        return LookupResult::Pending;
    }

    // Primed: copy WorkspaceMembers handle under brief read guard,
    // then iterate via Salsa query outside the guard.
    let ws = handles.workspace.read();
    let members_handle = ws.expect("primed implies workspace populated");
    drop(ws);

    let mut out: Vec<FileId> = Vec::new();
    for &m in members_handle.members(db).iter() {
        let idx = module_like_name_index(db, m);
        if let Some(hits) = idx.get(name) {
            out.extend_from_slice(hits);
        }
    }
    LookupResult::Ready(out)
}
```

`prime_workspace_name_index` (§6.2) signals completion with one atomic
store plus a condvar wake:

```rust
handles.primed.store(true, Ordering::Release);
let (_lock, cv) = &*handles.prime_signal;
cv.notify_all();
```

`lookup` itself only consults the atomic (Release/Acquire so the bit-flip
implies all input writes from prime are visible). The condvar is a
separate channel used **only** by `wait_until_primed` callers, which
park outside any Salsa snapshot (see §5 / §6.3 — the find_references
handler drops its snapshot before parking). No generations, no replay
log, no atomic swap.

### 4.5. Why no Salsa-tracked aggregator

Q3 measured warm lookups at 1–5.6 ms iterating ~22 k per-module indices.
A `#[salsa::tracked] workspace_name_index → Arc<HashMap<Name, Vec<FileId>>>`
would give O(1) lookup but invalidate on **any** `module_like_name_index`
change — i.e. on every file edit. Re-merging 22 k indices on every
keystroke (~500 ms locally measured) is worse than 5.6 ms once per
`find_references` click.

If future profiling identifies a hot path that runs more frequently than
`find_references` (e.g. completion-time lookup), a per-name aggregator
keyed by `Name` with `lru = N` is the natural follow-up. Out of scope for
v6.

## 5. Public API

```rust
// crates/hir-def/src/name_index_v6/mod.rs

impl WorkspaceNameIndex {
    /// Populate or refresh a single file's lexical digest. Called from
    /// `set_bsl_file_text` (§6) immediately after `set_file_text`.
    pub fn refresh(db: &mut dyn DefDatabase, file_id: FileId, names: Arc<FxHashSet<Name>>);

    /// Register a module-like grouping. Called from
    /// `prime_workspace_name_index` (§6.2) once per `(ModuleLikeId,
    /// Vec<FileId>)` group derived by the classifier. Creates the
    /// `ModuleLikeMembership` input cell and inserts it into
    /// `WorkspaceMembers`. Idempotent: re-registering an existing
    /// `ModuleLikeId` replaces its file list (used on MDO rename /
    /// extension reload).
    pub fn register_module(
        db: &mut dyn DefDatabase,
        mlid: ModuleLikeId,
        files: Vec<FileId>,
    );

    /// Drop a file from all module-likes that referenced it. Called on
    /// `Change::Delete`.
    pub fn remove(db: &mut dyn DefDatabase, file_id: FileId);

    /// Re-classify which `ModuleLikeId` owns a file. Called on MDO add /
    /// rename / delete, when the Configuration mutation in
    /// `process_changes` re-derives owner identity.
    pub fn rebind(db: &mut dyn DefDatabase, file_id: FileId, new: ModuleLikeId);

    /// Workspace-wide reverse lookup. Returns `Pending` while the
    /// initial prime is still running, `Ready(files)` once available.
    pub fn lookup(db: &dyn DefDatabase, name: &Name) -> LookupResult;

    /// Fast read of the prime-complete bit. Non-blocking; consults the
    /// `AtomicBool` only. Used by the live-fire comparator (§9 Landing 2)
    /// to skip comparison during the cold-start window.
    pub fn is_primed(db: &dyn DefDatabase) -> bool;
}

impl NameIndexHandles {
    /// Block the current thread until `primed = true` or `timeout` elapses.
    /// Returns `true` if primed within the window.
    ///
    /// **CONTRACT**: the caller MUST have dropped any Salsa snapshot
    /// before invoking this. Holding a snapshot here would deadlock
    /// `prime_workspace_name_index` (which needs `raw_database_mut()`).
    /// Used by `find_references` handler (§6.3) after observing
    /// `LookupResult::Pending`.
    pub fn wait_until_primed(&self, timeout: Duration) -> bool;
}

pub enum LookupResult {
    /// Index is primed; `files` is the full result for `name`.
    Ready(Vec<FileId>),
    /// Prime has not finished yet (cold startup, ≤ ~3 s window on ERP).
    /// Callers must NOT treat this as "no references" — the right LSP
    /// response is "indexing, please retry" (e.g. `MessageType::Info`).
    Pending,
}
```

Compared with v5's `IndexState::{Empty | Building | Ready | Failed}`
machine plus generation counters plus replay log plus atomic swap, v6
keeps **only one bit**: `primed: AtomicBool` on `NameIndexHandles`
(§4.4). It flips `false → true` exactly once, at the end of the boot
`prime_workspace_name_index`, and never flips back. Subsequent reloads
update memberships incrementally through `process_changes`; the bit
stays true. No replay log, no atomic swap, no generation, no Failed
state (a failed prime panics — same posture as Salsa-tracked queries
themselves).

`lookup` is non-blocking: it consults `primed` and returns `Ready` or
`Pending` immediately (§4.4 has the full snippet). Blocking inside
`lookup` while holding a Salsa snapshot would deadlock prime — see the
`Files`/ABBA discussion in §4.4. The find_references handler (§6.3)
recovers from `Pending` by dropping its snapshot, calling
`wait_until_primed`, and retaking a fresh snapshot for the retry.
This keeps the v5-style "no false-empty references" guarantee with a
single atomic bit plus a separate condvar used **only outside snapshot
context**.

The `name-index-strict` CI feature and `set_bsl_file_text` discipline carry
over from v5 §6.1 — they remain useful as guard rails against a future
contributor forgetting to call `WorkspaceNameIndex::refresh` after writing
to `FileTextInput`.

## 6. Integration

### 6.1. `process_changes` (workspace.rs)

```rust
pub fn process_changes(&mut self, suppress_metadata_bump: bool) -> (bool, bool) {
    // Phase 1: Salsa mutations (file_text + name_index)
    let mut digest_batch: Vec<(FileId, Arc<FxHashSet<Name>>)> = Vec::new();
    let mut remove_batch: Vec<FileId> = Vec::new();
    let mut rebind_batch: Vec<(FileId, ModuleLikeId)> = Vec::new();
    {
        let db = self.analysis_host.raw_database_mut();
        for change in changed_files {
            match change {
                Change::Create(text, _) | Change::Modify(text, _) => {
                    if is_bsl_source(&file_set, file_id) {
                        db.set_file_text(file_id, &text);
                        let digest = Arc::new(lex_to_digest(&text));
                        digest_batch.push((file_id, digest));
                    }
                }
                Change::Delete => {
                    if is_bsl_source(&file_set, file_id) {
                        remove_batch.push(file_id);
                    }
                }
            }
        }
        // … existing SourceRoot membership / metadata XML handling …

        // Phase 2: apply digests now (still holding db mut borrow — cheap).
        for (file_id, names) in digest_batch {
            WorkspaceNameIndex::refresh(db, file_id, names);
        }
        for file_id in remove_batch {
            WorkspaceNameIndex::remove(db, file_id);
        }
    }

    // After Phase 1 the mutable DB borrow is dropped. Phase 2 classifies
    // paths via the existing `RootDatabase::get_configuration(file_id)`
    // accessor (`crates/ide-db/src/database.rs:427` — see §15.4): it's a
    // `&self` method that internally runs a Salsa query and returns an
    // `Arc<Configuration>`, no held snapshot. Phase 3 reacquires the
    // MUTABLE borrow only for the rebind writes.
    {
        let db = self.analysis_host.raw_database();
        for (file_id, path) in newly_classified_files {
            let config = match db.get_configuration(file_id) {
                Some(c) => c,
                None => continue,
            };
            if let Some(mlid) = derive_module_like_id(&config, &path) {
                rebind_batch.push((file_id, mlid));
            }
        }
    }
    {
        let db = self.analysis_host.raw_database_mut();
        for (file_id, mlid) in rebind_batch {
            WorkspaceNameIndex::rebind(db, file_id, mlid);
        }
    }

    (true, config_file_changed)
}
```

`lex_to_digest` is the production version of the spike's `lex_file` — a
tokenize-and-collect-name-tokens pass, ~600 μs per file at ERP scale.
Predicate widens to `SyntaxKind::is_name_token` for `obj.Если()` parity
(v5 §8 carries over verbatim).

### 6.2. Boot — `parallel_prime_caches_v6`

```rust
// crates/bsl-analyzer/src/server.rs, in the `vfs_done` handler
// MUST run AFTER `warm_metadata_cache()` and `vfs_done = true` so that
// Configuration XML is fully parsed at classification time (no need for
// a second rebind pass).
fn prime_workspace_name_index(global_state: &mut GlobalState) {
    let _span = tracing::info_span!("prime_workspace_name_index").entered();

    // Step 1: parallel lex (rayon) — same pattern as
    // `crates/bsl-analyzer/src/bin/main.rs::run_deps_bench_index`.
    // No DB borrow held during the lex pass.
    let lexed: Vec<(FileId, PathBuf, Arc<FxHashSet<Name>>)> =
        all_bsl_files(global_state)
            .into_par_iter()
            .map(|(fid, path, text)| (fid, path, Arc::new(lex_to_digest(&text))))
            .collect();

    // Step 2: classify against the Configuration BEFORE taking the mut
    // DB borrow. Classification is read-only on `RootDatabase::get_configuration`
    // (`crates/ide-db/src/database.rs:427`, see §15.4) — a `&self` Salsa
    // query that returns `Arc<Configuration>`.
    let db = global_state.analysis_host.raw_database();
    let mut groups: FxHashMap<ModuleLikeId, Vec<FileId>> = Default::default();
    for (fid, path, _) in &lexed {
        let mlid = db
            .get_configuration(*fid)
            .and_then(|config| derive_module_like_id(&config, path))
            .unwrap_or(ModuleLikeId::OrphanFile(*fid));
        groups.entry(mlid).or_default().push(*fid);
    }
    drop(db); // explicit release of `&self` before next step takes `&mut`.

    // Step 3: serial Salsa input setup (Salsa db is not Sync for writes).
    let db = global_state.analysis_host.raw_database_mut();
    for (fid, _path, names) in &lexed {
        WorkspaceNameIndex::refresh(db, *fid, Arc::clone(names));
    }
    for (mlid, files) in groups {
        WorkspaceNameIndex::register_module(db, mlid, files);
    }

    // Step 4: flip the primed bit + wake `wait_until_primed` callers.
    // MUST happen last, after every input write above has been committed.
    // Release pairs with Acquire in `lookup` so cross-thread visibility
    // is guaranteed. The condvar notify is for any `find_references`
    // handler currently parked in `wait_until_primed` after observing
    // `LookupResult::Pending` (§6.3).
    let handles = db.name_index().clone();
    handles.primed.store(true, std::sync::atomic::Ordering::Release);
    let (_lock, cv) = &*handles.prime_signal;
    cv.notify_all();
}
```

Runs after `vfs_done` on a dedicated rayon scope. Spike measured ~2.4 s
parallel lex + ~10 ms Salsa populate for ERP. `parallel_prime_caches`-style
progress reporting via `$/progress` (existing LSP plumbing) keeps the
client informed; no new LSP message kind. Cancellation via the existing
`Cancelled::catch` pattern from `rust-analyzer/crates/ide-db/src/prime_caches.rs`.

### 6.3. `find_references` handler (bsl-analyzer/src/handlers/…)

The handler — NOT the inner `workspace_candidate_files` — owns the
Pending-recovery flow. The crucial discipline: a Salsa snapshot MUST be
dropped before waiting, then a fresh snapshot taken for the retry.

```rust
// handler runs on a request thread.
fn handle_references(
    global_state: &mut GlobalState,
    params: ReferenceParams,
) -> Vec<Location> {
    // First attempt under a snapshot.
    let outcome = {
        let db = global_state.analysis_host.snapshot();
        WorkspaceNameIndex::lookup(&db, &name)
    }; // snapshot dropped here.

    let files = match outcome {
        LookupResult::Ready(files) => files,
        LookupResult::Pending => {
            // Snapshot is already dropped (block above ended). Now it's
            // safe for prime to acquire raw_database_mut() and commit
            // input writes — no held snapshot blocking it.
            let handles = global_state.analysis_host.raw_database().name_index().clone();
            let primed = handles.wait_until_primed(Duration::from_secs(30));
            if !primed {
                // Prime took > 30 s — almost certainly a bug in prime
                // scheduling or a stalled rayon scope. Surface to user.
                global_state.show_message(
                    MessageType::ERROR,
                    "BSL name-index did not finish indexing within 30 s; \
                     check server log and reload the workspace.",
                );
                return Vec::new();
            }
            // Retake a fresh snapshot post-prime, retry lookup.
            let db = global_state.analysis_host.snapshot();
            match WorkspaceNameIndex::lookup(&db, &name) {
                LookupResult::Ready(files) => files,
                LookupResult::Pending => {
                    // Should not happen — we just observed primed=true.
                    tracing::error!("name-index regressed from primed to Pending");
                    return Vec::new();
                }
            }
        }
    };
    convert_to_locations(files)
}
```

`NameIndexHandles::wait_until_primed(timeout)` is a thin helper:

```rust
impl NameIndexHandles {
    /// Block the current thread until `primed = true` or `timeout` elapses.
    /// Returns `true` if primed within the window. MUST be called with
    /// NO Salsa snapshot held by the current thread (the caller is
    /// expected to have dropped any snapshot before invoking this).
    pub fn wait_until_primed(&self, timeout: Duration) -> bool {
        // Implementation uses a condvar that prime_workspace_name_index
        // signals at the end. Because no snapshot is held during this
        // wait, prime can freely acquire raw_database_mut() and commit
        // input writes.
        // …
    }
}
```

`workspace_candidate_files` (called by ide::references) stays
non-blocking and snapshot-safe:

```rust
fn workspace_candidate_files(db: &dyn DefDatabase, name: &Name) -> LookupResult {
    WorkspaceNameIndex::lookup(db, name)
}
```

User-visible behaviour during boot:

| Boot phase | First call outcome | UX |
|---|---|---|
| `t < prime_complete` (~2.5 s window) | Pending → drop snapshot → wait ~2.5 s → retake → Ready | Single ≤ 3 s delay on first cold-start call; client shows busy cursor |
| `t ≥ prime_complete`, name never looked up | Ready after ≤ 600 ms (cold per-mlid populate, spike Q3) | Single sub-second delay |
| `t ≥ prime_complete`, name warm | Ready after 1–6 ms | Instant |
| Prime stalled > 30 s | window/showMessage ERROR + empty | User sees explicit error |

The deadlock-avoidance rule (drop snapshot before wait, retake after) is
the load-bearing pattern. Pinned by test `lookup_pending_drop_snapshot_wait_retake`
(§10).

### 6.4. Memory & build characteristics

Spike measured (Q2+Q3 spike, on a STANDALONE database that does not hold
`FileTextInput`):

| Phase | Cost (ERP scale) |
|---|---|
| Parallel lex | 2.4 s (12 workers) |
| Salsa input populate | 10 ms (47 k inputs) |
| HashMap baseline comparison | 504 ms — not used in v6 |
| Spike RSS after prime | ~640 MB peak, ~500 MB steady |

**v6 ship state memory framing**: lazy-text is out of scope for v6 (§12).
The existing eager BSL `file_text` residency (~1.44 GB resident text on
ERP) remains. The name-index adds roughly **~70 MB of digest/index data**
on top of that — `Σ |FileLexDigest| + Σ |module_like_name_index| ≈ 70 MB`
based on spike measurement. The 640 / 500 MB spike RSS demonstrates the
future lazy-text target shape, NOT the v6 memory delta.

## 7. Snapshot coherence

This is the v5 → v6 architectural correctness win. v5's
`Arc<WorkspaceNameIndex>` lived outside Salsa: a `find_references` snapshot
could observe revision R for parse results and a different revision R+2
for the name index. v6's index lives entirely inside Salsa: any
`db.lookup(name)` call is bound to the snapshot's revision; partial-update
windows are impossible.

The non-Salsa code in v5 §7 (generation protocol, replay log, atomic swap,
`pause_before_swap` test hook) has **no equivalent in v6** — Salsa
provides all of it.

## 8. Token predicate

Carried verbatim from v5 §8: lex with `lexer::tokenize`, accept all tokens
matching `SyntaxKind::is_name_token` (Ident plus keyword-shaped tokens for
`obj.Если()` parity). Lowercase-fold via `Name::new(text.to_lowercase())`.
Parity test on a representative BSL fixture file required (test matrix
§10).

The bench (`--bench-index`, commit 185a865d) under-estimates production
cost by ~7% because it filters on `TokenKind::Ident` only. Production
predicate widens; v6 inherits the same lex pass at slightly higher
real-world cost (~3.0 s on ERP at 12 workers, projected).

## 9. Phased delivery (3 landings)

### Landing 1 — Salsa skeleton + classifier (~3 days)

- Move spike `spike_name_index_salsa.rs` types (FileLexDigest,
  ModuleLikeMembership, WorkspaceMembers, module_like_name_index) into
  `crates/hir-def/src/name_index_v6/mod.rs`.
- Move spike `spike_name_index.rs::derive_module_like_kind` into
  `crates/ide-db/src/name_index_classifier.rs`. Widen predicate to
  `SyntaxKind::is_name_token`.
- Add `WorkspaceNameIndex::{refresh, remove, rebind, lookup}` shim. NO
  caller switching yet — old `source_root_name_usage_query` keeps running.
- **Explicit cleanup**: after the moved code compiles, delete
  `crates/bsl-analyzer/src/spike_name_index.rs` and `spike_name_index_salsa.rs`,
  remove `pub mod spike_name_index*` lines from `bsl-analyzer/src/lib.rs`,
  and drop the `SpikeNameIndex` CLI subcommand from `main.rs`. Spike
  branch `spike/name-index-salsa-partitioned` stays in git history but
  the production tree carries no spike code into Landing 2.
- Tests:
  - Q2 incremental invariant: edit one digest, exactly one
    `module_like_name_index` memo invalidates (regression guard for
    Salsa dependency tracking).
  - Lookup parity vs `source_root_name_usage_query` on a fixture
    (covers predicate + lower-case-fold).
  - Snapshot coherence: lookup result on a snapshot matches the parse
    results on that snapshot.

### Landing 2 — wire into LSP (~5 days, bake ≥ 1 week before Landing 3)

- `set_bsl_file_text` helper in `GlobalState`; CI feature
  `name-index-strict` for direct `db.set_file_text` shim.
- `process_changes` two-phase update per §6.1.
- `prime_workspace_name_index` after `vfs_done` per §6.2.
- `find_references` route through `WorkspaceNameIndex::lookup`. The old
  `source_root_name_usage_query` REMAINS WIRED in parallel as a sanity
  comparator gated behind feature `name-index-compat`.
- **Comparator semantics (CI)**: run fixture setup → run `prime_workspace_name_index`
  to completion → compare sorted `Vec<FileId>` from old and new lookup on
  the SAME request snapshot. Both lookups MUST happen inside one Salsa
  `db.snapshot()` so they see the same revision. Mismatch is a CI failure.
  No comparison runs before prime completes (returns Empty in that
  window, which would otherwise produce spurious divergence).
- **Comparator semantics (live LSP bake)**: under feature `name-index-compat`,
  the live server logs divergence but does NOT block requests. Pre-prime
  window is skipped (`if !WorkspaceNameIndex::is_primed(db) { return; }`).
- Bake ≥ 1 week. Manual ERP-scale verification of `find_references` on
  hot symbols (`ОбщегоНазначения.СообщитьПользователю`, etc.) against the
  v5 baseline.

### Landing 3 — remove the old (~2 days)

- Delete `crates/hir-def/src/name_usage_index.rs`.
- Remove the `name-index-compat` feature gate.
- Remove `file_name_usage_query` LRU pin.
- Verify lazy-text becomes viable: separate ticket scoped to a follow-up
  PR after Landing 3 ships. The lazy-text experiment is independent.

### Effort summary

| Landing | Days | Risk |
|---|---|---|
| 1 | ~3 | Low — code mostly moves from spike, signatures stabilise |
| 2 | ~5 | Medium — concurrent live-fire with old query, careful testing |
| 3 | ~2 | Low — pure removal after bake |
| **Total** | **~10 + bake** | |

## 10. Test matrix

| Test | What it pins | Landing |
|---|---|---|
| `incremental_edit_invalidates_one_mli` | Salsa dependency graph correctness | 1 |
| `lookup_parity_vs_old_query` | predicate + lower-case-fold | 1 |
| `snapshot_coherence_on_revision_bump` | lookup result matches parse-time revision | 1 |
| `prime_caches_workload_size` | Memory budget regression guard (RSS post-prime < 800 MB on ERP) | 2 |
| `rebind_on_mdo_rename` | Configuration mutation → rebind path | 2 |
| `find_references_e2e_vs_v5_baseline` | output equivalence on ERP fixtures | 2 |
| `command_module_orphan_handling` | OrphanFile bucket isolation | 1 |
| `is_name_token_parity` | predicate matches `find_references_in_file` | 1 |
| `lookup_under_concurrent_edit_storm` | Salsa cancellation propagates correctly | 2 |
| `lookup_hottest_name_under_budget` | warm `lookup_workspace("ОбщегоНазначения")` ≤ 10 ms on ERP perf fixture (Q3 budget = 2× spike worst-case) | 2 |
| `name_index_memory_delta_under_budget` | post-prime RSS minus pre-prime RSS ≤ 100 MB on ERP perf fixture (covers digest + memo storage) | 2 |
| `lookup_returns_pending_when_unprimed` | unprimed handles → `lookup` returns `Pending` immediately, NEVER blocks (regression guard against the deadlock-prone blocking design). | 1 |
| `lookup_pending_drop_snapshot_wait_retake` | full handler flow: take snapshot, lookup → Pending, drop snapshot, `wait_until_primed`, spawn prime on second thread (needs raw_database_mut, MUST succeed because snapshot was dropped), retake snapshot, lookup → Ready. Pins the deadlock-avoidance discipline. | 1 |
| `wait_until_primed_with_held_snapshot_deadlocks` | negative test (timeout-bounded): caller holds snapshot → `wait_until_primed(timeout=100ms)` returns `false`. Documents the contract failure mode. | 1 |

Snapshot tests (`expect-test`) for at least three representative
configurations:
- Bare CommonModule with no Forms
- Catalog with ObjectModule + ManagerModule + 2 Forms
- Register with RecordSetModule + ManagerModule + Form

## 11. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Lex pass on cold start adds visible latency (spike: 2.4 s) | `prime_workspace_name_index` runs after `vfs_done` on a dedicated rayon scope; LSP traffic is unaffected. `$/progress` keeps client informed. Same pattern as RA `parallel_prime_caches`. |
| Predicate divergence between `lex_to_digest` and `find_references_in_file` | Shared `is_name_token` helper in one module; parity test (§10). |
| Salsa memory growth from per-mlid memos on a 20 k-mlid workspace | Memos are small (~3 KB avg = ~60 MB). No LRU needed; spike Q3 confirmed steady-state RSS. |
| Worst-case warm lookup 5.6 ms on hottest name perceived as slow | `find_references` is user-triggered, 5 ms is imperceptible. If future telemetry shows a hot path, per-name LRU aggregator follow-up (§12). |
| Future contributor adds a workspace-wide query that re-introduces `file_text` fan-out | Architecture doc + grep guard in CI (`grep -r 'source_root_name_usage_query' crates/` must return 0 hits after Landing 3). |
| `find_references` returns false-empty result during the ~2.5 s prime window | `lookup` returns `Pending` immediately (non-blocking — blocking inside a Salsa snapshot would deadlock prime; see `Files` doc-comment on ABBA). The find_references handler (§6.3) drops the snapshot, calls `wait_until_primed(30s)`, retakes snapshot, retries → `Ready`. User-visible: single ≤ 3 s busy cursor on first cold-start call. Pinned by `lookup_pending_drop_snapshot_wait_retake` (§10). |

## 12. Out of scope follow-ups

- **Per-name LRU aggregator** for hot `find_references` queries — only if
  profiling shows >100 lookups/second of the same name.
- **CommandModule / HTTPService / WebService** identity recovery (5.6%
  uncovered on ERP — bucketed as `OrphanFile` in v6).
- **CommonForms / CommonCommands**: like Commands above; UUIDs already in
  XML, just need accessors in `Configuration`.
- **Workspace symbols** (`textDocument/workspaceSymbol`) on top of the
  same index.
- **Lazy-text** itself — separate plan, runs after Landing 3 bake.
- **`MdObjectId` promotion** in `bsl-metadata` — v6 keeps `ModuleLikeId` as
  a local enum in `hir-def`. Promoting to a first-class public type in
  `bsl-metadata` happens if a second consumer materializes.

## 13. References to existing code touched

| File | Touch |
|---|---|
| `crates/hir-def/src/name_index_v6/**` (new) | Salsa types, tracked queries, refresh/remove/rebind/lookup |
| `crates/ide-db/src/name_index_classifier.rs` (new) | `derive_module_like_id` from spike, predicate widening |
| `crates/bsl-analyzer/src/workspace.rs:153` | two-phase `process_changes` |
| `crates/bsl-analyzer/src/global_state.rs` | `set_bsl_file_text` helper |
| `crates/bsl-analyzer/src/server.rs:200` | `prime_workspace_name_index` dispatch |
| `crates/ide/src/references.rs:99` | route through `WorkspaceNameIndex::lookup` |
| `crates/hir-def/src/name_usage_index.rs` | delete in Landing 3 |
| `crates/hir-def/src/lib.rs:72-75` | remove re-exports in Landing 3 |
| `crates/ide-db/src/database.rs:348-353` | remove `DefDatabase` query impl in Landing 3 |
| `crates/bsl-analyzer/src/spike_name_index*` | delete after Landing 1 lands |

## 14. Spike artefacts that get reused

- **`MetadataObject::uuid()` accessor** (commit 8abbd545, on develop):
  load-bearing, must be on the implementation branch.
- **`docs/roadmap/name-index-spike.md`**: scope contract that constrained
  the spike; keep as historical record.
- **`docs/roadmap/name-index.md` (v5)**: mark with a header `**Superseded
  by name-index-v6.md as of 2026-MM-DD.**` in Landing 1; do NOT delete —
  v5's snapshot-coherence analysis is the rationale for v6's existence.
- **Spike branch code**: deleted from the production tree after Landing 1;
  the branch itself stays in git history as the empirical record.

## 15. Decisions deferred to implementation

These are not architectural blockers:

- Exact `Name` interner: reuse `hir_def::Name` (current) or move to a
  dedicated `string-interner`-backed pool. Spike used `Arc<str>` — that's
  a third option. Benchmark on real workload.
- Exact `FileLexDigest` storage: `FxHashSet<Name>` vs `Vec<Name>` (smaller
  but lookup O(N) inside the union). Bench first.
- Whether `prime_workspace_name_index` reports per-MdoType progress or
  flat percentage to `$/progress`. UX call.

### 15.4. `workspace_config()` locking — verified safe

**Verified during plan v6 review**: there is no `GlobalState::workspace_config()`
in the current tree. The real API is `RootDatabase::get_configuration(file_id)
-> Option<Arc<bsl_metadata::Configuration>>` (`crates/ide-db/src/database.rs:427`).
It is a `&self` method that internally calls the Salsa query
`metadata::load_configuration(self, path_input)` — no external snapshot,
no held lock. The returned `Arc<Configuration>` outlives the immediate
call cleanly.

The §6.1 and §6.2 pseudocode use `db.get_configuration(file_id)` directly.
Phase 2 reads `&self`; Phase 3 reacquires `&mut self`. No deadlock.
For multi-root workspaces, `db.get_all_configurations(file_id)` is the
multi-config variant — wire-up is a one-line change at the classifier
call site, not an architectural change.

---

**Awaiting Codex pass.**
