//! Guard-predicate detector (Track 2 §1.5).
//!
//! Backward path-sensitive search through the CFG: given a statement
//! that performs a sensitive call (e.g. a privileged-module method
//! invocation), determine whether **every** path from the method's
//! entry point to that statement passes through a recognised guard
//! predicate's true branch.
//!
//! Today the only consumer is the §1.6
//! `PrivilegedModuleMethodCall` handler, which suppresses its
//! diagnostic when the call is guarded. The plan keeps the API
//! deliberately narrow until a second consumer materialises:
//!
//! - **Single registry shape.** A small handful of well-known guard
//!   predicates is hardcoded in [`default_registry`]; future ITS
//!   audits can extend the list without touching the algorithm.
//! - **Single semantic class for the algorithm.** The detector
//!   recognises *any* registered guard call as "guard true ⇒ caller
//!   is permitted". Distinguishing role-vs-user-vs-other is reserved
//!   for diagnostic messaging, not flow analysis (§1.5 plan note).
//!
//! # Algorithm
//!
//! "Must-be-guarded" backward DFS from the basic block containing the
//! call statement to the CFG entry point. A path is *guarded* if it
//! passes through a `Conditional` vertex's `TrueBranch` whose
//! condition expression is recognised as a guard call. The call is
//! guarded iff every path is guarded.
//!
//! Cycles (loop back-edges) terminate the DFS optimistically — by
//! definition any guard along an acyclic prefix already covers the
//! reachable callsite, so visiting the same block twice cannot
//! discover a NEW un-guarded path.
//!
//! Dead-code edges (`AdjacentCode`) are ignored — they don't
//! represent real flow.
//!
//! # Soundness
//!
//! The detector is sound *for use as a diagnostic suppressor*: it
//! reports `true` (guarded) only when the protection is provable on
//! every path. False negatives (a guarded call reported as
//! un-guarded) are acceptable: the diagnostic surfaces the call
//! anyway, biasing toward security alerts. False positives (an
//! un-guarded call reported as guarded) would silently swallow real
//! warnings — those are NOT acceptable, and the algorithm is
//! conservative in that direction.

use cfg::{CfgEdgeType, CfgVertex, ControlFlowGraph};
use cfg_types::{ExprId, StmtId};
use hir_def::{
    body::Body,
    hir::{BinaryOp, Expr},
    IdConversion, Name,
};
use petgraph::graph::NodeIndex;
use rustc_hash::FxHashSet;

/// Semantic class of a guard predicate.
///
/// **Algorithm scope today:** only `RoleCheck` and `PrivilegedQuery`
/// entries are recognised by [`condition_matches_guard`]. Their call
/// shape — bare `<Predicate>(args...)` — is sufficient evidence on
/// the true branch. `UserCheck` is reserved for an equality-aware
/// recogniser (`ТекущийПользователь() = "X"`) that does NOT exist
/// yet; entries with that semantics are deliberately absent from
/// [`default_registry`] until that recogniser lands. Codex round-A
/// BLOCKER: registering a `UserCheck` predicate today would let the
/// algorithm match a bare `ТекущийПользователь()` call as a guard,
/// which is type-incorrect in BSL but admissible on a synthesised
/// CFG and would silently swallow real warnings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuardSemantics {
    /// `РольДоступна("X")` returns true → caller is in role X.
    RoleCheck,
    /// `ТекущийПользователь() = "X"` → caller is named user X.
    /// Reserved — see the type-level doc.
    UserCheck,
    /// `ПривилегированныйРежим()` returns true → privileged frame
    /// is currently active. Useful as an alternative guard for
    /// platform APIs that document "if running unprivileged, raise
    /// error".
    PrivilegedQuery,
}

/// Single guard-predicate registry entry. `name` is matched
/// case-insensitively against the call's `Expr::Path` callee.
#[derive(Debug, Clone)]
pub struct GuardPredicate {
    /// Russian name of the guard predicate (e.g. `РольДоступна`).
    pub ru: &'static str,
    /// English alias (e.g. `IsInRole`). Some platform predicates have
    /// no English form; carry the empty string in that case.
    pub en: &'static str,
    /// Semantic class. Today purely informational.
    pub semantics: GuardSemantics,
}

/// Registry of recognised guard predicates. Constructed once via
/// [`default_registry`] and consulted on every backward DFS step;
/// `O(N)` linear scan is fine for `N ≤ 10`. Names are pre-lowercased
/// at construction so the hot-path lookup is a single
/// `to_lowercase()` plus N string compares (no per-entry
/// case-folding allocation per query).
#[derive(Debug, Clone, Default)]
pub struct GuardRegistry {
    /// Lowercased RU/EN names. Each entry's `(ru, en)` pair is split
    /// into two slots; the empty string filters out predicates with
    /// no English alias.
    lower_aliases: Vec<String>,
}

impl GuardRegistry {
    /// Build a registry from a list of entries; case-fold once.
    ///
    /// Entries whose [`GuardSemantics`] class is not currently
    /// supported by [`condition_matches_guard`] are silently dropped
    /// here — the call-shape recogniser cannot soundly approve them
    /// (see [`GuardSemantics::UserCheck`] doc). Codex round-A round-3
    /// BLOCKER: without this filter, a public caller could construct
    /// `GuardRegistry::new(vec![GuardPredicate { ..., semantics: UserCheck }])`
    /// and reintroduce the false-positive that the
    /// [`default_registry`] omission was intended to prevent.
    pub fn new(entries: Vec<GuardPredicate>) -> Self {
        let mut lower_aliases = Vec::with_capacity(entries.len() * 2);
        for p in &entries {
            if !is_supported(p.semantics) {
                tracing::warn!(
                    semantics = ?p.semantics,
                    name = %p.ru,
                    "GuardRegistry::new dropped a guard predicate whose semantics is not yet \
                     supported by the call-shape recogniser; see GuardSemantics doc."
                );
                continue;
            }
            lower_aliases.push(p.ru.to_lowercase());
            if !p.en.is_empty() {
                lower_aliases.push(p.en.to_lowercase());
            }
        }
        Self { lower_aliases }
    }

    /// Number of registered name aliases (RU+EN slots).
    pub fn alias_count(&self) -> usize {
        self.lower_aliases.len()
    }

    /// Test whether `name` matches any registered predicate
    /// (case-insensitive on RU or EN).
    pub fn matches(&self, name: &Name) -> bool {
        let n = name.as_str().to_lowercase();
        self.lower_aliases.iter().any(|alias| alias == &n)
    }
}

/// Whether [`condition_matches_guard`] can soundly approve a call
/// whose registry entry carries this semantic class. Updates here
/// MUST come with the matching recogniser change in
/// [`condition_matches_guard`] AND in
/// [`PrivilegedModuleMethodCall`]'s suppression intent (see
/// [`default_registry`] doc).
///
/// Today only `RoleCheck` is sound under bare-call recognition.
/// Both `PrivilegedQuery` (`ПривилегированныйРежим()` —
/// tautological in privileged modules; Codex round-2 BLOCKER) and
/// `UserCheck` (`ТекущийПользователь() = "X"` — needs equality-aware
/// recogniser) are filtered out so a custom-built `GuardRegistry`
/// cannot re-introduce the failure modes that the
/// [`default_registry`] omissions were designed to prevent.
fn is_supported(semantics: GuardSemantics) -> bool {
    match semantics {
        GuardSemantics::RoleCheck => true,
        // Tautological under bare-call recognition; see
        // `default_registry` doc for the privileged-module ambient
        // state argument.
        GuardSemantics::PrivilegedQuery => false,
        // Equality-aware recogniser not implemented; see the
        // `GuardSemantics` type doc for the soundness rationale.
        GuardSemantics::UserCheck => false,
    }
}

/// Curated default registry — the small set of platform predicates
/// whose true branch is sufficient evidence that the surrounded code
/// is authorised. Extend through PR with explicit ITS rationale, not
/// silent additions.
///
/// **`UserCheck` predicates are intentionally absent.** The
/// equality-aware recogniser (`ТекущийПользователь() = "X"`) is not
/// implemented; until it lands, registering a `UserCheck` entry would
/// let the algorithm match a bare call as a guard — see
/// [`GuardSemantics`] doc.
///
/// **`БезопасныйРежим` / `SafeMode` is intentionally absent** (Codex
/// §1.6 Group D MAJOR fix). The `UnsafeSafeModeMethodCall` diagnostic
/// (Blocker severity) explicitly flags the bare-condition shape
/// `Если БезопасныйРежим() Тогда …` as unsafe — `БезопасныйРежим` is
/// multi-state, not a clean Boolean, and best practice is to compare
/// explicitly with `<> Ложь` / `= Истина`. Recognising the bare call
/// as a guard would let an unsafe-by-construction pattern silence a
/// major privileged-call warning. Until the recogniser learns the
/// explicit-comparison shapes that `UnsafeSafeModeMethodCall` blesses,
/// SafeMode stays out of the default registry.
///
/// **`ПривилегированныйРежим` / `PrivilegedMode` is also intentionally
/// absent** (Codex §1.6 Group D round-2 BLOCKER fix). The bare-call
/// shape `Если ПривилегированныйРежим() Тогда …` is tautological —
/// the getter observes ambient runtime state, not user authorisation.
/// In a CommonModule that is itself declared `privileged=true` in
/// metadata, the getter returns `Истина` permanently, and recognising
/// it as a guard would silence the privileged-call diagnostic exactly
/// when the diagnostic is most needed (cross-module calls from inside
/// a privileged module). The `validateNestedCalls=true` default in
/// `PrivilegedModuleMethodCall` is set up to flag those edges; a
/// `PrivilegedMode()` guard would defeat that explicit review intent.
/// Reserved for an equality-aware recogniser in a future slice.
///
/// # Suppression intent: review marker, not security gate
///
/// `РольДоступна(...)` recognises ANY role name as a guard — the
/// argument is not inspected. The `PrivilegedModuleMethodCall`
/// diagnostic is a review marker ("a human checked access at this
/// call site"), not a security gate ("the role check authorises the
/// specific operation"). A pattern like
/// `Если РольДоступна("Чтение") Тогда ПривилегированныйМодуль.Удалить()`
/// silences the diagnostic — the role-name vs operation match is out
/// of scope for the call-shape recogniser. Future work that audits
/// role-name × operation pairings would land as a stronger semantic
/// class than `RoleCheck`.
pub fn default_registry() -> GuardRegistry {
    GuardRegistry::new(vec![
        GuardPredicate {
            ru: "РольДоступна", en: "IsInRole", semantics: GuardSemantics::RoleCheck
        },
        // Per-user role check. Distinct from `РольДоступна` — takes
        // an explicit user id, not the current user. Codex round-A
        // MAJOR: commonly used in privileged-mode patterns; absence
        // would surface false-negative diagnostic suppression.
        GuardPredicate {
            ru: "РольДоступнаПользователю",
            en: "IsInRoleByUser",
            semantics: GuardSemantics::RoleCheck,
        },
    ])
}

/// Public entry point. Returns `true` when **every** path from the
/// CFG entry to the basic block containing `stmt` passes through a
/// guard-true edge whose condition is recognised by `registry`.
///
/// Returns `false` if:
/// - the statement is not in any basic block (synthetic / orphan);
/// - the CFG has no entry point (malformed);
/// - any path from entry reaches the statement without crossing a
///   recognised guard.
pub fn is_stmt_guarded(
    cfg: &ControlFlowGraph,
    body: &Body,
    stmt: StmtId,
    registry: &GuardRegistry,
) -> bool {
    let Some(start) = find_block_containing(cfg, stmt) else {
        return false;
    };
    if cfg.entry_point().is_none() {
        return false;
    }
    let mut visited = FxHashSet::default();
    is_guarded_dfs(cfg, body, registry, start, &mut visited)
}

/// Recursive must-guarded DFS. Returns `true` iff every incoming path
/// to `current` passes through a guard-true edge.
fn is_guarded_dfs(
    cfg: &ControlFlowGraph,
    body: &Body,
    registry: &GuardRegistry,
    current: NodeIndex,
    visited: &mut FxHashSet<NodeIndex>,
) -> bool {
    if cfg.entry_point() == Some(current) {
        // Reached the method entry without crossing a guard-true edge.
        return false;
    }
    if !visited.insert(current) {
        // INVARIANT: revisiting via a back-edge always returns
        // `true`. Soundness: the FIRST visit to `current` came via
        // some acyclic prefix; if that prefix was un-guarded, the
        // outer DFS already returned `false`. The only way control
        // reaches this branch is when the FIRST visit was already
        // determined to be guarded — re-traversing the same node via
        // a back-edge cannot discover a new un-guarded path. (A loop
        // entered via an un-guarded path is rejected on the entry
        // edge, not by the cycle check.)
        return true;
    }

    let mut any_predecessor = false;
    for (pred, edge_type) in cfg.incoming_edges(current) {
        // Dead-code edges don't represent real flow; ignore them.
        if edge_type.is_dead_code_edge() {
            continue;
        }
        any_predecessor = true;

        // Direct guard-true: pred is `Conditional` AND we entered via
        // `TrueBranch` AND its condition is a recognised guard.
        if matches!(edge_type, CfgEdgeType::TrueBranch) {
            if let Some(CfgVertex::Conditional(cv)) = cfg.vertex(pred) {
                if condition_matches_guard(body, cv.condition, registry) {
                    // This path is guarded; do NOT recurse further on
                    // it (the guard fully authorises the prefix
                    // upstream of the conditional).
                    continue;
                }
            }
        }

        // Otherwise the path is only guarded if `pred` is itself
        // guarded all the way back to entry.
        if !is_guarded_dfs(cfg, body, registry, pred, visited) {
            return false;
        }
    }

    if !any_predecessor {
        // Block is unreachable from any predecessor (orphan / dead).
        // Treat as un-guarded — we have no evidence of authorisation.
        return false;
    }

    true
}

/// Inspect `condition` and answer "is this a guard call recognised by
/// the registry?".
///
/// Recognised shapes:
///
/// 1. **Direct call**: `Expr::Call { callee: Path(name), .. }` where
///    `name` is registered.
/// 2. **AND chain**: `Expr::BinaryOp { op: And, lhs, rhs }` — the
///    true branch implies BOTH operands are true, so it suffices
///    that EITHER operand is a registered guard call.
///
/// Other shapes (`Or`, `Not`, equality patterns) are deliberately
/// not recognised: they require deeper tracking that belongs in a
/// future slice. False negatives here are acceptable — the diagnostic
/// surfaces the call anyway and the user can refactor the guard into
/// a recognised shape.
///
/// Parenthesised forms aren't a concern in this codebase — BSL
/// HIR lowering normalises away every `(expr)` wrapper at the
/// AST→HIR boundary (`hir-def::body::lower::expr`), so the recogniser
/// sees the inner `Call` / `BinaryOp` directly.
fn condition_matches_guard(body: &Body, condition: ExprId, registry: &GuardRegistry) -> bool {
    match body.expr(condition) {
        Expr::Call { callee, .. } => {
            let callee_id = hir_def::ExprId::from_idx(*callee);
            matches!(body.expr(callee_id), Expr::Path(name) if registry.matches(name))
        }
        Expr::BinaryOp { op: BinaryOp::And, lhs, rhs } => {
            let lhs_id = hir_def::ExprId::from_idx(*lhs);
            let rhs_id = hir_def::ExprId::from_idx(*rhs);
            condition_matches_guard(body, lhs_id, registry)
                || condition_matches_guard(body, rhs_id, registry)
        }
        _ => false,
    }
}

/// Find the basic block containing `stmt`. Returns `None` if `stmt`
/// is not stored in any `BasicBlockVertex` (synthetic statement, or
/// the CFG was built for a different body).
///
/// NOTE: O(V·S) where V is the vertex count and S is the average
/// statements-per-block. Acceptable today (a real BSL method has
/// V ≤ ~50, S ≤ ~5), but if a future caller invokes this from a hot
/// path build a stmt→block index once per CFG and cache it.
fn find_block_containing(cfg: &ControlFlowGraph, stmt: StmtId) -> Option<NodeIndex> {
    cfg.vertices().find_map(|(idx, vertex)| {
        if let CfgVertex::BasicBlock(block) = vertex {
            if block.statements().contains(&stmt) {
                return Some(idx);
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfg::{BasicBlockVertex, ConditionalVertex};
    use hir_def::{
        body::Body,
        hir::{Expr, Literal},
    };
    use la_arena::RawIdx;

    fn name(s: &str) -> Name {
        Name::new(s)
    }

    /// Build a minimal `Body` whose expr arena contains the call
    /// `<callee_name>()` and return its [`ExprId`]. The returned id
    /// is what we plug into a `ConditionalVertex`'s `condition` field
    /// for the tests.
    fn body_with_guard_call(callee_name: &str) -> (Body, ExprId) {
        let mut body = Body::default();
        let callee_expr = body.alloc_expr(Expr::Path(name(callee_name)));
        let call = body.alloc_expr(Expr::Call { callee: callee_expr.to_idx(), args: Box::new([]) });
        (body, call)
    }

    fn body_with_literal_true() -> (Body, ExprId) {
        let mut body = Body::default();
        let lit = body.alloc_expr(Expr::Literal(Literal::Bool(true)));
        (body, lit)
    }

    /// Allocate an empty basic block. The block doesn't need real
    /// statements for these tests because `find_block_containing`
    /// scans by `StmtId` equality and we feed it whatever id the
    /// caller chooses.
    fn make_block_with_stmt(stmt: StmtId) -> BasicBlockVertex {
        let mut b = BasicBlockVertex::new();
        b.add_statement(stmt);
        b
    }

    fn synthetic_stmt(raw: u32) -> StmtId {
        StmtId::from_raw(RawIdx::from(raw))
    }

    #[test]
    fn missing_block_returns_false() {
        let cfg = ControlFlowGraph::new();
        let (body, _) = body_with_literal_true();
        let registry = default_registry();
        // No block contains stmt #0 — the registry can't help.
        assert!(!is_stmt_guarded(&cfg, &body, synthetic_stmt(0), &registry));
    }

    #[test]
    fn missing_entry_point_returns_false() {
        let mut cfg = ControlFlowGraph::new();
        // Add a block with a stmt but no entry point set.
        let stmt = synthetic_stmt(1);
        let _idx = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        let (body, _) = body_with_literal_true();
        let registry = default_registry();
        assert!(!is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn unguarded_linear_path_returns_false() {
        // entry → call_block → exit. No conditional in between.
        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let stmt = synthetic_stmt(1);
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        cfg.add_edge(entry, call_block, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let (body, _) = body_with_literal_true();
        let registry = default_registry();
        assert!(!is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn guarded_then_branch_returns_true() {
        // entry → conditional(РольДоступна) → TRUE → call_block → exit
        //                                  → FALSE → other_block → exit
        let (body, guard_expr) = body_with_guard_call("РольДоступна");
        let stmt = synthetic_stmt(1);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(guard_expr)));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        let other_block = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, call_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, other_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        cfg.add_edge(other_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        assert!(is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn guarded_else_branch_returns_false() {
        // The call is on the FALSE branch — un-guarded.
        let (body, guard_expr) = body_with_guard_call("РольДоступна");
        let stmt = synthetic_stmt(1);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(guard_expr)));
        let then_block = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, then_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, call_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(then_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        assert!(!is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn english_alias_is_recognised() {
        let (body, guard_expr) = body_with_guard_call("IsInRole");
        let stmt = synthetic_stmt(1);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(guard_expr)));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        let other_block = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, call_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, other_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        cfg.add_edge(other_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        assert!(is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn unrelated_predicate_does_not_match() {
        let (body, cond_expr) = body_with_guard_call("СовершенноДругаяФункция");
        let stmt = synthetic_stmt(1);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(cond_expr)));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        let other_block = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, call_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, other_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        cfg.add_edge(other_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        assert!(!is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn and_chain_recognises_either_operand() {
        // Build a Body holding `РольДоступна() И ДругаяПроверка()`.
        let mut body = Body::default();
        let role_callee = body.alloc_expr(Expr::Path(name("РольДоступна")));
        let role_call =
            body.alloc_expr(Expr::Call { callee: role_callee.to_idx(), args: Box::new([]) });
        let other_callee = body.alloc_expr(Expr::Path(name("ДругаяПроверка")));
        let other_call =
            body.alloc_expr(Expr::Call { callee: other_callee.to_idx(), args: Box::new([]) });
        let and_expr = body.alloc_expr(Expr::BinaryOp {
            op: BinaryOp::And,
            lhs: role_call.to_idx(),
            rhs: other_call.to_idx(),
        });

        let stmt = synthetic_stmt(1);
        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(and_expr)));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        let other_block = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, call_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, other_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        cfg.add_edge(other_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        assert!(is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn merged_paths_one_guarded_one_not_returns_false() {
        // entry → cond_guard → TRUE → call_block ← unguarded_path ← entry
        //                    → FALSE → unguarded_path
        // Both paths reach call_block; one is guarded, the other isn't.
        let (body, guard_expr) = body_with_guard_call("РольДоступна");
        let stmt = synthetic_stmt(1);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(guard_expr)));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, call_block, CfgEdgeType::TrueBranch).unwrap();
        // FALSE branch ALSO reaches call_block (e.g. via fallthrough).
        cfg.add_edge(cond, call_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        // The FALSE-branch path is un-guarded → must-be-guarded fails.
        assert!(!is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn dead_code_edges_are_ignored() {
        // entry → cond_guard → TRUE → call_block (via guard)
        //                    → FALSE → exit
        // Plus a dead AdjacentCode edge from a synthetic dead block
        // into call_block. The dead edge must NOT count as an
        // un-guarded predecessor.
        let (body, guard_expr) = body_with_guard_call("РольДоступна");
        let stmt = synthetic_stmt(1);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(guard_expr)));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        let dead_block = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, call_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, cfg.exit_point(), CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        cfg.add_edge(dead_block, call_block, CfgEdgeType::AdjacentCode).unwrap();

        let registry = default_registry();
        // The dead edge from `dead_block` is `AdjacentCode` — ignored.
        // The only live predecessor of call_block is the guarded
        // TrueBranch, so the call is guarded.
        assert!(is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn loop_back_edge_does_not_create_false_negative() {
        // Loop body where the call is inside the body, guarded by a
        // role check at loop entry. The loop back-edge points back at
        // the header — that's the cycle case the optimistic-revisit
        // logic must handle without losing the guard.
        let (body, guard_expr) = body_with_guard_call("РольДоступна");
        let stmt = synthetic_stmt(1);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(guard_expr)));
        let loop_header = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, loop_header, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, cfg.exit_point(), CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(loop_header, call_block, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(call_block, loop_header, CfgEdgeType::LoopIteration).unwrap();
        cfg.add_edge(loop_header, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        assert!(is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn registry_match_is_case_insensitive() {
        let registry = default_registry();
        assert!(registry.matches(&name("рольдоступна")));
        assert!(registry.matches(&name("РОЛЬДОСТУПНА")));
        assert!(registry.matches(&name("isinrole")));
        assert!(!registry.matches(&name("NotARealGuard")));
    }

    #[test]
    fn safe_mode_is_not_a_guard() {
        // Codex §1.6 Group D MAJOR fix: `БезопасныйРежим` /
        // `SafeMode` is the bare-condition shape that
        // `UnsafeSafeModeMethodCall` flags as Blocker-severity
        // unsafe. Recognising it as a guard would let an
        // unsafe-by-construction pattern silence a major
        // privileged-call warning. Pin the absence here so a future
        // edit can't silently re-add it.
        let registry = default_registry();
        assert!(!registry.matches(&name("БезопасныйРежим")));
        assert!(!registry.matches(&name("SafeMode")));
    }

    #[test]
    fn privileged_mode_query_is_not_a_guard() {
        // Codex §1.6 Group D round-2 BLOCKER fix: bare-call
        // `Если ПривилегированныйРежим() Тогда …` is tautological.
        // The getter observes ambient runtime state, not
        // authorisation. In a privileged CommonModule, it is
        // permanently `Истина`, so the suppression would silence the
        // very diagnostic targeting cross-privileged-module calls.
        // Pin the absence here.
        let registry = default_registry();
        assert!(!registry.matches(&name("ПривилегированныйРежим")));
        assert!(!registry.matches(&name("PrivilegedMode")));
    }

    #[test]
    fn current_user_is_not_a_guard_today() {
        // Codex round-A BLOCKER fix: `ТекущийПользователь()` requires
        // an equality-aware recogniser that does not exist yet, so it
        // must NOT be in the default registry.
        let registry = default_registry();
        assert!(!registry.matches(&name("ТекущийПользователь")));
        assert!(!registry.matches(&name("CurrentUser")));
    }

    #[test]
    fn custom_registry_drops_privileged_query_entries() {
        // Codex §1.6 Group D round-3 BLOCKER fix: `is_supported`
        // must reject `PrivilegedQuery` so a custom-built registry
        // cannot reintroduce the tautological-guard failure mode
        // (the §1.6 round-2 BLOCKER) — `default_registry` omission
        // alone is not enough; the public `GuardRegistry::new`
        // accepts arbitrary entries.
        let registry = GuardRegistry::new(vec![GuardPredicate {
            ru: "ПривилегированныйРежим",
            en: "PrivilegedMode",
            semantics: GuardSemantics::PrivilegedQuery,
        }]);
        assert_eq!(
            registry.alias_count(),
            0,
            "PrivilegedQuery entries must be dropped at construction"
        );
        assert!(!registry.matches(&name("ПривилегированныйРежим")));
        assert!(!registry.matches(&name("PrivilegedMode")));
    }

    #[test]
    fn custom_registry_drops_user_check_entries() {
        // Codex round-A round-3 BLOCKER fix: even a custom-built
        // registry must not be allowed to reintroduce the
        // `UserCheck` false-positive. `GuardRegistry::new` filters
        // out unsupported semantics, so a caller passing a
        // `UserCheck` predicate sees it silently dropped.
        let registry = GuardRegistry::new(vec![GuardPredicate {
            ru: "ТекущийПользователь",
            en: "CurrentUser",
            semantics: GuardSemantics::UserCheck,
        }]);
        assert_eq!(registry.alias_count(), 0, "UserCheck entries must be dropped at construction");
        assert!(!registry.matches(&name("ТекущийПользователь")));
        assert!(!registry.matches(&name("CurrentUser")));
    }

    #[test]
    fn custom_registry_keeps_supported_semantics_alongside_dropped_ones() {
        // Mixed registry: one RoleCheck (supported) and one UserCheck
        // (dropped). The supported entry must still match.
        let registry = GuardRegistry::new(vec![
            GuardPredicate {
                ru: "МояПроверкаРоли",
                en: "MyRoleCheck",
                semantics: GuardSemantics::RoleCheck,
            },
            GuardPredicate {
                ru: "ТекущийПользователь",
                en: "CurrentUser",
                semantics: GuardSemantics::UserCheck,
            },
        ]);
        assert_eq!(registry.alias_count(), 2, "only the RoleCheck entry's two aliases survive");
        assert!(registry.matches(&name("МояПроверкаРоли")));
        assert!(registry.matches(&name("MyRoleCheck")));
        assert!(!registry.matches(&name("ТекущийПользователь")));
    }

    #[test]
    fn role_by_user_is_recognised() {
        // Codex round-A MAJOR fix: `РольДоступнаПользователю` is a
        // distinct platform predicate (per-user, not current-user).
        let (body, guard_expr) = body_with_guard_call("РольДоступнаПользователю");
        let stmt = synthetic_stmt(1);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(guard_expr)));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        let other_block = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, call_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, other_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        cfg.add_edge(other_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        assert!(is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn or_chain_with_guard_is_not_recognised() {
        // `РольДоступна() ИЛИ ДругаяПроверка()` — true overall does
        // NOT imply role check is true. Recognising it as a guard
        // would be unsound. This test pins the conservative behaviour.
        let mut body = Body::default();
        let role_callee = body.alloc_expr(Expr::Path(name("РольДоступна")));
        let role_call =
            body.alloc_expr(Expr::Call { callee: role_callee.to_idx(), args: Box::new([]) });
        let other_callee = body.alloc_expr(Expr::Path(name("ДругаяПроверка")));
        let other_call =
            body.alloc_expr(Expr::Call { callee: other_callee.to_idx(), args: Box::new([]) });
        let or_expr = body.alloc_expr(Expr::BinaryOp {
            op: BinaryOp::Or,
            lhs: role_call.to_idx(),
            rhs: other_call.to_idx(),
        });

        let stmt = synthetic_stmt(1);
        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(or_expr)));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        let other_block = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, call_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, other_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        cfg.add_edge(other_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        assert!(!is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn negated_guard_is_not_recognised() {
        // `Не РольДоступна()` — true branch means role is NOT
        // available; recognising the inner call would be unsound.
        let mut body = Body::default();
        let role_callee = body.alloc_expr(Expr::Path(name("РольДоступна")));
        let role_call =
            body.alloc_expr(Expr::Call { callee: role_callee.to_idx(), args: Box::new([]) });
        let not_expr = body
            .alloc_expr(Expr::UnaryOp { op: hir_def::hir::UnaryOp::Not, expr: role_call.to_idx() });

        let stmt = synthetic_stmt(1);
        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(not_expr)));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        let other_block = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, call_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, other_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        cfg.add_edge(other_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        assert!(!is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn post_loop_call_is_unguarded_when_loop_skipped() {
        // entry → loop_header → FALSE → call_block (post-loop)
        //                   → TRUE → loop_body → loop_header (back-edge)
        // The call after the loop is reached when the loop is skipped
        // (zero iterations). No guard anywhere → must-be-guarded =
        // false.
        let stmt = synthetic_stmt(1);
        let cond_expr = {
            let mut b = Body::default();
            let lit = b.alloc_expr(Expr::Literal(Literal::Bool(true)));
            // Throw-away builder; the real condition body for the
            // loop_header is constructed below.
            let _ = (b, lit);
            // Use a non-guard call so the recogniser ignores it.
            let mut body2 = Body::default();
            let p = body2.alloc_expr(Expr::Path(name("Что-тоНеГуард")));
            let c = body2.alloc_expr(Expr::Call { callee: p.to_idx(), args: Box::new([]) });
            (body2, c)
        };
        let (body, header_cond) = cond_expr;

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let loop_header =
            cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(header_cond)));
        let loop_body = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        cfg.add_edge(entry, loop_header, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(loop_header, loop_body, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(loop_header, call_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(loop_body, loop_header, CfgEdgeType::LoopIteration).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        assert!(!is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn except_handler_path_is_not_guarded_by_try_block_role_check() {
        // CFG shape:
        //
        //   entry → try_vertex
        //   try_vertex → try_body_guard (TrueBranch) → call_block  (guarded)
        //   try_vertex → except_handler              → call_block  (NOT guarded)
        //
        // The except handler edge models "an exception escaped the
        // try body and the handler runs". `call_block` lies in the
        // common code after the try/except merge — every must-guard
        // path needs to clear the role check, but the except-handler
        // path reaches `call_block` without traversing the guard's
        // TrueBranch. Verifies that a try-block guard does NOT cover
        // the except-handler reach.
        let (body, guard_expr) = body_with_guard_call("РольДоступна");
        let stmt = synthetic_stmt(1);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let try_vertex = cfg.add_vertex(CfgVertex::TryExcept(cfg::TryExceptVertex::new()));
        let try_body_guard =
            cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(guard_expr)));
        let try_else = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        let except_handler = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));

        cfg.add_edge(entry, try_vertex, CfgEdgeType::Direct).unwrap();
        // Try body: route through the guard before reaching the
        // shared `call_block`.
        cfg.add_edge(try_vertex, try_body_guard, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(try_body_guard, call_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(try_body_guard, try_else, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(try_else, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        // Except handler: enters from `try_vertex` (an exception
        // raised in the try body before the guard had a chance to
        // approve), reaches `call_block` directly — no guard on this
        // edge.
        cfg.add_edge(try_vertex, except_handler, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(except_handler, call_block, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        // The except-handler path reaches `call_block` un-guarded.
        assert!(!is_stmt_guarded(&cfg, &body, stmt, &registry));
    }

    #[test]
    fn guard_call_with_arguments_is_recognised() {
        // Codex round-A MINOR: ensure the call-shape recogniser
        // doesn't accidentally require empty args (the production
        // form is `РольДоступна("Администратор")`).
        let mut body = Body::default();
        let callee = body.alloc_expr(Expr::Path(name("РольДоступна")));
        let arg = body.alloc_expr(Expr::Literal(Literal::String("Администратор".to_string())));
        let call =
            body.alloc_expr(Expr::Call { callee: callee.to_idx(), args: Box::new([arg.to_idx()]) });

        let stmt = synthetic_stmt(1);
        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.set_entry_point(entry);
        let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(call)));
        let call_block = cfg.add_vertex(CfgVertex::BasicBlock(make_block_with_stmt(stmt)));
        let other_block = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, call_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, other_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(call_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        cfg.add_edge(other_block, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let registry = default_registry();
        assert!(is_stmt_guarded(&cfg, &body, stmt, &registry));
    }
}
