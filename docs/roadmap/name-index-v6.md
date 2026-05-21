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
| Q3 warm lookup, typical (≤ 5 k hits) | 1–3.5 ms (HashMap; FST projected same) | ≤ 5 ms | GO |

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
    server.rs     — boot: prime_workspace_name_index inside
                    LoadingProgress::Finished, BEFORE vfs_done = true
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

/// Top-level name-index state. INPUT — exactly one instance per
/// `RootDatabaseImpl`, eagerly created at AnalysisHost initialisation
/// (§6.2 step 0). `workspace = None` until prime sets it to `Some(...)`;
/// because this is a Salsa input, both states are revision-coherent —
/// a snapshot taken before prime sees `None` even after the main
/// thread has flipped the atomic externally. This is the snapshot
/// coherence guarantee v5 lacked.
#[salsa::input(debug)]
pub struct IndexState {
    pub workspace: Option<WorkspaceMembers>,
}

/// Union of digests for one module-like. TRACKED — re-runs only when the
/// membership's digest list changes or one of its referenced
/// `FileLexDigest.names` changes.
#[salsa::tracked]
pub fn module_like_name_index(
    db: &dyn DefDatabase,
    membership: ModuleLikeMembership,
) -> Arc<ModuleNameIndex>;

/// Per-module-like name → files index. FST-backed, mirrors the RA
/// `ImportMap` pattern at `rust-analyzer/crates/hir-def/src/import_map.rs:107`.
///
/// `fst::Map` keys are lowercase-normalised name bytes. Values pack
/// `(start_offset << 32) | end_offset` into the side `files: Vec<FileId>`
/// slab — multiple files per name share a contiguous range.
pub struct ModuleNameIndex {
    pub fst: fst::Map<Vec<u8>>,
    pub files: Vec<FileId>,
}

/// Workspace-wide lookup. Plain function, NOT tracked: deliberate, so that
/// editing one file does not invalidate the global aggregator (which would
/// re-merge all ~10–20 k indices on every keystroke). Cost dominated by
/// per-mlid FST exact-match scans across cached memos — projected 1–6 ms
/// on ERP (spike Q3 measured this on HashMap; FST exact-match has the
/// same big-O and similar constant).
pub fn lookup_workspace(db: &dyn DefDatabase, name: &Name) -> Vec<FileId>;
```

`Name` is the existing `hir_def::Name` (already used in
`name_usage_index.rs`). `Arc<FxHashSet<Name>>` is the digest-storage shape
verified in the spike (`crates/bsl-analyzer/src/spike_name_index_salsa.rs`):
cheap `Arc::clone()` on read, full replace on `set_names(db).to(new_arc)`
when the file is re-lexed. `Arc<Vec<…>>` for membership lists is the same
pattern.

### 4.3a. Per-mlid FST build (inside `module_like_name_index`)

```rust
#[salsa::tracked]
pub fn module_like_name_index(
    db: &dyn DefDatabase,
    membership: ModuleLikeMembership,
) -> Arc<ModuleNameIndex> {
    // Gather (case-folded name → file_id) pairs across all digests in
    // this mlid. INVARIANT: every `Name` already passed through
    // `case_fold` (§4.4) when inserted into `FileLexDigest` by
    // `lex_to_digest` (§6.1). Lookup applies the same `case_fold` to
    // the query, so FST keys at insert time and query time agree on a
    // single bilingual case-folded byte representation.
    let mut pairs: Vec<(Arc<str>, FileId)> = Vec::new();
    for digest in membership.files(db).iter() {
        let fid = digest.file_id(db);
        for name in digest.names(db).iter() {
            pairs.push((name.clone(), fid));
        }
    }
    pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    // Walk sorted pairs, packing (start_offset << 32) | end_offset values
    // referencing a side files slab. Same shape as RA's import_map.rs:107.
    let mut builder = fst::MapBuilder::memory();
    let mut files: Vec<FileId> = Vec::with_capacity(pairs.len());
    let mut iter = pairs.iter().peekable();
    while let Some((name, _)) = iter.peek() {
        let name = name.clone();
        let start = files.len();
        while let Some((n, fid)) = iter.peek() {
            if **n != *name { break; }
            files.push(*fid);
            iter.next();
        }
        let end = files.len();
        let value = ((start as u64) << 32) | (end as u64);
        builder.insert(name.as_bytes(), value).expect("sorted-input FST insert");
    }
    let fst = builder.into_map();
    Arc::new(ModuleNameIndex { fst, files })
}
```

Build cost on a typical per-mlid (~30–50 names): microseconds.
On extreme outliers (large CommonModule with hundreds of utility names):
still sub-millisecond. Significantly smaller memo size than HashMap
(prefix-sharing — RA-measured 3–5× compactness for similar workloads).

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
use std::sync::OnceLock;
use rustc_hash::FxHasher;

#[derive(Debug, Default, Clone)]
pub struct NameIndexHandles {
    digests: Arc<DashMap<FileId, FileLexDigest, BuildHasherDefault<FxHasher>>>,
    memberships: Arc<DashMap<ModuleLikeId, ModuleLikeMembership, BuildHasherDefault<FxHasher>>>,
    /// Salsa input handle for the workspace-level state. Set ONCE at
    /// AnalysisHost initialisation (§6.2 step 0), before any worker can
    /// take a snapshot. After that, every reader (`lookup`, prime,
    /// process_changes) goes through `state.get().expect("init").workspace(db)`,
    /// which is REVISION-BOUND by Salsa — a worker holding a snapshot
    /// from before prime sees `workspace == None` even if a separate
    /// thread has already finished prime. This is the snapshot
    /// coherence v5 lacked: the "primed" bit is no longer a free
    /// AtomicBool that drifts ahead of any specific snapshot.
    state: Arc<OnceLock<IndexState>>,
}

impl NameIndexHandles {
    /// Create the singleton `IndexState` input cell. Called once from
    /// AnalysisHost startup (§6.2 step 0). Subsequent calls panic —
    /// the cell must remain a singleton.
    pub fn init(&self, db: &mut dyn DefDatabase) {
        let cell = IndexState::new(db, None);
        self.state.set(cell).expect("NameIndexHandles::init called twice");
    }
}
```

The handles struct itself no longer carries primed/workspace state
inline. Everything that needs to be revision-bound now lives inside
the Salsa input cell `IndexState`. Cross-thread visibility is handled
by Salsa's revision machinery, not by ad-hoc atomic ordering.

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
    let handles = db.name_index().clone();

    // Read the workspace pointer through the snapshot-coherent Salsa
    // input. A worker holding a snapshot from before prime sees
    // `workspace == None` even if a separate main-thread flip has
    // already happened in real time. That is the v6 fix for v5's
    // snapshot-incoherent `Arc<WorkspaceNameIndex>`.
    let state = handles.state.get().expect("NameIndexHandles::init must run before lookup");
    let workspace_handle = match state.workspace(db) {
        Some(w) => w,
        None => {
            // Snapshot is pre-prime. Caller (find_references handler)
            // should also have gated on `ctx.vfs_done` at entry (§6.3);
            // this Pending here is the inner safety net.
            return LookupResult::Pending;
        }
    };

    // CRITICAL: use full Unicode `to_lowercase()`, NOT `to_ascii_lowercase()`.
    // BSL identifiers are bilingual (RU/EN) and case-insensitive — ASCII
    // case-fold leaves Cyrillic untouched, so a lookup for "ОбщегоНазначения"
    // would miss the FST key "общегоназначения" stored at build time.
    // The build side (§4.3a, populated by `lex_to_digest` in §6.1) MUST
    // use the same casing pipeline. Shared helper `case_fold(name)` in
    // `crates/hir-def/src/name_index_v6/` enforces this; both sides go
    // through it.
    let lower = case_fold(name.as_str());
    let mut out: Vec<FileId> = Vec::new();
    for &m in workspace_handle.members(db).iter() {
        let idx = module_like_name_index(db, m);
        // Exact-name FST lookup: O(name_len). For fuzzy/prefix (future
        // `workspaceSymbol` LSP handler), swap for `fst::automaton::Subsequence`
        // or `Str::new(...).starts_with()` on the same `idx.fst`.
        if let Some(value) = idx.fst.get(lower.as_bytes()) {
            let start = (value >> 32) as usize;
            let end = (value & 0xFFFF_FFFF) as usize;
            out.extend_from_slice(&idx.files[start..end]);
        }
    }
    LookupResult::Ready(out)
}

/// Case-fold a BSL identifier for storage / lookup. Bilingual: must lowercase
/// both ASCII and Cyrillic alphabets. Implementation: `s.to_lowercase()` (the
/// full-Unicode method, NOT `to_ascii_lowercase`). Mirrors the casing side of
/// `hir_def::Name::eq_ignore_case`. Both the FST builder (§4.3a) and `lookup`
/// (§4.4) MUST route name strings through this helper before touching FST
/// bytes, or Cyrillic identifiers silently miss the index.
fn case_fold(name: &str) -> String {
    name.to_lowercase()
}
```

`prime_workspace_name_index` (§6.2) signals completion with one Salsa
input write:

```rust
let state = handles.state.get().expect("init");
state.set_workspace(db).to(Some(workspace_handle));
```

`lookup` reads `state.workspace(db)`. Salsa's revision machinery
guarantees that a worker holding a snapshot from before this write
observes `None`; a snapshot from after observes `Some(workspace_handle)`.
No external atomic ordering — Salsa is the source of truth.

That single revisioned input write — paired with the synchronous-prime-
on-main-thread ordering (§6.2) — is the entire state machine. No
generations, no replay log, no atomic swap, no condvar.

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

    /// Snapshot-coherent check for "prime has populated the workspace
    /// state". Implemented as `state.workspace(db).is_some()` — reads
    /// the Salsa input at the current snapshot's revision, so two
    /// concurrent threads observing different revisions get
    /// per-revision-coherent answers. Used by the live-fire comparator
    /// (§9 Landing 2) to skip comparison during the cold-start window.
    pub fn is_primed(db: &dyn DefDatabase) -> bool;
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
keeps **one Salsa input field**: `IndexState::workspace: Option<WorkspaceMembers>`
(§4.4). It flips `None → Some(workspace_handle)` exactly once, at the
end of the boot
`prime_workspace_name_index`, and never flips back. Subsequent reloads
update memberships incrementally through `process_changes`; the bit
stays true. No replay log, no atomic swap, no generation, no Failed
state (a failed prime panics — same posture as Salsa-tracked queries
themselves).

`lookup` is non-blocking: it consults `primed` (Acquire) and returns
`Ready` or `Pending` immediately. The "no false-empty references"
guarantee is upheld at a different layer — §6.2 schedules prime
synchronously on the main thread BEFORE `vfs_done = true`, so no
worker dispatch can run against an unprimed index in healthy flow.
`Pending` is therefore reserved as a failure-mode marker (prime panic
detection); it never appears as a steady-state UX state.

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
(v5 §8 carries over verbatim). Every name string MUST pass through the
shared `case_fold` helper (§4.4) before being inserted into the
`FxHashSet<Name>` — the FST keys at lookup time are also `case_fold`ed,
and any divergence (notably `to_ascii_lowercase` vs full-Unicode
`to_lowercase`) silently drops Cyrillic identifiers from the index.

### 6.2. Boot — synchronous prime inside `LoadingProgress::Finished`

**Critical scheduling decision**: prime runs **synchronously on the main
thread**, inside the existing `LoadingProgress::Finished` handler in
`crates/bsl-analyzer/src/server.rs:200`, BEFORE `state.vfs_done = true`.
The main `select!` loop is single-threaded and processes one event at
a time — by deferring the `vfs_done = true` flip until prime completes,
no LSP request can be dispatched against an unprimed index. The whole
"Pending recovery" dance is therefore not on the user-facing hot path.

Cost: ~2.5–3 s of "server initializing" inside `vfs_done` handling
(spike Q2/Q3 numbers). Acceptable per LSP norms; rust-analyzer does
similar synchronous prime work in its boot path.

Concurrency note: prime is the ONLY consumer of `raw_database_mut()`
during boot, and the main thread is the only owner of `&mut GlobalState`.
No worker can hold a snapshot at this point because:
- task_pool workers are only spawned by handler dispatchers, and those
  ran for `initialize` only (no Salsa queries possible there yet).
- VFS message handling (`handle_vfs_msg`) also runs on main thread.

**Step 0 — `NameIndexHandles::init()` at AnalysisHost construction.**
The singleton `IndexState` input cell must exist by the time the first
snapshot is ever cloned (`raw_database().clone()` in
`handlers/dispatch.rs:126`). Wire-up:
```rust
// crates/bsl-analyzer/src/analysis_host.rs
impl AnalysisHost {
    pub fn new() -> Self {
        let mut host = Self { db: RootDatabaseImpl::default() };
        host.db.name_index().init(&mut host.db);
        host
    }
}
```
Replacing the existing `#[derive(Default)]` with explicit `new` is a
one-line change. After this, every read of `state.workspace(db)` is
revision-coherent for any snapshot — including snapshots taken before
prime runs, which observe the initial `None`.

```rust
// crates/bsl-analyzer/src/server.rs, inside the LoadingProgress::Finished
// branch. Ordering invariant:
//
//   process_changes(true)
//   init_source_root()
//   warm_metadata_cache()      ← Configuration XML parsed; required input
//   prime_workspace_name_index ← NEW: must come AFTER warm_metadata_cache
//                                 (needs Configuration) and BEFORE the
//                                 `vfs_done = true` flip below
//   state.vfs_done = true       ← only flip after prime completes
//
// This is THE invariant that makes `vfs_done ⇒ primed` hold and lets
// the per-request hot path stay simple (§6.3).
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
    let mut all_memberships: Vec<ModuleLikeMembership> = Vec::with_capacity(groups.len());
    for (mlid, files) in groups {
        let handle = WorkspaceNameIndex::register_module(db, mlid, files);
        all_memberships.push(handle);
    }

    // Step 4: publish workspace via the singleton IndexState input cell.
    // Salsa revisions this write; the same revision is observable by any
    // snapshot taken after the corresponding `set_workspace` returns.
    // This single line replaces v5's generation counter + atomic-swap
    // protocol and v6-draft's external AtomicBool — the visibility
    // contract is now revision-bound by Salsa.
    let handles = db.name_index().clone();
    let state = handles.state.get().expect("NameIndexHandles::init must run at AnalysisHost::new");
    let workspace_handle = WorkspaceMembers::new(db, Arc::new(all_memberships));
    state.set_workspace(db).to(Some(workspace_handle));
}
```

Runs synchronously on the main thread inside `LoadingProgress::Finished`,
BEFORE `state.vfs_done = true` (the invariant pinned in the comment
above the function body). Spike measured ~2.4 s parallel lex (rayon
inside the step-1 closure) + ~10 ms Salsa populate (serial main-thread
writes) for ERP. The main `select!` loop is blocked for the duration —
LSP requests queue in `lsp-server`'s `recv` channel and are dispatched
only after `vfs_done = true` flips. No `$/progress` notifications are
needed because no requests are being serviced during the window; the
client just sees `null` for any request that did get dispatched before
the handler entered (gated at handler entry per §6.3) and standard LSP
"server initializing" UX otherwise. Cancellation is irrelevant on this
path: prime is single-threaded main-thread work, not a long-running
background task.

### 6.3. `find_references` handler (bsl-analyzer/src/handlers/request.rs)

Requests CAN be dispatched before `LoadingProgress::Finished` fires —
the LSP client may send find_references between `initialized` and the
end of the workspace load. The dispatcher passes `vfs_done` into
`LatencyRequestContext` (`crates/bsl-analyzer/src/handlers/dispatch.rs:144`),
so handlers can gate on it. The existing precedent is
`handle_semantic_tokens_full` (`request.rs:389`), which returns empty
when `vfs_done == false` with the comment "Client will re-request when
ready."

`handle_references` follows the same pattern:

```rust
// crates/bsl-analyzer/src/handlers/request.rs
pub fn handle_references(
    ctx: LatencyRequestContext,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>> {
    // Pre-vfs_done window: name-index is not yet primed. Returning
    // `Some(vec![])` is wrong — the client would treat it as
    // "no references found" with no indication that the workspace is
    // still loading. Return `None` instead; LSP spec lets the server
    // signal "no result available" rather than "result is empty".
    if !ctx.vfs_done {
        tracing::debug!("vfs not ready, deferring textDocument/references");
        return Ok(None);
    }
    // Post-vfs_done: prime has run synchronously inside
    // LoadingProgress::Finished (§6.2), so primed == true. Proceed
    // through the standard handler.
    handle_references_post_prime(ctx, params)
}
```

Inside the post-prime path, the lookup helper is dead-simple — `Pending`
is unreachable in healthy flow because §6.2 guarantees
`vfs_done == true ⇒ primed == true`:

```rust
// crates/ide/src/references.rs
fn workspace_candidate_files(db: &dyn DefDatabase, name: &Name) -> Vec<FileId> {
    match WorkspaceNameIndex::lookup(db, name) {
        LookupResult::Ready(files) => files,
        LookupResult::Pending => {
            // Should be unreachable: caller already checked vfs_done.
            tracing::error!(
                "name-index lookup returned Pending after vfs_done — \
                 indicates prime did not run or panicked; see span \
                 `prime_workspace_name_index` in server log"
            );
            Vec::new()
        }
    }
}
```

No drop-snapshot-wait-retake dance, no `wait_until_primed`, no condvar.
The complexity sits in §6.2's boot ordering and the `vfs_done` gate at
handler entry, not in the per-request hot path.

`textDocument/references` returns `Location[] | null`. Returning `null`
(`Ok(None)`) during the pre-vfs_done window is the LSP-spec-blessed way
to say "no result available yet" without misleading the client into
treating it as "empty result". Standard LSP clients (vscode, nvim) treat
`null` as "server has nothing for now; user retries"; an empty array
would be displayed as "no references found".

User-visible behaviour during boot:

| Boot phase | Handler behaviour | UX |
|---|---|---|
| `t < LoadingProgress::Finished`, find_references dispatched | `ctx.vfs_done == false` ⇒ return `Ok(None)` | Client sees `null`; standard LSP "no result yet" UX |
| `LoadingProgress::Finished` handler in progress (~2.5 s on ERP) | Main thread runs `process_changes` + `warm_metadata_cache` + `prime_workspace_name_index` synchronously; main `select!` loop blocked → no new dispatches during this window | Server "initializing" period; new requests queue in lsp-server's `recv` channel |
| `vfs_done = true` flips, main loop returns to `select!` | Queued requests dispatched, `primed = true` guaranteed | Normal operation |
| Any post-boot lookup | `Ready` instantly | Typical 1–6 ms; worst 5.6 ms |

The handler-level `vfs_done` gate (mirrors `handle_semantic_tokens_full`,
`request.rs:389`) ensures the `null` response, not an empty array, for
the pre-prime window. The `Pending` path exists only as a failure-mode
marker so the server doesn't silently return wrong results if prime
panics — NOT a UX state in steady operation.

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
ERP) remains. The name-index adds roughly **~30–40 MB of digest/index
data** on top of that:
- `Σ |FileLexDigest|` (the `Arc<FxHashSet<Name>>` digest sets) ≈ 25 MB
  on ERP (spike measurement: 532 k unique names × ~50 bytes interned).
- `Σ |module_like_name_index|` (per-mlid FSTs + side `files` slabs) ≈
  10–15 MB, projected from RA's reported FST compactness ratio for
  similar workloads (3–5× smaller than HashMap form, which would be
  ~50 MB). The 640 / 500 MB spike RSS demonstrates the
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
- `prime_workspace_name_index` inside `LoadingProgress::Finished`,
  BEFORE the `vfs_done = true` flip — per §6.2 ordering invariant.
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
| `cyrillic_lookup_matches_indexed_names` | round-trip on a Cyrillic identifier (e.g. `ОбщегоНазначения`): build digest → FST → look up via `WorkspaceNameIndex::lookup(_, "общегоназначения")` AND `lookup(_, "ОБЩЕГОНАЗНАЧЕНИЯ")` AND `lookup(_, "ОбщегоНазначения")`. All three MUST return the same `Ready(files)`. Regression guard against the `to_ascii_lowercase` vs full-Unicode `to_lowercase` divergence that silently drops Cyrillic identifiers. | 1 |
| `lookup_under_concurrent_edit_storm` | Salsa cancellation propagates correctly | 2 |
| `lookup_hottest_name_under_budget` | warm `lookup_workspace("ОбщегоНазначения")` ≤ 10 ms on ERP perf fixture (Q3 budget = 2× spike worst-case) | 2 |
| `name_index_memory_delta_under_budget` | post-prime RSS minus pre-prime RSS ≤ 50 MB on ERP perf fixture (covers digest + per-mlid FST memo storage; FST prefix-sharing brings the per-mlid footprint to roughly 3× smaller than the HashMap form) | 2 |
| `lookup_returns_pending_when_unprimed` | unprimed handles → `lookup` returns `Pending` immediately, NEVER blocks (regression guard against the deadlock-prone blocking design that earlier review attempts proposed). | 1 |
| `name_index_primed_after_boot_completes` | full boot harness: drive `LoadingProgress::Finished` to completion, assert `is_primed(db) == true` and `WorkspaceNameIndex::lookup(_, _) ⇒ Ready(_)` for at least one fixture symbol. Pins the synchronous-prime-before-vfs_done ordering (§6.2). | 2 |
| `find_references_never_observes_pending_after_boot` | integration test on ERP fixture: after boot completes, run 100 `find_references` requests concurrently; every result is `Ready`, none log the `tracing::error!("name-index lookup returned Pending after vfs_done")` line. | 2 |
| `find_references_returns_null_before_vfs_done` | integration test: send `textDocument/references` during the pre-vfs_done window; assert response is `null` (`Ok(None)`), NOT empty `[]`. Mirrors the established `handle_semantic_tokens_full` precedent (`request.rs:389`). Prevents the client UX from interpreting cold-start as "no references found". | 2 |

Snapshot tests (`expect-test`) for at least three representative
configurations:
- Bare CommonModule with no Forms
- Catalog with ObjectModule + ManagerModule + 2 Forms
- Register with RecordSetModule + ManagerModule + Form

## 11. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Lex pass on cold start adds visible latency (spike: 2.4 s) | `prime_workspace_name_index` runs synchronously on the main thread inside `LoadingProgress::Finished`, BEFORE `vfs_done = true`. The lex pass uses a rayon parallel iterator inside step 1 of the prime function (read-only file I/O + lexing — no DB borrow). Salsa input writes are serial main-thread work. LSP request handling is paused during the window; the `vfs_done` gate at handler entry (§6.3) prevents pre-prime requests from observing an empty index. |
| Predicate divergence between `lex_to_digest` and `find_references_in_file` | Shared `is_name_token` helper in one module; parity test (§10). |
| Salsa memory growth from per-mlid memos on a 20 k-mlid workspace | FST-backed memos are even smaller than the HashMap form measured by spike (~3 KB avg HashMap → ~1 KB avg FST per mlid = ~20 MB total). No LRU needed; verified by `name_index_memory_delta_under_budget` (§10). |
| Worst-case warm lookup 5.6 ms on hottest name perceived as slow | `find_references` is user-triggered, 5 ms is imperceptible. If future telemetry shows a hot path, per-name LRU aggregator follow-up (§12). |
| Future contributor adds a workspace-wide query that re-introduces `file_text` fan-out | Architecture doc + grep guard in CI (`grep -r 'source_root_name_usage_query' crates/` must return 0 hits after Landing 3). |
| `find_references` returns false-empty result during the ~2.5 s prime window | Two layers: (a) handler at entry gates on `ctx.vfs_done`; if false, returns `Ok(None)` (LSP `null` — "no result available", NOT empty array), matching the existing `handle_semantic_tokens_full` precedent (`request.rs:389`); (b) §6.2 prime runs synchronously inside `LoadingProgress::Finished` BEFORE `vfs_done = true` flips, so `vfs_done ⇒ primed`. Net effect: LSP client sees `null` until the workspace is loaded (most clients show "still indexing" UX), then `Ready` results. Pinned by `find_references_returns_null_before_vfs_done` + `find_references_never_observes_pending_after_boot` (§10). |

## 12. Out of scope follow-ups

- **Per-name LRU aggregator** for hot `find_references` queries — only if
  profiling shows >100 lookups/second of the same name.
- **CommandModule / HTTPService / WebService** identity recovery (5.6%
  uncovered on ERP — bucketed as `OrphanFile` in v6).
- **CommonForms / CommonCommands**: like Commands above; UUIDs already in
  XML, just need accessors in `Configuration`.
- **Workspace symbols** (`textDocument/workspaceSymbol`) on top of the
  same FST per-mlid indexes. Implementation: an LSP handler that builds
  a `fst::map::OpBuilder` union of every `module_like_name_index(mlid).fst`
  and runs an automaton (`fst::automaton::Subsequence` for fuzzy,
  `Str::new(...).starts_with()` for prefix) against the union stream.
  Matches RA's `world_symbols` pattern at
  `rust-analyzer/crates/ide-db/src/symbol_index.rs:225-282`. The
  per-mlid storage shape v6 ships is exactly what this handler needs —
  no migration required.
- **Did-you-mean diagnostics** (`docs/roadmap/workspace-symbols.md`
  §«Did-you-mean»): on `UnresolvedMethodCall` / `UnresolvedField` emit
  top-N nearest names via the same FST fuzzy automaton. Free side-effect
  once `workspaceSymbol` machinery exists.
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
