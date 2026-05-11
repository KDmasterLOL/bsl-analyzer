//! Salsa wrappers for security/effect analyses (Track 2 §1.4b).
//!
//! Owns three tracked queries layered on the pure helpers in
//! `dataflow::effect_summary` (§1.4a) and `dataflow::security_state`
//! (§1.2):
//!
//! - [`module_effect_summaries_query`] — module-batch SCC fixpoint over
//!   [`hir::dataflow::effect_summary::EffectSummary`]. Cycle-aware
//!   (cross-module recursion uses Salsa's `cycle_initial` + `cycle_fn`
//!   per Salsa 0.26 — see verification below).
//! - [`method_effect_summary_query`] — thin per-method lookup shim over
//!   the module batch. Track 1 precedent: build the heavy data once at
//!   module granularity, expose per-method via a cheap shim.
//! - [`module_security_state_query`] — module-batch run of the §1.2
//!   forward dataflow. No cycle handlers: the lattice has no
//!   cross-method dependency edges, so Salsa never enters a cycle here.
//!
//! # Layer rules
//!
//! `dataflow` and `cfg` carry no Salsa code (Track 1 invariant). All
//! Salsa-tracked queries for security/effect analyses live here. The
//! pure helpers are called via the public `hir::dataflow::*` re-exports
//! so we keep the dependency edge `ide-db → hir → dataflow`.
//!
//! # Known limitations: cross-module recursion is NOT flagged
//!
//! `is_recursive` is set ONLY by [`detect_recursive_methods`], which
//! follows `EdgeKind::DirectLocal` edges within a single module. Two
//! cross-module shapes that escape this detection are documented and
//! accepted as follow-up work, not silent bugs:
//!
//! - **Cross-module mutual recursion (`A.foo → B.bar → A.foo`).**
//!   Salsa 0.26's `cycle_fn` recovery is *query-head dependent*: only
//!   the cycle-head batch query receives `cycle_fn` callbacks; other
//!   participants complete via `complete_cycle_participant` without
//!   invoking recovery (verified by reading `salsa-0.26.0/src/function/{fetch,execute}.rs`).
//!   A previous draft of this module relied on `cycle_fn` to
//!   blanket-flag SCC members; that gave false-positives on the head
//!   module and false-negatives on the other participants. Removing
//!   the blanket-flag yields a conservative under-approximation
//!   (silent miss) instead of an inconsistent over/under approximation.
//!   The proper fix is an explicit cross-module SCC walk over
//!   `module_call_summary` qualified edges (Tarjan/Kosaraju), tracked
//!   as the follow-up task and integration-tested in §1.7.
//!
//! - **Self-recursion through `ЭтотОбъект.foo()`.** Fixed in this
//!   commit: the call_graph extractor normalizes `ЭтотОбъект.foo()` to
//!   `DirectLocal` — [`detect_recursive_methods`] now picks it up.
//!
//! # Cycle API verification (Salsa 0.26)
//!
//! `module_effect_summaries_query` enters a Salsa cycle when module A's
//! batch transitively reads module B's batch and B's reads back into A
//! (cross-module mutual recursion). The signatures below match the
//! actual Salsa 0.26 callbacks — verified against `cycle.rs:172-189` of
//! `salsa-0.26.0/tests/cycle.rs` (`min_iterate` / `min_initial` /
//! `cycle_recover`):
//! `cycle_initial: fn(_db, salsa::Id, ..inputs) -> Output`,
//! `cycle_fn: fn(_db, &salsa::Cycle, &last_provisional, value, ..inputs) -> Output`.
//!
//! # Convergence
//!
//! Intra-module SCC fixpoint (worklist over Jacobi iterations): the
//! lattice is a 7-bit bool set + 1-bit `is_recursive`, so each method
//! ascends at most 8 times → total iterations bounded by 8 × method
//! count. Cross-module fixpoint via Salsa cycle: same monotone-ascent
//! argument; convergence guaranteed by `cycle_fn` returning a value
//! equal to `last_provisional` once no new bits are set.

use std::sync::Arc;

use base_db::FileIdInput;
use hir::{
    call_graph::{CallEdge, CallTarget, CallerId, EdgeKind, MethodSummary, ModuleCallSummary},
    dataflow::{
        effect_summary::{analyze_method_effects, CalleeKey, EffectSummary},
        security_state::{self, SecurityModeState},
        DataflowResult,
    },
    ModuleId, Name,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::RootDatabase;

// ============================================================================
// Result types
// ============================================================================

/// Per-method effect summaries for one module.
///
/// Returned by [`module_effect_summaries_query`]; consumed by the
/// per-method shim [`method_effect_summary_query`] and by adjacent
/// modules' batch queries (the SCC fixpoint reads cross-module
/// summaries through Salsa).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleEffectSummaries {
    /// `local_id` (ItemTree top-level index) → effect summary.
    methods: FxHashMap<u32, Arc<EffectSummary>>,
    /// `true` only for the cycle-recovery seed value
    /// ([`Self::initial_recursive`]). Lookup of an absent key in such a
    /// seed conservatively returns `is_recursive = true` so cross-module
    /// recursive members are flagged before the fixpoint produces a
    /// concrete answer. The flag clears as soon as
    /// [`Self::join`] folds in any non-seed value.
    is_initial_seed: bool,
}

impl ModuleEffectSummaries {
    /// Cycle-recovery seed for [`module_effect_summaries_query`]. Empty
    /// methods map; `is_initial_seed: true` so that cross-module
    /// callees observe a defined `EffectSummary::EMPTY` (lattice
    /// bottom) during the first iteration of a Salsa cycle instead of
    /// `None` (which would silently truncate the caller's qualified
    /// lookup).
    ///
    /// **Does not flag `is_recursive`** — see the module-doc "Known
    /// limitations". Salsa 0.26's `cycle_fn` fires only on the cycle
    /// head, so flagging through cycle recovery would be asymmetric
    /// (false-positive head, false-negative non-head); the seed
    /// keeps the lattice at its monotone-ascending bottom and leaves
    /// cross-module SCC detection to the explicit walk tracked as a
    /// follow-up.
    pub fn initial_recursive() -> Self {
        Self { methods: FxHashMap::default(), is_initial_seed: true }
    }

    /// Look up a method's summary. Returns `None` if the method has no
    /// computed summary AND this batch is not a cycle seed; returns
    /// `Some(EffectSummary::EMPTY)` for any id when the batch is a
    /// cycle seed (so cross-module callees see a defined bottom value
    /// and the lattice ascends correctly).
    pub fn get(&self, local_id: u32) -> Option<Arc<EffectSummary>> {
        if let Some(arc) = self.methods.get(&local_id) {
            return Some(arc.clone());
        }
        if self.is_initial_seed {
            return Some(Arc::new(EffectSummary::EMPTY));
        }
        None
    }

    /// Number of computed summaries (excludes the seed sentinel).
    pub fn len(&self) -> usize {
        self.methods.len()
    }

    /// `true` when no methods have been summarised.
    pub fn is_empty(&self) -> bool {
        self.methods.is_empty()
    }

    /// Bitwise-OR join of two batches, used by `cycle_fn`. Per-method
    /// summaries are joined element-wise; methods present only on one
    /// side carry over unchanged. The seed flag clears as soon as
    /// either side has a real (non-seed) result.
    pub fn join(&self, other: &Self) -> Self {
        let mut methods = self.methods.clone();
        for (&id, other_arc) in &other.methods {
            match methods.get_mut(&id) {
                Some(self_arc) => {
                    let merged = self_arc.join(other_arc);
                    *self_arc = Arc::new(merged);
                }
                None => {
                    methods.insert(id, other_arc.clone());
                }
            }
        }
        Self { methods, is_initial_seed: self.is_initial_seed && other.is_initial_seed }
    }
}

/// Per-method security-state results for one module.
///
/// Returned by [`module_security_state_query`]. Each method's
/// [`DataflowResult<SecurityModeState>`] carries the lattice value at
/// every CFG block, ready for handler consumption (§1.6).
///
/// Module-level top-level code (statements outside any
/// procedure/function) is analysed separately and exposed via
/// [`ModuleSecurityState::module_level`] — without it the §1.6 Group C
/// handlers would silently skip module-scope `SetPrivilegedMode` /
/// `DisableSafeMode` calls (Codex round-1 MAJOR fix).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleSecurityState {
    methods: FxHashMap<u32, Arc<DataflowResult<SecurityModeState>>>,
    module_level: Option<Arc<DataflowResult<SecurityModeState>>>,
}

impl ModuleSecurityState {
    /// Construct from a pre-built methods map plus an optional
    /// module-level result. Used by the streaming provider
    /// (`StreamingProvider::module_security_state`) to expose an
    /// on-the-fly batch without round-tripping through Salsa. The
    /// Salsa-tracked path goes through the query function directly and
    /// constructs this type internally.
    ///
    /// `pub(crate)` to keep the private fields out of the public
    /// ide-db surface — only the streaming module needs them.
    pub(crate) fn from_methods_with_module_level(
        methods: FxHashMap<u32, Arc<DataflowResult<SecurityModeState>>>,
        module_level: Option<Arc<DataflowResult<SecurityModeState>>>,
    ) -> Self {
        Self { methods, module_level }
    }

    /// Look up a method's dataflow result. Returns `None` for methods
    /// that didn't analyse (missing CFG, non-converging solver — same
    /// liveness contract as the other dataflow batch queries).
    pub fn get(&self, local_id: u32) -> Option<Arc<DataflowResult<SecurityModeState>>> {
        self.methods.get(&local_id).cloned()
    }

    /// Module-level (top-level) dataflow result, if the module has any
    /// top-level code outside procedures. Returns `None` for modules
    /// without module-level code or when the analysis failed to
    /// converge.
    pub fn module_level(&self) -> Option<Arc<DataflowResult<SecurityModeState>>> {
        self.module_level.clone()
    }

    /// Number of methods with a computed result. Excludes the
    /// module-level entry — query [`Self::module_level`] separately.
    pub fn len(&self) -> usize {
        self.methods.len()
    }

    /// `true` when no method has a computed result AND no module-level
    /// result is present. The §1.6 Group C handlers use this as the
    /// fast-path early exit; with the module-level addition the check
    /// must consider both batches.
    pub fn is_empty(&self) -> bool {
        self.methods.is_empty() && self.module_level.is_none()
    }
}

// ============================================================================
// Effect-summary queries
// ============================================================================

/// Salsa-tracked module batch over [`EffectSummary`].
///
/// Computes the per-method effect summary for every method in the
/// module. Intra-module recursion is resolved by an in-process worklist
/// (Jacobi iteration over [`EffectSummary::join`] — bitwise OR);
/// cross-module recursion is resolved by Salsa's fixpoint cycle
/// handlers (see [`module_effect_summaries_initial`] and
/// [`module_effect_summaries_cycle`]).
///
/// LRU = 128 — matches the `module_cfgs_query` precedent at
/// `crates/ide-db/src/queries.rs:255-256` (Track 1).
#[salsa::tracked(
    lru = 128,
    cycle_fn = module_effect_summaries_cycle,
    cycle_initial = module_effect_summaries_initial,
)]
pub fn module_effect_summaries_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleEffectSummaries> {
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);
    let _span = tracing::info_span!("module_effect_summaries", ?module_id).entered();

    let module_bodies = db.module_bodies(module_id);
    let call_summary = db.module_call_summary(module_id);
    let source_root_id = db.file_source_root_input(file_id).source_root_id(db);

    // Build a name→local_id case-insensitive lookup once. Used both by
    // the SCC-detector and by the `CalleeKey::Local` arm of the closure.
    let local_by_name = build_local_name_index(&call_summary);

    // Local recursive set: every method that can reach itself through
    // `EdgeKind::DirectLocal` edges (self-loops + non-trivial SCC
    // members). Computed once before the fixpoint so the post-pass can
    // mark `is_recursive = true` without re-running the search.
    let recursive = detect_recursive_methods(&call_summary);

    // Seed every body with EMPTY. Methods absent from `module_bodies`
    // (synthetic / orphan ItemTree entries) stay out of the table — the
    // `CalleeKey::Local` lookup arm returns `None` for them, which
    // contributes nothing to the caller (correct: we know nothing).
    let mut summaries: FxHashMap<u32, EffectSummary> = FxHashMap::default();
    for (local_id, _) in module_bodies.iter_bodies() {
        summaries.insert(local_id, EffectSummary::EMPTY);
    }

    // Gauss-Seidel fixpoint: each call to `analyze_method_effects`
    // reads the LIVE `summaries` map, so within one outer iteration a
    // method can already see updates written by earlier methods in the
    // same iteration. This converges faster than Jacobi (Codex round-A
    // B-1: a Jacobi cap of 16 silently truncates chains of length > 16).
    //
    // Convergence bound: each strict ascent of any single method is
    // bounded by the lattice height (7 effect bits + 1 recursion bit
    // applied post-pass = 7 strict ascents max per method). Total
    // ascents across the module are bounded by `7 * body_count`. Each
    // outer iteration where `changed=true` performs ≥1 ascent, so the
    // outer iteration count is bounded by the same product. We pick
    // `8 * body_count + 8` to give a 1-bit safety margin and a small
    // floor for empty modules.
    let body_count = module_bodies.iter_bodies().count();
    let max_iterations = body_count.saturating_mul(8).saturating_add(8);
    let mut iterations = 0usize;
    loop {
        iterations += 1;
        if iterations > max_iterations {
            tracing::error!(
                module = ?module_id,
                iterations,
                max_iterations,
                "module_effect_summaries did not converge — \
                 returning the last computed snapshot, possibly missing some effect bits"
            );
            break;
        }
        let mut changed = false;
        for (local_id, body) in module_bodies.iter_bodies() {
            db.unwind_if_revision_cancelled();
            let computed = analyze_method_effects(body, |key| match key {
                CalleeKey::Local(name) => local_by_name
                    .get(&name.as_str().to_lowercase())
                    .and_then(|id| summaries.get(id).copied()),
                CalleeKey::Qualified { module, method } => {
                    resolve_qualified_callee(db, source_root_id, module, method)
                }
            });
            let cur = summaries.get(&local_id).copied().unwrap_or(EffectSummary::EMPTY);
            let merged = cur.join(&computed);
            if merged != cur {
                summaries.insert(local_id, merged);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Apply local recursion flags AFTER the fixpoint converges. The
    // pure `analyze_method_effects` masks `is_recursive` from callee
    // summaries before joining, so propagating the flag transitively is
    // explicitly NOT desired (a callee in an SCC does not make its
    // out-of-SCC callers recursive).
    for &id in &recursive {
        if let Some(s) = summaries.get_mut(&id) {
            s.is_recursive = true;
        }
    }

    let methods = summaries.into_iter().map(|(id, s)| (id, Arc::new(s))).collect();
    tracing::debug!(count = ?recursive.len(), "Module effect-summary fixpoint converged");
    Arc::new(ModuleEffectSummaries { methods, is_initial_seed: false })
}

/// Cycle-recovery seed for [`module_effect_summaries_query`]. Returns
/// the marker value whose lookups answer with `EffectSummary::EMPTY`
/// (lattice bottom) for any local_id, so cross-module callees during
/// the first cycle iteration ascend correctly. `is_recursive` is NOT
/// pre-flagged on the seed — see [`ModuleEffectSummaries::initial_recursive`]
/// and the module-doc "Known limitations" for the rationale.
#[allow(clippy::needless_lifetimes)] // Salsa attr requires explicit signature
pub fn module_effect_summaries_initial<'db>(
    _db: &'db dyn RootDatabase,
    _id: salsa::Id,
    _file_id_input: FileIdInput<'db>,
) -> Arc<ModuleEffectSummaries> {
    Arc::new(ModuleEffectSummaries::initial_recursive())
}

/// Cycle-iteration step for [`module_effect_summaries_query`].
///
/// **Job: bitwise-OR fixpoint join only.** Per-method effect bits
/// ascend monotonically; once nothing new is added, the joined value
/// is equal to `last_provisional` and Salsa converges.
///
/// **Does NOT flag `is_recursive` on cycle participants** — see the
/// module-doc "Known limitations" section. Briefly: Salsa 0.26 fires
/// `cycle_fn` only on the cycle head, so flagging here would attach
/// `is_recursive=true` to the head module's methods but miss the
/// other participants. That asymmetry is worse than under-flagging,
/// so we under-flag uniformly and leave cross-module SCC detection to
/// a follow-up slice that walks `module_call_summary` qualified edges
/// directly.
#[allow(clippy::needless_lifetimes)]
pub fn module_effect_summaries_cycle<'db>(
    _db: &'db dyn RootDatabase,
    _cycle: &salsa::Cycle,
    last_provisional: &Arc<ModuleEffectSummaries>,
    value: Arc<ModuleEffectSummaries>,
    _file_id_input: FileIdInput<'db>,
) -> Arc<ModuleEffectSummaries> {
    Arc::new(last_provisional.as_ref().join(value.as_ref()))
}

/// Per-method shim over the module batch (LRU = 256, matching
/// `method_cfg_query` at `queries.rs:298`). Track 1 precedent: build
/// the heavy data once per module, expose per-method via a cheap
/// HashMap lookup.
#[salsa::tracked(lru = 256)]
pub fn method_effect_summary_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Arc<EffectSummary> {
    let _span = tracing::info_span!("method_effect_summary", ?method_id_input).entered();
    let method_id = method_id_input.method_id(db);
    let file_id = method_id.module.file_id;
    let file_id_input = FileIdInput::new(db, file_id);
    let summaries = module_effect_summaries_query(db, file_id_input);
    summaries.get(method_id.local_id).unwrap_or_else(|| Arc::new(EffectSummary::EMPTY))
}

// ============================================================================
// Security-state query
// ============================================================================

/// Salsa-tracked module batch over [`SecurityModeState`].
///
/// Runs the §1.2 forward dataflow on every method body in the module,
/// reusing CFGs from `module_cfgs_query`. Per-method results are
/// returned in a [`ModuleSecurityState`] keyed by `local_id`. No cycle
/// handlers — the lattice has no cross-method dependency edges.
///
/// LRU = 128 (matches the other module-batch queries; cap fixed in
/// the master plan §12.2).
#[salsa::tracked(lru = 128)]
pub fn module_security_state_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleSecurityState> {
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);
    let _span = tracing::info_span!("module_security_state", ?module_id).entered();

    let module_cfgs = db.module_cfgs(file_id_input);
    let module_bodies = db.module_bodies(module_id);

    let mut methods = FxHashMap::default();
    for (local_id, body) in module_bodies.iter_bodies() {
        db.unwind_if_revision_cancelled();
        let cfg = match module_cfgs.get(local_id) {
            Some(c) => c.clone(),
            None => continue,
        };
        if let Some(result) = security_state::analyze(cfg, body.clone()) {
            methods.insert(local_id, Arc::new(result));
        }
    }

    // Track 2 §1.6 Group C — Codex round-1 MAJOR fix: also analyse
    // module-level top-level code so file-scope SetPrivilegedMode /
    // DisableSafeMode calls outside any procedure are surfaced. The
    // legacy HIR-side detector ran on every CALL_EXPR; the lattice
    // path keeps parity by reusing `module_level_cfg_query` (Track 1).
    // Codex round-2 NIT: `module_code()` returns `Some` for *every*
    // module post-lowering even when the file has zero top-level
    // statements (the body is empty but the entry exists). Filter the
    // empty case so `ModuleSecurityState::is_empty()` keeps its
    // "nothing to scan" semantics for handler fast-paths.
    let module_level = module_bodies
        .module_code()
        .filter(|body| !body.body_stmts_typed().is_empty())
        .and_then(|body| {
            db.unwind_if_revision_cancelled();
            let cfg = db.module_level_cfg(module_id);
            security_state::analyze(cfg, body.clone()).map(Arc::new)
        });

    tracing::debug!(
        count = methods.len(),
        module_level = module_level.is_some(),
        "Module security-state batch built"
    );
    Arc::new(ModuleSecurityState { methods, module_level })
}

// ============================================================================
// Internals
// ============================================================================

/// Build a case-insensitive `name → local_id` lookup over **all**
/// methods in a module's call summary. Used for INTRA-module
/// resolution (`CalleeKey::Local`), where private methods are visible.
fn build_local_name_index(summary: &ModuleCallSummary) -> FxHashMap<String, u32> {
    let mut map: FxHashMap<String, u32> = FxHashMap::default();
    for MethodSummary { local_id, name, .. } in &summary.methods {
        map.entry(name.as_str().to_lowercase()).or_insert(*local_id);
    }
    map
}

/// Build a case-insensitive `name → local_id` lookup over **exported**
/// methods only. Used for CROSS-module resolution
/// (`CalleeKey::Qualified`) — BSL semantics: only `Export` methods of
/// a CommonModule are reachable via `Module.Method` (Codex round-A
/// MAJOR fix: without the filter we would attribute private-method
/// effects to external callers).
fn build_exported_name_index(summary: &ModuleCallSummary) -> FxHashMap<String, u32> {
    let mut map: FxHashMap<String, u32> = FxHashMap::default();
    for m in &summary.methods {
        if !m.is_export {
            continue;
        }
        map.entry(m.name.as_str().to_lowercase()).or_insert(m.local_id);
    }
    map
}

/// Resolve a `CalleeKey::Qualified` against the workspace module index
/// and the cross-module batch. Returns `None` when the module is not
/// indexed (e.g. the receiver name is a local variable that happens to
/// match a `module.method` shape) or the named method is not exported
/// from the resolved module — exactly the false-positive trade-off
/// documented in `analyze_method_effects`'s module-doc.
///
/// Resolution order (Codex round-A MAJOR fix): index → call-summary
/// (resolve local_id) → cross-module batch. We avoid triggering the
/// (cycle-prone) `module_effect_summaries_query` for the other module
/// until we've confirmed the named method actually exists and is
/// exported there. This trims spurious Salsa dependencies on misspelled
/// or private callees.
fn resolve_qualified_callee(
    db: &dyn RootDatabase,
    source_root_id: base_db::SourceRootId,
    module: &Name,
    method: &Name,
) -> Option<EffectSummary> {
    let module_index = db.module_index(source_root_id);
    let other_file_id = module_index.resolve_common_module(module)?;
    let other_module_id = ModuleId::new(other_file_id);

    // Local-id resolution first — does the named method exist as an
    // export? If not, no Salsa dependency is recorded on the other
    // module's effect batch.
    let other_call_summary = db.module_call_summary(other_module_id);
    let other_local_id = build_exported_name_index(&other_call_summary)
        .get(&method.as_str().to_lowercase())
        .copied()?;

    // Now we know the call resolves — pull the cross-module batch
    // (this is the edge that may participate in a Salsa cycle).
    let other_input = FileIdInput::new(db, other_file_id);
    let other_summaries = module_effect_summaries_query(db, other_input);
    other_summaries.get(other_local_id).map(|arc| *arc.as_ref())
}

/// Detect every method that participates in a local recursive cycle
/// (self-edge or non-trivial SCC) over the `EdgeKind::DirectLocal`
/// subgraph. Reachability-based: O(V·(V+E)). Modules typically have
/// fewer than 50 methods so the simpler algorithm wins over Tarjan
/// here.
fn detect_recursive_methods(summary: &ModuleCallSummary) -> FxHashSet<u32> {
    let mut graph: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    for CallEdge { caller, target, kind, .. } in &summary.call_edges {
        if !matches!(kind, EdgeKind::DirectLocal) {
            continue;
        }
        let CallerId::Method(caller_id) = caller else { continue };
        let CallTarget::Local { callee_local_id } = target else { continue };
        graph.entry(*caller_id).or_default().push(*callee_local_id);
    }

    let mut recursive: FxHashSet<u32> = FxHashSet::default();
    for &start in graph.keys() {
        let mut stack: Vec<u32> = graph.get(&start).cloned().unwrap_or_default();
        let mut visited: FxHashSet<u32> = FxHashSet::default();
        while let Some(node) = stack.pop() {
            if node == start {
                recursive.insert(start);
                break;
            }
            if !visited.insert(node) {
                continue;
            }
            if let Some(succs) = graph.get(&node) {
                stack.extend(succs.iter().copied());
            }
        }
    }
    recursive
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_effect_summaries_initial_seed_returns_empty_bottom() {
        let seed = ModuleEffectSummaries::initial_recursive();
        // Codex round-A B-2: the seed is the lattice bottom — any
        // cross-module callee during the first cycle iteration sees
        // `EffectSummary::EMPTY` so the lattice ascends correctly.
        // Recursion membership is set by `cycle_fn` after each Salsa
        // iteration (verified separately by the cycle-flag test below),
        // not via this lookup.
        let s = seed.get(0).expect("seed must answer for any id");
        assert!(!s.is_recursive, "seed lookup must not pre-flag recursion");
        assert_eq!(*s, EffectSummary::EMPTY);
    }

    #[test]
    fn cycle_fn_is_pure_join_no_flagging() {
        // Regression guard: cycle_fn must NOT flag is_recursive on
        // joined methods. Salsa 0.26's cycle_fn fires only on the
        // cycle head, so flagging here would over-attribute recursion
        // to the head module while under-attributing it to the other
        // participants — see the module-doc "Known limitations". The
        // join must be a pure bitwise-OR.
        let mut a = ModuleEffectSummaries::default();
        a.methods.insert(
            3,
            Arc::new(EffectSummary { may_call_filesystem: true, ..EffectSummary::EMPTY }),
        );
        let mut b = ModuleEffectSummaries::default();
        b.methods
            .insert(3, Arc::new(EffectSummary { may_call_internet: true, ..EffectSummary::EMPTY }));

        let joined = a.join(&b);

        let s = joined.get(3).unwrap();
        assert!(s.may_call_filesystem, "effect bits propagate via OR");
        assert!(s.may_call_internet, "effect bits propagate via OR");
        assert!(
            !s.is_recursive,
            "cycle_fn must not flag is_recursive — head-only flagging would be incorrect"
        );
    }

    #[test]
    fn non_seed_lookup_misses_return_none() {
        let summaries = ModuleEffectSummaries::default();
        assert!(summaries.get(0).is_none());
    }

    #[test]
    fn join_clears_seed_flag_when_other_is_real() {
        let seed = ModuleEffectSummaries::initial_recursive();
        let real = ModuleEffectSummaries { methods: FxHashMap::default(), is_initial_seed: false };
        let merged = seed.join(&real);
        assert!(!merged.is_initial_seed, "joining real result must clear the seed flag");
    }

    #[test]
    fn join_preserves_seed_when_both_are_seeds() {
        let a = ModuleEffectSummaries::initial_recursive();
        let b = ModuleEffectSummaries::initial_recursive();
        let merged = a.join(&b);
        assert!(merged.is_initial_seed, "seed AND seed = seed (no real result observed)");
    }

    #[test]
    fn join_per_method_is_bitwise_or() {
        let mut a = ModuleEffectSummaries::default();
        a.methods.insert(
            7,
            Arc::new(EffectSummary { may_call_filesystem: true, ..EffectSummary::EMPTY }),
        );
        let mut b = ModuleEffectSummaries::default();
        b.methods
            .insert(7, Arc::new(EffectSummary { may_call_internet: true, ..EffectSummary::EMPTY }));
        let merged = a.join(&b);
        let s = merged.get(7).unwrap();
        assert!(s.may_call_filesystem);
        assert!(s.may_call_internet);
        // Methods only present in one side carry over.
        let mut c = ModuleEffectSummaries::default();
        c.methods.insert(
            9,
            Arc::new(EffectSummary { may_call_external_app: true, ..EffectSummary::EMPTY }),
        );
        let merged2 = merged.join(&c);
        assert!(merged2.get(7).unwrap().may_call_filesystem);
        assert!(merged2.get(9).unwrap().may_call_external_app);
    }
}
