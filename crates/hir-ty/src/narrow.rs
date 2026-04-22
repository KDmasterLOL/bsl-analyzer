//! Type-narrowing guard recognition — pure syntactic layer.
//!
//! Recognizes the ADR-01 MUST-grammar guard shapes inside an `Expr`
//! tree and returns a [`Guard`] describing the effect the guard has on
//! the true / false successor of a conditional. Consumed by
//! [`NarrowingAnalysis`](crate::narrow) (Task 6.2) through the
//! branch-aware [`dataflow::Transfer::transfer_edge`] hook added in
//! Task 6.0.
//!
//! # Scope (ADR-01 MUST-grammar)
//!
//! - `ТипЗнч(X) = Тип("Строка")` — narrows `X` to the string-named
//!   platform type on the true branch; on the false branch, `Union \
//!   Ty` via [`ty_difference`] (Task 6.3).
//! - `X = Неопределено` / `X <> Неопределено` — flips between
//!   `Ty::Undefined` and its `Union \ Undefined` complement.
//! - `ЗначениеЗаполнено(X)` — recognised syntactically, but narrowing
//!   is **not yet implemented**: both branches produce `Ty::Unknown`
//!   and leave the overlay untouched (see
//!   [`NarrowingTransfer::apply_guard`]). Precise narrowing needs
//!   `Union \ {Undefined, Null, "", 0, …}`, which requires value-
//!   level reasoning — a follow-up after Task 6.3.
//! - Symmetric orientations — `Тип("…") = ТипЗнч(X)`,
//!   `Неопределено <> X` — are accepted verbatim (BSL's `=` is not
//!   orientation-sensitive and authors routinely flip the sides).
//!
//! # Deferred (ADR-01 Q1 — not this task)
//!
//! - `ИЛИ`-composition (`ТипЗнч(X) = Тип("Строка") ИЛИ ТипЗнч(X) = Тип("Число")`).
//! - Negation (`Не ТипЗнч(X) = Тип("…")` or `Не ЗначениеЗаполнено(X)`).
//! - Nested guards (`Если A И B Тогда`).
//! - `X Есть Справочник`.
//! - Narrowing on non-`Path(Name)` receivers (fields, indexes, qualified paths).
//!
//! Anything outside the MUST-grammar returns [`None`] — the caller is
//! expected to propagate state unchanged across the branch, which is
//! exactly the default [`dataflow::Transfer::transfer_edge`] behaviour.
//!
//! # Purity
//!
//! This module does not touch the Salsa database, [`InferenceContext`],
//! or [`TyLoweringContext`]. Guards are recognized purely from
//! `Expr` / `Literal` shape plus case-insensitive name matches on
//! builtin function names. Resolution of the `Тип("…")` string literal
//! to a concrete [`Ty`] lives in the caller (Task 6.2), so this module
//! stays trivially testable and reusable (e.g. diagnostics that only
//! need to know "this is a type guard" without caring about the
//! target [`Ty`]).
//!
//! [`InferenceContext`]: crate::InferenceContext
//! [`TyLoweringContext`]: crate::TyLoweringContext
//! [`Ty`]: hir_def::ty::Ty

use cfg::CfgEdgeType;
use dataflow::{Lattice, Transfer};
use hir_def::body::Body;
use hir_def::hir::{BinaryOp, Expr, Literal, Stmt};
use hir_def::ty::Ty;
use hir_def::{ExprId, IdConversion, Name};
use la_arena::{Idx, RawIdx};
use rustc_hash::FxHashMap;

type ExprIdx = Idx<Expr>;
type StmtIdx = Idx<Stmt>;

/// One recognized guard shape and the variable it constrains.
///
/// Each variant describes the guard's effect implicitly via its name;
/// the mapping `Guard → (Ty_true, Ty_false)` lives in the narrowing
/// analysis (Task 6.2) because it needs the pre-guard union to compute
/// the false-branch complement (Task 6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Guard {
    /// `ТипЗнч(X) = Тип("SomeType")` (or reversed orientation).
    ///
    /// On the true branch, `X` narrows to the type named by
    /// `type_name`; on the false branch, to `Union \ Ty` if `X` was a
    /// union, else `Unknown`.
    ///
    /// `type_name` is kept as the raw string literal — the caller maps
    /// it through [`hir_def::ty::Ty::from_type_name`].
    TypeCheck { var: Name, type_name: String },

    /// `X = Неопределено` (or `Неопределено = X`).
    ///
    /// True branch: `X` is `Ty::Undefined`.
    /// False branch: `X` is `Union \ Undefined` (or `Unknown`).
    IsUndefined { var: Name },

    /// `X <> Неопределено` (or `Неопределено <> X`).
    ///
    /// Mirror of [`Guard::IsUndefined`] — branches swap.
    IsNotUndefined { var: Name },

    /// `ЗначениеЗаполнено(X)`.
    ///
    /// Recognised purely so hover / analyses can see the guard shape,
    /// but no narrowing is applied: [`NarrowingTransfer::apply_guard`]
    /// treats this guard as a no-op on both branches (see the module
    /// header for why). Precise narrowing is a post-Task-6.3 follow-up.
    ValueFilled { var: Name },
}

/// Recognize an ADR-01 MUST-grammar guard at `expr`. Returns [`None`]
/// for any shape outside the grammar.
///
/// This is a pure syntactic match on `body.expr_idx(expr)` — no
/// inference context, no database. Case-insensitive comparisons use
/// [`Name::eq_ignore_case`] so Russian (`ТипЗнч`) and English
/// (`TypeOf`) spellings are treated uniformly.
pub fn recognize_guard(expr: ExprIdx, body: &Body) -> Option<Guard> {
    match body.expr_idx(expr) {
        Expr::BinaryOp { lhs, rhs, op } => match op {
            BinaryOp::Eq => recognize_eq_guard(*lhs, *rhs, body, /*negated=*/ false),
            BinaryOp::Neq => recognize_eq_guard(*lhs, *rhs, body, /*negated=*/ true),
            _ => None,
        },
        Expr::Call { callee, args } => {
            let callee_name = path_name(*callee, body)?;
            if callee_name.eq_ignore_case(&Name::new("ЗначениеЗаполнено"))
                || callee_name.eq_ignore_case(&Name::new("ValueIsFilled"))
                || callee_name.eq_ignore_case(&Name::new("ValueFilled"))
            {
                let var = single_path_arg(args, body)?;
                return Some(Guard::ValueFilled { var });
            }
            None
        }
        _ => None,
    }
}

/// Handle `=` / `<>` guards. When `negated` is true, true- and
/// false-branch semantics flip, which we express by swapping the
/// variant produced for `X = Неопределено` vs `X <> Неопределено`.
///
/// The `ТипЗнч(X) = Тип("…")` shape is *not* sensitive to `<>` in
/// ADR-01's MUST set — `Не ТипЗнч(X) = Тип("…")` is an explicit
/// non-goal, so `<>` on a type check falls through to [`None`] rather
/// than producing a "negated-type-check" variant we'd have to carry
/// through the analysis.
fn recognize_eq_guard(lhs: ExprIdx, rhs: ExprIdx, body: &Body, negated: bool) -> Option<Guard> {
    // Try each orientation: the BSL author can write either side first.
    if let Some(g) =
        try_type_check(lhs, rhs, body, negated).or_else(|| try_type_check(rhs, lhs, body, negated))
    {
        return Some(g);
    }

    if let Some(var) =
        try_undefined_compare(lhs, rhs, body).or_else(|| try_undefined_compare(rhs, lhs, body))
    {
        return Some(if negated {
            Guard::IsNotUndefined { var }
        } else {
            Guard::IsUndefined { var }
        });
    }

    None
}

/// Try to match `ТипЗнч(var) = Тип("…")` with `lhs = ТипЗнч(var)` and
/// `rhs = Тип("…")`. Returns `None` if either side's shape is wrong,
/// or if the comparison is `<>` (which is an explicit non-goal).
fn try_type_check(lhs: ExprIdx, rhs: ExprIdx, body: &Body, negated: bool) -> Option<Guard> {
    if negated {
        return None;
    }
    let var = type_of_arg(lhs, body)?;
    let type_name = type_literal_arg(rhs, body)?;
    Some(Guard::TypeCheck { var, type_name })
}

/// Return `Some(var)` if `(lhs, rhs)` is `(Path(var), Literal(Undefined))`.
fn try_undefined_compare(lhs: ExprIdx, rhs: ExprIdx, body: &Body) -> Option<Name> {
    let var = path_name(lhs, body)?;
    match body.expr_idx(rhs) {
        Expr::Literal(Literal::Undefined) => Some(var),
        _ => None,
    }
}

/// Extract the variable from `ТипЗнч(var)` / `TypeOf(var)` — accepts
/// exactly one `Path(Name)` argument. Anything else (non-`Path` arg,
/// zero or multiple args, wrong callee name) returns [`None`].
fn type_of_arg(expr: ExprIdx, body: &Body) -> Option<Name> {
    let (callee_name, args) = call_parts(expr, body)?;
    if !callee_name.eq_ignore_case(&Name::new("ТипЗнч"))
        && !callee_name.eq_ignore_case(&Name::new("TypeOf"))
    {
        return None;
    }
    single_path_arg(args, body)
}

/// Extract the string literal from `Тип("…")` / `Type("…")`. Returns
/// [`None`] if the callee is wrong, argument count is not 1, or the
/// argument is not a string literal.
fn type_literal_arg(expr: ExprIdx, body: &Body) -> Option<String> {
    let (callee_name, args) = call_parts(expr, body)?;
    if !callee_name.eq_ignore_case(&Name::new("Тип"))
        && !callee_name.eq_ignore_case(&Name::new("Type"))
    {
        return None;
    }
    if args.len() != 1 {
        return None;
    }
    match body.expr_idx(args[0]) {
        Expr::Literal(Literal::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Return `(callee_name, args)` for a `Call { callee: Path(name), .. }`.
fn call_parts(expr: ExprIdx, body: &Body) -> Option<(Name, &[ExprIdx])> {
    match body.expr_idx(expr) {
        Expr::Call { callee, args } => {
            let name = path_name(*callee, body)?;
            Some((name, args.as_ref()))
        }
        _ => None,
    }
}

/// If `expr` is `Path(name)`, return `name`. Covers the variable-reference
/// case for guard receivers; `QualifiedPath`, `Field`, etc. are out of
/// scope for ADR-01's MUST-grammar.
fn path_name(expr: ExprIdx, body: &Body) -> Option<Name> {
    match body.expr_idx(expr) {
        Expr::Path(name) => Some(name.clone()),
        _ => None,
    }
}

/// Require a single `Path(name)` argument. Anything else (zero args,
/// multiple args, non-path arg) returns [`None`].
fn single_path_arg(args: &[ExprIdx], body: &Body) -> Option<Name> {
    if args.len() != 1 {
        return None;
    }
    path_name(args[0], body)
}

// ===========================================================================
// Narrowing analysis (M4 Task 6.2)
// ===========================================================================
//
// The lattice is `Name → Ty` (a narrowing *overlay* over each body's base
// `var_types`) plus a "pending guard" slot. Task 6.0's branch-aware
// `transfer_edge` hook consumes the slot to refine state per outgoing
// `TrueBranch` / `FalseBranch` edge. Task 6.3 will replace the current
// `Ty::Unknown` placeholders in the false / `Union \ Ty` cases with a
// proper smart-constructor; Task 6.4 will strengthen `transfer_stmt` to
// record the rhs's type after a reassignment. The plumbing here is the
// load-bearing part — those follow-ups swap the implementations of
// `apply_guard` and the `Assign` branch without touching the lattice or
// the solver wiring.

/// Lattice value for narrowing: an overlay of narrowed types keyed by
/// name, plus a transient "pending guard" slot.
///
/// # Overlay contract
///
/// - An **absent** `Name` means "no narrowing applies at this program
///   point — consult the base type from [`InferenceContext::var_types`]".
/// - A **present** `Name → Ty` mapping is *authoritative*: the caller
///   must show `Ty` instead of the base union.
/// - `Ty::Unknown` is **never** a valid mapped value. The
///   [`NarrowingTransfer::apply_guard`] insertion gate filters it out
///   (via [`insert_if_informative`]), because a stored `Ty::Unknown`
///   would be indistinguishable from "fall through to base" but would
///   still clobber any prior, more-informative narrowing on merge.
///   Callers may therefore assume every value read from the overlay
///   carries strictly more information than the base type.
///
/// `pending_guard` flows from a `Conditional` vertex to its `TrueBranch`
/// / `FalseBranch` successors. The solver guarantees it is cleared on
/// every edge by [`NarrowingTransfer::transfer_edge`], so it never
/// leaks into a merge point where it could be mis-applied.
///
/// [`InferenceContext::var_types`]: crate::InferenceContext
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NarrowState {
    narrowed: FxHashMap<Name, Ty>,
    pending_guard: Option<Guard>,
}

impl NarrowState {
    /// Create an empty narrowing state (bottom of the lattice).
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up the narrowed type for `name`. Returns `None` when the
    /// overlay does not constrain `name` at this program point —
    /// callers fall back to the base `var_types` lookup. A returned
    /// `Some(ty)` is guaranteed by the overlay contract not to be
    /// `Ty::Unknown`.
    pub fn get(&self, name: &Name) -> Option<&Ty> {
        self.narrowed.get(name)
    }

    /// Number of narrowed bindings. Exists for tests and introspection.
    pub fn len(&self) -> usize {
        self.narrowed.len()
    }

    /// Whether the overlay carries any narrowing.
    pub fn is_empty(&self) -> bool {
        self.narrowed.is_empty()
    }
}

impl Lattice for NarrowState {
    /// Point-wise join of `narrowed`: for each `Name` present on both
    /// sides, combine via [`Ty::union`] (the M3 smart constructor —
    /// deduplicates and canonicalises). Names present on only one side
    /// propagate unchanged.
    ///
    /// **Pending guards do not survive a join.** A guard is only
    /// meaningful on the single edge that carries it; if it reached a
    /// merge point it would get applied to paths that did not go
    /// through the guard, which is unsound. The solver's
    /// [`Transfer::transfer_edge`] implementation clears the slot on
    /// every edge (see [`NarrowingTransfer::transfer_edge`]), and
    /// joining further forces the slot to `None` as a belt-and-braces
    /// second line of defence.
    fn join(&self, other: &Self) -> Self {
        let mut narrowed = self.narrowed.clone();
        for (k, v_other) in &other.narrowed {
            match narrowed.get(k) {
                Some(v_self) if v_self == v_other => {}
                Some(v_self) => {
                    let merged = Ty::union(vec![v_self.clone(), v_other.clone()]);
                    narrowed.insert(k.clone(), merged);
                }
                None => {
                    narrowed.insert(k.clone(), v_other.clone());
                }
            }
        }
        NarrowState { narrowed, pending_guard: None }
    }
}

/// Forward-direction dataflow transfer for narrowing.
///
/// - [`Transfer::transfer_stmt`] — on an `Assign { target: Path(x), .. }`
///   statement, drops `x` from the overlay so reassignment dissolves
///   any upstream narrowing (ADR-01 Q3 locality). Task 6.4 will extend
///   this to *record* the rhs's inferred type so the downstream block
///   still sees a narrowed `x`; for now "kill" is sufficient.
///
/// - [`Transfer::transfer_expr`] — called by the solver on each
///   `Conditional` vertex's condition (see
///   `DataflowSolver::transfer_block`). Runs [`recognize_guard`] and
///   stashes the result in `pending_guard`.
///
/// - [`Transfer::transfer_edge`] — consumes `pending_guard` on every
///   outgoing edge. On `TrueBranch` / `FalseBranch` of a `Conditional`
///   vertex, applies the guard via [`NarrowingTransfer::apply_guard`].
///   Every other edge kind falls through to identity. The slot is
///   unconditionally cleared regardless of the edge kind to keep the
///   transient guard from leaking past the branch it belongs to.
///
/// **`base_types`** — per-body map from `Name` to its pre-narrow type
/// (populated by the lowering layer from the body's inferred
/// `var_types`). Needed by [`NarrowingTransfer::apply_guard`] to
/// compute the false-branch complement of a type check via
/// [`ty_difference`]. When a name is absent or maps to a non-union
/// type, the complement falls back to `Ty::Unknown` (sound).
///
/// **Visibility:** `pub(crate)` — external consumers reach the
/// narrowing overlay through the (forthcoming Task 6.6) Salsa query,
/// which returns a `NarrowState` keyed by program point. There is no
/// reason for a downstream crate to construct its own solver, and
/// keeping the transfer crate-local lets future refinements (Task 6.4)
/// evolve its surface freely.
pub(crate) struct NarrowingTransfer {
    base_types: FxHashMap<Name, Ty>,
}

impl NarrowingTransfer {
    pub(crate) fn new(base_types: FxHashMap<Name, Ty>) -> Self {
        Self { base_types }
    }

    /// Apply a recognized guard to the overlay on the given branch.
    ///
    /// True-branch narrowing is always precise:
    /// - `Guard::TypeCheck { var, type_name }` → `Ty::from_type_name`.
    /// - `Guard::IsUndefined { var }` → `Ty::Undefined`.
    /// - `Guard::IsNotUndefined { var }` → `base \ Undefined`.
    ///
    /// False-branch narrowing uses [`ty_difference`] over `base_types`
    /// to subtract the matched type from the pre-narrow union.
    ///
    /// **Overlay invariant.** `Ty::Unknown` is never stored in
    /// `state.narrowed` — it would be indistinguishable, at every
    /// consumer site (hover, `Semantics::type_of_expr`, Task 6.6's
    /// Salsa query), from "I know nothing more than the base type,"
    /// which is exactly what a *missing* entry already means. Instead,
    /// a computed `Ty::Unknown` is treated as **no new information**:
    /// we leave the overlay alone so any prior (still-valid) narrowing
    /// survives the branch. That preserves information in the common
    /// nested-guard case where an outer guard narrowed the variable
    /// precisely and an inner guard lacks base-type context to refine
    /// further.
    ///
    /// `Guard::ValueFilled` has no Task 6.3 refinement: precise
    /// narrowing requires `Union \ {Undefined, Null, "", 0, …}` which
    /// needs value-level reasoning. It therefore always produces
    /// `Ty::Unknown` and so never perturbs the overlay — same no-op
    /// behaviour as any imprecise result.
    fn apply_guard(&self, state: &mut NarrowState, guard: &Guard, on_true: bool) {
        match guard {
            Guard::TypeCheck { var, type_name } => {
                let matched = Ty::from_type_name(type_name);
                let narrowed = if on_true { matched } else { self.complement_of(var, &matched) };
                insert_if_informative(state, var, narrowed);
            }
            Guard::IsUndefined { var } => {
                let narrowed =
                    if on_true { Ty::Undefined } else { self.complement_of(var, &Ty::Undefined) };
                insert_if_informative(state, var, narrowed);
            }
            Guard::IsNotUndefined { var } => {
                let narrowed =
                    if on_true { self.complement_of(var, &Ty::Undefined) } else { Ty::Undefined };
                insert_if_informative(state, var, narrowed);
            }
            Guard::ValueFilled { var: _ } => {
                // Task 6.3 computes no refinement for ValueFilled →
                // `Ty::Unknown` is the only available answer, which by
                // the overlay invariant means "no change".
            }
        }
    }

    /// Compute `base_types[var] \ matched` via [`ty_difference`].
    /// Returns `Ty::Unknown` when the variable has no recorded base type.
    fn complement_of(&self, var: &Name, matched: &Ty) -> Ty {
        match self.base_types.get(var) {
            Some(base) => ty_difference(base, matched),
            None => Ty::Unknown,
        }
    }
}

impl Transfer<NarrowState> for NarrowingTransfer {
    fn transfer_stmt(&self, stmt_id: RawIdx, state: &NarrowState, body: &Body) -> NarrowState {
        let mut new_state = state.clone();
        let stmt_idx: StmtIdx = Idx::from_raw(stmt_id);
        if let Stmt::Assign { target, value: _ } = body.stmt_idx(stmt_idx) {
            if let Expr::Path(name) = body.expr_idx(*target) {
                new_state.narrowed.remove(name);
            }
        }
        new_state
    }

    fn transfer_expr(&self, expr_id: ExprId, state: &NarrowState, body: &Body) -> NarrowState {
        let mut new_state = state.clone();
        new_state.pending_guard = recognize_guard(expr_id.to_idx(), body);
        new_state
    }

    fn transfer_edge(&self, edge_kind: CfgEdgeType, state: &NarrowState) -> NarrowState {
        let mut new_state = state.clone();
        let pending = new_state.pending_guard.take();
        match (edge_kind, pending) {
            (CfgEdgeType::TrueBranch, Some(g)) => self.apply_guard(&mut new_state, &g, true),
            (CfgEdgeType::FalseBranch, Some(g)) => self.apply_guard(&mut new_state, &g, false),
            _ => {}
        }
        new_state
    }
}

/// Insert `ty` into the overlay for `var` iff it carries new
/// information, i.e. it is not `Ty::Unknown`. See the
/// [`NarrowingTransfer::apply_guard`] docs for the overlay invariant.
/// Extracted as a free fn so every guard arm routes through the same
/// gate — future refinements (Task 6.4) plug in here.
fn insert_if_informative(state: &mut NarrowState, var: &Name, ty: Ty) {
    if !matches!(ty, Ty::Unknown) {
        state.narrowed.insert(var.clone(), ty);
    }
}

/// Set-difference on types: `base \ matched`.
///
/// Used to compute the false-branch narrowing of a type check. Rules:
///
/// - `base` is `Ty::Union(members)` → drop every occurrence of
///   `matched` from `members` (by structural equality) and pass the
///   result through [`Ty::union`], which deduplicates, sorts, and
///   collapses singletons (`[x]` → `x`, `[]` → `Ty::Unknown`).
/// - `base` is any other `Ty` → return `Ty::Unknown`. We cannot refine
///   a non-union base: either it already equals `matched` (so the
///   complement is empty / exhausted — caller is on a dead branch) or
///   it is a single type disjoint from `matched` (so the complement is
///   `base`, but we cannot prove non-equality cheaply for every `Ty`
///   variant and falling back to `Ty::Unknown` is sound).
///
/// A pure function over `Ty` keeps this testable in isolation without
/// constructing a `NarrowingTransfer`.
fn ty_difference(base: &Ty, matched: &Ty) -> Ty {
    match base {
        Ty::Union(members) => {
            let remaining: Vec<Ty> = members.iter().filter(|m| *m != matched).cloned().collect();
            Ty::union(remaining)
        }
        _ => Ty::Unknown,
    }
}

/// Run the forward narrowing analysis on a body and return the
/// per-block fixed point.
///
/// This is the single non-test entry point that wires the CFG builder,
/// the `NarrowingTransfer`, and the `DataflowSolver` together. Task 6.6
/// will wrap it in a Salsa query keyed by `(file_id, owner)`; until
/// then it stays the facade so the forthcoming query can swap the
/// implementation without touching call sites.
///
/// `base_types` is the body's pre-narrow `Name → Ty` snapshot. It is
/// consumed by [`NarrowingTransfer::apply_guard`] to compute precise
/// `Union \ matched` complements on false branches of type checks
/// (see [`ty_difference`]). An empty map is sound: every false-branch
/// complement degrades to `Ty::Unknown` and callers fall back to the
/// base type lookup.
///
/// Returns `None` if the solver fails to converge within the default
/// iteration cap — an impossible outcome for lowered HIR bodies, kept
/// in the signature solely to match `DataflowSolver::solve`.
pub fn narrow_body(
    body: Body,
    base_types: FxHashMap<Name, Ty>,
) -> Option<dataflow::DataflowResult<NarrowState>> {
    let cfg = std::sync::Arc::new(cfg::CfgBuilder::new().build_graph_from_hir(
        body.body_stmts_typed(),
        &body,
        None,
    ));
    let mut solver = dataflow::DataflowSolver::new(cfg, body, NarrowingTransfer::new(base_types));
    solver.set_bottom_factory(NarrowState::new);
    solver.solve()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hir_def::hir::UnaryOp;

    /// Tiny builder that hand-rolls a `Body` with just enough expressions
    /// to exercise guard recognition. We never need statements or
    /// bindings for these tests — `recognize_guard` is pure on the
    /// expression arena.
    struct ExprBuilder {
        body: Body,
    }

    impl ExprBuilder {
        fn new() -> Self {
            Self { body: Body::new() }
        }

        fn alloc(&mut self, expr: Expr) -> ExprIdx {
            self.body.exprs_mut().alloc(expr)
        }

        fn path(&mut self, name: &str) -> ExprIdx {
            self.alloc(Expr::Path(Name::new(name)))
        }

        fn string_lit(&mut self, s: &str) -> ExprIdx {
            self.alloc(Expr::Literal(Literal::String(s.to_string())))
        }

        fn undefined(&mut self) -> ExprIdx {
            self.alloc(Expr::Literal(Literal::Undefined))
        }

        fn call(&mut self, callee: ExprIdx, args: Vec<ExprIdx>) -> ExprIdx {
            self.alloc(Expr::Call { callee, args: args.into_boxed_slice() })
        }

        fn bin(&mut self, lhs: ExprIdx, rhs: ExprIdx, op: BinaryOp) -> ExprIdx {
            self.alloc(Expr::BinaryOp { lhs, rhs, op })
        }

        fn assign(&mut self, target: ExprIdx, value: ExprIdx) -> StmtIdx {
            self.body.stmts_mut().alloc(Stmt::Assign { target, value })
        }

        /// Build an `If` stmt with a given condition, a single-statement
        /// then-branch, and no elsif / else — the minimum CFG-producing
        /// shape for narrowing e2e tests.
        fn if_then(&mut self, condition: ExprIdx, then_stmt: StmtIdx) -> StmtIdx {
            let if_stmt = hir_def::hir::IfStmt {
                condition,
                then_branch: Box::from([then_stmt]),
                elsif_branches: Box::from([]),
                else_branch: None,
            };
            self.body.stmts_mut().alloc(Stmt::If(Box::new(if_stmt)))
        }

        /// Build an `If` stmt with a single-statement then-branch and a
        /// single-statement else-branch — the shape Task 6.3 e2e tests
        /// need to assert on the false-branch IN state.
        fn if_then_else(
            &mut self,
            condition: ExprIdx,
            then_stmt: StmtIdx,
            else_stmt: StmtIdx,
        ) -> StmtIdx {
            let if_stmt = hir_def::hir::IfStmt {
                condition,
                then_branch: Box::from([then_stmt]),
                elsif_branches: Box::from([]),
                else_branch: Some(Box::from([else_stmt])),
            };
            self.body.stmts_mut().alloc(Stmt::If(Box::new(if_stmt)))
        }

        fn set_top_level(&mut self, stmts: Vec<StmtIdx>) {
            self.body.set_body_stmts(stmts.into_boxed_slice());
        }
    }

    #[test]
    fn recognizes_type_check_direct() {
        // `ТипЗнч(Х) = Тип("Строка")` — canonical orientation.
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let typznc_callee = b.path("ТипЗнч");
        let lhs = b.call(typznc_callee, vec![x]);
        let tip_callee = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tip_callee, vec![s]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::TypeCheck { var: Name::new("Х"), type_name: "Строка".to_string() })
        );
    }

    #[test]
    fn recognizes_type_check_reversed() {
        // `Тип("Строка") = ТипЗнч(Х)` — flipped sides, same meaning.
        let mut b = ExprBuilder::new();
        let tip_callee = b.path("Тип");
        let s = b.string_lit("Массив");
        let lhs = b.call(tip_callee, vec![s]);
        let x = b.path("Данные");
        let typznc_callee = b.path("ТипЗнч");
        let rhs = b.call(typznc_callee, vec![x]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::TypeCheck {
                var: Name::new("Данные"), type_name: "Массив".to_string()
            })
        );
    }

    #[test]
    fn recognizes_type_check_english_spelling() {
        // `TypeOf(X) = Type("String")` — English forms accepted verbatim
        // (BSL is bilingual and case-insensitive on builtin names).
        let mut b = ExprBuilder::new();
        let x = b.path("X");
        let typeof_callee = b.path("TypeOf");
        let lhs = b.call(typeof_callee, vec![x]);
        let type_callee = b.path("Type");
        let s = b.string_lit("String");
        let rhs = b.call(type_callee, vec![s]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::TypeCheck { var: Name::new("X"), type_name: "String".to_string() })
        );
    }

    #[test]
    fn recognizes_type_check_case_insensitive() {
        // Mixed case — `тИпЗнЧ` must still match, because identifier
        // comparison in BSL is fully case-insensitive.
        let mut b = ExprBuilder::new();
        let x = b.path("а");
        let typznc_callee = b.path("тИпЗнЧ");
        let lhs = b.call(typznc_callee, vec![x]);
        let tip_callee = b.path("ТИП");
        let s = b.string_lit("Число");
        let rhs = b.call(tip_callee, vec![s]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert!(matches!(recognize_guard(guard, &b.body), Some(Guard::TypeCheck { .. })));
    }

    #[test]
    fn recognizes_is_undefined() {
        // `Х = Неопределено`
        let mut b = ExprBuilder::new();
        let lhs = b.path("Х");
        let rhs = b.undefined();
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::IsUndefined { var: Name::new("Х") })
        );
    }

    #[test]
    fn recognizes_is_undefined_reversed() {
        // `Неопределено = Х` — same guard, flipped sides.
        let mut b = ExprBuilder::new();
        let lhs = b.undefined();
        let rhs = b.path("Х");
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::IsUndefined { var: Name::new("Х") })
        );
    }

    #[test]
    fn recognizes_is_not_undefined() {
        // `Х <> Неопределено`
        let mut b = ExprBuilder::new();
        let lhs = b.path("Х");
        let rhs = b.undefined();
        let guard = b.bin(lhs, rhs, BinaryOp::Neq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::IsNotUndefined { var: Name::new("Х") })
        );
    }

    #[test]
    fn recognizes_is_not_undefined_reversed() {
        // `Неопределено <> Х`
        let mut b = ExprBuilder::new();
        let lhs = b.undefined();
        let rhs = b.path("Х");
        let guard = b.bin(lhs, rhs, BinaryOp::Neq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::IsNotUndefined { var: Name::new("Х") })
        );
    }

    #[test]
    fn recognizes_value_filled() {
        // `ЗначениеЗаполнено(Х)`
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let callee = b.path("ЗначениеЗаполнено");
        let call = b.call(callee, vec![x]);

        assert_eq!(
            recognize_guard(call, &b.body),
            Some(Guard::ValueFilled { var: Name::new("Х") })
        );
    }

    #[test]
    fn recognizes_value_filled_english() {
        // `ValueIsFilled(X)` — documented English spelling.
        let mut b = ExprBuilder::new();
        let x = b.path("X");
        let callee = b.path("ValueIsFilled");
        let call = b.call(callee, vec![x]);

        assert_eq!(
            recognize_guard(call, &b.body),
            Some(Guard::ValueFilled { var: Name::new("X") })
        );
    }

    #[test]
    fn does_not_recognize_random_binary_op() {
        // `Х + 1` — arithmetic, not a guard. Must return `None` so the
        // solver leaves state unchanged.
        let mut b = ExprBuilder::new();
        let lhs = b.path("Х");
        let rhs = b.alloc(Expr::Literal(Literal::Number(1.0.try_into().unwrap())));
        let expr = b.bin(lhs, rhs, BinaryOp::Add);

        assert_eq!(recognize_guard(expr, &b.body), None);
    }

    #[test]
    fn does_not_recognize_negated_type_check() {
        // `ТипЗнч(Х) <> Тип("Строка")` is an explicit non-goal of
        // ADR-01 (see module doc). Falling through to `None` is the
        // contract — `Не` / `<>` on a type check does *not* synthesize
        // an inverted TypeCheck guard.
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let typznc_callee = b.path("ТипЗнч");
        let lhs = b.call(typznc_callee, vec![x]);
        let tip_callee = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tip_callee, vec![s]);
        let guard = b.bin(lhs, rhs, BinaryOp::Neq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_unary_not() {
        // `Не ЗначениеЗаполнено(Х)` — negation is deferred (ADR-01 Q1).
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let callee = b.path("ЗначениеЗаполнено");
        let call = b.call(callee, vec![x]);
        let negated = b.alloc(Expr::UnaryOp { expr: call, op: UnaryOp::Not });

        assert_eq!(recognize_guard(negated, &b.body), None);
    }

    #[test]
    fn does_not_recognize_or_composition() {
        // `ТипЗнч(Х) = Тип("Строка") ИЛИ ТипЗнч(Х) = Тип("Число")` —
        // explicitly deferred. The top-level expression is BinaryOp::Or
        // so `recognize_guard` falls through to `None`.
        let mut b = ExprBuilder::new();

        let build_tc = |b: &mut ExprBuilder, type_lit: &str| {
            let x = b.path("Х");
            let tz = b.path("ТипЗнч");
            let lhs = b.call(tz, vec![x]);
            let tp = b.path("Тип");
            let s = b.string_lit(type_lit);
            let rhs = b.call(tp, vec![s]);
            b.bin(lhs, rhs, BinaryOp::Eq)
        };

        let left = build_tc(&mut b, "Строка");
        let right = build_tc(&mut b, "Число");
        let or_expr = b.bin(left, right, BinaryOp::Or);

        assert_eq!(recognize_guard(or_expr, &b.body), None);
    }

    #[test]
    fn does_not_recognize_type_check_with_non_string_arg() {
        // `ТипЗнч(Х) = Тип(СомеПеременная)` — the `Тип(…)` argument
        // must be a string literal. A dynamic argument is out of scope.
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let tz = b.path("ТипЗнч");
        let lhs = b.call(tz, vec![x]);
        let tp = b.path("Тип");
        let dynamic_arg = b.path("СомеПеременная");
        let rhs = b.call(tp, vec![dynamic_arg]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_multi_arg_type_of() {
        // `ТипЗнч(Х, Y) = Тип("Строка")` — excess args.
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let y = b.path("Y");
        let tz = b.path("ТипЗнч");
        let lhs = b.call(tz, vec![x, y]);
        let tp = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tp, vec![s]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_value_filled_with_no_args() {
        // `ЗначениеЗаполнено()` — wrong arity.
        let mut b = ExprBuilder::new();
        let callee = b.path("ЗначениеЗаполнено");
        let call = b.call(callee, vec![]);

        assert_eq!(recognize_guard(call, &b.body), None);
    }

    #[test]
    fn does_not_recognize_value_filled_with_literal_arg() {
        // `ЗначениеЗаполнено("hi")` — only simple variable receivers
        // narrow, literals do not (ADR-01 scope).
        let mut b = ExprBuilder::new();
        let lit = b.string_lit("hi");
        let callee = b.path("ЗначениеЗаполнено");
        let call = b.call(callee, vec![lit]);

        assert_eq!(recognize_guard(call, &b.body), None);
    }

    #[test]
    fn does_not_recognize_field_receiver() {
        // `Объект.Поле = Неопределено` — narrowing on field receivers
        // is deferred (would need alias analysis). Must return `None`.
        let mut b = ExprBuilder::new();
        let obj = b.path("Объект");
        let field = b.alloc(Expr::Field { base: obj, field: Name::new("Поле") });
        let rhs = b.undefined();
        let guard = b.bin(field, rhs, BinaryOp::Eq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_missing_literal() {
        // `Х = 1` — `1` is not `Неопределено`, so no IsUndefined guard.
        let mut b = ExprBuilder::new();
        let lhs = b.path("Х");
        let rhs = b.alloc(Expr::Literal(Literal::Number(1.0.try_into().unwrap())));
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_path_eq_path() {
        // `Х = Y` — both sides are identifiers. Equality between two
        // variables is not a narrowing guard under ADR-01 (no known
        // type info to transfer). Pin the `None` so a future sloppy
        // refactor of `try_undefined_compare` can't accidentally
        // promote `Path == Path` into an IsUndefined-shaped guard.
        let mut b = ExprBuilder::new();
        let lhs = b.path("Х");
        let rhs = b.path("Y");
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_type_check_on_type_check() {
        // `ТипЗнч(Х) = ТипЗнч(Y)` — both sides look like `ТипЗнч(…)`;
        // neither side is a `Тип("…")` string literal. Must return
        // `None` — cross-variable type-equality is not an ADR-01
        // guard and narrowing both variables is out of scope.
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let tz1 = b.path("ТипЗнч");
        let lhs = b.call(tz1, vec![x]);
        let y = b.path("Y");
        let tz2 = b.path("ТипЗнч");
        let rhs = b.call(tz2, vec![y]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_ternary() {
        // `?(Х = Неопределено, А, Б)` — `recognize_guard` is called
        // on the `Ternary` node itself. It MUST NOT peek through and
        // return the guard hidden in `condition`: the solver asks
        // "what does THIS expression narrow?", not "what does its
        // condition narrow?". The latter is Task 6.2's job (the CFG
        // builder emits a `Conditional` vertex whose `condition`
        // expr-id is what `recognize_guard` gets called on — never
        // the Ternary).
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let und = b.undefined();
        let condition = b.bin(x, und, BinaryOp::Eq);
        let then_expr = b.path("А");
        let else_expr = b.path("Б");
        let ternary = b.alloc(Expr::Ternary { condition, then_expr, else_expr });

        assert_eq!(recognize_guard(ternary, &b.body), None);
    }

    #[test]
    fn does_not_recognize_value_filled_on_field() {
        // `ЗначениеЗаполнено(Объект.Поле)` — the receiver is a
        // `Field`, not a `Path`. Field narrowing is deferred (it
        // needs alias analysis that ADR-01 leaves out of scope).
        // `single_path_arg` rejects via `path_name`, but pin the
        // reject path with a dedicated test so it can't silently
        // regress.
        let mut b = ExprBuilder::new();
        let obj = b.path("Объект");
        let field = b.alloc(Expr::Field { base: obj, field: Name::new("Поле") });
        let callee = b.path("ЗначениеЗаполнено");
        let call = b.call(callee, vec![field]);

        assert_eq!(recognize_guard(call, &b.body), None);
    }

    // =====================================================================
    // Analysis tests (Task 6.2)
    // =====================================================================

    fn state_with(entries: &[(&str, Ty)]) -> NarrowState {
        let mut s = NarrowState::new();
        for (n, t) in entries {
            s.narrowed.insert(Name::new(n), t.clone());
        }
        s
    }

    #[test]
    fn lattice_join_empty_with_empty_is_empty() {
        // Bottom ⊔ Bottom = Bottom. A sanity baseline so later tests
        // can assume the join is monotone from ⊥.
        let a = NarrowState::new();
        let b = NarrowState::new();
        assert!(a.join(&b).is_empty());
    }

    #[test]
    fn lattice_join_propagates_one_sided_entry() {
        // {X → String} ⊔ {} = {X → String}. Absence on one side must
        // not erase a known fact on the other (otherwise narrowing in
        // one branch of an If would be invisible after the merge).
        let a = state_with(&[("Х", Ty::String)]);
        let b = NarrowState::new();
        let joined = a.join(&b);
        assert_eq!(joined.get(&Name::new("Х")), Some(&Ty::String));
    }

    #[test]
    fn lattice_join_equal_entries_stay_equal() {
        // {X → String} ⊔ {X → String} = {X → String}. Crucial for
        // fixed-point convergence — if join introduced spurious
        // Union(String, String) it would never stabilise.
        let a = state_with(&[("Х", Ty::String)]);
        let b = state_with(&[("Х", Ty::String)]);
        let joined = a.join(&b);
        assert_eq!(joined.get(&Name::new("Х")), Some(&Ty::String));
    }

    #[test]
    fn lattice_join_different_entries_go_to_union() {
        // {X → String} ⊔ {X → Number} = {X → Union(Number, String)}.
        // `Ty::union` canonicalises, so the order inside the union
        // should be deterministic across runs — pin it explicitly.
        let a = state_with(&[("Х", Ty::String)]);
        let b = state_with(&[("Х", Ty::Number)]);
        let joined = a.join(&b);
        let got = joined.get(&Name::new("Х")).expect("X must be present");
        let expected = Ty::union(vec![Ty::String, Ty::Number]);
        assert_eq!(got, &expected);
    }

    #[test]
    fn lattice_join_clears_pending_guard() {
        // A guard is only meaningful on the single edge that carries
        // it. Joining forces the slot back to `None` so any stray
        // guard that somehow reached a merge point cannot cause the
        // downstream transfer_edge to apply it to the wrong branch.
        let mut a = NarrowState::new();
        a.pending_guard = Some(Guard::IsUndefined { var: Name::new("Х") });
        let b = NarrowState::new();
        assert!(a.join(&b).pending_guard.is_none());
        assert!(b.join(&a).pending_guard.is_none());
    }

    /// Build a `NarrowingTransfer` with no base-type knowledge — forces
    /// every false-branch complement to degrade to `Ty::Unknown`.
    fn transfer_no_bases() -> NarrowingTransfer {
        NarrowingTransfer::new(FxHashMap::default())
    }

    /// Build a `NarrowingTransfer` with the given `Name → Ty` entries as
    /// the pre-narrow snapshot feeding false-branch `ty_difference`.
    fn transfer_with_bases(entries: &[(&str, Ty)]) -> NarrowingTransfer {
        let mut bases = FxHashMap::default();
        for (name, ty) in entries {
            bases.insert(Name::new(name), ty.clone());
        }
        NarrowingTransfer::new(bases)
    }

    #[test]
    fn apply_guard_type_check_true_maps_to_named_ty() {
        // `ТипЗнч(Х) = Тип("Строка")` on the true branch maps Х to
        // Ty::String (lowered via `Ty::from_type_name`). This is the
        // path users will actually observe in hover.
        let tr = transfer_no_bases();
        let mut s = NarrowState::new();
        tr.apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("Х"), type_name: "Строка".to_string() },
            true,
        );
        assert_eq!(s.get(&Name::new("Х")), Some(&Ty::String));
    }

    #[test]
    fn apply_guard_type_check_false_without_base_is_overlay_noop() {
        // No pre-narrow type known for Х — the false-branch complement
        // degrades to Ty::Unknown. By the overlay contract, a computed
        // Ty::Unknown must NOT appear in the map (indistinguishable
        // from "no narrowing" and could clobber prior precise info).
        let tr = transfer_no_bases();
        let mut s = NarrowState::new();
        tr.apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("Х"), type_name: "Строка".to_string() },
            false,
        );
        assert_eq!(s.get(&Name::new("Х")), None);
    }

    #[test]
    fn apply_guard_type_check_false_narrows_binary_union_to_singleton() {
        // Core Task 6.3 invariant: `Union(Number, String) \ String
        // = Number`. The smart-constructor must collapse the residual
        // single-element union down to the non-union singleton.
        let tr = transfer_with_bases(&[("Х", Ty::union(vec![Ty::Number, Ty::String]))]);
        let mut s = NarrowState::new();
        tr.apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("Х"), type_name: "Строка".to_string() },
            false,
        );
        assert_eq!(s.get(&Name::new("Х")), Some(&Ty::Number));
    }

    #[test]
    fn apply_guard_type_check_false_narrows_ternary_union_to_union() {
        // `Union(Number, String, Date) \ String = Union(Number, Date)`
        // — multi-member residue stays a union, canonicalised by
        // `Ty::union` (deterministic sort + dedup).
        let tr = transfer_with_bases(&[("Х", Ty::union(vec![Ty::Number, Ty::String, Ty::Date]))]);
        let mut s = NarrowState::new();
        tr.apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("Х"), type_name: "Строка".to_string() },
            false,
        );
        let expected = Ty::union(vec![Ty::Number, Ty::Date]);
        assert_eq!(s.get(&Name::new("Х")), Some(&expected));
    }

    #[test]
    fn apply_guard_type_check_false_on_exhausted_union_is_overlay_noop() {
        // Degenerate input: `base == matched` is a dead false-branch.
        // `ty_difference(String, String)` returns `Ty::Unknown` because
        // the non-union `String` base falls into the generic fallback.
        // By the overlay contract, Ty::Unknown is a no-op — overlay
        // stays absent and readers fall back to the base type.
        let tr = transfer_with_bases(&[("Х", Ty::String)]);
        let mut s = NarrowState::new();
        tr.apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("Х"), type_name: "Строка".to_string() },
            false,
        );
        assert_eq!(s.get(&Name::new("Х")), None);
    }

    #[test]
    fn apply_guard_imprecise_refinement_preserves_prior_narrowing() {
        // Outer guard narrowed Х to String precisely; inner guard
        // fires on the false branch of an unrelated `ТипЗнч` test
        // with NO base_types context → computes Ty::Unknown → by the
        // overlay contract, MUST NOT clobber the still-valid outer
        // narrowing. This is the load-bearing invariant that gates
        // the "insert only if informative" change: without it, nested
        // guards would routinely destroy outer precision.
        let tr = transfer_no_bases();
        let mut s = state_with(&[("Х", Ty::String)]);
        tr.apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("Х"), type_name: "Число".to_string() },
            false,
        );
        assert_eq!(
            s.get(&Name::new("Х")),
            Some(&Ty::String),
            "imprecise refinement must leave prior narrowing intact"
        );
    }

    #[test]
    fn apply_guard_is_undefined_true_maps_to_undefined() {
        let tr = transfer_no_bases();
        let mut s = NarrowState::new();
        tr.apply_guard(&mut s, &Guard::IsUndefined { var: Name::new("Х") }, true);
        assert_eq!(s.get(&Name::new("Х")), Some(&Ty::Undefined));
    }

    #[test]
    fn apply_guard_is_undefined_false_narrows_union_minus_undefined() {
        // `Х = Неопределено` on the false branch knows Х ≠ Undefined.
        // Over `Union(String, Undefined)` that leaves just `String`.
        let tr = transfer_with_bases(&[("Х", Ty::union(vec![Ty::String, Ty::Undefined]))]);
        let mut s = NarrowState::new();
        tr.apply_guard(&mut s, &Guard::IsUndefined { var: Name::new("Х") }, false);
        assert_eq!(s.get(&Name::new("Х")), Some(&Ty::String));
    }

    #[test]
    fn apply_guard_is_not_undefined_false_maps_to_undefined() {
        // Mirror case — `Х <> Неопределено` false-branch knows Х IS
        // Undefined. This IS precise (no union needed), so it must
        // not fall back to Unknown.
        let tr = transfer_no_bases();
        let mut s = NarrowState::new();
        tr.apply_guard(&mut s, &Guard::IsNotUndefined { var: Name::new("Х") }, false);
        assert_eq!(s.get(&Name::new("Х")), Some(&Ty::Undefined));
    }

    #[test]
    fn apply_guard_is_not_undefined_true_narrows_union_minus_undefined() {
        // `Х <> Неопределено` on the true branch is the main "null-
        // check narrows to the non-null union member" shape. Must
        // strip Undefined from the pre-narrow union.
        let tr = transfer_with_bases(&[("Х", Ty::union(vec![Ty::String, Ty::Undefined]))]);
        let mut s = NarrowState::new();
        tr.apply_guard(&mut s, &Guard::IsNotUndefined { var: Name::new("Х") }, true);
        assert_eq!(s.get(&Name::new("Х")), Some(&Ty::String));
    }

    // --- ty_difference pure unit tests --------------------------------------

    #[test]
    fn ty_difference_union_minus_member_collapses_to_singleton() {
        let base = Ty::union(vec![Ty::Number, Ty::String]);
        assert_eq!(ty_difference(&base, &Ty::String), Ty::Number);
    }

    #[test]
    fn ty_difference_union_minus_missing_member_returns_whole_union() {
        // `Union(A, B) \ C` when C ∉ {A, B} is the whole union.
        // The smart-constructor still canonicalises, so equality is
        // structural.
        let base = Ty::union(vec![Ty::Number, Ty::String]);
        assert_eq!(ty_difference(&base, &Ty::Date), base);
    }

    #[test]
    fn ty_difference_multi_member_union_keeps_residue_as_union() {
        let base = Ty::union(vec![Ty::Number, Ty::String, Ty::Date]);
        let expected = Ty::union(vec![Ty::Number, Ty::Date]);
        assert_eq!(ty_difference(&base, &Ty::String), expected);
    }

    #[test]
    fn ty_difference_non_union_base_returns_unknown() {
        // We cannot refine a non-union base: either it equals `matched`
        // (exhausted) or it is disjoint (unchanged), and we choose the
        // sound conservative answer `Ty::Unknown` so callers read the
        // base type via fall-through.
        assert_eq!(ty_difference(&Ty::String, &Ty::String), Ty::Unknown);
        assert_eq!(ty_difference(&Ty::String, &Ty::Number), Ty::Unknown);
    }

    #[test]
    fn ty_difference_chain_to_exhaustion_stays_sound() {
        // Chained subtraction: strip members one at a time. After the
        // first step the 2-member union collapses to a singleton, so
        // the second subtraction hits the non-union fallback and
        // returns `Ty::Unknown` (sound — caller reads the base type).
        let base = Ty::union(vec![Ty::Number, Ty::String]);
        let step1 = ty_difference(&base, &Ty::Number);
        assert_eq!(step1, Ty::String);
        let step2 = ty_difference(&step1, &Ty::String);
        assert_eq!(step2, Ty::Unknown);
    }

    #[test]
    fn transfer_expr_stashes_recognized_guard() {
        // The solver drives transfer_expr on the Conditional vertex's
        // condition. Pin that the guard lands in `pending_guard` so
        // transfer_edge can consume it downstream.
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let und = b.undefined();
        let condition = b.bin(x, und, BinaryOp::Eq);

        let tr = transfer_no_bases();
        let state = tr.transfer_expr(ExprId::from_idx(condition), &NarrowState::new(), &b.body);
        assert_eq!(state.pending_guard, Some(Guard::IsUndefined { var: Name::new("Х") }));
    }

    #[test]
    fn transfer_expr_non_guard_condition_clears_pending() {
        // A condition that isn't an ADR-01 guard (e.g. `Х > 0`) must
        // NOT leave a stale pending guard in place. Otherwise an
        // earlier guard could leak across an unrelated Conditional.
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let one = b.alloc(Expr::Literal(Literal::Number(1.0.try_into().unwrap())));
        let condition = b.bin(x, one, BinaryOp::Gt);

        let mut initial = NarrowState::new();
        initial.pending_guard = Some(Guard::IsUndefined { var: Name::new("Stale") });
        let tr = transfer_no_bases();
        let state = tr.transfer_expr(ExprId::from_idx(condition), &initial, &b.body);
        assert!(state.pending_guard.is_none());
    }

    #[test]
    fn transfer_edge_true_branch_applies_pending_guard() {
        let tr = transfer_no_bases();
        let mut state = NarrowState::new();
        state.pending_guard =
            Some(Guard::TypeCheck { var: Name::new("Х"), type_name: "Число".to_string() });
        let out = tr.transfer_edge(CfgEdgeType::TrueBranch, &state);
        assert_eq!(out.get(&Name::new("Х")), Some(&Ty::Number));
        assert!(out.pending_guard.is_none(), "guard must be consumed");
    }

    #[test]
    fn transfer_edge_false_branch_applies_pending_guard() {
        // False branch of `Х <> Неопределено` narrows Х to Undefined.
        let tr = transfer_no_bases();
        let mut state = NarrowState::new();
        state.pending_guard = Some(Guard::IsNotUndefined { var: Name::new("Х") });
        let out = tr.transfer_edge(CfgEdgeType::FalseBranch, &state);
        assert_eq!(out.get(&Name::new("Х")), Some(&Ty::Undefined));
        assert!(out.pending_guard.is_none());
    }

    #[test]
    fn transfer_edge_direct_clears_pending_without_applying() {
        // Defensive: a guard that somehow landed in a `Direct` edge
        // must not be applied (Direct edges are sequential fall-
        // through, not conditional branches). Clearing without
        // applying is the correct behaviour — the state is identity.
        let tr = transfer_no_bases();
        let mut state = state_with(&[("Х", Ty::String)]);
        state.pending_guard = Some(Guard::IsUndefined { var: Name::new("Х") });
        let out = tr.transfer_edge(CfgEdgeType::Direct, &state);
        assert_eq!(out.get(&Name::new("Х")), Some(&Ty::String), "narrowing must be untouched");
        assert!(out.pending_guard.is_none(), "guard must be cleared on Direct edge");
    }

    #[test]
    fn transfer_stmt_assign_to_path_kills_narrowed_entry() {
        // After `Х = Y`, any prior narrowing of Х from an outer guard
        // is gone — the reassignment dissolves it. Task 6.4 will
        // *replace* the entry with the new rhs's type; until then,
        // "kill" is the sound choice.
        let mut b = ExprBuilder::new();
        let x_tgt = b.path("Х");
        let y_val = b.path("Y");
        let assign = b.assign(x_tgt, y_val);

        let tr = transfer_no_bases();
        let state_in = state_with(&[("Х", Ty::String)]);
        let state_out = tr.transfer_stmt(assign.into_raw(), &state_in, &b.body);
        assert_eq!(state_out.get(&Name::new("Х")), None);
    }

    #[test]
    fn transfer_stmt_assign_to_non_path_preserves_narrowed() {
        // `Объект.Поле = 1` — the target is a Field, not a Path(Name).
        // Such assignments must NOT kill any narrowed variable (they
        // touch a field, not a binding — ADR-01 leaves field-level
        // tracking out of scope).
        let mut b = ExprBuilder::new();
        let obj = b.path("Объект");
        let target = b.alloc(Expr::Field { base: obj, field: Name::new("Поле") });
        let one = b.alloc(Expr::Literal(Literal::Number(1.0.try_into().unwrap())));
        let assign = b.assign(target, one);

        let tr = transfer_no_bases();
        let state_in = state_with(&[("Х", Ty::String)]);
        let state_out = tr.transfer_stmt(assign.into_raw(), &state_in, &b.body);
        assert_eq!(state_out.get(&Name::new("Х")), Some(&Ty::String));
    }

    #[test]
    fn e2e_if_type_check_narrows_then_block() {
        // Hand-build
        //
        //   Если ТипЗнч(Х) = Тип("Строка") Тогда
        //       Х = Х  // no-op — keeps the then-block non-empty
        //   КонецЕсли
        //
        // then run the full pipeline: CfgBuilder → DataflowSolver.
        // The then-block's IN state must carry `Х → Ty::String`,
        // because transfer_edge applied the TypeCheck guard on the
        // TrueBranch edge from the Conditional vertex.
        //
        // This is the integration proof that Task 6.0's transfer_edge
        // hook, Task 6.1's recognize_guard, and Task 6.2's NarrowState
        // / NarrowingTransfer all wire together end-to-end on a real
        // CFG. If the assertion fails, either the pending-guard flow
        // is broken, or the CFG is not emitting TrueBranch edges in
        // the shape we expect.
        let mut b = ExprBuilder::new();

        // Condition: `ТипЗнч(Х) = Тип("Строка")`
        let x_arg = b.path("Х");
        let typznc = b.path("ТипЗнч");
        let lhs = b.call(typznc, vec![x_arg]);
        let tip = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tip, vec![s]);
        let condition = b.bin(lhs, rhs, BinaryOp::Eq);

        // Then-branch body: trivial self-assignment `Х = Х` to keep
        // the block non-empty without triggering the reassignment
        // kill (the kill fires on `Assign { target: Path(x), .. }`,
        // which means the then-block's narrowing is already pinned
        // BEFORE the assign runs — the IN state is what we assert).
        let x_tgt = b.path("Х");
        let x_val = b.path("Х");
        let assign = b.assign(x_tgt, x_val);

        let if_stmt = b.if_then(condition, assign);
        b.set_top_level(vec![if_stmt]);

        // Go through the crate-level entry point so the test
        // exercises the same wiring that Task 6.6's Salsa query will
        // eventually wrap. No base-types needed — the true-branch
        // narrowing of a TypeCheck is always precise without them.
        let body = b.body.clone();
        let result =
            narrow_body(body, FxHashMap::default()).expect("narrowing analysis must converge");
        let cfg = result.cfg();

        // Find the then-block: the successor of the Conditional
        // vertex along a TrueBranch edge whose target's IN state
        // contains the narrowing.
        use cfg::CfgVertex;
        let cond_idx = cfg
            .vertices()
            .find(|(_, v)| matches!(v, CfgVertex::Conditional(_)))
            .map(|(idx, _)| idx)
            .expect("CFG must contain a Conditional vertex for the Если");

        let then_block_idx = cfg
            .outgoing_edges(cond_idx)
            .find(|(_, kind)| **kind == CfgEdgeType::TrueBranch)
            .map(|(idx, _)| idx)
            .expect("Conditional vertex must have a TrueBranch successor");

        let then_in = result
            .block_in(then_block_idx)
            .expect("then-block must have an IN state after solving");
        assert_eq!(
            then_in.get(&Name::new("Х")),
            Some(&Ty::String),
            "IN[then-block] must carry Х → String after TrueBranch narrowing, got {then_in:?}"
        );
    }

    #[test]
    fn e2e_if_type_check_else_branch_narrows_union_complement() {
        // Hand-build
        //
        //   Если ТипЗнч(Х) = Тип("Строка") Тогда
        //       Х = Х
        //   Иначе
        //       Х = Х
        //   КонецЕсли
        //
        // with a pre-narrow base type `Х: Union(Number, String)` fed
        // into the solver. The false-branch (Иначе) block's IN state
        // must carry `Х → Number` — the Task 6.3 invariant that
        // `Union(Number, String) \ String = Number` is observed
        // end-to-end through the CFG and solver, not just inside
        // `apply_guard`.
        let mut b = ExprBuilder::new();

        // Condition: `ТипЗнч(Х) = Тип("Строка")`
        let x_arg = b.path("Х");
        let typznc = b.path("ТипЗнч");
        let lhs = b.call(typznc, vec![x_arg]);
        let tip = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tip, vec![s]);
        let condition = b.bin(lhs, rhs, BinaryOp::Eq);

        // Then-branch: `Х = Х`. Needed so the CFG emits a distinct
        // then-block (an empty branch would collapse into the merge).
        let x_tgt_then = b.path("Х");
        let x_val_then = b.path("Х");
        let assign_then = b.assign(x_tgt_then, x_val_then);

        // Else-branch: `Х = Х` — same reason. IN state of this block
        // is what we assert on.
        let x_tgt_else = b.path("Х");
        let x_val_else = b.path("Х");
        let assign_else = b.assign(x_tgt_else, x_val_else);

        let if_stmt = b.if_then_else(condition, assign_then, assign_else);
        b.set_top_level(vec![if_stmt]);

        let body = b.body.clone();
        let mut bases = FxHashMap::default();
        bases.insert(Name::new("Х"), Ty::union(vec![Ty::Number, Ty::String]));
        let result = narrow_body(body, bases).expect("narrowing analysis must converge");
        let cfg = result.cfg();

        use cfg::CfgVertex;
        let cond_idx = cfg
            .vertices()
            .find(|(_, v)| matches!(v, CfgVertex::Conditional(_)))
            .map(|(idx, _)| idx)
            .expect("CFG must contain a Conditional vertex for the Если");

        let else_block_idx = cfg
            .outgoing_edges(cond_idx)
            .find(|(_, kind)| **kind == CfgEdgeType::FalseBranch)
            .map(|(idx, _)| idx)
            .expect("Conditional vertex must have a FalseBranch successor");

        let else_in = result
            .block_in(else_block_idx)
            .expect("else-block must have an IN state after solving");
        assert_eq!(
            else_in.get(&Name::new("Х")),
            Some(&Ty::Number),
            "IN[else-block] must carry Х → Number (= Union(Number, String) \\ String), got {else_in:?}"
        );
    }
}
