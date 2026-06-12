use std::sync::Arc;
use stdx::case::CaseExt;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnownValue {
    Bool(bool),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValueOverlay(Inner);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Inner {
    #[default]
    Unreachable,
    Reachable(FxHashMap<SmolStr, KnownValue>),
}

impl ValueOverlay {
    pub fn unreachable() -> Self {
        Self(Inner::Unreachable)
    }

    pub fn empty() -> Self {
        Self(Inner::Reachable(FxHashMap::default()))
    }

    pub fn get(&self, name: &str) -> Option<KnownValue> {
        match &self.0 {
            Inner::Unreachable => None,
            Inner::Reachable(map) => map.get(&lowercase_key(name)).copied(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.0 {
            Inner::Unreachable => true,
            Inner::Reachable(map) => map.is_empty(),
        }
    }

    pub fn is_reachable(&self) -> bool {
        matches!(self.0, Inner::Reachable(_))
    }

    fn set(&mut self, name: &str, value: KnownValue) {
        debug_assert!(
            self.is_reachable(),
            "ValueOverlay::set called on Unreachable — caller must transition explicitly",
        );
        if let Inner::Reachable(map) = &mut self.0 {
            map.insert(lowercase_key(name), value);
        }
    }

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

#[derive(Debug, Default, Clone, Copy)]
pub struct ValueStateProvider;

impl Transfer<ValueOverlay> for ValueStateProvider {
    fn transfer_stmt(&self, stmt_id: RawIdx, state: &ValueOverlay, body: &Body) -> ValueOverlay {
        let mut next = state.clone();
        step(&mut next, body, StmtId::from_raw(stmt_id));
        next
    }

    fn transfer_loop_var_bind(&self, loop_var: BindingId, state: &mut ValueOverlay, body: &Body) {
        if !state.is_reachable() {
            return;
        }
        let name = body.binding_idx(loop_var.to_idx()).name.as_str();
        state.kill(name);
    }
}

pub fn step(state: &mut ValueOverlay, body: &Body, stmt_id: StmtId) {
    if !state.is_reachable() {
        return;
    }
    if let Stmt::Assign { target, value } = body.stmt(stmt_id) {
        apply_assignment(state, body, *target, *value);
    }
}

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
    let Some(name) = expect_path_name(body, target) else {
        return;
    };
    match body.expr(ExprId::from_idx(value)) {
        Expr::Literal(Literal::Bool(b)) => state.set(name.as_str(), KnownValue::Bool(*b)),
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
    SmolStr::new(name.fold_lower())
}

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

    #[test]
    fn join_idempotent() {
        let a = overlay(&[("x", true), ("y", false)]);
        assert_eq!(a.join(&a), a);
    }

    #[test]
    fn join_commutative() {
        let a = overlay(&[("x", true)]);
        let b = overlay(&[("y", false)]);
        assert_eq!(a.join(&b), b.join(&a));
    }

    #[test]
    fn join_associative() {
        let a = overlay(&[("x", true), ("y", true)]);
        let b = overlay(&[("y", true), ("z", false)]);
        let c = overlay(&[("x", true), ("z", false)]);
        assert_eq!(a.join(&b).join(&c), a.join(&b.join(&c)));
    }

    #[test]
    fn unreachable_is_join_identity() {
        let a = overlay(&[("x", true)]);
        assert_eq!(ValueOverlay::unreachable().join(&a), a);
        assert_eq!(a.join(&ValueOverlay::unreachable()), a);
    }

    #[test]
    fn empty_reachable_intersects_to_empty() {
        let a = overlay(&[("x", true)]);
        let merged = ValueOverlay::empty().join(&a);
        assert_eq!(merged, ValueOverlay::empty());
    }

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
        assert_eq!(bot.join(&a).join(&b), bot.join(&a.join(&b)));
    }

    #[test]
    fn join_keeps_agreed_binding() {
        let a = overlay(&[("x", true)]);
        let b = overlay(&[("x", true), ("y", false)]);
        let merged = a.join(&b);
        assert_eq!(merged.get("x"), Some(KnownValue::Bool(true)));
        assert!(merged.get("y").is_none());
    }

    #[test]
    fn join_conflict_kills_binding() {
        let a = overlay(&[("x", true)]);
        let b = overlay(&[("x", false)]);
        let merged = a.join(&b);
        assert!(merged.get("x").is_none());
    }
}
