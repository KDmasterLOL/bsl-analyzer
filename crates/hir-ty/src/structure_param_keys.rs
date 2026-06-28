//! Stage 2 — interprocedural structure keys.
//!
//! A method that inserts keys into one of its by-reference parameters (directly, or by forwarding
//! that parameter to a further helper) is summarised here as a per-parameter key projection. At a
//! construction site, passing a tracked structure local *whole* to such a method folds the callee's
//! summary into the local, so keys added inside helpers / child methods surface at the call site.
//!
//! Light syntactic analysis (body walk + call resolution), NOT full inference — this keeps cycles
//! confined to `summary ↔ summary` (one salsa query, one cycle handler), exactly like
//! [`crate::method_graph::method_return_type_query`]. Soft: completion/hover only, never a
//! diagnostic. Ordinary modules only (effective/weaving inference is out of scope).

use std::cell::OnceCell;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use stdx::case::{eq_ignore_case, CaseExt};

use bsl_types::builders::Builders;
use bsl_types::kind::{Projection, ProjectionField, ProjectionOrigin, TypeId};
use hir_def::body::Body;
use hir_def::hir::{Expr, Stmt};
use hir_def::resolver::Resolver;
use hir_def::{MethodId, MethodIdInput, ModuleId};

use crate::db::HirDatabase;
use crate::method_resolution::{resolve_qualified_call, resolve_three_level_call};
use crate::structure_keys::{
    collect_structure_shapes, shape_to_projection, SeedRoots, StructureShape, ValueSource,
};

/// Per-parameter inserted-key projections for one method. An absent index = no keys inserted or
/// forwarded for that parameter.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StructureParamSummary {
    pub per_param: FxHashMap<u32, Arc<Projection>>,
}

#[salsa::tracked(
    lru = 262144,
    cycle_fn = structure_param_keys_cycle,
    cycle_initial = structure_param_keys_initial,
)]
pub fn structure_param_keys_query<'db>(
    db: &'db dyn HirDatabase,
    method: MethodIdInput<'db>,
) -> Arc<StructureParamSummary> {
    let mid = method.method_id(db);
    let _span = tracing::info_span!(
        "structure_param_keys",
        file_id = mid.module.file_id.0,
        local_id = mid.local_id,
    )
    .entered();

    let body = db.method_body(method);
    let symbol_tree = db.symbol_tree(mid.module);
    let Some(msym) = symbol_tree.find_method_by_id(mid) else {
        return Arc::new(StructureParamSummary::default());
    };

    // By-reference params are tracked roots; `Знач` params are a caller-invisible copy (skip).
    let byref: Vec<String> =
        msym.params.iter().filter(|p| !p.is_val).map(|p| p.name.as_str().fold_lower()).collect();
    if byref.is_empty() {
        return Arc::new(StructureParamSummary::default());
    }

    // Params are live from method entry, so forwarding interleaves correctly in statement order.
    let resolver = Resolver::with_builtins_and_workspace(mid.module);
    let forwarder = Forwarder::new(db, &resolver, mid.module, &body);
    let shapes = collect_structure_shapes(&body, SeedRoots::ParamNames(&byref), Some(&forwarder));

    let no_expr_types = FxHashMap::default();
    let mut per_param: FxHashMap<u32, Arc<Projection>> = FxHashMap::default();
    for (i, param) in msym.params.iter().enumerate() {
        if param.is_val {
            continue;
        }
        let name = param.name.as_str().fold_lower();
        if let Some(shape) = shapes.get(&name) {
            if let Some(projection) = shape_to_projection(db, &no_expr_types, shape, 0) {
                per_param.insert(i as u32, projection);
            }
        }
    }

    Arc::new(StructureParamSummary { per_param })
}

#[allow(
    clippy::needless_lifetimes,
    reason = "Salsa callback signature requires explicit lifetimes"
)]
pub fn structure_param_keys_initial<'db>(
    _db: &'db dyn HirDatabase,
    _id: salsa::Id,
    _method: MethodIdInput<'db>,
) -> Arc<StructureParamSummary> {
    Arc::new(StructureParamSummary::default())
}

#[allow(
    clippy::needless_lifetimes,
    reason = "Salsa callback signature requires explicit lifetimes"
)]
pub fn structure_param_keys_cycle<'db>(
    db: &'db dyn HirDatabase,
    _cycle: &salsa::Cycle,
    last_provisional: &Arc<StructureParamSummary>,
    value: Arc<StructureParamSummary>,
    _method: MethodIdInput<'db>,
) -> Arc<StructureParamSummary> {
    join_summaries(db, last_provisional, &value)
}

/// Order-independent lattice join on the product lattice (pointwise per parameter): field set =
/// union by folded name; on a name collision the type is `db.union` with `Unknown` as bottom — never
/// last-value-wins, which would downgrade a known type and break monotonicity. The merged field
/// order is canonicalised so the interned projection (and thus the salsa value) is independent of
/// join order, which the cycle fixpoint requires to converge.
fn join_summaries(
    db: &dyn HirDatabase,
    a: &StructureParamSummary,
    b: &StructureParamSummary,
) -> Arc<StructureParamSummary> {
    let mut per_param = a.per_param.clone();
    for (idx, proj_b) in &b.per_param {
        per_param
            .entry(*idx)
            .and_modify(|proj_a| *proj_a = merge_projections(db, proj_a, proj_b))
            .or_insert_with(|| Arc::clone(proj_b));
    }
    Arc::new(StructureParamSummary { per_param })
}

fn merge_projections(db: &dyn HirDatabase, a: &Projection, b: &Projection) -> Arc<Projection> {
    let mut fields: Vec<ProjectionField> = a.fields.to_vec();
    for fb in b.fields.iter() {
        if let Some(fa) = fields.iter_mut().find(|f| eq_ignore_case(&f.name, &fb.name)) {
            fa.ty = join_ty(db, fa.ty, fb.ty);
        } else {
            fields.push(fb.clone());
        }
    }
    // Canonical order: the join must be commutative at the interned-representation level.
    fields.sort_by_key(|f| f.name.fold_lower());
    Arc::new(Projection::new(fields.into(), ProjectionOrigin::StructureLiteral, None))
}

fn join_ty(db: &dyn HirDatabase, a: TypeId, b: TypeId) -> TypeId {
    let unknown = db.unknown();
    if a == unknown {
        b
    } else if b == unknown || a == b {
        a
    } else {
        db.union(vec![a, b])
    }
}

/// Resolves forwarded calls within one body and folds callee summaries into tracked structure
/// shapes. Built once per body; resolution caches (shadowing locals, global exports) are computed at
/// most once. Shared by the Stage-1 construction-site fold (in `infer`) and the Stage-2 summary's
/// parameter forwarding — both drive it through the collector's source-ordered statement pass.
pub(crate) struct Forwarder<'a> {
    db: &'a dyn HirDatabase,
    resolver: &'a Resolver,
    module: ModuleId,
    /// Names (lowercased) that are body bindings or assignment targets — a same-named method is
    /// shadowed and must not be resolved as a callee (mirrors the inference dispatch guard).
    shadowing_locals: FxHashSet<String>,
    /// Lazily-built lowercased method-name → `MethodId` of the visible global common modules.
    global_methods: OnceCell<FxHashMap<String, MethodId>>,
}

impl<'a> Forwarder<'a> {
    pub(crate) fn new(
        db: &'a dyn HirDatabase,
        resolver: &'a Resolver,
        module: ModuleId,
        body: &Body,
    ) -> Self {
        let mut shadowing_locals = FxHashSet::default();
        for (_, binding) in body.bindings_iter() {
            shadowing_locals.insert(binding.name.as_str().fold_lower());
        }
        for (_, stmt) in body.stmts_iter() {
            if let Stmt::Assign { target, .. } = stmt {
                if let Expr::Path(name) = body.expr_idx(*target) {
                    shadowing_locals.insert(name.as_str().fold_lower());
                }
            }
        }
        Self { db, resolver, module, shadowing_locals, global_methods: OnceCell::new() }
    }

    /// Fold a single call's callee summary into the tracked roots it is passed (whole). A no-op
    /// unless `call` is a resolvable call passing at least one tracked structure root.
    pub(crate) fn fold_call(
        &self,
        body: &Body,
        shapes: &mut FxHashMap<String, StructureShape>,
        frozen: &FxHashSet<String>,
        call: &Expr,
    ) {
        let Expr::Call { callee, args } = call else { return };

        // Arguments that are tracked, still-live roots passed whole (`F(С)`, not `F(С.Поле)`).
        let tracked: Vec<(usize, String)> = args
            .iter()
            .enumerate()
            .filter_map(|(j, arg)| match body.expr_idx(*arg) {
                Expr::Path(name) => {
                    let key = name.as_str().fold_lower();
                    (shapes.contains_key(&key) && !frozen.contains(&key)).then_some((j, key))
                }
                _ => None,
            })
            .collect();
        if tracked.is_empty() {
            return;
        }

        let Some(callee_mid) = self.resolve_callee(body, body.expr_idx(*callee)) else { return };
        let summary = structure_param_keys_query(self.db, MethodIdInput::new(self.db, callee_mid));
        if summary.per_param.is_empty() {
            return;
        }

        let callee_tree = self.db.symbol_tree(callee_mid.module);
        let callee_params = callee_tree.find_method_by_id(callee_mid).map(|m| &m.params);

        for (j, root) in tracked {
            // A `Знач` callee param receives a copy — inserted keys never reach the caller.
            if callee_params.is_some_and(|ps| ps.get(j).is_some_and(|p| p.is_val)) {
                continue;
            }
            let Some(projection) = summary.per_param.get(&(j as u32)) else { continue };
            let shape = shapes.get_mut(&root).expect("root is tracked");
            for field in projection.fields.iter() {
                shape.upsert(&field.name, ValueSource::Resolved(field.ty));
            }
        }
    }

    /// Resolve a call's callee to a `MethodId` using only syntactic forms (no inferred receiver):
    /// same-module bare, global common-module bare, common-module `Модуль.F`, three-level
    /// `Справочники.X.F`. A bare name shadowed by a local/param is not a method call → `None`.
    fn resolve_callee(&self, body: &Body, callee: &Expr) -> Option<MethodId> {
        match callee {
            Expr::Path(name) => {
                let lower = name.as_str().fold_lower();
                if self.shadowing_locals.contains(&lower) {
                    return None;
                }
                if let Some(method) = self.db.symbol_tree(self.module).find_method(name) {
                    return Some(method.id);
                }
                self.global_methods().get(&lower).copied()
            }
            Expr::Field { base, field } => {
                let Expr::Path(module_name) = body.expr_idx(*base) else { return None };
                resolve_qualified_call(self.db, module_name, field, self.resolver)
                    .ok()
                    .map(|r| r.method_id)
            }
            Expr::QualifiedPath(qname) => match qname.segments() {
                [mdo_type, mdo_name, method_name] => resolve_three_level_call(
                    self.db,
                    mdo_type,
                    mdo_name,
                    method_name,
                    self.resolver,
                )
                .ok()
                .map(|r| r.method_id),
                _ => None,
            },
            _ => None,
        }
    }

    fn global_methods(&self) -> &FxHashMap<String, MethodId> {
        self.global_methods.get_or_init(|| {
            let mut map = FxHashMap::default();
            for (_module, method_name, method_id) in
                self.resolver.global_common_module_exports(self.db)
            {
                map.entry(method_name.as_str().fold_lower()).or_insert(method_id);
            }
            map
        })
    }
}
