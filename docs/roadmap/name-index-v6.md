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
                  — derive_module_like_id(config, file_path) — moved from
                    spike, uses bsl-metadata accessors

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
#[salsa::input(debug)]
pub struct FileLexDigest<'db> {
    pub file_id: FileId,
    #[return_ref]
    pub names: FxHashSet<Name>,
}

/// Per-ModuleLikeId membership list. INPUT — populated once per module.
/// On membership change (file added/removed within a Form, MDO renamed,
/// …) replace the whole input; Salsa invalidates dependents.
#[salsa::input(debug)]
pub struct ModuleLikeMembership<'db> {
    pub id: ModuleLikeId,
    #[return_ref]
    pub files: Vec<FileLexDigest<'db>>,
}

/// Workspace registry — list of all known module-likes. INPUT — updated on
/// MDO add/remove events from `process_changes`.
#[salsa::input(debug)]
pub struct WorkspaceMembers<'db> {
    #[return_ref]
    pub members: Vec<ModuleLikeMembership<'db>>,
}

/// Union of digests for one module-like. TRACKED — re-runs only when the
/// membership's digest list changes or one of its referenced
/// `FileLexDigest.names` changes.
#[salsa::tracked]
pub fn module_like_name_index<'db>(
    db: &'db dyn DefDatabase,
    membership: ModuleLikeMembership<'db>,
) -> Arc<ModuleNameIndex>;

/// Workspace-wide lookup. Plain function, NOT tracked: deliberate, so that
/// editing one file does not invalidate the global aggregator (which would
/// re-merge all ~10–20 k indices on every keystroke). Cost is dominated by
/// hash lookups across cached per-module memos — 1–6 ms on ERP (spike Q3).
pub fn lookup_workspace<'db>(
    db: &'db dyn DefDatabase,
    name: &Name,
) -> Vec<FileId>;
```

`Name` is the existing `hir_def::Name` (already used in
`name_usage_index.rs`). `FxHashSet`/`Arc<…>` traits-side: `Update` impl is
derived by `#[salsa::input]` for the digest set as a whole; mutating
individual entries requires the consumer to call `set_names(db).to(new)`
with a fresh `FxHashSet`.

### 4.4. Input handle storage — `NameIndexHandles`

`WorkspaceNameIndex::refresh(db, file_id, names)` mutates the **existing**
`FileLexDigest` cell for `file_id`. Same for `rebind` on
`ModuleLikeMembership`. To find those handles after populate, v6 needs a
side map from key → `Copy` Salsa input handle. This is the **same pattern
base-db already uses for `FileTextInput`** (`crates/base-db/src/lib.rs:176`,
the `Files` struct), not a new architectural exception.

```rust
// crates/hir-def/src/name_index_v6/handles.rs
#[derive(Debug, Default, Clone)]
pub struct NameIndexHandles {
    digests: Arc<DashMap<FileId, FileLexDigest<'static>, FxHasher>>,
    memberships: Arc<DashMap<ModuleLikeId, ModuleLikeMembership<'static>, FxHasher>>,
    workspace: Arc<RwLock<Option<WorkspaceMembers<'static>>>>,
}
```

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

`WorkspaceNameIndex::refresh` flow:

```rust
fn refresh(db: &mut dyn DefDatabase, file_id: FileId, names: FxHashSet<Name>) {
    use salsa::Setter;
    // Short-lock: get-copy-drop-set.
    let existing = db.name_index().digests.get(&file_id).map(|e| *e.value());
    match existing {
        Some(digest) => digest.set_names(db).to(names),
        None => {
            let digest = FileLexDigest::new(db, file_id, names);
            let prev = db.name_index().digests.insert(file_id, digest);
            debug_assert!(prev.is_none(), "single-mutator violated");
        }
    }
}
```

`remove` deletes from `digests`; the removed `FileLexDigest` becomes
unreachable from any `ModuleLikeMembership` once `rebind` updates the
membership lists. (Salsa GC reclaims it on the next revision bump.)

`rebind` follows the same get-copy-drop-set shape on `memberships`.
`lookup` reads `workspace` under a short read guard, copies the
`WorkspaceMembers` handle, drops the guard, then iterates members via the
Salsa query — no Salsa write call inside the guard, so no ABBA risk.

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
    pub fn refresh(db: &mut dyn DefDatabase, file_id: FileId, names: FxHashSet<Name>);

    /// Drop a file from all module-likes that referenced it. Called on
    /// `Change::Delete`.
    pub fn remove(db: &mut dyn DefDatabase, file_id: FileId);

    /// Re-classify which `ModuleLikeId` owns a file. Called on MDO add /
    /// rename / delete, when the Configuration mutation in
    /// `process_changes` re-derives owner identity.
    pub fn rebind(db: &mut dyn DefDatabase, file_id: FileId, new: ModuleLikeId);

    /// Workspace-wide reverse lookup. Already shown above.
    pub fn lookup(db: &dyn DefDatabase, name: &Name) -> Vec<FileId>;
}
```

The state machine (`Empty / Building / Ready / Failed`) and `LookupResult::Pending`
that v5 invented are **gone**: Salsa-tracked queries either return a result
(cached or freshly computed) or block until the revision is consistent.
`Cancelled::catch` handles the cancel-on-input-change case at no extra
cost.

The `name-index-strict` CI feature and `set_bsl_file_text` discipline carry
over from v5 §6.1 — they remain useful as guard rails against a future
contributor forgetting to call `WorkspaceNameIndex::refresh` after writing
to `FileTextInput`.

## 6. Integration

### 6.1. `process_changes` (workspace.rs)

```rust
pub fn process_changes(&mut self, suppress_metadata_bump: bool) -> (bool, bool) {
    // Phase 1: Salsa mutations (file_text + name_index)
    let mut digest_batch: Vec<(FileId, FxHashSet<Name>)> = Vec::new();
    let mut remove_batch: Vec<FileId> = Vec::new();
    let mut rebind_batch: Vec<(FileId, ModuleLikeId)> = Vec::new();
    {
        let db = self.analysis_host.raw_database_mut();
        for change in changed_files {
            match change {
                Change::Create(text, _) | Change::Modify(text, _) => {
                    if is_bsl_source(&file_set, file_id) {
                        db.set_file_text(file_id, &text);
                        let digest = lex_to_digest(&text);
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

    // After Phase 1 the mutable DB borrow is dropped. Phase 2 reacquires
    // a READ snapshot through `workspace_config()` and classifies paths
    // against the post-mutation revision. Phase 3 reacquires the MUTABLE
    // borrow ONLY for the rebind writes. The two reacquires must not
    // overlap with `workspace_config()`'s internal snapshot if it holds
    // one (open question, §15.4).
    {
        let config = self.workspace_config();
        for (file_id, path) in newly_classified_files {
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
    let lexed: Vec<(FileId, PathBuf, FxHashSet<Name>)> =
        all_bsl_files(global_state)
            .into_par_iter()
            .map(|(fid, path, text)| (fid, path, lex_to_digest(&text)))
            .collect();

    // Step 2: classify against the Configuration BEFORE taking the mut
    // DB borrow. Classification is read-only on workspace_config.
    let config = global_state.workspace_config();
    let mut groups: FxHashMap<ModuleLikeId, Vec<FileId>> = Default::default();
    for (fid, path, _) in &lexed {
        let mlid = derive_module_like_id(&config, path)
            .unwrap_or(ModuleLikeId::OrphanFile(*fid));
        groups.entry(mlid).or_default().push(*fid);
    }

    // Step 3: serial Salsa input setup (Salsa db is not Sync for writes).
    let db = global_state.analysis_host.raw_database_mut();
    for (fid, _path, names) in &lexed {
        WorkspaceNameIndex::refresh(db, *fid, names.clone());
    }
    for (mlid, files) in groups {
        WorkspaceNameIndex::register_module(db, mlid, files);
    }
}
```

Runs after `vfs_done` on a dedicated rayon scope. Spike measured ~2.4 s
parallel lex + ~10 ms Salsa populate for ERP. `parallel_prime_caches`-style
progress reporting via `$/progress` (existing LSP plumbing) keeps the
client informed; no new LSP message kind. Cancellation via the existing
`Cancelled::catch` pattern from `rust-analyzer/crates/ide-db/src/prime_caches.rs`.

### 6.3. `find_references` (ide/src/references.rs)

```rust
// Drop in `WorkspaceNameIndex::lookup`. Compare-and-swap with the old
// `source_root_name_usage_query`-driven `workspace_candidate_files`.
fn workspace_candidate_files(db: &dyn DefDatabase, name: &Name) -> Vec<FileId> {
    WorkspaceNameIndex::lookup(db, name)
}
```

No state-wait timeout, no `Pending` enum. Salsa blocks the lookup until
the revision is consistent; cancellation propagates via `Cancelled::catch`.
If the index hasn't been primed yet (very early in startup, before
`vfs_done`), `WorkspaceMembers` is empty and `lookup` returns an empty
`Vec` — caller sees `ReferencesResult::Empty`. This matches v5's
fallback-after-timeout behaviour but is achieved through the natural
Salsa input lifecycle rather than a custom state machine.

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

Implication for §6.1/§6.2 pseudocode: substitute `workspace_config()`
with `db.get_configuration(any_file_in_root)` (or
`db.get_all_configurations(any_file_id)` for multi-root workspaces).
Phase 2 reads `&self`; Phase 3 reacquires `&mut self`. No deadlock.

If the API surface needs to evolve (e.g. for source-root-scoped indexing),
the substitution is local to the call sites in §6.1/§6.2 and the
classifier — not an architectural change.

---

**Awaiting Codex pass.**
