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
//! - `ЗначениеЗаполнено(X)` — true-branch narrows the variable's union
//!   by removing the type-level "unfilled" witnesses `Ty::Undefined`
//!   and `Ty::Null` (Track 1 Step K). The false branch is a no-op
//!   because it admits value-level "empty" shapes (`""`, `0`,
//!   empty `Date`) that `Ty` cannot represent. See
//!   [`NarrowingTransfer::apply_guard`] for the contract.
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
use hir_def::{DefWithBodyId, ExprId, IdConversion, ModuleId, Name};

use crate::lower::builtin_names::ty_from_bare_name;
use la_arena::{Idx, RawIdx};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use std::time::Instant;
use vfs::FileId;

use crate::db::HirDatabase;

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
    /// it through [`hir_def::ty::ty_from_bare_name`].
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
    /// True-branch narrows by removing `Ty::Undefined` and `Ty::Null`
    /// from the variable's `Ty::Union` base — the type-level half of
    /// `ЗначениеЗаполнено`. False-branch is a no-op because it also
    /// admits value-level "empty" shapes (`""`, `0`, empty `Date`)
    /// that `Ty` cannot witness. See
    /// [`NarrowingTransfer::apply_guard`] for the precise contract.
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
// `TrueBranch` / `FalseBranch` edge. Task 6.3 added the `Union \ Ty`
// smart-constructor so false branches refine over union bases. Task 6.4
// records the rhs's inferred type on a reassignment, replacing the
// earlier kill-only behaviour for every rhs shape we can type without
// leaving this module. The plumbing remains load-bearing: future
// refinements swap implementations of `apply_guard`, `infer_rhs_type`,
// or the `Assign` branch without touching the lattice or solver wiring.

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
    ///
    /// Case-folds `name` before the lookup so `Х` and `х` hit the same
    /// entry — BSL is case-insensitive and all narrowing writes fold
    /// through [`fold_name`] at insert time.
    pub fn get(&self, name: &Name) -> Option<&Ty> {
        self.narrowed.get(&fold_name(name))
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
    /// Point-wise join of `narrowed`, taking **only** keys present on
    /// *both* sides (intersection), each combined via [`Ty::union`].
    ///
    /// **Why intersection, not union, of keys.** An absent entry means
    /// "no narrowing — consult the base type". If `Х` is narrowed on
    /// one incoming path but not on the other, the merged program
    /// point cannot commit to the narrowing: on the other path `Х`
    /// could be the full base type. Keeping the one-sided entry would
    /// be unsound — e.g. `Если <unrecognized_cond> Тогда Х = 42
    /// КонецЕсли` would falsely report `Х: Number` after the merge
    /// even though the fall-through path never touched `Х`. Dropping
    /// the entry degrades back to the base type, which is sound.
    ///
    /// **Keys on both sides.** Equal values stay unchanged; different
    /// values combine via [`Ty::union`] (the smart constructor — it
    /// deduplicates, sorts, and collapses singletons).
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
        let mut narrowed = FxHashMap::default();
        for (k, v_self) in &self.narrowed {
            if let Some(v_other) = other.narrowed.get(k) {
                let merged = if v_self == v_other {
                    v_self.clone()
                } else {
                    Ty::union(vec![v_self.clone(), v_other.clone()])
                };
                narrowed.insert(k.clone(), merged);
            }
        }
        NarrowState { narrowed, pending_guard: None }
    }
}

/// Forward-direction dataflow transfer for narrowing.
///
/// - [`Transfer::transfer_stmt`] — on an `Assign { target: Path(x), ..
///   value }` statement, infers `value`'s type via
///   [`NarrowingTransfer::infer_rhs_type`] and records it as the new
///   narrowing for `x` (ADR-01 Q3 locality). If inference cannot
///   produce anything more precise than `Ty::Unknown`, the entry is
///   dropped instead, since the assignment definitively overwrites
///   any prior narrowing of `x` and leaving the stale entry would
///   break the overlay contract.
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
    /// - `Guard::TypeCheck { var, type_name }` → `ty_from_bare_name`.
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
    /// `Guard::ValueFilled` (`ЗначениеЗаполнено(X)`) narrows the **true**
    /// branch by removing `Ty::Undefined` and `Ty::Null` from the
    /// variable's base — a precise type-level refinement, distinct
    /// from the value-level claims (`= ""`, `= 0`, etc.) that
    /// `ЗначениеЗаполнено` also makes at runtime. The **false** branch
    /// is left untouched: claiming "definitely Undefined or Null" on
    /// the false branch would discard `Ty::String`/`Ty::Number` arms
    /// that could legitimately be empty / zero, contradicting the
    /// guard's runtime semantics.
    fn apply_guard(&self, state: &mut NarrowState, guard: &Guard, on_true: bool) {
        match guard {
            Guard::TypeCheck { var, type_name } => {
                let matched = ty_from_bare_name(type_name);
                let narrowed = if on_true {
                    // True branch: take the matched type, but refine it
                    // with the base when the base is strictly more
                    // precise. Today the only such pair is
                    // `matched = Ty::Array` ↔ `base = Ty::TypedArray(_)`:
                    // `ty_from_bare_name("Массив")` cannot reconstruct
                    // an element witness from the surface name, so a
                    // direct write would clobber `TypedArray(String)`
                    // back to bare `Array`. Promoting the base preserves
                    // the element through the guard.
                    self.refine_matched_with_base(matched, var)
                } else {
                    self.complement_of(var, &matched)
                };
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
            Guard::ValueFilled { var } => {
                if on_true {
                    let Some(base) = self.base_types.get(&fold_name(var)) else {
                        return;
                    };
                    let narrowed = ty_difference_unfilled_witnesses(base);
                    // Skip the write when the residual equals the base
                    // — the guard didn't actually narrow anything (no
                    // `Undefined` / `Null` to remove). Storing the
                    // unchanged base would clobber a prior precise
                    // overlay entry on the same variable; the overlay
                    // contract says "no entry means no narrowing", so
                    // leaving it alone preserves earlier precision.
                    if &narrowed != base {
                        insert_if_informative(state, var, narrowed);
                    }
                }
                // False branch: `ЗначениеЗаполнено(X) = false` admits
                // `Undefined`, `Null`, **and** value-level "empty"
                // shapes (`""`, `0`, empty `Date`, …). Type-level
                // narrowing can't represent those, and dropping
                // non-witness arms from the base would unsoundly
                // claim more than the guard establishes — leave the
                // overlay alone.
            }
        }
    }

    /// Refine the matched guard type with the variable's recorded base
    /// when the base is strictly more precise.
    ///
    /// `ty_from_bare_name` resolves surface names like `"Массив"` /
    /// `"Array"` to bare [`Ty::Array`] — it has no element witness to
    /// reconstruct. If the base for `var` is a [`Ty::TypedArray`], the
    /// guard `Если ТипЗнч(М) = Тип("Массив") Тогда …` should narrow to
    /// the typed form (so iteration / field access inside the branch
    /// still see the element type), not downgrade to bare `Array`.
    ///
    /// **Soundness rule.** When the base is a union that mixes
    /// `Ty::Array` and `Ty::TypedArray(_)` (and possibly non-array
    /// arms), the true branch must keep **all** array-shaped arms,
    /// not only the typed ones. Dropping the bare `Array` arm would
    /// claim more precision than the guard actually established —
    /// the runtime value could still be the un-witnessed array, and
    /// pretending it is `TypedArray(X)` would let iteration emit a
    /// fictitious element type.
    ///
    /// Other matched/base pairs are left alone today; widen the rule
    /// here when a similar precision gap surfaces (e.g. a future
    /// `Ty::TypedMap`).
    fn refine_matched_with_base(&self, matched: Ty, var: &Name) -> Ty {
        match (&matched, self.base_types.get(&fold_name(var))) {
            (Ty::Array, Some(base @ Ty::TypedArray(_))) => base.clone(),
            (Ty::Array, Some(Ty::Union(arms))) => {
                let array_arms: Vec<Ty> = arms
                    .iter()
                    .filter(|a| matches!(a, Ty::Array | Ty::TypedArray(_)))
                    .cloned()
                    .collect();
                if array_arms.is_empty() {
                    matched
                } else {
                    Ty::union(array_arms)
                }
            }
            _ => matched,
        }
    }

    /// Compute `base_types[var] \ matched` via [`ty_difference`].
    /// Returns `Ty::Unknown` when the variable has no recorded base type.
    ///
    /// Folds `var` so mixed-case sources (`Х` / `х`) hit the same seed
    /// entry — the overlay already round-trips through [`fold_name`] on
    /// every write, so the base map must honour the same invariant.
    ///
    /// When `matched` is [`Ty::Array`], the difference is computed with
    /// the Array ↔ TypedArray subtype relation: a `TypedArray(_)` arm
    /// IS an array (Phase 0 algebra), so it must be removed from the
    /// false branch alongside any bare `Array` arms. Structural
    /// `ty_difference` would otherwise treat the two variants as
    /// disjoint and leave a `TypedArray(_)` arm on the false branch,
    /// which the overlay would then surface as "value definitely
    /// exists and is a typed array" — an unsound contradiction with
    /// the guard's "is NOT an array" claim.
    fn complement_of(&self, var: &Name, matched: &Ty) -> Ty {
        let Some(base) = self.base_types.get(&fold_name(var)) else {
            return Ty::Unknown;
        };
        if matches!(matched, Ty::Array) {
            return ty_difference_array_aware(base);
        }
        ty_difference(base, matched)
    }

    /// Minimal, pure inference for the rhs of a `Stmt::Assign` — enough
    /// for Task 6.4's reassignment-locality invariant.
    ///
    /// Returns a precise `Ty` for shapes where it is knowable without
    /// leaving this module:
    /// - `Expr::Literal` — canonical per-variant lowering (matches
    ///   [`infer_literal`](crate::InferenceContext::infer_literal)
    ///   verbatim so hover, Task 6.6, and this transfer never diverge).
    /// - `Expr::Path(name)` — overlay wins over `base_types`. That is,
    ///   if the rhs is a variable currently narrowed in this program
    ///   point, the assignee inherits the *narrowed* type, not the
    ///   base. This is what makes `Если Т(Y) = Тип("Строка") Тогда Х =
    ///   Y` see `Х: String` in the then-branch.
    ///
    /// Anything else — calls, binary ops, new, field access, ternary —
    /// returns `Ty::Unknown`. The caller (`transfer_stmt`) then DROPs
    /// the overlay entry for the assignee rather than storing
    /// `Unknown` (overlay contract: no Unknown ever stored). This
    /// matches Task 6.2's kill-on-reassignment behaviour for the
    /// shapes we still cannot handle here, so the upgrade is
    /// strictly-more-informative.
    fn infer_rhs_type(&self, value: ExprIdx, state: &NarrowState, body: &Body) -> Ty {
        match body.expr_idx(value) {
            Expr::Literal(lit) => match lit {
                Literal::Number(_) => Ty::Number,
                Literal::String(_) => Ty::String,
                Literal::Date(_) => Ty::Date,
                Literal::Bool(_) => Ty::Boolean,
                Literal::Undefined => Ty::Undefined,
                Literal::Null => Ty::Null,
            },
            Expr::Path(name) => {
                let folded = fold_name(name);
                state
                    .narrowed
                    .get(&folded)
                    .cloned()
                    .or_else(|| self.base_types.get(&folded).cloned())
                    .unwrap_or(Ty::Unknown)
            }
            _ => Ty::Unknown,
        }
    }
}

impl Transfer<NarrowState> for NarrowingTransfer {
    fn transfer_stmt(&self, stmt_id: RawIdx, state: &NarrowState, body: &Body) -> NarrowState {
        let mut new_state = state.clone();
        let stmt_idx: StmtIdx = Idx::from_raw(stmt_id);
        if let Stmt::Assign { target, value } = body.stmt_idx(stmt_idx) {
            if let Expr::Path(name) = body.expr_idx(*target) {
                // Task 6.4: an assignment definitively overwrites the
                // target — any prior narrowing is stale. Try to infer
                // the rhs's type and record it; if inference is out of
                // scope (returns `Ty::Unknown`), drop the entry so the
                // overlay cannot keep showing the outdated narrowing.
                let new_ty = self.infer_rhs_type(*value, &new_state, body);
                let folded = fold_name(name);
                if matches!(new_ty, Ty::Unknown) {
                    new_state.narrowed.remove(&folded);
                } else {
                    new_state.narrowed.insert(folded, new_ty);
                }
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
        state.narrowed.insert(fold_name(var), ty);
    }
}

/// Case-fold a [`Name`] for use as a narrowing-overlay key.
///
/// BSL is case-insensitive (per ADR-01 and `name.eq_ignore_case`), but
/// [`Name`]'s derived `Hash` / `Eq` are case-sensitive — a `SmolStr` wrapper.
/// Without normalisation, `Если ТипЗнч(х) = Тип("Строка") Тогда А = Х` would
/// write the narrowed overlay under `Name("х")` and then miss on the hover
/// lookup under `Name("Х")`, violating ADR-01 Q4. This helper canonicalises
/// the spelling to lowercase before both inserting into and reading from any
/// narrowing-related `FxHashMap<Name, _>` — including `NarrowState::narrowed`,
/// `NarrowingTransfer::base_types`, and the per-body seed built by
/// [`build_base_types_for_body`]. All internal iterators already see
/// pre-folded keys (they never call `fold_name` themselves), so the
/// invariant holds structurally once every write site and every explicit
/// lookup folds.
fn fold_name(n: &Name) -> Name {
    Name::new(&n.as_str().to_lowercase())
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

/// `base \ {Ty::Undefined, Ty::Null}` — the type-level half of
/// `ЗначениеЗаполнено(X)` true-branch narrowing.
///
/// `is_unfilled_witness` is intentionally narrow: only `Ty::Undefined`
/// and `Ty::Null` qualify. Value-level "unfilled" shapes (empty
/// `Ty::String`, zero `Ty::Number`, empty `Ty::Date`) are NOT removed —
/// `Ty` carries no value witness for them, and pretending we did would
/// surface `String` / `Number` arms as "definitely non-empty" when the
/// runtime can still observe an empty string / zero. Non-union bases
/// collapse to `Ty::Unknown` (overlay no-op), matching the structural
/// fallback in [`ty_difference`] / [`ty_difference_array_aware`].
fn ty_difference_unfilled_witnesses(base: &Ty) -> Ty {
    match base {
        Ty::Union(members) => {
            let remaining: Vec<Ty> =
                members.iter().filter(|m| !is_unfilled_witness(m)).cloned().collect();
            Ty::union(remaining)
        }
        _ => Ty::Unknown,
    }
}

/// `Ty::Undefined` and `Ty::Null` are the only type-level shapes
/// `ЗначениеЗаполнено` can rule out from a static `Ty::Union`. Anything
/// else — primitives that may be empty at runtime, structural types,
/// metadata refs — stays in the residual.
fn is_unfilled_witness(ty: &Ty) -> bool {
    matches!(ty, Ty::Undefined | Ty::Null)
}

/// `base \ Ty::Array` with the Array ↔ TypedArray subtype relation
/// honoured: any `Ty::Array` or `Ty::TypedArray(_)` arm is removed.
///
/// Structural [`ty_difference`] treats `Ty::Array` and
/// `Ty::TypedArray(X)` as disjoint variants because Rust `PartialEq`
/// compares them by tag. Phase 0 introduced the algebraic rule
/// `TypedArray(_) ≤ Array`; the false branch of `Если ТипЗнч(М) =
/// Тип("Массив")` must therefore exclude **both** Array and
/// TypedArray arms — otherwise a `Union(TypedArray(String), Number)`
/// base would survive the false branch unchanged, surfacing
/// `TypedArray(String)` to consumers that asked for "definitely not
/// an array" and silently contradicting the guard.
///
/// Non-union, non-array bases collapse to [`Ty::Unknown`] (overlay
/// no-op) just like the structural fallback. A non-union *array*
/// base (`Ty::Array` or `Ty::TypedArray(_)`) collapses to `Ty::Unknown`
/// because the false branch is dead — there is no remainder to
/// surface.
fn ty_difference_array_aware(base: &Ty) -> Ty {
    match base {
        Ty::Union(members) => {
            let remaining: Vec<Ty> = members
                .iter()
                .filter(|m| !matches!(m, Ty::Array | Ty::TypedArray(_)))
                .cloned()
                .collect();
            Ty::union(remaining)
        }
        Ty::Array | Ty::TypedArray(_) => Ty::Unknown,
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
    let cfg =
        Arc::new(cfg::CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None));
    let mut solver = dataflow::DataflowSolver::new(cfg, body, NarrowingTransfer::new(base_types));
    solver.set_bottom_factory(NarrowState::new);
    solver.solve()
}

/// Salsa-facing wrapper around [`narrow_body`].
///
/// Resolves `owner` through [`hir_def::ModuleBodies`] to the `Body` that
/// lives in `file_id`, seeds `base_types` from the body's own
/// [`Expr::Path`] inferred types (see [`build_base_types_for_body`]),
/// and returns the solver result wrapped in `Arc` so consumers share a
/// single allocation.
///
/// Follows the same "plain function, no `#[salsa::tracked]` attribute"
/// pattern as [`crate::infer::infer_query`] / [`crate::infer::type_of_expr_query`].
/// The database implementation (`RootDatabaseImpl` in `ide-db`) delegates
/// [`HirDatabase::narrow`] straight to this function; the tracked layer
/// underneath (`module_bodies`, `infer`) supplies incremental invalidation.
///
/// Returns `None` when:
/// - `owner` does not resolve to a body that lives in this file
///   (stale call site after refactor, or mismatched `DefWithBodyId`);
/// - [`narrow_body`] fails to converge within the default iteration cap
///   (impossible for lowered HIR, kept for signature compatibility).
pub fn narrow_query(
    db: &dyn HirDatabase,
    file_id: FileId,
    owner: DefWithBodyId,
) -> Option<Arc<dataflow::DataflowResult<NarrowState>>> {
    let _span = tracing::info_span!("narrow_query", ?file_id, ?owner).entered();
    let total_start = Instant::now();

    // Per-stage Δ instrumentation. Slow `arg_diagnostics_query` summary
    // already showed `narrow_ms` dominates; this breakdown attributes
    // those ~60 ms / owner across the steps inside narrow_query so
    // optimization targeting (cached CFG, in-place join, etc.) gets a
    // ranked list rather than a single black-box Δ.
    let resolve_start = Instant::now();
    let module_id = ModuleId { file_id };
    let module_bodies = db.module_bodies(module_id);

    let body: &Body = match owner {
        DefWithBodyId::ModuleCode => module_bodies.module_code()?,
        DefWithBodyId::Method(local_id) => {
            module_bodies.lower_result(local_id).map(|lr| &lr.body)?
        }
    };
    let resolve_ns = resolve_start.elapsed().as_nanos();

    // Phase O.17: route through the per-owner Salsa cell instead of
    // `infer_query`. Warm hits become a single `Arc::clone` on the
    // `infer_method` / `infer_module_code` cell; cold cross-file hits
    // invalidate only the touched method rather than every body in
    // the file.
    let infer_start = Instant::now();
    let infer_routed = crate::infer::infer_owner(db, file_id, owner);
    let per_body_types = Some(infer_routed.expr_types());
    let infer_ns = infer_start.elapsed().as_nanos();

    let base_types_start = Instant::now();
    let base_types = build_base_types_for_body(body, per_body_types);
    let base_types_ns = base_types_start.elapsed().as_nanos();

    // Inline CFG build + solve so the `narrow_body` test helper signature
    // stays untouched while we get isolated timers for the two heaviest
    // chunks. The `body.clone()` is forced by `DataflowSolver::new`
    // taking `Body` by value — kept measurable so it can be revisited
    // separately if the clone shows up as a hot allocation.
    let body_clone_start = Instant::now();
    let body_owned = body.clone();
    let body_clone_ns = body_clone_start.elapsed().as_nanos();

    let cfg_build_start = Instant::now();
    let cfg = Arc::new(cfg::CfgBuilder::new().build_graph_from_hir(
        body_owned.body_stmts_typed(),
        &body_owned,
        None,
    ));
    let cfg_build_ns = cfg_build_start.elapsed().as_nanos();

    let solve_start = Instant::now();
    let mut solver =
        dataflow::DataflowSolver::new(cfg, body_owned, NarrowingTransfer::new(base_types));
    solver.set_bottom_factory(NarrowState::new);
    let solved = solver.solve();
    let solve_ns = solve_start.elapsed().as_nanos();

    let stages = NarrowQueryStages {
        total_ns: total_start.elapsed().as_nanos(),
        resolve_ns,
        infer_ns,
        base_types_ns,
        body_clone_ns,
        cfg_build_ns,
        solve_ns,
    };
    log_narrow_query_stages(owner, &stages);

    Some(Arc::new(solved?))
}

/// Per-stage time budget for one [`narrow_query`] call.
///
/// Together they cover total wall time of the call. Microsecond-keyed
/// fields surface in the slow-path log when the call exceeds
/// [`log_narrow_query_stages`]'s threshold so optimization work has a
/// concrete target ranking instead of one opaque per-owner Δ.
struct NarrowQueryStages {
    /// Wall time of the entire `narrow_query` call.
    total_ns: u128,
    /// `db.module_bodies(module_id)` lookup + `Body` resolution by owner.
    resolve_ns: u128,
    /// `db.infer(file_id)` Salsa hit (Arc clone in steady state) +
    /// per-body type-map fetch.
    infer_ns: u128,
    /// `build_base_types_for_body` — linear scan over `Expr::Path` nodes.
    base_types_ns: u128,
    /// `body.clone()` forced by the `DataflowSolver::new` by-value contract.
    body_clone_ns: u128,
    /// `CfgBuilder::build_graph_from_hir` — currently rebuilt every call
    /// (no Salsa cache hit), the prime suspect for narrow_ms dominance.
    cfg_build_ns: u128,
    /// `DataflowSolver::solve` — fixed-point iterations over `NarrowState`.
    solve_ns: u128,
}

/// Emit a per-call stage breakdown when narrow_query takes longer than
/// the slow-path threshold. Filters out the median-fast calls so the
/// log stays a usable signal: the hot file `ОбщегоНазначения` produces
/// 240 owners; with a 20 ms gate only those above the median surface,
/// keeping the scan compact while preserving every interesting tail.
fn log_narrow_query_stages(owner: DefWithBodyId, stages: &NarrowQueryStages) {
    const SLOW_NS: u128 = 20_000_000; // 20 ms
    if stages.total_ns < SLOW_NS {
        return;
    }
    let to_us = |ns: u128| (ns / 1_000) as u64;
    tracing::info!(
        owner = ?owner,
        total_us = to_us(stages.total_ns),
        resolve_us = to_us(stages.resolve_ns),
        infer_us = to_us(stages.infer_ns),
        base_types_us = to_us(stages.base_types_ns),
        body_clone_us = to_us(stages.body_clone_ns),
        cfg_build_us = to_us(stages.cfg_build_ns),
        solve_us = to_us(stages.solve_ns),
        "narrow_query stages",
    );
}

/// Build a per-body `Name → Ty` base map by scanning [`Expr::Path`]
/// nodes and reading their inferred types from the body's per-expr map.
///
/// Why per-body, not from [`InferenceResult::var_types`]: the file-
/// global `var_types` map keys variables by `String::to_lowercase()`,
/// whereas [`NarrowingTransfer::base_types`] uses `Name` — whose
/// `Hash` / `Eq` are **case-sensitive** — to look up entries keyed on
/// the original-case names that appear inside `Expr::Path`. Routing
/// through the body's own `expr_types` preserves source case and
/// scopes collisions to a single procedure.
///
/// **Policy:** first-writer wins. Arena iteration order matches source
/// order, so we pick the type associated with the first occurrence of
/// each name — usually its declared / initial-assignment type, which is
/// the value that best plays the role of a "pre-narrow base" for the
/// [`ty_difference`]-driven false-branch complement.
///
/// **Soundness.** A stale base never over-narrows. If the seed is
/// narrower than the true reaching type (e.g., first assign was
/// `Х = 42` but `Х` was later rewritten to `"abc"`), [`ty_difference`]
/// on the false branch sees a non-Union base → degrades to
/// [`Ty::Unknown`] → [`insert_if_informative`] skips → overlay stays
/// unchanged. The worst case is losing else-branch precision, never a
/// wrong overlay entry. Task 6.7 can upgrade the seed to the merged
/// reaching type without violating this invariant.
fn build_base_types_for_body(
    body: &Body,
    per_body_types: Option<&FxHashMap<hir_def::ExprId, Ty>>,
) -> FxHashMap<Name, Ty> {
    let mut base_types: FxHashMap<Name, Ty> = FxHashMap::default();
    let Some(per_body) = per_body_types else {
        return base_types;
    };
    for (expr_id, expr) in body.exprs_iter() {
        if let Expr::Path(name) = expr {
            if let Some(ty) = per_body.get(&expr_id) {
                // Fold the key so a mixed-case source (`Х` and `х` both
                // referring to the same BSL variable) lands on the same
                // entry — the overlay round-trips through `fold_name`
                // at every write, so the seed must honour the same
                // invariant.
                base_types.entry(fold_name(name)).or_insert_with(|| ty.clone());
            }
        }
    }
    base_types
}

/// Return the narrowed type of `name` observed at the program point
/// occupied by `expr_idx`, or `None` when no overlay applies.
///
/// **Pre-narrow on guard receivers (ADR-01 Q4).** The receiver of a
/// guard expression — e.g., the `Х` inside `ТипЗнч(Х) = Тип("Строка")` —
/// lives in the Conditional vertex's `condition` sub-tree. Narrowing
/// is applied on the vertex's *outgoing* True / False edges (Task 6.2
/// wires the pending-guard through [`dataflow::Transfer::transfer_edge`]),
/// so the Conditional's IN state still carries the base (pre-narrow)
/// overlay. Expressions inside the then / else bodies live in
/// successor BasicBlocks whose IN state carries the narrowed overlay.
///
/// This function implements the lookup by finding the CFG vertex whose
/// evaluation covers `expr_idx` and returning `block_in[vertex].get(name)`.
/// Task 6.6 will wrap the call site in a Salsa query and merge the
/// result into [`Semantics::type_of_expr`]; until then, this is the
/// raw reader that exercises the pre-narrow invariant end-to-end.
///
/// Returns `None` when `expr_idx` isn't reachable from any CFG vertex
/// — e.g., a parameter's default value expression (those live outside
/// the method body proper).
pub fn narrowed_type_at(
    result: &dataflow::DataflowResult<NarrowState>,
    expr_idx: ExprIdx,
    name: &Name,
) -> Option<Ty> {
    let body = result.body();
    let cfg = result.cfg();

    let node = containing_vertex(body, cfg, expr_idx)?;
    result.block_in(node)?.get(name).cloned()
}

/// Merge the narrowing overlay with the base [`Ty`] for an expression
/// lookup (originally lived in `hir::Semantics`).
///
/// Hover/completion through `Semantics::type_of_expr` and the argument-
/// validation query both need the same overlay, so the function lives in
/// `hir-ty` where the validation query can also reach it without a
/// `hir → hir-ty → hir` cycle.
///
/// Only applies when the expression is an [`Expr::Path`] — narrowing
/// targets named variables. For all other shapes we pass the base type
/// through unchanged.
///
/// Fallback rules (in order):
/// 1. `db.type_narrowing_enabled() == false` (Task 6.7 feature flag;
///    workspace opt-out) → `base`.
/// 2. Non-`Path` expr → `base`.
/// 3. `db.narrow(...)` returns `None` (body not in this file, provider
///    opted out) → `base`.
/// 4. Overlay has no entry for this `Name` at this program point
///    (variable untouched by any guard that dominates the expression)
///    → `base`.
/// 5. Overlay entry is [`Ty::Unknown`] (e.g., false-branch complement
///    against a non-union base — Task 6.3 `ty_difference` degrades
///    soundly) → `base`.
/// 6. Otherwise → the narrowed [`Ty`].
pub fn narrow_or_base<DB: HirDatabase + ?Sized>(
    db: &DB,
    file_id: FileId,
    owner: DefWithBodyId,
    body: &Body,
    expr_id: ExprId,
    base: Ty,
) -> Ty {
    if !db.type_narrowing_enabled() {
        return base;
    }
    let Expr::Path(name) = body.expr(expr_id) else {
        return base;
    };
    let Some(result) = db.narrow(file_id, owner) else {
        return base;
    };
    match narrowed_type_at(&result, expr_id.to_idx(), name) {
        Some(narrowed) if !matches!(narrowed, Ty::Unknown) => narrowed,
        _ => base,
    }
}

/// Find the CFG vertex whose evaluation covers `expr_idx`.
///
/// Mirrors the virtualization rule in [`cfg::CfgBuilder`]:
/// `If` / `PreprocIf` / `While` / `For` / `ForEach` / `Try` statements
/// never appear in a `BasicBlock::statements()` — their condition /
/// from / to / collection sub-expressions are instead pinned at the
/// specialised vertex that represents the statement itself. All other
/// ("linear") statements flow into the BasicBlock arena.
///
/// The walk is O(body_size) per call — a single hover-type lookup
/// visits every expression in the body at most once. Fine for this
/// use case; Task 6.6's Salsa cache will memoise the full
/// `narrow_query` result, so repeated hovers on the same body pay
/// this traversal only once per revision.
fn containing_vertex(
    body: &Body,
    cfg: &cfg::ControlFlowGraph,
    expr_idx: ExprIdx,
) -> Option<cfg::NodeIndex> {
    use cfg::CfgVertex;

    for (node_idx, vertex) in cfg.vertices() {
        let covers = match vertex {
            CfgVertex::BasicBlock(bb) => bb
                .statements()
                .iter()
                .any(|stmt_id| stmt_covers_expr(body, stmt_id.to_idx(), expr_idx)),
            CfgVertex::Conditional(v) => expr_covers_expr(body, v.condition.to_idx(), expr_idx),
            CfgVertex::WhileLoop(v) => expr_covers_expr(body, v.condition.to_idx(), expr_idx),
            CfgVertex::ForLoop(v) => {
                expr_covers_expr(body, v.from.to_idx(), expr_idx)
                    || expr_covers_expr(body, v.to.to_idx(), expr_idx)
            }
            CfgVertex::ForEachLoop(v) => expr_covers_expr(body, v.collection.to_idx(), expr_idx),
            CfgVertex::TryExcept(_)
            | CfgVertex::Label(_)
            | CfgVertex::PreprocCondition(_)
            | CfgVertex::Exit => false,
        };
        if covers {
            return Some(node_idx);
        }
    }
    None
}

/// Recursively check whether `stmt` contains `target` in any of its
/// expression children.
///
/// Only covers the "linear" statement shapes that can appear in a
/// BasicBlock (`Expr` / `Assign` / `Return` / `Raise` / `Execute` /
/// `AddHandler` / `RemoveHandler`). Virtualized statements (`If` /
/// `While` / `For` / `ForEach` / `Try` / `PreprocIf`) are handled by
/// their specialised vertices in [`containing_vertex`].
fn stmt_covers_expr(body: &Body, stmt_idx: StmtIdx, target: ExprIdx) -> bool {
    match body.stmt_idx(stmt_idx) {
        Stmt::Expr(e) => expr_covers_expr(body, *e, target),
        Stmt::Assign { target: lhs, value } => {
            expr_covers_expr(body, *lhs, target) || expr_covers_expr(body, *value, target)
        }
        Stmt::Return { value } | Stmt::Raise { value } => {
            value.as_ref().is_some_and(|v| expr_covers_expr(body, *v, target))
        }
        Stmt::Execute { expr } => expr_covers_expr(body, *expr, target),
        Stmt::AddHandler { event, handler } | Stmt::RemoveHandler { event, handler } => {
            expr_covers_expr(body, *event, target) || expr_covers_expr(body, *handler, target)
        }
        Stmt::VarDecl { .. } | Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => {
            false
        }
        Stmt::If(_)
        | Stmt::PreprocIf(_)
        | Stmt::While { .. }
        | Stmt::For { .. }
        | Stmt::ForEach { .. }
        | Stmt::Try { .. } => false,
    }
}

/// Recursively check whether `target` is anywhere in the sub-tree
/// rooted at `root`. Stops at `Literal`, `Path`, and `QualifiedPath`
/// leaves — the only expression shapes with no nested [`ExprIdx`].
fn expr_covers_expr(body: &Body, root: ExprIdx, target: ExprIdx) -> bool {
    if root == target {
        return true;
    }
    match body.expr_idx(root) {
        Expr::Missing | Expr::Path(_) | Expr::QualifiedPath(_) | Expr::Literal(_) => false,
        Expr::BinaryOp { lhs, rhs, .. } => {
            expr_covers_expr(body, *lhs, target) || expr_covers_expr(body, *rhs, target)
        }
        Expr::UnaryOp { expr, .. } => expr_covers_expr(body, *expr, target),
        Expr::Ternary { condition, then_expr, else_expr } => {
            expr_covers_expr(body, *condition, target)
                || expr_covers_expr(body, *then_expr, target)
                || expr_covers_expr(body, *else_expr, target)
        }
        Expr::Call { callee, args } => {
            expr_covers_expr(body, *callee, target)
                || args.iter().any(|a| expr_covers_expr(body, *a, target))
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_covers_expr(body, *receiver, target)
                || args.iter().any(|a| expr_covers_expr(body, *a, target))
        }
        Expr::Index { base, index } => {
            expr_covers_expr(body, *base, target) || expr_covers_expr(body, *index, target)
        }
        Expr::Field { base, .. } => expr_covers_expr(body, *base, target),
        Expr::New { args, .. } => args.iter().any(|a| expr_covers_expr(body, *a, target)),
        Expr::Array(elems) => elems.iter().any(|e| expr_covers_expr(body, *e, target)),
        Expr::Await { expr } => expr_covers_expr(body, *expr, target),
    }
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

        /// Build an `If` stmt with one `ИначеЕсли`-branch and no else
        /// — the minimum shape that produces a second CFG Conditional
        /// vertex, used by Task 6.5 to exercise receiver resolution
        /// on elsif conditions.
        fn if_then_elsif(
            &mut self,
            condition: ExprIdx,
            then_stmt: StmtIdx,
            elsif_cond: ExprIdx,
            elsif_stmt: StmtIdx,
        ) -> StmtIdx {
            let if_stmt = hir_def::hir::IfStmt {
                condition,
                then_branch: Box::from([then_stmt]),
                elsif_branches: Box::from([(elsif_cond, Box::from([elsif_stmt]))]),
                else_branch: None,
            };
            self.body.stmts_mut().alloc(Stmt::If(Box::new(if_stmt)))
        }

        /// Build a `Пока … Цикл … КонецЦикла` stmt with a single-
        /// statement body, exercising CFG's `WhileLoop` vertex.
        fn while_stmt(&mut self, condition: ExprIdx, body_stmt: StmtIdx) -> StmtIdx {
            self.body.stmts_mut().alloc(Stmt::While { condition, body: Box::from([body_stmt]) })
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
            // Fold the key to match the production invariant: every
            // overlay write in `NarrowingTransfer` routes through
            // `fold_name`, so the test helper must not short-circuit
            // that — otherwise case-insensitivity regressions would
            // hide behind raw `Name::new(...)` keys.
            s.narrowed.insert(fold_name(&Name::new(n)), t.clone());
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
    fn lattice_join_drops_one_sided_entry() {
        // {X → String} ⊔ {} = {} — dropped. An absent entry on the
        // other side means the other path made no commitment narrower
        // than the base type, so keeping `X → String` would misreport
        // a branch-local fact as merge-global. This is the soundness
        // fix that makes Task 6.4's reassignment-locality work:
        // `Если <cond> Тогда Х = 42 КонецЕсли` must not leak `Х →
        // Number` past КонецЕсли. Dropping the entry degrades to the
        // base type, which is sound.
        let a = state_with(&[("Х", Ty::String)]);
        let b = NarrowState::new();
        let joined = a.join(&b);
        assert_eq!(joined.get(&Name::new("Х")), None);

        // Commutativity sanity: swapping arguments must not change the
        // outcome (join is defined as a lattice operation).
        let joined_rev = b.join(&a);
        assert_eq!(joined_rev.get(&Name::new("Х")), None);
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
            // Match the production seed: `build_base_types_for_body`
            // folds every key before inserting, so tests that read via
            // `complement_of` (which also folds) need the same spelling
            // on the way in.
            bases.insert(fold_name(&Name::new(name)), ty.clone());
        }
        NarrowingTransfer::new(bases)
    }

    #[test]
    fn apply_guard_type_check_true_maps_to_named_ty() {
        // `ТипЗнч(Х) = Тип("Строка")` on the true branch maps Х to
        // Ty::String (lowered via `ty_from_bare_name`). This is the
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
    fn apply_guard_type_check_true_promotes_array_to_typed_array_base() {
        // Phase 0 regression guard: `Если ТипЗнч(М) = Тип("Массив")`
        // must NOT downgrade a `TypedArray(String)` base to bare
        // `Ty::Array`. The matched type from `ty_from_bare_name`
        // ("Массив") is `Ty::Array` (no element witness), so without
        // the refinement the overlay would clobber the element type
        // and iteration inside the branch would lose `String`.
        let tr = transfer_with_bases(&[("М", Ty::TypedArray(Box::new(Ty::String)))]);
        let mut s = NarrowState::new();
        tr.apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("М"), type_name: "Массив".to_string() },
            true,
        );
        assert_eq!(s.get(&Name::new("М")), Some(&Ty::TypedArray(Box::new(Ty::String))));
    }

    #[test]
    fn apply_guard_type_check_true_promotes_array_through_union_base() {
        // Base `Union(TypedArray(Number), Undefined)` ∩ guard `Массив`
        // should narrow to `TypedArray(Number)` — the typed-array arm
        // of the union, not bare `Array`. Otherwise the JSDoc-typed
        // `Параметры.Список` (lowered to TypedArray ∪ Undefined) would
        // lose element info on every `Если ТипЗнч(…) = Тип("Массив")`
        // probe.
        let typed = Ty::TypedArray(Box::new(Ty::Number));
        let tr = transfer_with_bases(&[("М", Ty::union(vec![typed.clone(), Ty::Undefined]))]);
        let mut s = NarrowState::new();
        tr.apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("М"), type_name: "Массив".to_string() },
            true,
        );
        assert_eq!(s.get(&Name::new("М")), Some(&typed));
    }

    #[test]
    fn apply_guard_type_check_true_preserves_both_array_and_typed_array_arms() {
        // Soundness: `Union(TypedArray(String), Array, Number)` ∩
        // `Array` must keep BOTH array-shaped arms. Dropping the
        // bare `Array` arm would claim "definitely a typed array of
        // strings" when the runtime value could still be the
        // un-witnessed array, leading iteration to emit a
        // fictitious element type.
        let typed = Ty::TypedArray(Box::new(Ty::String));
        let tr =
            transfer_with_bases(&[("М", Ty::union(vec![typed.clone(), Ty::Array, Ty::Number]))]);
        let mut s = NarrowState::new();
        tr.apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("М"), type_name: "Массив".to_string() },
            true,
        );
        let expected = Ty::union(vec![typed, Ty::Array]);
        assert_eq!(s.get(&Name::new("М")), Some(&expected));
    }

    #[test]
    fn apply_guard_type_check_false_removes_typed_array_arm() {
        // Soundness for the FALSE branch: `Union(TypedArray(String),
        // Number) \ Array` must remove `TypedArray(String)` because
        // a typed array IS an array (Phase 0 subtype rule). The
        // structural `ty_difference` would have left it intact —
        // surfacing "definitely a typed array of strings" to
        // consumers who asked for "definitely not an array."
        let typed = Ty::TypedArray(Box::new(Ty::String));
        let tr = transfer_with_bases(&[("М", Ty::union(vec![typed, Ty::Number]))]);
        let mut s = NarrowState::new();
        tr.apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("М"), type_name: "Массив".to_string() },
            false,
        );
        assert_eq!(s.get(&Name::new("М")), Some(&Ty::Number));
    }

    #[test]
    fn apply_guard_type_check_false_drops_typed_array_only_base_to_dead() {
        // Non-union TypedArray base, false branch: dead. Overlay
        // no-op (Ty::Unknown contract). Pre-Phase-0 the structural
        // `ty_difference` already returned Unknown for non-union
        // bases — preserve the existing dead-branch behaviour.
        let typed = Ty::TypedArray(Box::new(Ty::String));
        let tr = transfer_with_bases(&[("М", typed)]);
        let mut s = NarrowState::new();
        tr.apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("М"), type_name: "Массив".to_string() },
            false,
        );
        assert_eq!(s.get(&Name::new("М")), None);
    }

    #[test]
    fn apply_guard_type_check_true_keeps_array_when_base_has_no_typed_array() {
        // Negative: when no TypedArray sits in the base, the guard
        // type wins as before. Pins that the refinement is scoped
        // exclusively to Array ↔ TypedArray and does not perturb
        // other narrowing paths.
        let tr = transfer_with_bases(&[("М", Ty::union(vec![Ty::Number, Ty::String]))]);
        let mut s = NarrowState::new();
        tr.apply_guard(
            &mut s,
            &Guard::TypeCheck { var: Name::new("М"), type_name: "Массив".to_string() },
            true,
        );
        assert_eq!(s.get(&Name::new("М")), Some(&Ty::Array));
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

    #[test]
    fn apply_guard_value_filled_true_strips_undefined_and_null() {
        // `ЗначениеЗаполнено(Х)` true-branch removes both Undefined and
        // Null from the base union. `String` and other arms survive —
        // value-level "empty" shapes (`""`) are not type-level.
        let tr =
            transfer_with_bases(&[("Х", Ty::union(vec![Ty::String, Ty::Undefined, Ty::Null]))]);
        let mut s = NarrowState::new();
        tr.apply_guard(&mut s, &Guard::ValueFilled { var: Name::new("Х") }, true);
        assert_eq!(s.get(&Name::new("Х")), Some(&Ty::String));
    }

    #[test]
    fn apply_guard_value_filled_true_strips_only_null() {
        // Base without `Undefined` — `Null` alone is also an unfilled
        // witness and must be removed.
        let tr = transfer_with_bases(&[("Х", Ty::union(vec![Ty::Number, Ty::Null]))]);
        let mut s = NarrowState::new();
        tr.apply_guard(&mut s, &Guard::ValueFilled { var: Name::new("Х") }, true);
        assert_eq!(s.get(&Name::new("Х")), Some(&Ty::Number));
    }

    #[test]
    fn apply_guard_value_filled_false_leaves_overlay_untouched() {
        // False branch admits `Undefined`, `Null`, AND value-level
        // empty shapes (`""`, `0`, …). Type-level narrowing can't
        // represent the latter, so dropping non-witness arms from the
        // base would be unsound. Overlay stays empty.
        let tr =
            transfer_with_bases(&[("Х", Ty::union(vec![Ty::String, Ty::Undefined, Ty::Null]))]);
        let mut s = NarrowState::new();
        tr.apply_guard(&mut s, &Guard::ValueFilled { var: Name::new("Х") }, false);
        assert_eq!(s.get(&Name::new("Х")), None);
    }

    #[test]
    fn apply_guard_value_filled_true_no_witness_in_base_is_noop() {
        // Base has no Undefined / Null members → nothing to remove.
        // The arm short-circuits when the residual equals the base
        // (Codex pair-mode MEDIUM): writing the unchanged base would
        // clobber any prior precise narrowing on the same variable.
        // No overlay entry is the canonical "no claim" shape.
        let base = Ty::union(vec![Ty::Number, Ty::String]);
        let tr = transfer_with_bases(&[("Х", base.clone())]);
        let mut s = NarrowState::new();
        tr.apply_guard(&mut s, &Guard::ValueFilled { var: Name::new("Х") }, true);
        assert_eq!(s.get(&Name::new("Х")), None);
    }

    #[test]
    fn apply_guard_value_filled_true_preserves_prior_overlay_when_no_witness() {
        // Headline of the MEDIUM fix: a prior precise narrowing must
        // survive a `ЗначениеЗаполнено` true branch when the base has
        // no `Undefined` / `Null` to strip. Without the equality
        // short-circuit the unchanged base would clobber the prior
        // String into the broader Union, widening the overlay.
        let base = Ty::union(vec![Ty::Number, Ty::String]);
        let tr = transfer_with_bases(&[("Х", base)]);
        let mut s = NarrowState::new();
        s.narrowed.insert(fold_name(&Name::new("Х")), Ty::String);
        tr.apply_guard(&mut s, &Guard::ValueFilled { var: Name::new("Х") }, true);
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
    fn ty_difference_unfilled_witnesses_strips_both_undefined_and_null() {
        let base = Ty::union(vec![Ty::String, Ty::Undefined, Ty::Null]);
        assert_eq!(ty_difference_unfilled_witnesses(&base), Ty::String);
    }

    #[test]
    fn ty_difference_unfilled_witnesses_keeps_other_arms_untouched() {
        // String and Number stay in the residual; only `Undefined`
        // and `Null` are dropped.
        let base = Ty::union(vec![Ty::Number, Ty::String, Ty::Undefined, Ty::Null]);
        let expected = Ty::union(vec![Ty::Number, Ty::String]);
        assert_eq!(ty_difference_unfilled_witnesses(&base), expected);
    }

    #[test]
    fn ty_difference_unfilled_witnesses_non_union_collapses_to_unknown() {
        // Same conservative answer as `ty_difference` for non-union
        // bases — the caller's `apply_guard` short-circuit reads the
        // base type via fall-through.
        assert_eq!(ty_difference_unfilled_witnesses(&Ty::String), Ty::Unknown);
        assert_eq!(ty_difference_unfilled_witnesses(&Ty::Undefined), Ty::Unknown);
    }

    #[test]
    fn is_unfilled_witness_recognizes_only_undefined_and_null() {
        assert!(is_unfilled_witness(&Ty::Undefined));
        assert!(is_unfilled_witness(&Ty::Null));
        // Value-level "empty" shapes are NOT type-level witnesses.
        assert!(!is_unfilled_witness(&Ty::String));
        assert!(!is_unfilled_witness(&Ty::Number));
        assert!(!is_unfilled_witness(&Ty::Date));
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
    fn transfer_stmt_assign_from_untyped_rhs_drops_narrowed_entry() {
        // `Х = Y` where Y has no known type (no overlay entry, no
        // base_types entry): `infer_rhs_type` yields Ty::Unknown, so
        // the assignment drops the prior narrowing of Х rather than
        // keeping stale information. Overlay contract: no Unknown
        // stored, no stale narrowing left behind.
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

    // --- Task 6.4: reassignment-locality (rhs inference) --------------------

    #[test]
    fn transfer_stmt_assign_number_literal_records_number() {
        let mut b = ExprBuilder::new();
        let x_tgt = b.path("Х");
        let num = b.alloc(Expr::Literal(Literal::Number(42.0.try_into().unwrap())));
        let assign = b.assign(x_tgt, num);

        let tr = transfer_no_bases();
        // Outer narrowing (Х: String) must be OVERWRITTEN by the
        // reassignment, not joined with it — assignment is
        // destructive.
        let state_in = state_with(&[("Х", Ty::String)]);
        let state_out = tr.transfer_stmt(assign.into_raw(), &state_in, &b.body);
        assert_eq!(state_out.get(&Name::new("Х")), Some(&Ty::Number));
    }

    #[test]
    fn transfer_stmt_assign_string_literal_records_string() {
        let mut b = ExprBuilder::new();
        let x_tgt = b.path("Х");
        let s = b.string_lit("hello");
        let assign = b.assign(x_tgt, s);

        let tr = transfer_no_bases();
        let state_out = tr.transfer_stmt(assign.into_raw(), &NarrowState::new(), &b.body);
        assert_eq!(state_out.get(&Name::new("Х")), Some(&Ty::String));
    }

    #[test]
    fn transfer_stmt_assign_undefined_literal_records_undefined() {
        // `Х = Неопределено` — the reassignment-to-Undefined case is
        // the shape users write most often and narrowing must catch it
        // (otherwise an immediately-following `Если Х <> Неопределено`
        // guard cannot see the correct pre-narrow type).
        let mut b = ExprBuilder::new();
        let x_tgt = b.path("Х");
        let und = b.undefined();
        let assign = b.assign(x_tgt, und);

        let tr = transfer_no_bases();
        let state_out = tr.transfer_stmt(assign.into_raw(), &NarrowState::new(), &b.body);
        assert_eq!(state_out.get(&Name::new("Х")), Some(&Ty::Undefined));
    }

    #[test]
    fn transfer_stmt_assign_bool_literal_records_boolean() {
        let mut b = ExprBuilder::new();
        let x_tgt = b.path("Х");
        let v = b.alloc(Expr::Literal(Literal::Bool(true)));
        let assign = b.assign(x_tgt, v);
        let tr = transfer_no_bases();
        let out = tr.transfer_stmt(assign.into_raw(), &NarrowState::new(), &b.body);
        assert_eq!(out.get(&Name::new("Х")), Some(&Ty::Boolean));
    }

    #[test]
    fn transfer_stmt_assign_date_literal_records_date() {
        let mut b = ExprBuilder::new();
        let x_tgt = b.path("Х");
        let v = b.alloc(Expr::Literal(Literal::Date("20260101".into())));
        let assign = b.assign(x_tgt, v);
        let tr = transfer_no_bases();
        let out = tr.transfer_stmt(assign.into_raw(), &NarrowState::new(), &b.body);
        assert_eq!(out.get(&Name::new("Х")), Some(&Ty::Date));
    }

    #[test]
    fn transfer_stmt_assign_null_literal_records_null() {
        let mut b = ExprBuilder::new();
        let x_tgt = b.path("Х");
        let v = b.alloc(Expr::Literal(Literal::Null));
        let assign = b.assign(x_tgt, v);
        let tr = transfer_no_bases();
        let out = tr.transfer_stmt(assign.into_raw(), &NarrowState::new(), &b.body);
        assert_eq!(out.get(&Name::new("Х")), Some(&Ty::Null));
    }

    #[test]
    fn transfer_stmt_assign_from_base_typed_rhs_records_base_type() {
        // `Х = Y` with `Y: Ty::Number` known in base_types but no
        // overlay entry for Y: the rhs type comes from base_types and
        // Х inherits it.
        let mut b = ExprBuilder::new();
        let x_tgt = b.path("Х");
        let y_val = b.path("Y");
        let assign = b.assign(x_tgt, y_val);

        let tr = transfer_with_bases(&[("Y", Ty::Number)]);
        let state_out = tr.transfer_stmt(assign.into_raw(), &NarrowState::new(), &b.body);
        assert_eq!(state_out.get(&Name::new("Х")), Some(&Ty::Number));
    }

    #[test]
    fn transfer_stmt_assign_from_narrowed_rhs_prefers_overlay_over_base() {
        // `Х = Y` where Y: base Union(Number, String) but Y is
        // *narrowed* to String in the current overlay (e.g. inside an
        // `Если ТипЗнч(Y) = Тип("Строка")` branch): the assignment
        // must propagate the narrowed String, not the base Union.
        let mut b = ExprBuilder::new();
        let x_tgt = b.path("Х");
        let y_val = b.path("Y");
        let assign = b.assign(x_tgt, y_val);

        let tr = transfer_with_bases(&[("Y", Ty::union(vec![Ty::Number, Ty::String]))]);
        let state_in = state_with(&[("Y", Ty::String)]);
        let state_out = tr.transfer_stmt(assign.into_raw(), &state_in, &b.body);
        assert_eq!(state_out.get(&Name::new("Х")), Some(&Ty::String));
    }

    #[test]
    fn transfer_stmt_assign_from_complex_rhs_drops_entry() {
        // `Х = Y + 1` — `Expr::BinaryOp` is out of Task 6.4's
        // inference scope → infer_rhs_type returns Ty::Unknown → the
        // overlay drops any prior narrowing of Х (cannot keep stale
        // info across a destructive assignment).
        let mut b = ExprBuilder::new();
        let x_tgt = b.path("Х");
        let y = b.path("Y");
        let one = b.alloc(Expr::Literal(Literal::Number(1.0.try_into().unwrap())));
        let sum = b.bin(y, one, BinaryOp::Add);
        let assign = b.assign(x_tgt, sum);

        let tr = transfer_no_bases();
        let state_in = state_with(&[("Х", Ty::String)]);
        let state_out = tr.transfer_stmt(assign.into_raw(), &state_in, &b.body);
        assert_eq!(state_out.get(&Name::new("Х")), None);
    }

    #[test]
    fn transfer_stmt_assign_literal_does_not_touch_unrelated_entries() {
        // Reassignment of Х must not perturb the narrowing of Y.
        let mut b = ExprBuilder::new();
        let x_tgt = b.path("Х");
        let num = b.alloc(Expr::Literal(Literal::Number(7.0.try_into().unwrap())));
        let assign = b.assign(x_tgt, num);

        let tr = transfer_no_bases();
        let state_in = state_with(&[("Х", Ty::String), ("Y", Ty::Boolean)]);
        let state_out = tr.transfer_stmt(assign.into_raw(), &state_in, &b.body);
        assert_eq!(state_out.get(&Name::new("Y")), Some(&Ty::Boolean));
        assert_eq!(state_out.get(&Name::new("Х")), Some(&Ty::Number));
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
        bases.insert(fold_name(&Name::new("Х")), Ty::union(vec![Ty::Number, Ty::String]));
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

    #[test]
    fn e2e_reassignment_in_then_block_records_new_type_in_out_state() {
        // Task 6.4 e2e:
        //
        //   Если ТипЗнч(Х) = Тип("Строка") Тогда
        //       Х = 42        // reassign to Number
        //   КонецЕсли
        //
        // Then-block's IN state has Х → String (from the guard).
        // After `transfer_stmt` fires on the assign, the OUT state
        // must have Х → Number — the guard's narrowing is overwritten
        // by the reassignment (destructive semantics). Without Task
        // 6.4's rhs-type inference, OUT would have Х absent from the
        // overlay ("kill" semantics), which loses the Number
        // information that downstream narrowing should be able to
        // exploit.
        let mut b = ExprBuilder::new();

        let x_arg = b.path("Х");
        let typznc = b.path("ТипЗнч");
        let lhs = b.call(typznc, vec![x_arg]);
        let tip = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tip, vec![s]);
        let condition = b.bin(lhs, rhs, BinaryOp::Eq);

        let x_tgt = b.path("Х");
        let num = b.alloc(Expr::Literal(Literal::Number(42.0.try_into().unwrap())));
        let assign = b.assign(x_tgt, num);

        let if_stmt = b.if_then(condition, assign);
        b.set_top_level(vec![if_stmt]);

        let body = b.body.clone();
        let result =
            narrow_body(body, FxHashMap::default()).expect("narrowing analysis must converge");
        let cfg = result.cfg();

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
            "IN[then-block] carries guard narrowing, got {then_in:?}"
        );

        let then_out = result
            .block_out(then_block_idx)
            .expect("then-block must have an OUT state after solving");
        assert_eq!(
            then_out.get(&Name::new("Х")),
            Some(&Ty::Number),
            "OUT[then-block] must reflect the Х = 42 reassignment, got {then_out:?}"
        );
    }

    #[test]
    fn e2e_one_sided_reassignment_does_not_leak_past_merge() {
        // Soundness regression for the Task 6.4 join fix:
        //
        //   Если ТипЗнч(Х) = Тип("Строка") Тогда
        //       Х = 42
        //   КонецЕсли
        //
        // In the then-block: Х → Number (reassignment). The else
        // path (implicit fall-through) does not touch Х — after the
        // merge, no single value can be committed for Х, so the
        // overlay MUST drop the entry. If `join` propagated the
        // one-sided `Х → Number`, readers past КонецЕсли would
        // falsely believe Х is always Number.
        //
        // We assert via the Exit vertex's IN state, which is where
        // the post-merge join lands.
        let mut b = ExprBuilder::new();

        let x_arg = b.path("Х");
        let typznc = b.path("ТипЗнч");
        let lhs = b.call(typznc, vec![x_arg]);
        let tip = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tip, vec![s]);
        let condition = b.bin(lhs, rhs, BinaryOp::Eq);

        let x_tgt = b.path("Х");
        let num = b.alloc(Expr::Literal(Literal::Number(42.0.try_into().unwrap())));
        let assign = b.assign(x_tgt, num);

        let if_stmt = b.if_then(condition, assign);
        b.set_top_level(vec![if_stmt]);

        let body = b.body.clone();
        let result =
            narrow_body(body, FxHashMap::default()).expect("narrowing analysis must converge");
        let cfg = result.cfg();

        use cfg::CfgVertex;
        let exit_idx = cfg
            .vertices()
            .find(|(_, v)| matches!(v, CfgVertex::Exit))
            .map(|(idx, _)| idx)
            .expect("CFG must contain an Exit vertex");

        let exit_in =
            result.block_in(exit_idx).expect("Exit vertex must have an IN state after solving");
        assert_eq!(
            exit_in.get(&Name::new("Х")),
            None,
            "one-sided `Х → Number` must NOT survive the post-КонецЕсли merge, got {exit_in:?}"
        );
    }

    // ── Task 6.5: narrowed_type_at reader (pre-narrow on guard receivers,
    // narrowed on then/else-body expressions).
    //
    // Builds a canonical if-else shape once and captures the ExprIdx
    // values we want to probe — the receiver `Х` inside `ТипЗнч(Х)`,
    // the `Х` inside the then-body's `Х = Х`, and same in the
    // else-body. The tests below reuse this helper to pin down
    // Task 6.5's lookup rule on each position.
    struct NarrowProbe {
        result: dataflow::DataflowResult<NarrowState>,
        then_body_path: ExprIdx,
        else_body_path: Option<ExprIdx>,
    }

    fn build_probe_if_then_else(bases: FxHashMap<Name, Ty>) -> NarrowProbe {
        let mut b = ExprBuilder::new();

        let receiver = b.path("Х");
        let typznc = b.path("ТипЗнч");
        let lhs = b.call(typznc, vec![receiver]);
        let tip = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tip, vec![s]);
        let condition = b.bin(lhs, rhs, BinaryOp::Eq);

        // Then-body: `Х = Х`. `then_body_path` is the rhs `Х` —
        // reading it sees the narrowed overlay from the TrueBranch
        // edge applied to the then-block's IN state.
        let then_lhs = b.path("Х");
        let then_body_path = b.path("Х");
        let then_assign = b.assign(then_lhs, then_body_path);

        // Else-body: `Х = Х`. `else_body_path` is the rhs `Х` —
        // reading it sees the FalseBranch complement applied to the
        // else-block's IN state.
        let else_lhs = b.path("Х");
        let else_body_path = b.path("Х");
        let else_assign = b.assign(else_lhs, else_body_path);

        let if_stmt = b.if_then_else(condition, then_assign, else_assign);
        b.set_top_level(vec![if_stmt]);

        let body = b.body.clone();
        let result = narrow_body(body, bases).expect("narrowing analysis must converge");
        NarrowProbe { result, then_body_path, else_body_path: Some(else_body_path) }
    }

    fn build_probe_if_then_only() -> NarrowProbe {
        let mut b = ExprBuilder::new();

        let receiver = b.path("Х");
        let typznc = b.path("ТипЗнч");
        let lhs = b.call(typznc, vec![receiver]);
        let tip = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tip, vec![s]);
        let condition = b.bin(lhs, rhs, BinaryOp::Eq);

        let then_lhs = b.path("Х");
        let then_body_path = b.path("Х");
        let then_assign = b.assign(then_lhs, then_body_path);

        let if_stmt = b.if_then(condition, then_assign);
        b.set_top_level(vec![if_stmt]);

        let body = b.body.clone();
        let result =
            narrow_body(body, FxHashMap::default()).expect("narrowing analysis must converge");
        NarrowProbe { result, then_body_path, else_body_path: None }
    }

    #[test]
    fn narrowed_type_at_guard_receiver_returns_pre_narrow() {
        // ADR-01 Q4 (stronger shape, per pair-review MUST-FIX):
        //
        //     Х = 42                                          // entry BB
        //     Если ТипЗнч(Х) = Тип("Строка") Тогда            // Conditional
        //         Х = Х                                       // then-block
        //     КонецЕсли
        //
        // The preceding assignment seeds the overlay with
        // `Х → Number` BEFORE the Если, so `block_in` of the
        // Conditional vertex is a non-empty overlay. A hover on the
        // receiver `Х` inside `ТипЗнч(Х)` must return
        // `Some(Ty::Number)` — the pre-narrow value from the prior
        // assignment.
        //
        // The weaker "empty-overlay → None" shape (the original
        // Task 6.5 test) could pass even if `containing_vertex`
        // failed to locate the receiver (returning `None`
        // short-circuits `narrowed_type_at` to `None` too). This
        // version distinguishes three possible regressions:
        //   - `Some(Ty::Number)` — correct (pre-narrow).
        //   - `Some(Ty::String)` — dispatched to then-block's IN
        //     state (post-narrow bug).
        //   - `None` — dispatched nowhere (walk missed the vertex).
        let mut b = ExprBuilder::new();

        let pre_target = b.path("Х");
        let num42 = b.alloc(Expr::Literal(Literal::Number(42.0.try_into().unwrap())));
        let pre_assign = b.assign(pre_target, num42);

        let receiver = b.path("Х");
        let typznc = b.path("ТипЗнч");
        let lhs = b.call(typznc, vec![receiver]);
        let tip = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tip, vec![s]);
        let condition = b.bin(lhs, rhs, BinaryOp::Eq);

        let then_lhs = b.path("Х");
        let then_rhs = b.path("Х");
        let then_assign = b.assign(then_lhs, then_rhs);
        let if_stmt = b.if_then(condition, then_assign);

        b.set_top_level(vec![pre_assign, if_stmt]);

        let body = b.body.clone();
        let result =
            narrow_body(body, FxHashMap::default()).expect("narrowing analysis must converge");

        assert_eq!(
            narrowed_type_at(&result, receiver, &Name::new("Х")),
            Some(Ty::Number),
            "receiver must see pre-narrow overlay (Х → Number from prior assign), NOT the post-narrow String from the guard"
        );

        // Control: the then-body rhs still sees the TrueBranch
        // narrowing. Pinning this in the same test guards against a
        // future regression that disables narrowing wholesale.
        assert_eq!(
            narrowed_type_at(&result, then_rhs, &Name::new("Х")),
            Some(Ty::String),
            "then-body Х must see the guard's narrowing to Строка"
        );
    }

    #[test]
    fn narrowed_type_at_then_body_sees_narrowed() {
        // A read of `Х` inside the then-body sees the narrowed overlay
        // applied by the TrueBranch edge. This is the primary
        // user-facing win of narrowing: hover on `Х` inside `Если
        // ТипЗнч(Х) = Тип("Строка") Тогда … КонецЕсли` reports
        // `Строка`, not the original union / unknown.
        let probe = build_probe_if_then_else(FxHashMap::default());
        assert_eq!(
            narrowed_type_at(&probe.result, probe.then_body_path, &Name::new("Х")),
            Some(Ty::String),
            "then-body Х must carry the TrueBranch narrowing Х → Строка"
        );
    }

    #[test]
    fn narrowed_type_at_else_body_sees_complement() {
        // Binary union base seeded (`Union(Number, String)`). On the
        // FalseBranch of `ТипЗнч(Х) = Тип("Строка")`, Task 6.3's
        // `ty_difference` collapses the union to the remaining
        // member, so the else-block's IN state pins Х → Number.
        let mut bases = FxHashMap::default();
        bases.insert(fold_name(&Name::new("Х")), Ty::union(vec![Ty::Number, Ty::String]));

        let probe = build_probe_if_then_else(bases);
        let else_expr = probe.else_body_path.expect("else branch is present");
        assert_eq!(
            narrowed_type_at(&probe.result, else_expr, &Name::new("Х")),
            Some(Ty::Number),
            "else-body Х must carry the FalseBranch complement Union(Number,String) \\ String = Number"
        );
    }

    #[test]
    fn narrowed_type_at_untouched_var_returns_none() {
        // Looking up a name the analysis never narrows must return
        // `None` at every position — even in positions where *other*
        // names are narrowed. Protects against accidentally leaking
        // one variable's overlay to another.
        let probe = build_probe_if_then_else(FxHashMap::default());
        assert_eq!(
            narrowed_type_at(&probe.result, probe.then_body_path, &Name::new("Y")),
            None,
            "unrelated variable Y must not pick up any narrowing"
        );
    }

    #[test]
    fn narrowed_type_at_after_konec_esli_drops_one_sided_narrowing() {
        // Task 6.4 intersection-join: one-sided narrowings (only the
        // then-block narrows Х, the else is absent) must NOT survive
        // the post-КонецЕсли merge. We don't have a convenient
        // post-merge expression in the `if_then_only` probe (the only
        // expr after the If is the method's implicit Exit vertex, and
        // no expressions live there), so we assert directly on the
        // Exit vertex's IN state: `narrowed_type_at` is just a
        // syntactic re-projection of `block_in`, and this test closes
        // the loop with the Exit-vertex assertion the existing
        // `e2e_one_sided_reassignment_does_not_leak_past_merge`
        // exercises through the same channel.
        use cfg::CfgVertex;

        let probe = build_probe_if_then_only();
        let cfg = probe.result.cfg();
        let exit_idx = cfg
            .vertices()
            .find(|(_, v)| matches!(v, CfgVertex::Exit))
            .map(|(idx, _)| idx)
            .expect("CFG must contain an Exit vertex");
        let exit_in = probe.result.block_in(exit_idx).expect("Exit IN must be populated");
        assert_eq!(
            exit_in.get(&Name::new("Х")),
            None,
            "one-sided narrowing Х → Строка must drop at post-КонецЕсли merge (intersection join)"
        );
    }

    #[test]
    fn narrowed_type_at_missing_expr_returns_none() {
        // Safety: when `expr_idx` isn't reachable from any CFG vertex
        // (here, a stray expression we allocate but never wire into
        // any statement or vertex), `narrowed_type_at` must return
        // `None` rather than panicking or picking a random vertex.
        let probe = build_probe_if_then_else(FxHashMap::default());
        let stray_expr = Idx::<Expr>::from_raw(RawIdx::from(u32::MAX - 1));
        assert_eq!(
            narrowed_type_at(&probe.result, stray_expr, &Name::new("Х")),
            None,
            "expression not reachable from any CFG vertex must return None"
        );
    }

    #[test]
    fn narrowed_type_at_elsif_condition_receiver_sees_pre_narrow() {
        // Per pair-review NIT: exercise the secondary Conditional
        // vertex that `ИначеЕсли` produces.
        //
        //     Если ТипЗнч(Х) = Тип("Строка") Тогда
        //         Х = Х
        //     ИначеЕсли ТипЗнч(Х) = Тип("Дата") Тогда     // ← receiver
        //         Х = Х
        //     КонецЕсли
        //
        // With base_types seeded as `Х: Union(Number, String)`, the
        // FalseBranch from the first Conditional applies
        // `Union(Number, String) \ String = Number`. The ИначеЕсли
        // Conditional's IN state therefore carries `Х → Number`,
        // which is the pre-narrow type the receiver must see (as
        // distinct from the elsif's own narrowing target, `Дата`).
        let mut b = ExprBuilder::new();

        let x1 = b.path("Х");
        let typznc1 = b.path("ТипЗнч");
        let lhs1 = b.call(typznc1, vec![x1]);
        let tip1 = b.path("Тип");
        let s1 = b.string_lit("Строка");
        let rhs1 = b.call(tip1, vec![s1]);
        let cond1 = b.bin(lhs1, rhs1, BinaryOp::Eq);

        let then_lhs = b.path("Х");
        let then_rhs = b.path("Х");
        let then_assign = b.assign(then_lhs, then_rhs);

        let elsif_receiver = b.path("Х");
        let typznc2 = b.path("ТипЗнч");
        let lhs2 = b.call(typznc2, vec![elsif_receiver]);
        let tip2 = b.path("Тип");
        let s2 = b.string_lit("Дата");
        let rhs2 = b.call(tip2, vec![s2]);
        let cond2 = b.bin(lhs2, rhs2, BinaryOp::Eq);

        let elsif_lhs = b.path("Х");
        let elsif_rhs = b.path("Х");
        let elsif_assign = b.assign(elsif_lhs, elsif_rhs);

        let if_stmt = b.if_then_elsif(cond1, then_assign, cond2, elsif_assign);
        b.set_top_level(vec![if_stmt]);

        let body = b.body.clone();
        let mut bases = FxHashMap::default();
        bases.insert(fold_name(&Name::new("Х")), Ty::union(vec![Ty::Number, Ty::String]));
        let result = narrow_body(body, bases).expect("narrowing analysis must converge");

        assert_eq!(
            narrowed_type_at(&result, elsif_receiver, &Name::new("Х")),
            Some(Ty::Number),
            "elsif-condition receiver must see the FalseBranch-complement from the first Conditional (Number), not its own elsif narrowing target (Дата)"
        );

        // Control: inside the elsif's then-body, the overlay is
        // narrowed to the elsif's target type (Дата).
        assert_eq!(
            narrowed_type_at(&result, elsif_rhs, &Name::new("Х")),
            Some(Ty::Date),
            "elsif then-body Х must see the TrueBranch narrowing to Дата"
        );
    }

    #[test]
    fn narrowed_type_at_while_condition_receiver_sees_pre_narrow() {
        // Per pair-review NIT: exercise the `WhileLoop` vertex.
        //
        //     Х = 42
        //     Пока ТипЗнч(Х) = Тип("Строка") Цикл   // ← receiver
        //         Х = Х
        //     КонецЦикла
        //
        // `WhileLoop` vertex has two in-edges: the direct edge from
        // the entry block (carrying `Х → Number` from the prior
        // assignment) and the `LoopIteration` back-edge from the
        // body (carrying `Х → String`, because the TrueBranch edge
        // applied the guard on the first iteration). The join is
        // `Ty::union([Number, String])`, which is the pre-narrow
        // overlay the receiver must observe — *not* the String the
        // body sees inside each iteration, and not the Number from
        // the initial entry alone.
        let mut b = ExprBuilder::new();

        let pre_target = b.path("Х");
        let num42 = b.alloc(Expr::Literal(Literal::Number(42.0.try_into().unwrap())));
        let pre_assign = b.assign(pre_target, num42);

        let receiver = b.path("Х");
        let typznc = b.path("ТипЗнч");
        let lhs = b.call(typznc, vec![receiver]);
        let tip = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tip, vec![s]);
        let condition = b.bin(lhs, rhs, BinaryOp::Eq);

        let body_lhs = b.path("Х");
        let body_rhs = b.path("Х");
        let body_assign = b.assign(body_lhs, body_rhs);

        let while_stmt = b.while_stmt(condition, body_assign);
        b.set_top_level(vec![pre_assign, while_stmt]);

        let body = b.body.clone();
        let result =
            narrow_body(body, FxHashMap::default()).expect("narrowing analysis must converge");

        assert_eq!(
            narrowed_type_at(&result, receiver, &Name::new("Х")),
            Some(Ty::union(vec![Ty::Number, Ty::String])),
            "while-condition receiver must see the merged pre-narrow overlay Union(Number, String), not either side in isolation"
        );

        // Control: inside the loop body, the guard's narrowing to
        // Строка is visible through the TrueBranch edge.
        assert_eq!(
            narrowed_type_at(&result, body_rhs, &Name::new("Х")),
            Some(Ty::String),
            "while-body Х must see the TrueBranch narrowing to Строка"
        );
    }
}
