use super::{Lattice, Transfer};
use cfg_types::IdConversion;
use fixedbitset::FixedBitSet;
use hir_def::body::Body;
use hir_def::hir::{Expr, Stmt};
use hir_def::{BindingId, ExprId};
use la_arena::RawIdx;
use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use std::sync::Arc;
use stdx::case::CaseExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableIndex {
    name_to_idx: FxHashMap<SmolStr, usize>,
    idx_to_name: Vec<SmolStr>,
    expr_idx_cache: FxHashMap<ExprId, usize>,
    binding_idx_cache: FxHashMap<hir_def::BindingId, usize>,
}

impl VariableIndex {
    pub fn from_body(body: &Body) -> Arc<Self> {
        let mut name_to_idx = FxHashMap::default();
        let mut idx_to_name = Vec::new();

        for (_local_id, binding) in body.bindings_iter() {
            let lowercase: SmolStr = binding.name.as_str().fold_lower().into();

            if !name_to_idx.contains_key(&lowercase) {
                let idx = idx_to_name.len();
                name_to_idx.insert(lowercase.clone(), idx);
                idx_to_name.push(lowercase);
            }
        }

        fn collect_implicit_vars(
            stmts: &[hir_def::hir::StmtIdx],
            body: &Body,
            name_to_idx: &mut FxHashMap<SmolStr, usize>,
            idx_to_name: &mut Vec<SmolStr>,
        ) {
            for &stmt_id in stmts {
                match body.stmt_idx(stmt_id) {
                    Stmt::Assign { target, .. } => {
                        if let Expr::Path(name) = body.expr_idx(*target) {
                            let lowercase: SmolStr = name.as_str().fold_lower().into();
                            if !name_to_idx.contains_key(&lowercase) {
                                let idx = idx_to_name.len();
                                name_to_idx.insert(lowercase.clone(), idx);
                                idx_to_name.push(lowercase);
                            }
                        }
                    }
                    Stmt::If(if_stmt) => {
                        collect_implicit_vars(&if_stmt.then_branch, body, name_to_idx, idx_to_name);
                        for (_cond, stmts) in if_stmt.elsif_branches.iter() {
                            collect_implicit_vars(stmts, body, name_to_idx, idx_to_name);
                        }
                        if let Some(ref else_stmts) = if_stmt.else_branch {
                            collect_implicit_vars(else_stmts, body, name_to_idx, idx_to_name);
                        }
                    }
                    Stmt::While { body: loop_body, .. } => {
                        collect_implicit_vars(loop_body, body, name_to_idx, idx_to_name);
                    }
                    Stmt::For { body: loop_body, .. } => {
                        collect_implicit_vars(loop_body, body, name_to_idx, idx_to_name);
                    }
                    Stmt::ForEach { body: loop_body, .. } => {
                        collect_implicit_vars(loop_body, body, name_to_idx, idx_to_name);
                    }
                    Stmt::Try { body: try_body, except, .. } => {
                        collect_implicit_vars(try_body, body, name_to_idx, idx_to_name);
                        collect_implicit_vars(except, body, name_to_idx, idx_to_name);
                    }
                    _ => {}
                }
            }
        }

        collect_implicit_vars(body.body_stmts_typed(), body, &mut name_to_idx, &mut idx_to_name);

        let mut expr_idx_cache = FxHashMap::default();
        for (expr_id, expr) in body.exprs_iter() {
            if let Expr::Path(name) = expr {
                let lowercase: SmolStr = name.as_str().fold_lower().into();
                if let Some(&idx) = name_to_idx.get(&lowercase) {
                    expr_idx_cache.insert(expr_id, idx);
                }
            }
        }

        let mut binding_idx_cache = FxHashMap::default();
        for (binding_id, binding) in body.bindings_iter() {
            let lowercase: SmolStr = binding.name.as_str().fold_lower().into();
            if let Some(&idx) = name_to_idx.get(&lowercase) {
                binding_idx_cache.insert(binding_id, idx);
            }
        }

        Arc::new(Self { name_to_idx, idx_to_name, expr_idx_cache, binding_idx_cache })
    }

    pub fn get_index(&self, var_name: &str) -> Option<usize> {
        let lowercase: SmolStr = var_name.fold_lower().into();
        self.name_to_idx.get(&lowercase).copied()
    }

    #[inline]
    pub fn get_index_by_smolstr(&self, lowercase_name: &SmolStr) -> Option<usize> {
        self.name_to_idx.get(lowercase_name).copied()
    }

    #[inline]
    pub fn name_to_idx_map(&self) -> &FxHashMap<SmolStr, usize> {
        &self.name_to_idx
    }

    #[inline]
    pub fn get_index_by_expr(&self, expr_id: ExprId) -> Option<usize> {
        self.expr_idx_cache.get(&expr_id).copied()
    }

    #[inline]
    pub fn get_index_by_binding(&self, binding_id: hir_def::BindingId) -> Option<usize> {
        self.binding_idx_cache.get(&binding_id).copied()
    }

    pub fn size(&self) -> usize {
        self.idx_to_name.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Liveness {
    live_vars: FixedBitSet,
    var_index: Arc<VariableIndex>,
}

impl Liveness {
    pub fn new(var_index: Arc<VariableIndex>) -> Self {
        Self { live_vars: FixedBitSet::with_capacity(var_index.size()), var_index }
    }

    pub fn is_live(&self, var_name: &str) -> bool {
        self.var_index.get_index(var_name).map(|idx| self.live_vars.contains(idx)).unwrap_or(false)
    }

    /// Approximate live heap bytes for Salsa's `memory_usage` report: the
    /// `live_vars` bitset words (`ceil(bits / 8)`). The shared `Arc<VariableIndex>`
    /// is counted as the pointer only — its payload is shared across every program
    /// point of the method — so it is omitted here.
    pub fn estimated_heap(&self) -> usize {
        self.live_vars.len().div_ceil(8)
    }

    #[inline]
    pub fn is_live_by_idx(&self, idx: usize) -> bool {
        self.live_vars.contains(idx)
    }

    pub fn insert(&mut self, var_name: &str) {
        if let Some(idx) = self.var_index.get_index(var_name) {
            self.live_vars.insert(idx);
        }
    }

    #[inline]
    pub fn insert_by_idx(&mut self, idx: usize) {
        self.live_vars.insert(idx);
    }

    pub fn remove(&mut self, var_name: &str) {
        if let Some(idx) = self.var_index.get_index(var_name) {
            self.live_vars.set(idx, false);
        }
    }

    #[inline]
    pub fn remove_by_idx(&mut self, idx: usize) {
        self.live_vars.set(idx, false);
    }

    #[inline]
    pub fn live_vars(&self) -> &FixedBitSet {
        &self.live_vars
    }

    #[inline]
    pub fn live_vars_mut(&mut self) -> &mut FixedBitSet {
        &mut self.live_vars
    }

    pub fn len(&self) -> usize {
        self.live_vars.count_ones(..)
    }

    pub fn is_empty(&self) -> bool {
        self.live_vars.is_clear()
    }

    pub fn var_index(&self) -> &Arc<VariableIndex> {
        &self.var_index
    }
}

impl Lattice for Liveness {
    fn join(&self, other: &Self) -> Self {
        debug_assert!(
            Arc::ptr_eq(&self.var_index, &other.var_index),
            "Cannot join liveness sets from different methods"
        );

        let mut result = self.live_vars.clone();
        result.union_with(&other.live_vars);

        Self { live_vars: result, var_index: self.var_index.clone() }
    }

    fn join_in_place(&mut self, other: &Self) {
        debug_assert!(
            Arc::ptr_eq(&self.var_index, &other.var_index),
            "Cannot join liveness sets from different methods"
        );

        self.live_vars.union_with(&other.live_vars);
    }
}

pub struct LivenessTransfer;

impl Transfer<Liveness> for LivenessTransfer {
    fn transfer_stmt(&self, stmt_id: RawIdx, state: &Liveness, body: &Body) -> Liveness {
        use hir_def::StmtId;

        let mut in_state = state.clone();

        let stmt = body.stmt(StmtId::from_raw(stmt_id));

        match stmt {
            Stmt::VarDecl { bindings } => {
                for &binding_id in bindings.iter() {
                    if let Some(idx) =
                        in_state.var_index().get_index_by_binding(BindingId::from_idx(binding_id))
                    {
                        in_state.remove_by_idx(idx);
                    }
                }
            }

            Stmt::Assign { target, value } => {
                let target_id = ExprId::from_idx(*target);
                if matches!(body.expr(target_id), Expr::Path(_)) {
                    if let Some(idx) = in_state.var_index().get_index_by_expr(target_id) {
                        in_state.remove_by_idx(idx);
                    }
                } else {
                    collect_expr_vars(target_id, body, &mut in_state);
                }

                collect_expr_vars(ExprId::from_idx(*value), body, &mut in_state);
            }

            Stmt::Expr(expr_id) => {
                collect_expr_vars(ExprId::from_idx(*expr_id), body, &mut in_state);
            }

            Stmt::Return { value } => {
                if let Some(expr_id) = value {
                    collect_expr_vars(ExprId::from_idx(*expr_id), body, &mut in_state);
                }
            }

            Stmt::Raise { value } => {
                if let Some(expr_id) = value {
                    collect_expr_vars(ExprId::from_idx(*expr_id), body, &mut in_state);
                }
            }

            Stmt::If(if_stmt) => {
                collect_expr_vars(ExprId::from_idx(if_stmt.condition), body, &mut in_state);
            }

            Stmt::While { condition, .. } => {
                collect_expr_vars(ExprId::from_idx(*condition), body, &mut in_state);
            }

            Stmt::For { var, from, to, .. } => {
                collect_expr_vars(ExprId::from_idx(*from), body, &mut in_state);
                collect_expr_vars(ExprId::from_idx(*to), body, &mut in_state);

                if let Some(idx) =
                    in_state.var_index().get_index_by_binding(BindingId::from_idx(*var))
                {
                    in_state.remove_by_idx(idx);
                }
            }

            Stmt::ForEach { var, collection, .. } => {
                collect_expr_vars(ExprId::from_idx(*collection), body, &mut in_state);

                if let Some(idx) =
                    in_state.var_index().get_index_by_binding(BindingId::from_idx(*var))
                {
                    in_state.remove_by_idx(idx);
                }
            }

            Stmt::Try { .. } => {}

            Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => {}

            Stmt::Execute { expr } => {
                collect_expr_vars(ExprId::from_idx(*expr), body, &mut in_state);
            }

            Stmt::AddHandler { .. } | Stmt::RemoveHandler { .. } => {}

            Stmt::PreprocIf(_) => {}
        }

        in_state
    }

    fn transfer_expr(&self, expr_id: hir_def::ExprId, state: &Liveness, body: &Body) -> Liveness {
        let mut in_state = state.clone();
        collect_expr_vars(expr_id, body, &mut in_state);
        in_state
    }

    fn transfer_stmt_in_place(&self, stmt_id: RawIdx, state: &mut Liveness, body: &Body) {
        use hir_def::StmtId;

        let stmt = body.stmt(StmtId::from_raw(stmt_id));

        match stmt {
            Stmt::VarDecl { bindings } => {
                for &binding_id in bindings.iter() {
                    if let Some(idx) =
                        state.var_index().get_index_by_binding(BindingId::from_idx(binding_id))
                    {
                        state.remove_by_idx(idx);
                    }
                }
            }

            Stmt::Assign { target, value } => {
                let target_id = ExprId::from_idx(*target);
                if matches!(body.expr(target_id), Expr::Path(_)) {
                    if let Some(idx) = state.var_index().get_index_by_expr(target_id) {
                        state.remove_by_idx(idx);
                    }
                } else {
                    collect_expr_vars(target_id, body, state);
                }
                collect_expr_vars(ExprId::from_idx(*value), body, state);
            }

            Stmt::Expr(expr_id) => {
                collect_expr_vars(ExprId::from_idx(*expr_id), body, state);
            }

            Stmt::Return { value } => {
                if let Some(expr_id) = value {
                    collect_expr_vars(ExprId::from_idx(*expr_id), body, state);
                }
            }

            Stmt::Raise { value } => {
                if let Some(expr_id) = value {
                    collect_expr_vars(ExprId::from_idx(*expr_id), body, state);
                }
            }

            Stmt::If(if_stmt) => {
                collect_expr_vars(ExprId::from_idx(if_stmt.condition), body, state);
            }

            Stmt::While { condition, .. } => {
                collect_expr_vars(ExprId::from_idx(*condition), body, state);
            }

            Stmt::For { var, from, to, .. } => {
                collect_expr_vars(ExprId::from_idx(*from), body, state);
                collect_expr_vars(ExprId::from_idx(*to), body, state);
                if let Some(idx) = state.var_index().get_index_by_binding(BindingId::from_idx(*var))
                {
                    state.remove_by_idx(idx);
                }
            }

            Stmt::ForEach { var, collection, .. } => {
                collect_expr_vars(ExprId::from_idx(*collection), body, state);
                if let Some(idx) = state.var_index().get_index_by_binding(BindingId::from_idx(*var))
                {
                    state.remove_by_idx(idx);
                }
            }

            Stmt::Try { .. } => {}

            Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => {}

            Stmt::Execute { expr } => {
                collect_expr_vars(ExprId::from_idx(*expr), body, state);
            }

            Stmt::AddHandler { .. } | Stmt::RemoveHandler { .. } => {}

            Stmt::PreprocIf(_) => {}
        }
    }

    fn transfer_expr_in_place(&self, expr_id: hir_def::ExprId, state: &mut Liveness, body: &Body) {
        collect_expr_vars(expr_id, body, state);
    }
}

fn collect_expr_vars(expr_id: ExprId, body: &Body, liveness: &mut Liveness) {
    if let Some(idx) = liveness.var_index().get_index_by_expr(expr_id) {
        liveness.insert_by_idx(idx);
        return;
    }

    let expr = body.expr(expr_id);

    match expr {
        Expr::Missing => {}

        Expr::Path(name) => {
            if let Some(idx) = liveness.var_index().get_index(name.as_str()) {
                liveness.insert_by_idx(idx);
            }
        }

        Expr::Literal(_) => {}

        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_expr_vars(ExprId::from_idx(*lhs), body, liveness);
            collect_expr_vars(ExprId::from_idx(*rhs), body, liveness);
        }

        Expr::UnaryOp { expr, .. } => {
            collect_expr_vars(ExprId::from_idx(*expr), body, liveness);
        }

        Expr::Call { callee, args } => {
            collect_expr_vars(ExprId::from_idx(*callee), body, liveness);
            for &arg_expr in args.iter() {
                collect_expr_vars(ExprId::from_idx(arg_expr), body, liveness);
            }
        }

        Expr::MethodCall { receiver, args, .. } => {
            collect_expr_vars(ExprId::from_idx(*receiver), body, liveness);
            for &arg_expr in args.iter() {
                collect_expr_vars(ExprId::from_idx(arg_expr), body, liveness);
            }
        }

        Expr::Field { base, .. } => {
            collect_expr_vars(ExprId::from_idx(*base), body, liveness);
        }

        Expr::Index { base, index } => {
            collect_expr_vars(ExprId::from_idx(*base), body, liveness);
            collect_expr_vars(ExprId::from_idx(*index), body, liveness);
        }

        Expr::Ternary { condition, then_expr, else_expr } => {
            collect_expr_vars(ExprId::from_idx(*condition), body, liveness);
            collect_expr_vars(ExprId::from_idx(*then_expr), body, liveness);
            collect_expr_vars(ExprId::from_idx(*else_expr), body, liveness);
        }

        Expr::New { args, .. } => {
            for &arg_expr in args.iter() {
                collect_expr_vars(ExprId::from_idx(arg_expr), body, liveness);
            }
        }

        Expr::Await { expr } => {
            collect_expr_vars(ExprId::from_idx(*expr), body, liveness);
        }

        Expr::QualifiedPath(_) => {}

        Expr::Array(elements) => {
            for &elem_expr in elements.iter() {
                collect_expr_vars(ExprId::from_idx(elem_expr), body, liveness);
            }
        }
    }
}

pub fn liveness_analysis_direct(
    body: &hir_def::Body,
    cfg: &cfg::ControlFlowGraph,
    var_index: std::sync::Arc<VariableIndex>,
    max_iterations: usize,
) -> Option<crate::DataflowResult<Liveness>> {
    let transfer = LivenessTransfer;
    let mut solver =
        crate::DataflowSolver::new(std::sync::Arc::new(cfg.clone()), body.clone(), transfer);
    solver.set_direction(crate::Direction::Backward);
    solver.set_max_iterations(max_iterations);
    solver.set_bottom_factory(|| Liveness::new(var_index.clone()));
    solver.solve()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleLiveness {
    results: rustc_hash::FxHashMap<u32, std::sync::Arc<crate::DataflowResult<Liveness>>>,
}

impl ModuleLiveness {
    pub fn new(
        results: rustc_hash::FxHashMap<u32, std::sync::Arc<crate::DataflowResult<Liveness>>>,
    ) -> Self {
        Self { results }
    }

    pub fn get(&self, local_id: u32) -> Option<&std::sync::Arc<crate::DataflowResult<Liveness>>> {
        self.results.get(&local_id)
    }

    pub fn len(&self) -> usize {
        self.results.len()
    }

    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// Approximate live heap bytes for Salsa's `memory_usage` report: the per-method
    /// results table plus each owned [`DataflowResult<Liveness>`]. `ModuleLiveness`
    /// is the owning store; the per-method `liveness_analysis` accessor query returns
    /// clones of these same `Arc`s and reports zero to avoid double counting.
    pub fn estimated_heap(&self) -> usize {
        let mut bytes = crate::map_table_bytes::<
            u32,
            std::sync::Arc<crate::DataflowResult<Liveness>>,
        >(self.results.len());
        for result in self.results.values() {
            bytes += liveness_result_heap(result);
        }
        bytes
    }
}

/// Heap of a single liveness [`DataflowResult`], summing per-block bitset words.
pub fn liveness_result_heap(result: &crate::DataflowResult<Liveness>) -> usize {
    result.estimated_heap_with(|l| l.estimated_heap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_var_index(var_names: &[&str]) -> Arc<VariableIndex> {
        let mut name_to_idx = FxHashMap::default();
        let mut idx_to_name = Vec::new();

        for &name in var_names {
            let lowercase: SmolStr = name.fold_lower().into();
            if !name_to_idx.contains_key(&lowercase) {
                let idx = idx_to_name.len();
                name_to_idx.insert(lowercase.clone(), idx);
                idx_to_name.push(lowercase);
            }
        }

        Arc::new(VariableIndex {
            name_to_idx,
            idx_to_name,
            expr_idx_cache: FxHashMap::default(),
            binding_idx_cache: FxHashMap::default(),
        })
    }

    #[test]
    fn test_liveness_insert() {
        let var_index = create_var_index(&["Переменная"]);
        let mut liveness = Liveness::new(var_index);
        liveness.insert("Переменная");
        assert!(liveness.is_live("Переменная"));
        assert!(liveness.is_live("переменная"));
    }

    #[test]
    fn test_liveness_join() {
        let var_index = create_var_index(&["X", "Y", "Z"]);

        let mut a = Liveness::new(var_index.clone());
        a.insert("X");
        a.insert("Y");

        let mut b = Liveness::new(var_index);
        b.insert("Y");
        b.insert("Z");

        let joined = a.join(&b);
        assert!(joined.is_live("X"));
        assert!(joined.is_live("Y"));
        assert!(joined.is_live("Z"));
    }

    #[test]
    fn test_liveness_join_idempotent() {
        let var_index = create_var_index(&["X"]);
        let mut a = Liveness::new(var_index);
        a.insert("X");
        let joined = a.join(&a);
        assert_eq!(joined, a);
    }

    #[test]
    fn test_liveness_join_commutative() {
        let var_index = create_var_index(&["X", "Y"]);
        let mut a = Liveness::new(var_index.clone());
        a.insert("X");
        let mut b = Liveness::new(var_index);
        b.insert("Y");
        assert_eq!(a.join(&b), b.join(&a));
    }
}
