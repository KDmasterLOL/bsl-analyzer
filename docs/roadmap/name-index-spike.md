# Name-index spike — scope contract

**Status**: completed. Findings folded into `name-index-v6.md` (current plan).
**Branch**: `spike/name-index-salsa-partitioned`.
**Effort**: 3-4 days, throwaway code.

## Why this spike exists

`docs/roadmap/name-index.md` (v5) proposes a **non-Salsa** `Arc<WorkspaceNameIndex>`
adjunct on `RootDatabaseImpl` to replace `source_root_name_usage_query`. Two
adversarial reviews surfaced concerns that v5 left unanswered:

1. **Snapshot coherence**: v5 shares the same `Arc<WorkspaceNameIndex>` across
   cloned Salsa snapshots. A `find_references` snapshot can observe one Salsa
   revision and a *different*, concurrently refreshed index revision. Generation
   counters detect index mutation but do not bind a lookup to the request's
   Salsa revision. (Codex review, agent `af45b24576aa34cfb`.)

2. **Reinvention**: rust-analyzer's `SymbolIndex` / `ImportMap` solve the same
   problem with **partitioned `#[salsa::tracked]` queries** + rayon priming +
   `Cancelled::catch` — no generation counters, no replay log, no atomic swap,
   no `set_bsl_file_text` discipline. v5 §11 / §14 record no rationale for
   rejecting that approach because no prior pass evaluated it.

This spike measures whether a **Salsa-per-MdObject** alternative is viable
**before** we write a v6 plan against it.

## Three load-bearing questions

The spike exists to answer three questions with numbers. Each has a measurable
exit criterion. Anything outside these three is out of scope.

### Q1 — ModuleLikeId coverage on a real workspace

**Question**: what fraction of `.bsl` files in a real configuration map to a
stable `ModuleLikeId` derived from MDO UUIDs?

**How to measure**: build `pub fn module_like_id(file_id: FileId) -> Option<ModuleLikeId>`
that wraps the existing `ide-db/src/metadata.rs::build_module_metadata` (which
already recovers owner MDO for ManagerModule/ObjectModule/RecordSetModule
paths) and pairs it with the `MetadataObject::uuid()` accessor landed in
8abbd545. Run on `~/src/pt/erp/src/cf` and report:

- total `.bsl` files
- files mapping to `ModuleLikeId::CommonModule(uuid)`
- files mapping to `ModuleLikeId::Root(uuid)` (Catalog/Document/Report/…)
- files mapping to `ModuleLikeId::Form(parent_uuid|own_uuid, form_name)`
- files mapping to `ModuleLikeId::OrphanFile(file_id)` (no owner MDO, e.g.
  scripts under `Ext/`)
- files with **no** identity at all (failure)

**Exit criterion**:
- **≥ 90% coverage** by stable identity → go.
- **70-90%** → go, but document the gap in v6 §scope.
- **< 70%** → stop, file a separate issue against `bsl-metadata` /
  `project-model` for missing identity recovery, return to v5.

### Q2 — Does the Salsa-per-MdObject prototype unblock lazy text?

**Question**: when the workspace name-index is built via `#[salsa::input] FileLexDigest`
+ per-MdObject `#[salsa::tracked]` queries, does Salsa demand `db.file_text(fid)`
for files outside the user's current open set?

**How to measure**: instrument `db.file_text` with `tracing::trace_span!` for the
duration of the spike. Run three scenarios:

| Scenario | Expected `file_text` reads |
|---|---|
| Full populate (`spike-name-index --populate-all`) | ≥ 25 000 (every file lexed once) |
| Single-file edit, then `module_like_name_index(owning_mdo)` recomputed | exactly 1 |
| `lookup_workspace("Foo")` on warm cache, no files in mem_docs | **0** |

**Exit criterion**:
- All three scenarios match expected counts → Salsa-per-MdObject genuinely
  decouples name-index from `file_text` after populate. **Go.**
- Third scenario > 0 → Salsa-per-MdObject still demands file_text indirectly
  (via `db.parse(fid) → db.file_text(fid)` somewhere in the tracked-query
  graph). **Stop**: rewrite the prototype to bypass the Salsa parse layer
  entirely, or fall back to v5 with a snapshot-coherence fix.

### Q3 — Lookup latency without an explicit aggregator

**Question**: what is the cost of `lookup_workspace(name)` when it iterates
all `module_like_name_index(mdo)` results vs. when a v5-style monolithic
HashMap answers in O(1)?

**How to measure**: run `lookup_workspace("ОбщегоНазначения.СообщитьПользователю")`
(a name with ~thousands of hits on ERP) under three configurations:

| Configuration | Reported |
|---|---|
| `source_root_name_usage_query` (current v5 fallback baseline) | latency, file_text reads |
| Salsa-per-MdObject, cold cache | latency, # tracked queries invoked |
| Salsa-per-MdObject, warm cache | latency, # hash-map lookups |

**Exit criterion**:
- Warm latency ≤ 5 ms → no aggregator needed in v6.
- Warm latency 5-50 ms → add a Salsa-tracked aggregator atop per-MdObject
  indices; document invalidation cost in v6.
- Warm latency > 50 ms → re-think gran­ularity (per-source-root? per-MDO-type?)
  or accept this overhead is the cost of decoupling.

## What we build

```
crates/hir-def/src/name_usage_index_v6.rs       (new, ~250 lines, throwaway)
crates/bsl-analyzer/src/bin/main.rs             (+ Commands::SpikeNameIndex CLI)
```

That is the entire footprint. No other files touched.

### Sketch

```rust
// crates/hir-def/src/name_usage_index_v6.rs

#[derive(Copy, Clone, Eq, Hash, PartialEq)]
pub enum ModuleLikeId {
    CommonModule(Uuid),
    Root(Uuid),                                 // Catalog/Document/Report/…
    Form { owner: Option<Uuid>, name: Name },   // form may own UUID or inherit
    OrphanFile(FileId),                         // Ext/, no MDO identity
}

#[salsa::input]
pub struct FileLexDigest<'db> {
    #[return_ref]
    pub names: FxHashSet<Name>,
}

#[salsa::tracked]
fn module_like_files(db: &dyn DefDatabase, mid: ModuleLikeId) -> Arc<[FileId]>;

#[salsa::tracked]
fn module_like_name_index(db: &dyn DefDatabase, mid: ModuleLikeId)
    -> Arc<ModuleNameIndex>;

#[salsa::tracked]
fn all_module_likes(db: &dyn DefDatabase) -> Arc<[ModuleLikeId]>;

pub fn lookup_workspace(db: &dyn DefDatabase, name: &Name) -> Arc<[FileId]> {
    all_module_likes(db).iter()
        .flat_map(|mid| module_like_name_index(db, *mid).files_with(name).iter().copied())
        .collect()
}
```

### CLI surface

```
bsl-analyzer-app spike-name-index \
    --source-dir <path> \
    [--populate-all]            # default: populate FileLexDigest for all files
    [--single-edit <file>]      # then edit one file and re-measure
    [--lookup <name>]           # finally run lookup_workspace(name)
    [--instrument-file-text]    # gate on the tracing::trace_span counter
```

Output: a single JSON blob with the metrics for Q1/Q2/Q3.

## What we do NOT build

- ❌ `process_changes` / `set_file_text` integration — the spike sets
  `FileLexDigest` directly from the CLI.
- ❌ LSP `find_references` rewiring — the spike does not touch
  `ide::references::find_references`.
- ❌ `parallel_prime_caches`-analog (rayon + crossbeam) — populate synchronously
  in the CLI.
- ❌ CommandModule coverage — out of scope (no `bsl-metadata` parser today;
  separate slice after v6).
- ❌ `ChartOfCalculationTypes` / `ExternalDataSource` UUID — these go through
  `load_simple_metadata_objects_parallel` without XML parsing today; treat as
  `OrphanFile` in Q1 and report the size of that bucket.
- ❌ Removal of the existing `name_usage_index.rs` — it stays alive in parallel
  for Q3 baseline.

## Decision tree after spike

After the three numbers are in, the spike outcome dictates the next action:

| Q1 | Q2 | Q3 | Action |
|---|---|---|---|
| ≥ 90% | all green | warm ≤ 5 ms | Write v6 plan: Salsa-per-MdObject + parallel_prime_caches + process_changes integration. Mark v5 superseded. ~6-8 days. |
| ≥ 90% | all green | warm 5-50 ms | Write v6 plan with Salsa-tracked aggregator. ~7-9 days. |
| ≥ 90% | Q2.3 > 0 | — | **Re-investigate**. Either rework prototype to bypass `db.parse`, or fall back to v5 with snapshot-coherence patch. |
| 70-90% | all green | — | Write v6 plan with explicit coverage caveat. Schedule follow-up for the uncovered slice (Commands, simple-load MDOs). |
| < 70% | — | — | Stop spike. File `bsl-metadata` / `project-model` issue for identity recovery. Park v6 until that lands. |

## Status of related artefacts

- `docs/roadmap/name-index.md` (v5) — **stays as-is** until spike concludes.
  After spike, either marked `superseded by name-index-v6.md` or revived with a
  snapshot-coherence patch, depending on outcome.
- `crates/bsl-metadata` UUID accessor (commit 8abbd545) — load-bearing for Q1,
  must be on the branch where the spike is built.
- `--bench-index` (commit 185a865d) — independent measurement of the existing
  HashMap-shape build cost. Used as a sanity baseline for Q3 warm latency.

## Out of scope follow-ups

These are deliberately *not* part of the spike:

- Disk persistence of the index across sessions.
- Workspace symbols provider built on the index.
- Lazy-text initiative itself — runs after v6 ships and gets its own
  regression sweep.
- SDBL identifier indexing.
- `MdObjectId` first-class type in `bsl-metadata` — the spike uses an enum
  local to `name_usage_index_v6.rs`. Promoting it to a public `bsl-metadata`
  type is a separate decision triggered by v6 implementation.
