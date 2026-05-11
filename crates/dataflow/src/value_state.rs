//! Value-state overlay — per-variable forward constant propagation.
//!
//! Tracks which local variables are known to hold a specific literal value
//! at each program point. Today only [`KnownValue::Bool`] is modelled —
//! that is the minimum needed by `dataflow::security_state` (§1.2) to
//! disambiguate calls like
//! `Значение = Истина; УстановитьПривилегированныйРежим(Значение);` —
//! but the enum is structured so number / string literals can be added
//! when a future analysis needs them.
//!
//! # Layer rules
//!
//! Lives in `dataflow` (not `hir-ty`) on purpose. The plan's round-2 fix
//! (B3) showed that putting `value_state` in `hir-ty` would create a
//! `dataflow → hir-ty` cycle, since `dataflow` is a leaf of the semantic
//! stack and `hir-ty` already depends on `dataflow`. The analysis here
//! is purely structural over `Body`/`Stmt`/`Expr`; it does not need
//! types or resolver context.
//!
//! # Scope
//!
//! - Intra-method only. Assignment-through-call (`Установить(Значение)`)
//!   does not propagate.
//! - Equal-only join: at a CFG merge, a binding is retained only if all
//!   incoming edges agree on its value. Disagreement kills the binding,
//!   matching the standard "agreement → known, conflict → unknown"
//!   semantics of forward constant-propagation lattices.
//! - Non-literal RHS kills the binding ("we don't know any more"). This
//!   is conservative; future widening could keep it alive across call
//!   chains, but that's deliberately deferred (see §9.3 of the master
//!   plan).

use std::sync::Arc;

use cfg::{ControlFlowGraph, NodeIndex};
use hir_def::{
    body::Body,
    hir::{Expr, ExprIdx, Literal, Stmt},
    BindingId, ExprId, IdConversion, Name, StmtId,
};
use la_arena::RawIdx;
use rustc_hash::FxHashMap;
use smol_str::SmolStr;

use crate::{DataflowSolver, Direction, Lattice, Transfer};

/// A single known value attached to a local variable. Extensible: today
/// only booleans are tracked because §1.2's `recognize_call` is the only
/// caller. New variants must preserve the `Eq + Hash + Clone` bounds the
/// lattice depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnownValue {
    /// `Истина` / `True` / `Ложь` / `False` literal that propagated to
    /// this point unchanged.
    Bool(bool),
}

/// Forward value-state overlay.
///
/// Opaque newtype wrapper around a private two-level enum so the public
/// surface stays read-only:
///
/// - The lattice bottom is reachable only via [`ValueOverlay::unreachable`].
///   The solver seeds non-entry blocks with this and the join treats it
///   as the identity (`Unreachable.join(a) = a`) — without a dedicated
///   bottom, an empty `Reachable` map would be mis-interpreted as
///   "every variable is known but disagrees with everything" and would
///   erase all known bindings on the first merge.
/// - A reachable point with a known-value map is reachable only via
///   [`ValueOverlay::empty`] (entry seed) or as the result of a transfer
///   step. A binding's *presence* means "definitely this value at every
///   path that reaches here"; *absence* means "no agreement / unknown".
///
/// External code cannot pattern-match the inner enum or mutate the map
/// directly — see the `Inner` definition (private) below. The only
/// public mutator is [`step`], which honours the no-promotion contract.
///
/// This mirrors the planned `SecurityModeState::{Unreachable, Reachable}`
/// in §1.2 of the master plan; the lattice-law unit tests below mirror
/// the §1.7 lattice-laws test referenced there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValueOverlay(Inner);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Inner {
    /// Bottom element. The solver seeds every non-entry block with this
    /// before the worklist runs.
    #[default]
    Unreachable,
    /// Reachable program point with the listed known-value bindings.
    Reachable(FxHashMap<SmolStr, KnownValue>),
}

impl ValueOverlay {
    /// The lattice bottom. Use this in `set_bottom_factory`.
    pub fn unreachable() -> Self {
        Self(Inner::Unreachable)
    }

    /// A reachable point with no known-value bindings yet. Use this in
    /// `set_initial_state` for the entry block.
    pub fn empty() -> Self {
        Self(Inner::Reachable(FxHashMap::default()))
    }

    /// Look up a variable's known value at this point. Names are
    /// case-insensitive in BSL — callers do NOT lowercase first; the
    /// helper does it. Returns `None` for `Unreachable` and for
    /// `Reachable` points where the name has no agreed-on value.
    pub fn get(&self, name: &str) -> Option<KnownValue> {
        match &self.0 {
            Inner::Unreachable => None,
            Inner::Reachable(map) => map.get(&lowercase_key(name)).copied(),
        }
    }

    /// `true` when this is `Unreachable` or a `Reachable` map with no
    /// bindings.
    pub fn is_empty(&self) -> bool {
        match &self.0 {
            Inner::Unreachable => true,
            Inner::Reachable(map) => map.is_empty(),
        }
    }

    /// `true` when this point is reachable.
    pub fn is_reachable(&self) -> bool {
        matches!(self.0, Inner::Reachable(_))
    }

    /// Record `name → value`. **Caller must already be `Reachable`** —
    /// the transfer function only fires after the solver has joined
    /// predecessors into a non-`Unreachable` IN-state, so promoting
    /// from `Unreachable` would mask a logical bug elsewhere. In debug
    /// builds this asserts; in release it is a silent no-op (preferring
    /// liveness over a panic if a future caller misuses the API).
    fn set(&mut self, name: &str, value: KnownValue) {
        debug_assert!(
            self.is_reachable(),
            "ValueOverlay::set called on Unreachable — caller must transition explicitly",
        );
        if let Inner::Reachable(map) = &mut self.0 {
            map.insert(lowercase_key(name), value);
        }
    }

    /// Drop any known value for `name`. Same `Unreachable` contract as
    /// [`Self::set`].
    fn kill(&mut self, name: &str) {
        debug_assert!(
            self.is_reachable(),
            "ValueOverlay::kill called on Unreachable — caller must transition explicitly",
        );
        if let Inner::Reachable(map) = &mut self.0 {
            map.remove(&lowercase_key(name));
        }
    }
}

impl Lattice for ValueOverlay {
    /// Equal-only join. `Unreachable` is the identity. For two
    /// `Reachable` operands, a binding survives the merge iff every
    /// incoming edge agrees on its exact value; any disagreement (or
    /// one-sided absence) drops the binding.
    fn join(&self, other: &Self) -> Self {
        let inner = match (&self.0, &other.0) {
            (Inner::Unreachable, x) | (x, Inner::Unreachable) => x.clone(),
            (Inner::Reachable(a), Inner::Reachable(b)) => {
                let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
                let mut out = FxHashMap::default();
                out.reserve(small.len());
                for (name, value) in small {
                    if large.get(name) == Some(value) {
                        out.insert(name.clone(), *value);
                    }
                }
                Inner::Reachable(out)
            }
        };
        Self(inner)
    }
}

/// Transfer function for the value-state lattice.
///
/// Stateless — all dependencies (`Body`) are passed through the trait
/// API by the solver. Constructed via `ValueStateProvider::default()`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ValueStateProvider;

impl Transfer<ValueOverlay> for ValueStateProvider {
    fn transfer_stmt(&self, stmt_id: RawIdx, state: &ValueOverlay, body: &Body) -> ValueOverlay {
        let mut next = state.clone();
        step(&mut next, body, StmtId::from_raw(stmt_id));
        next
    }

    fn transfer_loop_var_bind(&self, loop_var: BindingId, state: &mut ValueOverlay, body: &Body) {
        // The CFG models `Стмт::For`/`Стмт::ForEach` as a standalone
        // vertex, not as a statement inside any basic block, so
        // `transfer_stmt` is never invoked for the loop header. The
        // solver calls this hook instead — kill any prior value-fact
        // for the rebound loop variable so a previously-known bool
        // does not bleed across the rebind into the loop body.
        if !state.is_reachable() {
            return;
        }
        let name = body.binding_idx(loop_var.to_idx()).name.as_str();
        state.kill(name);
    }
}

/// Apply one statement to `state` in-place. This is the public form of
/// the transfer function — §1.2 uses it to replay statements within a
/// basic block when the §1.2 transfer needs the value-state at a point
/// *after* a same-block assignment but *before* a same-block call.
///
/// `Unreachable` states pass through unchanged (no statement reasons
/// about an unreachable point).
pub fn step(state: &mut ValueOverlay, body: &Body, stmt_id: StmtId) {
    if !state.is_reachable() {
        return;
    }
    if let Stmt::Assign { target, value } = body.stmt(stmt_id) {
        apply_assignment(state, body, *target, *value);
    }
    // `Стмт::For` / `Стмт::ForEach` are NOT lowered into any basic
    // block — the CFG materialises them as `ForLoopVertex` /
    // `ForEachLoopVertex`, where the solver invokes
    // `Transfer::transfer_loop_var_bind` (default impl above) to kill
    // the rebound binding. So this `step` only sees `Стмт::Assign`
    // (and a handful of stmt kinds with no value-state effect).
}

/// Replay the value-state forward through a slice of basic-block
/// statements, starting from `block_in` and applying each statement in
/// order up to (but **not** including) `until_idx`. The returned overlay
/// is the value-state at the program point just before
/// `statements[until_idx]` executes — exactly what §1.2's transfer
/// needs when it sees a `SetPrivilegedMode(arg)` call mid-block and
/// wants to know whether `arg` was assigned a literal earlier in the
/// same block.
///
/// `until_idx == 0` returns a clone of `block_in`; `until_idx ==
/// statements.len()` is the block-exit state (equivalent to the
/// solver's `block_out`).
pub fn replay_within_block(
    block_in: &ValueOverlay,
    body: &Body,
    statements: &[StmtId],
    until_idx: usize,
) -> ValueOverlay {
    debug_assert!(
        until_idx <= statements.len(),
        "replay_within_block: until_idx={} out of range for {} statements",
        until_idx,
        statements.len(),
    );
    let mut state = block_in.clone();
    for &stmt_id in statements.iter().take(until_idx) {
        step(&mut state, body, stmt_id);
    }
    state
}

fn apply_assignment(state: &mut ValueOverlay, body: &Body, target: ExprIdx, value: ExprIdx) {
    // Only direct local-variable assignments are tracked. Field /
    // index / call assignments are conservatively ignored: they cannot
    // *introduce* a known value and they cannot *kill* one (the binding
    // they touch is not a local).
    let Some(name) = expect_path_name(body, target) else {
        return;
    };
    match body.expr(ExprId::from_idx(value)) {
        Expr::Literal(Literal::Bool(b)) => state.set(name.as_str(), KnownValue::Bool(*b)),
        // Any non-bool-literal RHS makes the binding unknown. We could
        // refine this later (e.g. propagate through `Перем y = x;` when
        // `x` is known), but the §1.2 caller only needs literal/unknown.
        _ => state.kill(name.as_str()),
    }
}

fn expect_path_name(body: &Body, expr: ExprIdx) -> Option<&Name> {
    match body.expr(ExprId::from_idx(expr)) {
        Expr::Path(name) => Some(name),
        _ => None,
    }
}

fn lowercase_key(name: &str) -> SmolStr {
    SmolStr::new(name.to_lowercase())
}

/// Run the value-state analysis over a method body and return the
/// solver result, or `None` if the worklist failed to converge within
/// `DEFAULT_MAX_ITERATIONS`.
///
/// `cfg` is shared via `Arc` so callers (notably §1.2's
/// `security_state`) can reuse the same graph between passes.
pub fn analyze(
    cfg: Arc<ControlFlowGraph>,
    body: Body,
) -> Option<crate::DataflowResult<ValueOverlay>> {
    let mut solver = DataflowSolver::new(cfg, body, ValueStateProvider);
    solver.set_direction(Direction::Forward);
    solver.set_bottom_factory(ValueOverlay::unreachable);
    solver.set_initial_state(ValueOverlay::empty());
    solver.solve()
}

/// Look up a binding's known value at the entry of `block`. The §1.2
/// transfer iterates basic-block statements itself, so this is the
/// natural granularity to expose: the block's IN-state captures every
/// merge-point fact the call site can rely on.
pub fn known_value_at_block_entry(
    result: &crate::DataflowResult<ValueOverlay>,
    block: NodeIndex,
    name: &str,
) -> Option<KnownValue> {
    result.block_in(block)?.get(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlay(pairs: &[(&str, bool)]) -> ValueOverlay {
        let mut o = ValueOverlay::empty();
        for (name, value) in pairs {
            // Test fixture only — `set` is module-private, so this stays
            // inside the module-level `tests` mod.
            o.set(name, KnownValue::Bool(*value));
        }
        o
    }

    #[test]
    fn unreachable_is_default_and_empty() {
        let bot = ValueOverlay::unreachable();
        assert!(bot.get("x").is_none());
        assert!(bot.is_empty());
        assert!(!bot.is_reachable());
        assert_eq!(ValueOverlay::default(), ValueOverlay::unreachable());
    }

    #[test]
    fn empty_lookup_returns_none() {
        assert!(ValueOverlay::empty().get("x").is_none());
    }

    #[test]
    fn set_then_get_round_trips_case_insensitively() {
        let mut o = ValueOverlay::empty();
        o.set("Значение", KnownValue::Bool(true));
        assert_eq!(o.get("значение"), Some(KnownValue::Bool(true)));
        assert_eq!(o.get("ЗНАЧЕНИЕ"), Some(KnownValue::Bool(true)));
    }

    #[test]
    fn kill_drops_binding() {
        let mut o = overlay(&[("x", true)]);
        o.kill("X");
        assert!(o.get("x").is_none());
    }

    // Lattice law: idempotence — `a.join(a) == a`.
    #[test]
    fn join_idempotent() {
        let a = overlay(&[("x", true), ("y", false)]);
        assert_eq!(a.join(&a), a);
    }

    // Lattice law: commutativity — `a.join(b) == b.join(a)`.
    #[test]
    fn join_commutative() {
        let a = overlay(&[("x", true)]);
        let b = overlay(&[("y", false)]);
        assert_eq!(a.join(&b), b.join(&a));
    }

    // Lattice law: associativity — `(a.join(b)).join(c) == a.join(b.join(c))`.
    #[test]
    fn join_associative() {
        let a = overlay(&[("x", true), ("y", true)]);
        let b = overlay(&[("y", true), ("z", false)]);
        let c = overlay(&[("x", true), ("z", false)]);
        assert_eq!(a.join(&b).join(&c), a.join(&b.join(&c)));
    }

    // Lattice law: bottom identity — `bottom.join(a) == a`. The bottom
    // is `Unreachable`, NOT the empty `Reachable` map — see the type
    // doc-comment for why the two are distinct.
    #[test]
    fn unreachable_is_join_identity() {
        let a = overlay(&[("x", true)]);
        assert_eq!(ValueOverlay::unreachable().join(&a), a);
        assert_eq!(a.join(&ValueOverlay::unreachable()), a);
    }

    // Empty `Reachable` is also a left/right identity ONLY when the
    // other operand is `Reachable` and shares no bindings: empty agrees
    // with empty trivially. Pin the asymmetry.
    #[test]
    fn empty_reachable_intersects_to_empty() {
        let a = overlay(&[("x", true)]);
        let merged = ValueOverlay::empty().join(&a);
        assert_eq!(merged, ValueOverlay::empty());
    }

    // Symmetric Unreachable cases that round out the lattice contract
    // (Codex round-1 §1.3 MINOR: laws hold but tests didn't exercise
    // every Unreachable pairing).
    #[test]
    fn unreachable_join_unreachable_is_unreachable() {
        let bot = ValueOverlay::unreachable();
        assert_eq!(bot.join(&bot), bot);
    }

    #[test]
    fn unreachable_idempotent() {
        let bot = ValueOverlay::unreachable();
        assert_eq!(bot.join(&bot.clone()), bot);
    }

    #[test]
    fn unreachable_associative_with_reachable() {
        let bot = ValueOverlay::unreachable();
        let a = overlay(&[("x", true)]);
        let b = overlay(&[("x", true), ("y", false)]);
        // (⊥ ⊔ a) ⊔ b == ⊥ ⊔ (a ⊔ b)
        assert_eq!(bot.join(&a).join(&b), bot.join(&a.join(&b)));
    }

    // Equal-only semantics: agreement preserves the binding.
    #[test]
    fn join_keeps_agreed_binding() {
        let a = overlay(&[("x", true)]);
        let b = overlay(&[("x", true), ("y", false)]);
        let merged = a.join(&b);
        assert_eq!(merged.get("x"), Some(KnownValue::Bool(true)));
        // y is only in `b` — equal-only join drops it.
        assert!(merged.get("y").is_none());
    }

    // Equal-only semantics: conflict drops the binding.
    #[test]
    fn join_conflict_kills_binding() {
        let a = overlay(&[("x", true)]);
        let b = overlay(&[("x", false)]);
        let merged = a.join(&b);
        assert!(merged.get("x").is_none());
    }
}
