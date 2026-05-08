//! Generic temp-resource open-set dataflow.
//!
//! Tracks resources that have been *opened* but not yet *closed* on each
//! reachable path. The two BSL diagnostics that consume this analysis —
//! `MissingTempStorageDeletion` and `MissingTemporaryFileDeletion` — share
//! the same control-flow shape (a `Get*` call opens a resource keyed by
//! some identifier, a later `Delete*` / move call closes the same key) but
//! disagree on what the *key* is. Storage's key is the structural form of
//! the address-argument expression (`Path`, `Field`, `Index`); File's key
//! is the LHS variable name a `GetTempFileName()` result was assigned to.
//! The dataflow framework is generic over `R: Clone + Hash + Eq` and the
//! caller plugs a [`ResourceProvider<R>`] adapter that classifies a
//! single expression into an [`ResourceEvent`] on a per-handler basis.
//!
//! ## Lattice and transfer
//!
//! [`OpenSet<R>`] is the abstract domain — a hash map from each open
//! resource to the set of *opening expression* `ExprIdx`s on the path so
//! far. Site-tracking is keyed on `ExprIdx` (not `StmtId`) because
//! `Get*` / `Delete*` calls can also live inside vertex-condition
//! expressions (e.g. `Если ПолучитьИзВременногоХранилища(адрес) =
//! Неопределено Тогда`), which the framework processes via
//! [`Transfer::transfer_expr`] rather than `transfer_stmt`. The
//! `ExprIdx` resolves through `BodySourceMap::expr_range` regardless of
//! whether the expression sits inside a basic-block statement or on a
//! `Conditional` / `WhileLoop` / `ForLoop` vertex's condition.
//!
//! Transfer order on a single statement / vertex-expression is
//! "open-first, close-after" via depth-first pre-order walk over the
//! expression subtree (see [`walk_stmt_subexprs`] / [`walk_expr`]). A
//! provider that legitimately opens and closes within the same
//! statement (rare in practice) gets the open recorded first; the
//! follow-up close then clears it. Open additively merges new sites
//! into the existing entry (does NOT overwrite), so a second `Get` of
//! the same resource records both Get sites; the next `Delete` clears
//! the whole entry, matching the canonical "delete kills all open
//! Gets" semantics of the existing AST-based handlers.
//!
//! Join at merge points unions the maps key-by-key; for keys present in
//! both predecessors the inner site sets are unioned. This is **MAY**
//! semantics — a resource is considered "open at exit" if any path
//! reaches exit with the resource still open. The dead-fallthrough
//! successor of an unconditional jump (`Return` / `Raise` / `Goto` /
//! `Break` / `Continue`) carries the [`cfg::CfgEdgeType::AdjacentCode`]
//! marker; [`Transfer::transfer_edge`] drops that to bottom so paths
//! that never actually reach the next block do not seed the exit state
//! with phantom "leaks".
//!
//! ## Diagnostic emission
//!
//! After [`analyze_open_resources`] solves the dataflow, callers query
//! [`OpenResourcesResult::open_at_exit`] — every `(resource,
//! opening-expr)` pair that survives to the exit block on at least one
//! path. A diagnostic should be emitted at each `ExprIdx` in those
//! sets; the `R` is purely an internal grouping key. Grouping by `R`
//! instead of just collecting sites preserves the existing AST
//! handlers' contract that a `Delete*` of resource `R` cancels every
//! prior `Get*` of `R`, including ones from sibling branches that
//! would otherwise leak across a merge point — see
//! [`OpenSet::join_in_place`] for the join law that backs this.

use cfg::{CfgEdgeType, ControlFlowGraph};
use hir_def::body::Body;
use hir_def::hir::{Expr, ExprIdx, Stmt};
use hir_def::{ExprId, IdConversion, StmtId};
use la_arena::RawIdx;
use rustc_hash::{FxHashMap, FxHashSet};
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::{DataflowResult, DataflowSolver, Direction, Lattice, Transfer, DEFAULT_MAX_ITERATIONS};

/// Classification of a single expression.
///
/// [`ResourceProvider::classify`] returns `Some(Open(r))` if the
/// expression is the resource-opening site for `r` (e.g. a
/// `ПолучитьИзВременногоХранилища(arg)` call) or `Some(Close(r))` if
/// it is the closing site. Expressions that are neither return
/// `None` and are skipped by the transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceEvent<R>
where
    R: Clone + Eq + Hash,
{
    Open(R),
    Close(R),
}

/// Provider mapping individual expressions to resource events.
///
/// Implementations live in handler crates (or `hir-ty` if shared by
/// multiple handlers); this trait carries no BSL-specific knowledge so
/// the dataflow crate stays generic over the resource identifier `R`.
///
/// ## Why expression-level classification, not statement-level
///
/// The CFG decomposes branch / loop conditions into separate vertex
/// kinds whose carrier is an [`ExprId`], not a [`StmtId`] — see
/// `cfg::CfgVertex::Conditional`, `WhileLoop`, `ForLoop`,
/// `ForEachLoop`. The framework calls
/// [`Transfer::transfer_expr`] rather than `transfer_stmt` for those,
/// and a provider keyed on `StmtId` would silently miss every `Get*`
/// / `Delete*` that lives inside a condition (e.g.
/// `Если ПолучитьИзВременногоХранилища(адрес) <> Неопределено Тогда`).
/// Classifying per-expression and walking the subtree depth-first
/// catches every nested call at any control-flow position.
///
/// ## Contract
///
/// - `classify` is called on every sub-expression of every statement
///   in every basic block, plus every sub-expression of every
///   vertex-condition expression. Implementations must be
///   deterministic and side-effect-free; the same `expr_idx` may be
///   re-classified many times during fixed-point iteration.
/// - The closure-style walk preserves source order, so a provider
///   that opens-then-closes within the same expression (rare) gets
///   the open recorded before the close.
pub trait ResourceProvider<R>
where
    R: Clone + Eq + Hash,
{
    fn classify(&self, body: &Body, expr_idx: ExprIdx) -> Option<ResourceEvent<R>>;
}

/// Open-set lattice element.
///
/// See module-level docs for the site-tracking rationale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSet<R>
where
    R: Clone + Eq + Hash,
{
    open: FxHashMap<R, FxHashSet<ExprIdx>>,
}

impl<R> Default for OpenSet<R>
where
    R: Clone + Eq + Hash,
{
    fn default() -> Self {
        Self { open: FxHashMap::default() }
    }
}

impl<R> OpenSet<R>
where
    R: Clone + Eq + Hash,
{
    /// Empty open-set — the lattice's bottom.
    pub fn bottom() -> Self {
        Self::default()
    }

    /// `true` iff no resources are currently open.
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// Borrow the underlying `(resource, opening-expr-sites)` map.
    /// Consumers typically read this from the exit block to drive
    /// diagnostic emission — see [`OpenResourcesResult::open_at_exit`].
    pub fn as_map(&self) -> &FxHashMap<R, FxHashSet<ExprIdx>> {
        &self.open
    }

    fn open_at(&mut self, r: R, site: ExprIdx) {
        self.open.entry(r).or_default().insert(site);
    }

    fn close(&mut self, r: &R) {
        self.open.remove(r);
    }
}

impl<R> Lattice for OpenSet<R>
where
    R: Clone + Eq + Hash,
{
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();
        result.join_in_place(other);
        result
    }

    fn join_in_place(&mut self, other: &Self) {
        for (key, sites) in &other.open {
            let entry = self.open.entry(key.clone()).or_default();
            for site in sites {
                entry.insert(*site);
            }
        }
    }
}

/// Transfer adapter that runs a [`ResourceProvider<R>`] over the
/// solver's per-statement and per-expression hooks.
struct ResourceTransfer<P, R>
where
    R: Clone + Eq + Hash,
    P: ResourceProvider<R>,
{
    provider: P,
    _r: PhantomData<R>,
}

impl<P, R> ResourceTransfer<P, R>
where
    R: Clone + Eq + Hash,
    P: ResourceProvider<R>,
{
    fn apply(&self, body: &Body, expr_idx: ExprIdx, state: &mut OpenSet<R>) {
        match self.provider.classify(body, expr_idx) {
            Some(ResourceEvent::Open(r)) => state.open_at(r, expr_idx),
            Some(ResourceEvent::Close(r)) => state.close(&r),
            None => {}
        }
    }
}

impl<P, R> Transfer<OpenSet<R>> for ResourceTransfer<P, R>
where
    R: Clone + Eq + Hash,
    P: ResourceProvider<R>,
{
    fn transfer_stmt(&self, stmt_id: RawIdx, state: &OpenSet<R>, body: &Body) -> OpenSet<R> {
        let mut next = state.clone();
        let stmt = StmtId::from_raw(stmt_id);
        walk_stmt_subexprs(body, stmt, &mut |expr_idx| {
            self.apply(body, expr_idx, &mut next);
        });
        next
    }

    fn transfer_expr(&self, expr_id: ExprId, state: &OpenSet<R>, body: &Body) -> OpenSet<R> {
        let mut next = state.clone();
        walk_expr(body, expr_id.to_idx(), &mut |expr_idx| {
            self.apply(body, expr_idx, &mut next);
        });
        next
    }

    fn transfer_edge(&self, edge_kind: CfgEdgeType, state: &OpenSet<R>) -> OpenSet<R> {
        // Drop the dead-fallthrough successor of unconditional jumps
        // (Return / Raise / Goto / Break / Continue) so paths that never
        // actually reach the next block do not seed the exit state with
        // resources that look like leaks. Mirrors the same edge filter
        // used by `path_terminates::PathTerminatesTransfer::transfer_edge`.
        if matches!(edge_kind, CfgEdgeType::AdjacentCode) {
            OpenSet::bottom()
        } else {
            state.clone()
        }
    }
}

/// Walk every `ExprIdx` carried by `stmt_id`, depth-first pre-order.
/// Covers every `Stmt` variant whose payload includes an expression
/// (block-bodied statements like `If` / `While` / `For` / `Try`
/// don't appear here — the CFG already decomposed their conditions
/// into separate vertex-condition expressions, which the framework
/// processes via [`Transfer::transfer_expr`]). Source order is
/// preserved so a provider that opens-then-closes within the same
/// statement gets the open recorded first.
fn walk_stmt_subexprs<F: FnMut(ExprIdx)>(body: &Body, stmt_id: StmtId, f: &mut F) {
    match body.stmt(stmt_id) {
        Stmt::Expr(e) => walk_expr(body, *e, f),
        Stmt::Assign { target, value } => {
            walk_expr(body, *target, f);
            walk_expr(body, *value, f);
        }
        Stmt::Return { value: Some(e) } => walk_expr(body, *e, f),
        Stmt::Raise { value: Some(e) } => walk_expr(body, *e, f),
        Stmt::Execute { expr } => walk_expr(body, *expr, f),
        Stmt::AddHandler { event, handler }
        | Stmt::RemoveHandler { event, handler } => {
            walk_expr(body, *event, f);
            walk_expr(body, *handler, f);
        }
        // `VarDecl` carries `BindingIdx`s, not `ExprIdx`s — default
        // values live on the bindings and are walked separately at
        // the binding layer when needed (no current consumer).
        Stmt::VarDecl { .. }
        // Block-bodied statements have already been decomposed by
        // the CFG: their conditions become vertex-condition exprs
        // (handled by `transfer_expr`), and their bodies become
        // separate basic blocks (each statement re-visited via
        // `transfer_stmt`). Re-walking them here would double-count.
        | Stmt::If(_)
        | Stmt::PreprocIf(_)
        | Stmt::While { .. }
        | Stmt::For { .. }
        | Stmt::ForEach { .. }
        | Stmt::Try { .. }
        // Bare-payload statements without expressions.
        | Stmt::Return { value: None }
        | Stmt::Raise { value: None }
        | Stmt::Break
        | Stmt::Continue
        | Stmt::Goto(_)
        | Stmt::Label(_) => {}
    }
}

/// Walk an expression and every sub-expression, depth-first
/// pre-order. The visitor sees the outer expression first, then
/// each child in source order. Every variant of [`Expr`] that
/// transitively carries an `ExprIdx` is covered; leaves (`Path`,
/// `Literal`, `QualifiedPath`, `Missing`) terminate the walk.
fn walk_expr<F: FnMut(ExprIdx)>(body: &Body, expr_idx: ExprIdx, f: &mut F) {
    f(expr_idx);
    match body.expr_idx(expr_idx) {
        Expr::Call { callee, args } => {
            walk_expr(body, *callee, f);
            for &arg in args.iter() {
                walk_expr(body, arg, f);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            walk_expr(body, *receiver, f);
            for &arg in args.iter() {
                walk_expr(body, arg, f);
            }
        }
        Expr::Field { base, .. } => walk_expr(body, *base, f),
        Expr::Index { base, index } => {
            walk_expr(body, *base, f);
            walk_expr(body, *index, f);
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            walk_expr(body, *lhs, f);
            walk_expr(body, *rhs, f);
        }
        Expr::UnaryOp { expr, .. } => walk_expr(body, *expr, f),
        Expr::Ternary { condition, then_expr, else_expr } => {
            walk_expr(body, *condition, f);
            walk_expr(body, *then_expr, f);
            walk_expr(body, *else_expr, f);
        }
        Expr::New { args, .. } => {
            for &arg in args.iter() {
                walk_expr(body, arg, f);
            }
        }
        Expr::Array(items) => {
            for &item in items.iter() {
                walk_expr(body, item, f);
            }
        }
        Expr::Await { expr } => walk_expr(body, *expr, f),
        Expr::Path(_) | Expr::Literal(_) | Expr::QualifiedPath(_) | Expr::Missing => {}
    }
}

/// Resolved open-resources result for one body.
///
/// Pin every `(resource, opening-expr-site)` pair that survives to
/// the exit block on at least one path; consumers iterate the inner
/// site sets to emit one diagnostic per Get expression.
pub struct OpenResourcesResult<R>
where
    R: Clone + Eq + Hash,
{
    inner: DataflowResult<OpenSet<R>>,
    exit_block: petgraph::graph::NodeIndex,
    empty_map: FxHashMap<R, FxHashSet<ExprIdx>>,
}

impl<R> OpenResourcesResult<R>
where
    R: Clone + Eq + Hash,
{
    /// Resources still open on at least one path at the exit block.
    /// Empty when every path closed every resource it opened, **and**
    /// when the exit block has no OUT entry — which only happens on
    /// degenerate CFGs where the exit is unreachable. The caller does
    /// not need to distinguish those two zero-leak shapes.
    pub fn open_at_exit(&self) -> &FxHashMap<R, FxHashSet<ExprIdx>> {
        self.inner.block_out(self.exit_block).map(|set| set.as_map()).unwrap_or(&self.empty_map)
    }
}

/// Run the open-resources analysis on a body.
///
/// Takes `body` and `cfg` by reference (clones internally so the
/// solver can own its inputs) for parity with
/// [`crate::path_terminates::analyze_path_terminates`] and the rest
/// of the dataflow public surface — handlers typically already hold
/// `Arc<ModuleBodies>` / `Arc<ControlFlowGraph>` and would otherwise
/// have to clone explicitly at every call site.
///
/// Returns `None` only if the framework's solver fails to converge
/// within [`DEFAULT_MAX_ITERATIONS`] (pathological CFG); on success
/// every block has its IN/OUT computed and consumers can read the
/// exit OUT via [`OpenResourcesResult::open_at_exit`].
pub fn analyze_open_resources<P, R>(
    body: &Body,
    cfg: &ControlFlowGraph,
    provider: P,
) -> Option<OpenResourcesResult<R>>
where
    R: Clone + Eq + Hash,
    P: ResourceProvider<R>,
{
    let exit_block = cfg.exit_point();
    let transfer = ResourceTransfer { provider, _r: PhantomData };
    let mut solver = DataflowSolver::new(Arc::new(cfg.clone()), body.clone(), transfer);
    solver.set_direction(Direction::Forward);
    solver.set_bottom_factory(OpenSet::<R>::bottom);
    solver.set_max_iterations(DEFAULT_MAX_ITERATIONS);
    let inner = solver.solve()?;
    Some(OpenResourcesResult { inner, exit_block, empty_map: FxHashMap::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_expr(n: u32) -> ExprIdx {
        ExprIdx::from_raw(RawIdx::from_u32(n))
    }

    #[test]
    fn open_set_join_unions_inner_site_sets() {
        let s1 = raw_expr(1);
        let s2 = raw_expr(2);
        let s3 = raw_expr(3);

        let mut a: OpenSet<&str> = OpenSet::bottom();
        a.open_at("X", s1);

        let mut b: OpenSet<&str> = OpenSet::bottom();
        b.open_at("X", s2);
        b.open_at("Y", s3);

        let joined = a.join(&b);
        let map = joined.as_map();
        assert_eq!(map.len(), 2);
        let x = map.get("X").expect("X must survive join");
        assert!(x.contains(&s1));
        assert!(x.contains(&s2));
        let y = map.get("Y").expect("Y must survive join (only in b)");
        assert!(y.contains(&s3));
    }

    #[test]
    fn open_set_close_clears_all_sites() {
        let s1 = raw_expr(1);
        let s2 = raw_expr(2);

        let mut state: OpenSet<&str> = OpenSet::bottom();
        state.open_at("X", s1);
        state.open_at("X", s2);
        assert_eq!(state.as_map().get("X").unwrap().len(), 2);

        state.close(&"X");
        assert!(state.is_empty(), "close must drop the entry, not just shrink it");
    }

    #[test]
    fn open_set_lattice_idempotence_and_commutativity() {
        let s1 = raw_expr(1);
        let s2 = raw_expr(2);

        let mut a: OpenSet<&str> = OpenSet::bottom();
        a.open_at("X", s1);
        let mut b: OpenSet<&str> = OpenSet::bottom();
        b.open_at("X", s2);

        // Idempotence: a.join(a) == a.
        assert_eq!(a.join(&a), a);
        // Commutativity: a.join(b) == b.join(a).
        assert_eq!(a.join(&b), b.join(&a));
        // Bottom identity: bottom.join(a) == a.
        assert_eq!(OpenSet::<&str>::bottom().join(&a), a);
    }

    #[test]
    fn open_set_join_in_place_is_consistent_with_join() {
        let s1 = raw_expr(1);
        let s2 = raw_expr(2);
        let s3 = raw_expr(3);

        let mut a: OpenSet<&str> = OpenSet::bottom();
        a.open_at("X", s1);
        a.open_at("Y", s2);

        let mut b: OpenSet<&str> = OpenSet::bottom();
        b.open_at("Y", s3);
        b.open_at("Z", s1);

        let merged = a.join(&b);
        let mut in_place = a.clone();
        in_place.join_in_place(&b);
        assert_eq!(in_place, merged);
    }
}
