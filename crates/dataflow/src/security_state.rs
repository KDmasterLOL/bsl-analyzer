//! Security-mode state — privilege & safe-mode lifetime tracking.
//!
//! Forward dataflow over a saturating per-method counter pair: one
//! counter for privileged-mode frames (`УстановитьПривилегированныйРежим`),
//! one for unsafe-frame depth (the unified counter touched by both
//! `УстановитьБезопасныйРежим` and `УстановитьОтключениеБезопасногоРежима`,
//! which point in opposite polarities). 1C runtime semantics are
//! counter-based — `Истина`/`Ложь` arguments don't toggle a boolean,
//! they push/pop frames; the saturating-counter shape models that
//! faithfully without falsely classifying `Истина; Истина; Ложь` as
//! "off" (which a 4-valued lattice would).
//!
//! # Combined lattice
//!
//! Plan §1.2's literal sample structures `recognize_call(stmt, registry,
//! ctx)` as a separate "context" lookup of the value-state at the call
//! site. We embed a [`ValueOverlay`] inside [`SecurityModeState`]
//! instead, so the transfer can constant-fold a `SetPrivilegedMode(arg)`
//! argument in-place from the same lattice value it just stepped. This
//! is a single-pass equivalent of the plan's two-pass design with
//! identical precision (the two passes are independent only in the
//! sense that value-state's transfer doesn't read counters; combining
//! the carriers does not couple them analytically).
//!
//! # Layer rules
//!
//! Lives in `dataflow` (not `hir-ty`, not `ide-db`). Reads the curated
//! catalogue from `bsl_platform::security::registry()` via `&str`-keyed
//! lookup — no `Name`-keyed API, no `hir-def` cycle. The Salsa wrapper
//! (`module_security_state_query`) lives in `ide-db/src/effects.rs` per
//! Track 1 precedent (§1.4 of the master plan).
//!
//! # Recognized call shapes
//!
//! `recognize_security_call` only fires for `Expr::Call` with an
//! `Expr::Path` callee — i.e. unqualified global calls. Qualified calls
//! (`Module.SetPrivilegedMode(…)`) and `Expr::MethodCall` (`obj.Method`)
//! are NOT recognised; they cannot reach the global-only registry index
//! by design (the registry contains only `EntryKind::GlobalMethod` and
//! `EntryKind::Constructor` entries). The plan §1.6 confirms this is
//! the same scope the legacy handler uses.
//!
//! Each statement's full expression tree is walked for security calls
//! (basic-block leaves via [`Transfer::transfer_stmt`], compound-stmt
//! conditions / loop bounds via [`Transfer::transfer_expr_in_place`]).
//! Calls are visited in post-order, matching BSL's left-to-right
//! evaluation of nested arguments — `Сообщить(SetPriv(Истина))` opens
//! the privileged frame at runtime, and the lattice's counter reflects
//! that after the inner call's effect is applied. The two consumers
//! ([`open_events`] for diagnostics, the lattice transfers for
//! downstream consumers) walk the same tree shape so their counter
//! views can never diverge.

use std::sync::Arc;

pub use bsl_platform::security::Category;

use bsl_platform::security::{registry, Role};
use cfg::{CfgVertex, ControlFlowGraph};
use hir_def::{
    body::Body,
    hir::{Expr, Literal, Stmt},
    BindingId, ExprId, IdConversion, StmtId,
};
use la_arena::RawIdx;

use crate::value_state::{KnownValue, ValueOverlay, ValueStateProvider};
use crate::{DataflowResult, DataflowSolver, Direction, Lattice, Transfer};

/// Saturation bound. The privileged/safe-mode frame depth is clamped
/// here; real BSL code rarely nests deeper than 2-3, K=8 leaves
/// generous headroom while keeping the per-block memory at 8 bytes.
pub const K_MAX: u8 = 8;

/// One side of the may/must counter pair.
///
/// Two-arm encoding lets the lattice preserve precision across the
/// `AtLeast`/`Exact` transition that any unknown-bool argument forces:
/// after `Установить(unknown_bool)` the may-counter is ≥ k+1 but the
/// exact value is unknown, hence `AtLeast(k+1)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaturatingCount {
    /// Counter is exactly this value.
    Exact(u8),
    /// Counter is ≥ this value (precise upper bound lost after seeing
    /// an unknown-bool argument).
    AtLeast(u8),
}

impl SaturatingCount {
    /// Lower bound — the smallest value the counter could possibly hold
    /// at this program point. Used by callers that want a yes/no "is
    /// the frame definitely active" answer.
    pub fn lower_bound(self) -> u8 {
        match self {
            Self::Exact(n) | Self::AtLeast(n) => n,
        }
    }

    /// `true` when the lower bound is non-zero, i.e. the frame is
    /// definitely active on every path.
    pub fn is_definitely_open(self) -> bool {
        self.lower_bound() > 0
    }

    /// Increment, saturating at [`K_MAX`].
    pub fn inc(self) -> Self {
        match self {
            Self::Exact(n) if n < K_MAX => Self::Exact(n + 1),
            Self::Exact(_) => Self::AtLeast(K_MAX),
            Self::AtLeast(k) if k < K_MAX => Self::AtLeast(k + 1),
            Self::AtLeast(_) => Self::AtLeast(K_MAX),
        }
    }

    /// Decrement. `Exact(0)` and `AtLeast(0)` saturate at `0`/`AtLeast(0)`
    /// respectively; the dataflow stays conservative because runtime
    /// underflow (more `Ложь` than `Истина`) is a separate diagnostic
    /// surface, not a lattice error.
    pub fn dec(self) -> Self {
        match self {
            Self::Exact(0) => Self::Exact(0),
            Self::Exact(n) => Self::Exact(n - 1),
            Self::AtLeast(0) => Self::AtLeast(0),
            Self::AtLeast(k) => Self::AtLeast(k - 1),
        }
    }

    /// max-join (used for `may`). Round-5 fix: preserves `AtLeast`
    /// precision when both sides are `AtLeast`.
    pub fn join_max(a: Self, b: Self) -> Self {
        match (a, b) {
            (Self::Exact(x), Self::Exact(y)) => Self::Exact(x.max(y)),
            (Self::Exact(x), Self::AtLeast(y)) | (Self::AtLeast(y), Self::Exact(x)) => {
                Self::AtLeast(x.max(y))
            }
            (Self::AtLeast(x), Self::AtLeast(y)) => Self::AtLeast(x.max(y)),
        }
    }

    /// min-join (used for `must`). Round-5 fix: returning `Exact(min)`
    /// for `AtLeast(x).join(AtLeast(y))` would violate idempotence
    /// (`AtLeast(5).join(AtLeast(5)) == AtLeast(5)` must hold). Keep
    /// the `AtLeast` arm.
    pub fn join_min(a: Self, b: Self) -> Self {
        match (a, b) {
            (Self::Exact(x), Self::Exact(y)) => Self::Exact(x.min(y)),
            (Self::Exact(x), Self::AtLeast(y)) | (Self::AtLeast(y), Self::Exact(x)) => {
                if x <= y {
                    Self::Exact(x)
                } else {
                    Self::AtLeast(y)
                }
            }
            (Self::AtLeast(x), Self::AtLeast(y)) => Self::AtLeast(x.min(y)),
        }
    }
}

/// May/must counter pair for one frame kind.
///
/// `may` is the largest counter value reachable on any predecessor
/// path (point-wise max-join); `must` is the smallest (point-wise
/// min-join). They form a counter-domain partial order:
/// `(may_a, must_a) ⊑ (may_b, must_b) iff may_a ≤ may_b ∧ must_a ≥ must_b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivilegeCounter {
    pub may: SaturatingCount,
    pub must: SaturatingCount,
}

impl PrivilegeCounter {
    /// Method-entry seed: counter is exactly 0 on both axes.
    pub const ENTRY: Self =
        Self { may: SaturatingCount::Exact(0), must: SaturatingCount::Exact(0) };

    /// Open a frame (constant-true argument with `opens_unsafe_when=true`,
    /// or constant-false with `opens_unsafe_when=false`).
    pub fn open(self) -> Self {
        Self { may: self.may.inc(), must: self.must.inc() }
    }

    /// Close a frame.
    pub fn close(self) -> Self {
        Self { may: self.may.dec(), must: self.must.dec() }
    }

    /// Unknown-bool argument: worst-of-both-paths conservatively. The
    /// `True` branch increments → `may` grows; the `False` branch
    /// decrements → `must` shrinks. Round-3/round-4 round (M-2): NOT
    /// `may = p.may.lower_bound()`-style preservation — that would
    /// suppress real "may be active" cases where `p.may = Exact(0)`.
    pub fn unknown(self) -> Self {
        Self { may: self.may.inc(), must: self.must.dec() }
    }
}

/// Pair of counters, one per frame kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityCounters {
    /// Privileged-mode frame depth
    /// (`УстановитьПривилегированныйРежим` toggles).
    pub privilege: PrivilegeCounter,
    /// Unsafe-frame depth
    /// (`УстановитьБезопасныйРежим(Ложь)` and
    /// `УстановитьОтключениеБезопасногоРежима(Истина)` both push;
    /// the opposite values pop).
    pub unsafe_frame: PrivilegeCounter,
}

impl SecurityCounters {
    /// Method-entry seed: both counters at `ENTRY`.
    pub const ENTRY: Self =
        Self { privilege: PrivilegeCounter::ENTRY, unsafe_frame: PrivilegeCounter::ENTRY };
}

/// Forward security-mode state — opaque newtype around a private inner
/// enum, mirroring [`ValueOverlay`].
///
/// `Unreachable` is the lattice bottom; `Reachable` carries the
/// per-frame counters plus the embedded value overlay needed to
/// constant-fold call arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecurityModeState(Inner);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Inner {
    #[default]
    Unreachable,
    Reachable {
        counters: SecurityCounters,
        overlay: ValueOverlay,
    },
}

impl SecurityModeState {
    /// Lattice bottom. Use this in `set_bottom_factory`.
    pub fn unreachable() -> Self {
        Self(Inner::Unreachable)
    }

    /// Method-entry seed: counters at zero, overlay empty. Use this in
    /// `set_initial_state`.
    pub fn entry() -> Self {
        Self(Inner::Reachable { counters: SecurityCounters::ENTRY, overlay: ValueOverlay::empty() })
    }

    /// Read the counters at this point. Returns `None` for
    /// `Unreachable`.
    pub fn counters(&self) -> Option<&SecurityCounters> {
        match &self.0 {
            Inner::Unreachable => None,
            Inner::Reachable { counters, .. } => Some(counters),
        }
    }

    /// `true` when this point is reachable.
    pub fn is_reachable(&self) -> bool {
        matches!(self.0, Inner::Reachable { .. })
    }
}

impl Lattice for SecurityModeState {
    /// `Unreachable` is the identity. For two `Reachable` operands,
    /// counters are joined point-wise (may = max, must = min) and the
    /// embedded overlay is joined per [`Lattice`] for [`ValueOverlay`]
    /// (equal-only intersect).
    fn join(&self, other: &Self) -> Self {
        let inner = match (&self.0, &other.0) {
            (Inner::Unreachable, x) | (x, Inner::Unreachable) => x.clone(),
            (
                Inner::Reachable { counters: a, overlay: oa },
                Inner::Reachable { counters: b, overlay: ob },
            ) => {
                let counters = SecurityCounters {
                    privilege: PrivilegeCounter {
                        may: SaturatingCount::join_max(a.privilege.may, b.privilege.may),
                        must: SaturatingCount::join_min(a.privilege.must, b.privilege.must),
                    },
                    unsafe_frame: PrivilegeCounter {
                        may: SaturatingCount::join_max(a.unsafe_frame.may, b.unsafe_frame.may),
                        must: SaturatingCount::join_min(a.unsafe_frame.must, b.unsafe_frame.must),
                    },
                };
                Inner::Reachable { counters, overlay: oa.join(ob) }
            }
        };
        Self(inner)
    }
}

/// Stateless transfer function for the security-state lattice.
#[derive(Debug, Default, Clone, Copy)]
pub struct SecurityStateProvider;

impl Transfer<SecurityModeState> for SecurityStateProvider {
    fn transfer_stmt(
        &self,
        stmt_id: RawIdx,
        state: &SecurityModeState,
        body: &Body,
    ) -> SecurityModeState {
        let mut next = state.clone();
        let Inner::Reachable { counters, overlay } = &mut next.0 else {
            return next;
        };
        let typed_stmt = StmtId::from_raw(stmt_id);

        // Codex round-3 stop-hook fix: recognise security calls anywhere
        // in the statement's expression tree (not just the statement-
        // root), and apply counter changes for each — matching the
        // every-CALL_EXPR coverage `open_events` already gives. Without
        // this, a diagnostic would surface for
        // `Сообщить(УстановитьБезопасныйРежим(Ложь))` while the lattice
        // counter stayed at zero, so downstream consumers (future leak
        // detection, transitive effect_summary, …) would see an
        // inconsistent picture.
        //
        // Order: recognise BEFORE stepping the overlay (Codex round-1
        // MINOR fix kept) so the call's arg is folded with the entry
        // overlay. Calls are visited in `walk_expr_for_calls`'s post-
        // order, which matches BSL's left-to-right evaluation of nested
        // arguments — the inner call's counter effect is applied before
        // the outer's, exactly as the runtime would.
        let mut calls = Vec::new();
        collect_call_exprs_in_stmt(body, typed_stmt, &mut calls);
        for call_expr in calls {
            if let Some(info) = recognize_security_call(body, call_expr, overlay) {
                apply_call_to_counters(counters, info);
            }
        }

        // Update the embedded overlay for any `Стмт::Assign` — this
        // delegates to value_state's `step`, so the two crates stay
        // in sync without duplicated logic.
        crate::value_state::step(overlay, body, typed_stmt);

        next
    }

    fn transfer_expr_in_place(
        &self,
        expr_id: hir_def::ExprId,
        state: &mut SecurityModeState,
        body: &Body,
    ) {
        // Counter parity for compound-stmt condition expressions
        // (`Если`/`Пока` headers, `Для … По …` bounds, foreach iter
        // expressions). Codex round-3 stop-hook noted that adding
        // condition-call diagnostics in `open_events` without updating
        // the lattice would let the counter drift away from runtime
        // semantics — `Если УстановитьБезопасныйРежим(Ложь) Тогда` opens
        // the unsafe frame at runtime, so the counter must reflect that
        // for any successor block to read.
        let Inner::Reachable { counters, overlay } = &mut state.0 else {
            return;
        };
        let mut calls = Vec::new();
        walk_expr_for_calls(body, expr_id.to_idx(), &mut calls);
        for call_expr in calls {
            if let Some(info) = recognize_security_call(body, call_expr, overlay) {
                apply_call_to_counters(counters, info);
            }
        }
    }

    fn transfer_loop_var_bind(
        &self,
        loop_var: BindingId,
        state: &mut SecurityModeState,
        body: &Body,
    ) {
        // The CFG models `Стмт::For`/`Стмт::ForEach` as their own
        // vertex; the solver invokes this hook so we can kill any
        // value-state fact attached to the rebound binding. Counters
        // are unaffected (the loop header is not a security call).
        if let Inner::Reachable { overlay, .. } = &mut state.0 {
            ValueStateProvider.transfer_loop_var_bind(loop_var, overlay, body);
        }
    }
}

/// Argument-value classification for a recognized security call.
#[derive(Debug, Clone, Copy)]
enum ArgValue {
    /// First argument constant-folded to `Истина` (literal or local
    /// known to be `Bool(true)` at this point).
    KnownTrue,
    /// First argument constant-folded to `Ложь`.
    KnownFalse,
    /// First argument is missing or non-folding.
    Unknown,
}

#[derive(Debug, Clone, Copy)]
struct SecurityCallInfo {
    category: Category,
    /// Value of the `Role::ModeBool { opens_unsafe_when: _ }` polarity
    /// from the registry entry. `true` ⇒ `Истина`-arg opens the frame;
    /// `false` ⇒ `Ложь`-arg opens.
    opens_unsafe_when: bool,
    arg: ArgValue,
}

fn recognize_security_call(
    body: &Body,
    call_expr: ExprId,
    overlay: &ValueOverlay,
) -> Option<SecurityCallInfo> {
    let Expr::Call { callee, args } = body.expr(call_expr) else {
        return None;
    };
    let Expr::Path(callee_name) = body.expr(ExprId::from_idx(*callee)) else {
        return None;
    };
    let entry = registry().lookup_global(callee_name.as_str())?;
    if !matches!(entry.category, Category::PrivilegedMode | Category::SafeMode) {
        return None;
    }
    let opens_unsafe_when = entry.params.iter().find_map(|p| match p.role {
        Role::ModeBool { opens_unsafe_when } if p.index == 0 => Some(opens_unsafe_when),
        _ => None,
    })?;
    let arg = args
        .first()
        .map(|&arg_idx| classify_arg(body, ExprId::from_idx(arg_idx), overlay))
        .unwrap_or(ArgValue::Unknown);
    Some(SecurityCallInfo { category: entry.category, opens_unsafe_when, arg })
}

fn classify_arg(body: &Body, arg_expr: ExprId, overlay: &ValueOverlay) -> ArgValue {
    match body.expr(arg_expr) {
        Expr::Literal(Literal::Bool(true)) => ArgValue::KnownTrue,
        Expr::Literal(Literal::Bool(false)) => ArgValue::KnownFalse,
        Expr::Path(name) => match overlay.get(name.as_str()) {
            Some(KnownValue::Bool(true)) => ArgValue::KnownTrue,
            Some(KnownValue::Bool(false)) => ArgValue::KnownFalse,
            None => ArgValue::Unknown,
        },
        _ => ArgValue::Unknown,
    }
}

/// Run the security-state forward dataflow over a method body. Pure
/// helper — the Salsa wrapper that caches per-module batches lives in
/// `ide-db::effects` (§1.4 of the master plan).
///
/// Returns `None` if the worklist solver fails to converge within
/// `DEFAULT_MAX_ITERATIONS`; that is the same liveness contract used
/// by every other dataflow analysis in this crate.
pub fn analyze(
    cfg: Arc<ControlFlowGraph>,
    body: Body,
) -> Option<DataflowResult<SecurityModeState>> {
    let mut solver = DataflowSolver::new(cfg, body, SecurityStateProvider);
    solver.set_direction(Direction::Forward);
    solver.set_bottom_factory(SecurityModeState::unreachable);
    solver.set_initial_state(SecurityModeState::entry());
    solver.solve()
}

/// One recognised "frame-open" event in a method body.
///
/// Yielded by [`open_events`] for every call site whose
/// `apply_call_to_counters` would `open()` or treat as `unknown()`
/// the targeted frame counter — i.e. every site the §1.6 Group C
/// handler should surface as a security diagnostic. Calls whose const-
/// folded argument is the polarity that *closes* the frame are
/// suppressed (no event yielded). Unknown-bool argument is conservatively
/// treated as opening (matching the legacy "non-literal-False ⇒ emit"
/// behaviour for `SetPrivilegedMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenEvent {
    /// Which frame this call manipulates — `PrivilegedMode` ⇒ source for
    /// `SetPrivilegedMode`, `SafeMode` ⇒ source for `DisableSafeMode`.
    pub category: Category,
    /// `Expr::Path` that names the called API. Use
    /// `BodySourceMap::expr_range(callee)` to recover the IDENT range
    /// the diagnostic should attach to.
    pub callee: ExprId,
    /// Statement carrying the call when known (basic-block leaf
    /// statements). `None` for calls discovered inside compound-stmt
    /// condition expressions (e.g. `If <cond> Then ...`), which live
    /// in dedicated CFG vertices instead of any `BasicBlock`.
    pub stmt: Option<StmtId>,
}

/// Walk every CFG vertex of `result.cfg()` and yield one [`OpenEvent`]
/// for each recognised "frame-open" call site. Pure helper — no Salsa
/// access, no allocation beyond the returned `Vec`. The returned events
/// follow the CFG vertex iteration order; callers that need source-order
/// stability should sort by `result.body().source_map().expr_range(callee)`
/// (handler-side concern, not done here).
///
/// **Coverage shape.** BasicBlock vertices replay the value overlay
/// statement-by-statement so that `Значение = Истина;
/// SetPrivilegedMode(Значение)` is folded as `KnownTrue` rather than
/// `Unknown` — exactly the §1.3 const-prop precision §1.6 Group C
/// needs. Conditional / WhileLoop / ForLoop / ForEachLoop vertices
/// also have their condition / iter expressions walked for security
/// calls, using the vertex IN-state's overlay (no per-stmt replay
/// needed — these vertices carry exactly one expression each). Codex
/// round-3 stop-hook fix: omitting condition vertices regressed the
/// legacy `lower_call_expr` behaviour, which fired on every
/// `CALL_EXPR` regardless of where it appeared.
pub fn open_events(result: &DataflowResult<SecurityModeState>) -> Vec<OpenEvent> {
    let body = result.body();
    let mut events = Vec::new();
    let mut calls_buf: Vec<ExprId> = Vec::new();
    for (vertex_idx, vertex) in result.cfg().vertices() {
        let Some(in_state) = result.block_in(vertex_idx) else { continue };
        let Inner::Reachable { overlay: vertex_overlay, .. } = &in_state.0 else { continue };
        match vertex {
            CfgVertex::BasicBlock(bb) => {
                let mut overlay = vertex_overlay.clone();
                for &stmt_id in bb.statements() {
                    // Codex round-2 stop-hook fix: recognise security calls
                    // anywhere in the statement's expression tree, not just
                    // at the statement-root (legacy `lower_call_expr` fired
                    // on every CALL_EXPR regardless of nesting depth, so
                    // e.g. `Сообщить(УстановитьБезопасныйРежим(Ложь))` had
                    // to keep surfacing).
                    calls_buf.clear();
                    collect_call_exprs_in_stmt(body, stmt_id, &mut calls_buf);
                    emit_for_calls(body, &calls_buf, &overlay, Some(stmt_id), &mut events);
                    crate::value_state::step(&mut overlay, body, stmt_id);
                }
            }
            CfgVertex::Conditional(v) => {
                calls_buf.clear();
                walk_expr_for_calls(body, v.condition.to_idx(), &mut calls_buf);
                emit_for_calls(body, &calls_buf, vertex_overlay, None, &mut events);
            }
            CfgVertex::WhileLoop(v) => {
                calls_buf.clear();
                walk_expr_for_calls(body, v.condition.to_idx(), &mut calls_buf);
                emit_for_calls(body, &calls_buf, vertex_overlay, None, &mut events);
            }
            CfgVertex::ForLoop(v) => {
                calls_buf.clear();
                walk_expr_for_calls(body, v.from.to_idx(), &mut calls_buf);
                walk_expr_for_calls(body, v.to.to_idx(), &mut calls_buf);
                emit_for_calls(body, &calls_buf, vertex_overlay, v.stmt_id, &mut events);
            }
            CfgVertex::ForEachLoop(v) => {
                calls_buf.clear();
                walk_expr_for_calls(body, v.collection.to_idx(), &mut calls_buf);
                emit_for_calls(body, &calls_buf, vertex_overlay, v.stmt_id, &mut events);
            }
            // TryExcept / Label / PreprocCondition / Exit carry no
            // runtime-evaluated security-relevant expressions.
            _ => {}
        }
    }
    events
}

/// Recognise each call in `calls`, yielding an `OpenEvent` for those
/// whose const-folded argument transitions a frame counter to "open".
fn emit_for_calls(
    body: &Body,
    calls: &[ExprId],
    overlay: &ValueOverlay,
    stmt: Option<StmtId>,
    events: &mut Vec<OpenEvent>,
) {
    for &call_expr in calls {
        let Some(info) = recognize_security_call(body, call_expr, overlay) else {
            continue;
        };
        if !call_opens_frame(&info) {
            continue;
        }
        if let Expr::Call { callee, .. } = body.expr(call_expr) {
            events.push(OpenEvent {
                category: info.category,
                callee: ExprId::from_idx(*callee),
                stmt,
            });
        }
    }
}

/// Collect every call-like expression reachable from a leaf statement's
/// expression tree. Returns the call expressions in post-order so nested
/// calls in arguments are recognised before the outer call; this matches
/// BSL's left-to-right evaluation of nested calls.
///
/// Compound statements (`Стмт::If`/`While`/loops) do NOT live in basic
/// blocks: their condition / iter expressions are visited separately
/// in [`open_events`] via the corresponding `ConditionalVertex` /
/// `WhileLoop` / `ForLoop` / `ForEachLoop` CFG nodes. This helper
/// covers ONLY the leaf-statement expression tree.
fn collect_call_exprs_in_stmt(body: &Body, stmt: StmtId, out: &mut Vec<ExprId>) {
    let roots: &[hir_def::hir::ExprIdx] = match body.stmt(stmt) {
        Stmt::Expr(e) => std::slice::from_ref(e),
        Stmt::Assign { target, value } => {
            walk_expr_for_calls(body, *target, out);
            walk_expr_for_calls(body, *value, out);
            return;
        }
        Stmt::Return { value: Some(v) } => std::slice::from_ref(v),
        Stmt::Raise { value: Some(v) } => std::slice::from_ref(v),
        Stmt::Execute { expr } => std::slice::from_ref(expr),
        Stmt::AddHandler { event, handler } => {
            walk_expr_for_calls(body, *event, out);
            walk_expr_for_calls(body, *handler, out);
            return;
        }
        Stmt::RemoveHandler { event, handler } => {
            walk_expr_for_calls(body, *event, out);
            walk_expr_for_calls(body, *handler, out);
            return;
        }
        _ => return,
    };
    for &root in roots {
        walk_expr_for_calls(body, root, out);
    }
}

fn walk_expr_for_calls(body: &Body, expr_idx: hir_def::hir::ExprIdx, out: &mut Vec<ExprId>) {
    let typed = ExprId::from_idx(expr_idx);
    match body.expr(typed) {
        Expr::Call { callee, args } => {
            walk_expr_for_calls(body, *callee, out);
            for &a in args.iter() {
                walk_expr_for_calls(body, a, out);
            }
            out.push(typed);
        }
        Expr::MethodCall { receiver, args, .. } => {
            // Qualified `obj.Method(...)` calls are not in the global-
            // method registry, so they cannot match a security call —
            // but their receiver / arguments may contain nested global
            // calls that do.
            walk_expr_for_calls(body, *receiver, out);
            for &a in args.iter() {
                walk_expr_for_calls(body, a, out);
            }
            out.push(typed);
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            walk_expr_for_calls(body, *lhs, out);
            walk_expr_for_calls(body, *rhs, out);
        }
        Expr::UnaryOp { expr, .. } => walk_expr_for_calls(body, *expr, out),
        Expr::Ternary { condition, then_expr, else_expr } => {
            walk_expr_for_calls(body, *condition, out);
            walk_expr_for_calls(body, *then_expr, out);
            walk_expr_for_calls(body, *else_expr, out);
        }
        Expr::Index { base, index } => {
            walk_expr_for_calls(body, *base, out);
            walk_expr_for_calls(body, *index, out);
        }
        Expr::Field { base, .. } => walk_expr_for_calls(body, *base, out),
        Expr::New { args, .. } => {
            for &a in args.iter() {
                walk_expr_for_calls(body, a, out);
            }
            out.push(typed);
        }
        Expr::Array(items) => {
            for &i in items.iter() {
                walk_expr_for_calls(body, i, out);
            }
        }
        Expr::Await { expr } => walk_expr_for_calls(body, *expr, out),
        Expr::Missing | Expr::Literal(_) | Expr::Path(_) | Expr::QualifiedPath(_) => {}
    }
}

/// Mirror of [`apply_call_to_counters`]'s open-vs-close decision, lifted
/// out so [`open_events`] can decide whether to yield without mutating
/// any counter. Kept private — the lattice's transfer is the
/// authoritative consumer of [`SecurityCallInfo`].
fn call_opens_frame(info: &SecurityCallInfo) -> bool {
    match info.arg {
        ArgValue::KnownTrue if info.opens_unsafe_when => true,
        ArgValue::KnownFalse if !info.opens_unsafe_when => true,
        ArgValue::KnownTrue | ArgValue::KnownFalse => false,
        // Unknown ⇒ worst-of-both-paths: legacy `lower_call_expr`
        // emitted on every non-literal-False arg, so to preserve test
        // behaviour we yield here too. Const-prop has already collapsed
        // every `Перем = literal; Установить(Перем)` chain that the
        // legacy used to over-approximate.
        ArgValue::Unknown => true,
    }
}

fn apply_call_to_counters(counters: &mut SecurityCounters, info: SecurityCallInfo) {
    let target = match info.category {
        Category::PrivilegedMode => &mut counters.privilege,
        Category::SafeMode => &mut counters.unsafe_frame,
        // recognize_security_call already filtered other categories.
        _ => return,
    };
    *target = match info.arg {
        // arg matches `opens_unsafe_when` ⇒ open the frame.
        ArgValue::KnownTrue if info.opens_unsafe_when => target.open(),
        ArgValue::KnownFalse if !info.opens_unsafe_when => target.open(),
        // arg is the opposite polarity ⇒ close the frame.
        ArgValue::KnownTrue | ArgValue::KnownFalse => target.close(),
        // Unknown ⇒ worst-of-both-paths (may grows, must shrinks).
        ArgValue::Unknown => target.unknown(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reachable(p: PrivilegeCounter, u: PrivilegeCounter) -> SecurityModeState {
        SecurityModeState(Inner::Reachable {
            counters: SecurityCounters { privilege: p, unsafe_frame: u },
            overlay: ValueOverlay::empty(),
        })
    }

    fn pc(may: u8, must: u8) -> PrivilegeCounter {
        PrivilegeCounter { may: SaturatingCount::Exact(may), must: SaturatingCount::Exact(must) }
    }

    // SaturatingCount: idempotence on Exact and AtLeast.
    #[test]
    fn saturating_join_max_idempotent() {
        for k in 0..=K_MAX {
            assert_eq!(
                SaturatingCount::join_max(SaturatingCount::Exact(k), SaturatingCount::Exact(k)),
                SaturatingCount::Exact(k),
            );
            assert_eq!(
                SaturatingCount::join_max(SaturatingCount::AtLeast(k), SaturatingCount::AtLeast(k)),
                SaturatingCount::AtLeast(k),
            );
        }
    }

    #[test]
    fn saturating_join_min_idempotent() {
        for k in 0..=K_MAX {
            assert_eq!(
                SaturatingCount::join_min(SaturatingCount::Exact(k), SaturatingCount::Exact(k)),
                SaturatingCount::Exact(k),
            );
            // Round-5 B-1 regression guard: AtLeast(k).join_min(AtLeast(k))
            // must NOT collapse to Exact(k).
            assert_eq!(
                SaturatingCount::join_min(SaturatingCount::AtLeast(k), SaturatingCount::AtLeast(k)),
                SaturatingCount::AtLeast(k),
            );
        }
    }

    #[test]
    fn saturating_inc_saturates_at_k_max() {
        let mut s = SaturatingCount::Exact(0);
        for _ in 0..(K_MAX as usize + 5) {
            s = s.inc();
        }
        // After K_MAX+5 increments, the counter must have crossed into
        // AtLeast(K_MAX) — not silently wrapped or panicked.
        assert!(matches!(s, SaturatingCount::AtLeast(K_MAX)));
    }

    #[test]
    fn saturating_dec_saturates_at_zero() {
        let s = SaturatingCount::Exact(0).dec();
        assert_eq!(s, SaturatingCount::Exact(0));
        let s = SaturatingCount::AtLeast(0).dec();
        assert_eq!(s, SaturatingCount::AtLeast(0));
    }

    // SecurityModeState: Unreachable is the lattice identity.
    #[test]
    fn unreachable_is_join_identity() {
        let bot = SecurityModeState::unreachable();
        let r = reachable(pc(2, 1), pc(1, 0));
        assert_eq!(bot.join(&r), r);
        assert_eq!(r.join(&bot), r);
    }

    #[test]
    fn join_idempotent_on_reachable() {
        let r = reachable(pc(2, 1), pc(0, 0));
        assert_eq!(r.join(&r), r);
    }

    #[test]
    fn join_commutative_on_reachable() {
        let a = reachable(pc(1, 1), pc(0, 0));
        let b = reachable(pc(2, 0), pc(1, 1));
        assert_eq!(a.join(&b), b.join(&a));
    }

    // Counter-domain LUB: (may_a, must_a) ⊑ (may_b, must_b)
    // iff may_a ≤ may_b ∧ must_a ≥ must_b.
    #[test]
    fn counter_lub_is_pointwise_max_min() {
        let a = reachable(pc(1, 1), pc(0, 0));
        let b = reachable(pc(0, 0), pc(0, 0));
        let merged = a.join(&b);
        let merged_p = merged.counters().unwrap().privilege;
        // may = max(1, 0) = 1; must = min(1, 0) = 0.
        assert_eq!(merged_p.may, SaturatingCount::Exact(1));
        assert_eq!(merged_p.must, SaturatingCount::Exact(0));
    }

    // PrivilegeCounter helpers — `Истина; Истина; Ложь` end state.
    #[test]
    fn three_calls_two_open_one_close_leave_must_one() {
        let p = PrivilegeCounter::ENTRY.open().open().close();
        // After {+1, +1, -1}: counter = 1 on every path, so must = 1.
        // The plan's round-2 fix ensured a 4-valued lattice could not
        // collapse this to "off"; pin the behaviour here.
        assert_eq!(p.must, SaturatingCount::Exact(1));
        assert_eq!(p.may, SaturatingCount::Exact(1));
    }

    #[test]
    fn unknown_arg_widens_may_and_shrinks_must() {
        let p = PrivilegeCounter::ENTRY.open().unknown();
        // After {+1, ?}: may grew to 2 (worst-true path), must dropped
        // to 0 (worst-false path).
        assert_eq!(p.may, SaturatingCount::Exact(2));
        assert_eq!(p.must, SaturatingCount::Exact(0));
    }

    #[test]
    fn unknown_at_entry_creates_at_least_state() {
        let p = PrivilegeCounter::ENTRY.unknown();
        // From (0, 0): unknown widens may to (Exact(1)), must clamps at
        // 0. The "open" and "closed" branches of `Установить(?)` collapse
        // into "may=1, must=0".
        assert_eq!(p.may, SaturatingCount::Exact(1));
        assert_eq!(p.must, SaturatingCount::Exact(0));
    }

    // Lattice law coverage with a non-empty overlay: round-1 review
    // flagged that all earlier laws-tests used the empty overlay only.
    fn reachable_with_overlay(p: PrivilegeCounter, overlay: ValueOverlay) -> SecurityModeState {
        SecurityModeState(Inner::Reachable {
            counters: SecurityCounters { privilege: p, unsafe_frame: PrivilegeCounter::ENTRY },
            overlay,
        })
    }

    #[test]
    fn idempotent_with_reachable_arm_engaged() {
        // Coverage point distinct from `join_idempotent_on_reachable`:
        // pin that `Reachable.join(Reachable)` (the heavy arm of the
        // lattice's `match`) is idempotent for arbitrary counter
        // values, including the asymmetric `(may=2, must=1)` case the
        // §1.7 lattice-laws test will need.
        let r = reachable_with_overlay(pc(2, 1), ValueOverlay::empty());
        assert_eq!(r.join(&r), r);
        let r2 = r.clone();
        assert_eq!(r.join(&r2), r);
    }

    #[test]
    fn unreachable_identity_with_non_empty_overlay() {
        let bot = SecurityModeState::unreachable();
        let r = reachable_with_overlay(pc(1, 1), ValueOverlay::empty());
        assert_eq!(bot.join(&r), r);
        assert_eq!(r.join(&bot), r);
    }
}
