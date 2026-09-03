use std::sync::Arc;
use stdx::case::CaseExt;

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleEffectSummaries {
    methods: FxHashMap<hir::MethodKey, Arc<EffectSummary>>,
    is_initial_seed: bool,
}

impl ModuleEffectSummaries {
    pub fn initial_recursive() -> Self {
        Self { methods: FxHashMap::default(), is_initial_seed: true }
    }

    pub fn get(&self, local_id: hir::MethodKey) -> Option<Arc<EffectSummary>> {
        if let Some(arc) = self.methods.get(&local_id) {
            return Some(arc.clone());
        }
        if self.is_initial_seed {
            return Some(Arc::new(EffectSummary::EMPTY));
        }
        None
    }

    pub fn len(&self) -> usize {
        self.methods.len()
    }

    pub fn is_empty(&self) -> bool {
        self.methods.is_empty()
    }

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

/// `heap_size` estimators for the effect/security memos — see
/// [`crate::queries::heap_estimate`] for the shared approximation rationale.
mod heap_estimate {
    use super::{DataflowResult, EffectSummary, ModuleEffectSummaries, SecurityModeState};
    use crate::queries::heap_estimate::map_table_bytes;
    use std::sync::Arc;

    pub(super) fn module_effect_summaries_heap(v: &Arc<ModuleEffectSummaries>) -> usize {
        // Each `Arc<EffectSummary>` heap-allocates a small `Copy` struct; count the
        // table backbone plus one payload per entry.
        let len = v.len();
        map_table_bytes::<hir::MethodKey, Arc<EffectSummary>>(len)
            + len * std::mem::size_of::<EffectSummary>()
    }

    /// A `method_effect_summary` result is a clone of an `Arc` owned by
    /// `module_effect_summaries`; report only the shared pointer.
    pub(super) fn shared_effect_summary_heap(_v: &Arc<EffectSummary>) -> usize {
        0
    }

    pub(super) fn security_state_heap(v: &Option<Arc<DataflowResult<SecurityModeState>>>) -> usize {
        v.as_ref().map_or(0, |r| r.estimated_heap_with(|s| s.estimated_heap()))
    }
}

#[salsa::tracked(
    lru = 128,
    cycle_fn = module_effect_summaries_cycle,
    cycle_initial = module_effect_summaries_initial,
    heap_size = heap_estimate::module_effect_summaries_heap,
    returns(clone),
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

    let local_by_name = build_local_name_index(&call_summary);

    let recursive = detect_recursive_methods(&call_summary);

    let mut summaries: FxHashMap<hir::MethodKey, EffectSummary> = FxHashMap::default();
    for (local_id, _) in module_bodies.iter_bodies() {
        summaries.insert(local_id, EffectSummary::EMPTY);
    }

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
                    .get(&name.as_str().fold_lower())
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

    for &id in &recursive {
        if let Some(s) = summaries.get_mut(&id) {
            s.is_recursive = true;
        }
    }

    let methods = summaries.into_iter().map(|(id, s)| (id, Arc::new(s))).collect();
    tracing::debug!(count = ?recursive.len(), "Module effect-summary fixpoint converged");
    Arc::new(ModuleEffectSummaries { methods, is_initial_seed: false })
}

#[allow(
    clippy::needless_lifetimes,
    reason = "Salsa callback signature requires explicit lifetimes"
)]
pub fn module_effect_summaries_initial<'db>(
    _db: &'db dyn RootDatabase,
    _id: salsa::Id,
    _file_id_input: FileIdInput<'db>,
) -> Arc<ModuleEffectSummaries> {
    Arc::new(ModuleEffectSummaries::initial_recursive())
}

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

#[salsa::tracked(lru = 256, heap_size = heap_estimate::shared_effect_summary_heap, returns(clone))]
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

/// Security-mode dataflow of one method, computed from its own body and CFG
/// so that a body edit re-runs it for the edited method only. Retained at the
/// cap of the per-method chain (see `queries::set_dataflow_lru_sweep_mode`).
#[salsa::tracked(lru = 8192, heap_size = heap_estimate::security_state_heap, returns(clone))]
pub fn method_security_state_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Option<Arc<DataflowResult<SecurityModeState>>> {
    let _span = tracing::info_span!("method_security_state", ?method_id_input).entered();
    let cfg = crate::queries::method_cfg_query(db, method_id_input);
    let body = db.method_body_ref(method_id_input);
    security_state::analyze(cfg, (**body).clone()).map(Arc::new)
}

/// Security-mode dataflow of the module-level code; `None` when the module
/// has no statements outside methods.
#[salsa::tracked(lru = 128, heap_size = heap_estimate::security_state_heap, returns(clone))]
pub fn module_code_security_state_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Option<Arc<DataflowResult<SecurityModeState>>> {
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);
    let _span = tracing::info_span!("module_code_security_state", ?module_id).entered();
    let module_bodies = db.module_bodies_ref(module_id);
    let body = module_bodies.module_code().filter(|body| !body.body_stmts_typed().is_empty())?;
    db.unwind_if_revision_cancelled();
    let cfg = db.module_level_cfg(module_id);
    security_state::analyze(cfg, body.clone()).map(Arc::new)
}

pub(crate) fn set_security_state_lru_capacity(db: &mut dyn RootDatabase, cap: usize) {
    method_security_state_query::set_lru_capacity(db, cap);
}

fn build_local_name_index(summary: &ModuleCallSummary) -> FxHashMap<String, hir::MethodKey> {
    let mut map: FxHashMap<String, hir::MethodKey> = FxHashMap::default();
    for MethodSummary { local_id, name, .. } in &summary.methods {
        map.entry(name.as_str().fold_lower()).or_insert(*local_id);
    }
    map
}

fn build_exported_name_index(summary: &ModuleCallSummary) -> FxHashMap<String, hir::MethodKey> {
    let mut map: FxHashMap<String, hir::MethodKey> = FxHashMap::default();
    for m in &summary.methods {
        if !m.is_export {
            continue;
        }
        map.entry(m.name.as_str().fold_lower()).or_insert(m.local_id);
    }
    map
}

fn resolve_qualified_callee(
    db: &dyn RootDatabase,
    source_root_id: base_db::SourceRootId,
    module: &Name,
    method: &Name,
) -> Option<EffectSummary> {
    let module_index = db.module_index(source_root_id);
    let other_file_id = module_index.resolve_common_module(module)?;
    let other_module_id = ModuleId::new(other_file_id);

    let other_call_summary = db.module_call_summary(other_module_id);
    let other_local_id = build_exported_name_index(&other_call_summary)
        .get(&method.as_str().fold_lower())
        .copied()?;

    let other_input = FileIdInput::new(db, other_file_id);
    let other_summaries = module_effect_summaries_query(db, other_input);
    other_summaries.get(other_local_id).map(|arc| *arc.as_ref())
}

fn detect_recursive_methods(summary: &ModuleCallSummary) -> FxHashSet<hir::MethodKey> {
    let mut graph: FxHashMap<hir::MethodKey, Vec<hir::MethodKey>> = FxHashMap::default();
    for CallEdge { caller, target, kind, .. } in &summary.call_edges {
        if !matches!(kind, EdgeKind::DirectLocal) {
            continue;
        }
        let CallerId::Method(caller_id) = caller else { continue };
        let CallTarget::Local { callee_local_id } = target else { continue };
        graph.entry(*caller_id).or_default().push(*callee_local_id);
    }

    let mut recursive: FxHashSet<hir::MethodKey> = FxHashSet::default();
    for &start in graph.keys() {
        let mut stack: Vec<hir::MethodKey> = graph.get(&start).cloned().unwrap_or_default();
        let mut visited: FxHashSet<hir::MethodKey> = FxHashSet::default();
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

    fn k(n: u32) -> hir::MethodKey {
        hir::MethodKey::first(&format!("М{n}"))
    }

    #[test]
    fn module_effect_summaries_initial_seed_returns_empty_bottom() {
        let seed = ModuleEffectSummaries::initial_recursive();
        let s = seed.get(k(0)).expect("seed must answer for any id");
        assert!(!s.is_recursive, "seed lookup must not pre-flag recursion");
        assert_eq!(*s, EffectSummary::EMPTY);
    }

    #[test]
    fn cycle_fn_is_pure_join_no_flagging() {
        let mut a = ModuleEffectSummaries::default();
        a.methods.insert(
            k(3),
            Arc::new(EffectSummary { may_call_filesystem: true, ..EffectSummary::EMPTY }),
        );
        let mut b = ModuleEffectSummaries::default();
        b.methods.insert(
            k(3),
            Arc::new(EffectSummary { may_call_internet: true, ..EffectSummary::EMPTY }),
        );

        let joined = a.join(&b);

        let s = joined.get(k(3)).unwrap();
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
        assert!(summaries.get(k(0)).is_none());
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
            k(7),
            Arc::new(EffectSummary { may_call_filesystem: true, ..EffectSummary::EMPTY }),
        );
        let mut b = ModuleEffectSummaries::default();
        b.methods.insert(
            k(7),
            Arc::new(EffectSummary { may_call_internet: true, ..EffectSummary::EMPTY }),
        );
        let merged = a.join(&b);
        let s = merged.get(k(7)).unwrap();
        assert!(s.may_call_filesystem);
        assert!(s.may_call_internet);
        let mut c = ModuleEffectSummaries::default();
        c.methods.insert(
            k(9),
            Arc::new(EffectSummary { may_call_external_app: true, ..EffectSummary::EMPTY }),
        );
        let merged2 = merged.join(&c);
        assert!(merged2.get(k(7)).unwrap().may_call_filesystem);
        assert!(merged2.get(k(9)).unwrap().may_call_external_app);
    }
}
