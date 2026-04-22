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
//!   platform type on the true branch; `Union \ Ty` on the false
//!   branch (the latter is Task 6.3's concern).
//! - `X = Неопределено` / `X <> Неопределено` — flips between
//!   `Ty::Undefined` and its complement over the union.
//! - `ЗначениеЗаполнено(X)` — strips `Undefined` / `Null` from a union
//!   on the true branch; keeps only `Undefined` / `Null` on the false
//!   branch.
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
    /// True branch: `X` loses `Undefined` / `Null` from its union.
    /// False branch: `X` is constrained to `Undefined` / `Null` /
    /// empty-value subset. The caller decides precision (Task 6.3).
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
/// An absent `Name` means "no narrowing applies at this program point
/// — consult the base type from [`InferenceContext::var_types`]". A
/// present `Name → Ty` mapping is *authoritative*: the caller should
/// show `Ty` instead of the base union.
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
    /// analysis has not constrained `name` at this program point —
    /// callers fall back to the base `var_types` lookup.
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
///   vertex, applies the guard via [`apply_guard`]. Every other edge
///   kind falls through to identity. The slot is unconditionally
///   cleared regardless of the edge kind to keep the transient guard
///   from leaking past the branch it belongs to.
///
/// **Visibility:** `pub(crate)` — external consumers reach the
/// narrowing overlay through the (forthcoming Task 6.6) Salsa query,
/// which returns a `NarrowState` keyed by program point. There is no
/// reason for a downstream crate to construct its own solver, and
/// keeping the transfer crate-local lets future refinements
/// (Task 6.3 / 6.4) evolve its surface freely.
pub(crate) struct NarrowingTransfer;

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
            (CfgEdgeType::TrueBranch, Some(g)) => apply_guard(&mut new_state, &g, true),
            (CfgEdgeType::FalseBranch, Some(g)) => apply_guard(&mut new_state, &g, false),
            _ => {}
        }
        new_state
    }
}

/// Apply a recognized guard to the overlay on the given branch.
///
/// Scope for Task 6.2 — the precise cases that do not require a
/// `Union \ Ty` set-difference:
///
/// - `Guard::TypeCheck { var, type_name }` on `true`: map `var` to
///   `Ty::from_type_name(type_name)` (primitive / collection / lowered
///   builtin — the same resolution used by XML `AttributeType` lowering
///   in M3).
/// - `Guard::IsUndefined { var }` on `true`, and `Guard::IsNotUndefined
///   { var }` on `false`: map `var` to `Ty::Undefined` verbatim.
///
/// Every other (branch, guard) combination needs the pre-narrow union
/// to compute the complement. That is Task 6.3's smart-constructor;
/// until it lands, we map the variable to `Ty::Unknown` — which
/// defeats the narrowing for that branch, but keeps the overlay
/// otherwise sound (a caller reading `Ty::Unknown` falls through to
/// the base type exactly as if no narrowing had happened).
fn apply_guard(state: &mut NarrowState, guard: &Guard, on_true: bool) {
    match guard {
        Guard::TypeCheck { var, type_name } => {
            let narrowed = if on_true { Ty::from_type_name(type_name) } else { Ty::Unknown };
            state.narrowed.insert(var.clone(), narrowed);
        }
        Guard::IsUndefined { var } => {
            let narrowed = if on_true { Ty::Undefined } else { Ty::Unknown };
            state.narrowed.insert(var.clone(), narrowed);
        }
        Guard::IsNotUndefined { var } => {
            let narrowed = if on_true { Ty::Unknown } else { Ty::Undefined };
            state.narrowed.insert(var.clone(), narrowed);
        }
        Guard::ValueFilled { var } => {
            // Both branches need `Union \ {Undefined, Null}` precision
            // — Task 6.3. Placeholder until then.
            state.narrowed.insert(var.clone(), Ty::Unknown);
        }
    }
}

/// Run the forward narrowing analysis on a body and return the
/// per-block fixed point.
///
/// This is the single non-test entry point that wires the CFG builder,
/// the `NarrowingTransfer`, and the `DataflowSolver` together. Task 6.6
/// will wrap it in a Salsa query keyed by `(file_id, owner)`; until
/// then it stays `pub(crate)` so the forthcoming query lives behind
/// the same facade and no downstream crate grows a direct dependency
/// on the solver internals.
///
/// Returns `None` if the solver fails to converge within the default
/// iteration cap — an impossible outcome for lowered HIR bodies, kept
/// in the signature solely to match `DataflowSolver::solve`.
pub fn narrow_body(body: Body) -> Option<dataflow::DataflowResult<NarrowState>> {
    let cfg = std::sync::Arc::new(cfg::CfgBuilder::new().build_graph_from_hir(
        body.body_stmts_typed(),
        &body,
        None,
    ));
    let mut solver = dataflow::DataflowSolver::new(cfg, body, NarrowingTransfer);
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

    #[test]
    fn apply_guard_type_check_true_maps_to_named_ty() {
        // `ТипЗнч(Х) = Тип("Строка")` on the true branch maps Х to
        // Ty::String (lowered via `Ty::from_type_name`). This is the
        // path users will actually observe in hover.
        let mut s = NarrowState::new();
        apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("Х"), type_name: "Строка".to_string() },
            true,
        );
        assert_eq!(s.get(&Name::new("Х")), Some(&Ty::String));
    }

    #[test]
    fn apply_guard_type_check_false_falls_back_to_unknown() {
        // Task 6.3 will replace Unknown with `Union \ Ty` precision.
        // For now, pin the Unknown placeholder so the 6.3 changeover
        // has a baseline to diff against.
        let mut s = NarrowState::new();
        apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("Х"), type_name: "Строка".to_string() },
            false,
        );
        assert_eq!(s.get(&Name::new("Х")), Some(&Ty::Unknown));
    }

    #[test]
    fn apply_guard_is_undefined_true_maps_to_undefined() {
        let mut s = NarrowState::new();
        apply_guard(&mut s, &Guard::IsUndefined { var: Name::new("Х") }, true);
        assert_eq!(s.get(&Name::new("Х")), Some(&Ty::Undefined));
    }

    #[test]
    fn apply_guard_is_not_undefined_false_maps_to_undefined() {
        // Mirror case — `Х <> Неопределено` false-branch knows Х IS
        // Undefined. This IS precise (no union needed), so it must
        // not fall back to Unknown.
        let mut s = NarrowState::new();
        apply_guard(&mut s, &Guard::IsNotUndefined { var: Name::new("Х") }, false);
        assert_eq!(s.get(&Name::new("Х")), Some(&Ty::Undefined));
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

        let tr = NarrowingTransfer;
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
        let tr = NarrowingTransfer;
        let state = tr.transfer_expr(ExprId::from_idx(condition), &initial, &b.body);
        assert!(state.pending_guard.is_none());
    }

    #[test]
    fn transfer_edge_true_branch_applies_pending_guard() {
        let tr = NarrowingTransfer;
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
        let tr = NarrowingTransfer;
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
        let tr = NarrowingTransfer;
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

        let tr = NarrowingTransfer;
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

        let tr = NarrowingTransfer;
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
        // eventually wrap.
        let body = b.body.clone();
        let result = narrow_body(body).expect("narrowing analysis must converge");
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
}
