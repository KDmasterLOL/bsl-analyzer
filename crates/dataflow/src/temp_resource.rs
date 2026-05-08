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
//! caller plugs a [`ResourceProvider<R>`] adapter that maps statements to
//! open / close events on a per-handler basis.
//!
//! ## Lattice and transfer
//!
//! [`OpenSet<R>`] is the abstract domain — a hash map from each open
//! resource to the set of `StmtId`s that opened it on the path so far
//! (see [`OpenSet`] for the rationale behind site-tracking).
//!
//! Transfer order on a single statement is "open first, then close" — a
//! provider that legitimately closes within a single statement (rare in
//! practice; we have not encountered it in either BSL handler) is allowed
//! to close what it just opened. Open additively merges new sites into
//! the existing entry (does NOT overwrite), so a second `Get` of the same
//! resource records both Get sites; the next `Delete` clears the whole
//! entry, matching the canonical "delete kills all open Gets" semantics
//! of the existing AST-based handlers.
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
//! opening-site)` pair that survives to the exit block on at least one
//! path. A diagnostic should be emitted at each `StmtId` in those sets;
//! the `R` is purely an internal grouping key. Grouping by `R` instead
//! of just collecting sites preserves the existing AST handlers'
//! contract that a `Delete*` of resource `R` cancels every prior `Get*`
//! of `R`, including ones from sibling branches that would otherwise
//! leak across a merge point — see [`OpenSet::join_in_place`] for the
//! join law that backs this.
//!
//! ## Why site-tracking instead of `FxHashSet<R>`
//!
//! A naive `OpenSet<R> = FxHashSet<R>` formulation forces the consumer
//! to re-scan the body for every `Get*` and check membership against the
//! exit set. That over-emits in the canonical `Get → Delete → Get`
//! sequence with the same key: at exit `R ∈ open-set`, both `Get` sites
//! match, but only the second one actually leaks. With site-tracking the
//! `Delete*` between the two clears the inner site set, the second `Get`
//! seeds it with just its own `StmtId`, and only that StmtId surfaces at
//! exit. The hash map's key set is still the "is this resource open?"
//! summary the lattice law cares about; the inner sets are extra
//! provenance the consumer needs and the lattice carries cheaply.

use cfg::{CfgEdgeType, ControlFlowGraph};
use hir_def::body::Body;
use hir_def::StmtId;
use la_arena::RawIdx;
use rustc_hash::{FxHashMap, FxHashSet};
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::{DataflowResult, DataflowSolver, Direction, Lattice, Transfer, DEFAULT_MAX_ITERATIONS};

/// Provider mapping statements to resource open / close events.
///
/// Implementations live in handler crates (or `hir-ty` if shared by
/// multiple handlers); this trait carries no BSL-specific knowledge so
/// the dataflow crate stays generic over the resource identifier `R`.
///
/// ## Contract
///
/// - [`Self::opens`] returns `Some(r)` if `stmt_id` opens resource `r`
///   (e.g. `ПолучитьИмяВременногоФайла()` assigning to a variable —
///   `r` is the variable's lowercase name).
/// - [`Self::closes`] returns `Some(r)` if `stmt_id` closes resource
///   `r`. The "close" is interpreted as "every prior `open` of the same
///   `r` is now consumed"; if a single statement closes multiple
///   resources, the provider must return them via [`Self::closes_many`]
///   (default forwards to `closes`).
/// - Both methods may be called many times on the same statement during
///   fixed-point iteration — implementations should be deterministic
///   and side-effect-free.
pub trait ResourceProvider<R>
where
    R: Clone + Eq + Hash,
{
    /// Single-resource open. Default returns `None` so providers that
    /// only override [`Self::opens_many`] still compile.
    fn opens(&self, _body: &Body, _stmt_id: StmtId) -> Option<R> {
        None
    }

    /// Single-resource close. Default returns `None`; see [`Self::opens`].
    fn closes(&self, _body: &Body, _stmt_id: StmtId) -> Option<R> {
        None
    }

    /// Multi-resource open hook for statements that open more than one
    /// resource at once. Default delegates to [`Self::opens`] so simple
    /// providers don't have to implement two methods.
    fn opens_many(&self, body: &Body, stmt_id: StmtId) -> Vec<R> {
        self.opens(body, stmt_id).into_iter().collect()
    }

    /// Multi-resource close hook for statements that close more than one
    /// resource at once (e.g. `УдалитьФайлы(a, b)`). Default delegates
    /// to [`Self::closes`].
    fn closes_many(&self, body: &Body, stmt_id: StmtId) -> Vec<R> {
        self.closes(body, stmt_id).into_iter().collect()
    }
}

/// Open-set lattice element.
///
/// See module-level docs for the site-tracking rationale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSet<R>
where
    R: Clone + Eq + Hash,
{
    open: FxHashMap<R, FxHashSet<StmtId>>,
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

    /// Borrow the underlying `(resource, opening-sites)` map. Consumers
    /// typically read this from the exit block to drive diagnostic
    /// emission — see [`OpenResourcesResult::open_at_exit`].
    pub fn as_map(&self) -> &FxHashMap<R, FxHashSet<StmtId>> {
        &self.open
    }

    fn open_at(&mut self, r: R, site: StmtId) {
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
/// solver's per-statement and per-edge hooks.
struct ResourceTransfer<P, R>
where
    R: Clone + Eq + Hash,
    P: ResourceProvider<R>,
{
    provider: P,
    _r: PhantomData<R>,
}

impl<P, R> Transfer<OpenSet<R>> for ResourceTransfer<P, R>
where
    R: Clone + Eq + Hash,
    P: ResourceProvider<R>,
{
    fn transfer_stmt(&self, stmt_id: RawIdx, state: &OpenSet<R>, body: &Body) -> OpenSet<R> {
        let mut next = state.clone();
        let stmt = StmtId::from_raw(stmt_id);
        for r in self.provider.opens_many(body, stmt) {
            next.open_at(r, stmt);
        }
        for r in self.provider.closes_many(body, stmt) {
            next.close(&r);
        }
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

/// Resolved open-resources result for one body.
///
/// Pin every `(resource, opening-site)` pair that survives to the exit
/// block on at least one path; consumers iterate the inner site sets
/// to emit one diagnostic per Get site.
pub struct OpenResourcesResult<R>
where
    R: Clone + Eq + Hash,
{
    inner: DataflowResult<OpenSet<R>>,
    exit_block: petgraph::graph::NodeIndex,
    empty_map: FxHashMap<R, FxHashSet<StmtId>>,
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
    pub fn open_at_exit(&self) -> &FxHashMap<R, FxHashSet<StmtId>> {
        self.inner.block_out(self.exit_block).map(|set| set.as_map()).unwrap_or(&self.empty_map)
    }
}

/// Run the open-resources analysis on a body.
///
/// Returns `None` only if the framework's solver fails to converge
/// within [`DEFAULT_MAX_ITERATIONS`] (pathological CFG); on success
/// every block has its IN/OUT computed and consumers can read the
/// exit OUT via [`OpenResourcesResult::open_at_exit`].
pub fn analyze_open_resources<P, R>(
    body: Body,
    cfg: Arc<ControlFlowGraph>,
    provider: P,
) -> Option<OpenResourcesResult<R>>
where
    R: Clone + Eq + Hash,
    P: ResourceProvider<R>,
{
    let exit_block = cfg.exit_point();
    let transfer = ResourceTransfer { provider, _r: PhantomData };
    let mut solver = DataflowSolver::new(cfg, body, transfer);
    solver.set_direction(Direction::Forward);
    solver.set_bottom_factory(OpenSet::<R>::bottom);
    solver.set_max_iterations(DEFAULT_MAX_ITERATIONS);
    let inner = solver.solve()?;
    Some(OpenResourcesResult { inner, exit_block, empty_map: FxHashMap::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(n: u32) -> StmtId {
        StmtId::from_raw(RawIdx::from_u32(n))
    }

    #[test]
    fn open_set_join_unions_inner_site_sets() {
        let s1 = raw(1);
        let s2 = raw(2);
        let s3 = raw(3);

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
        let s1 = raw(1);
        let s2 = raw(2);

        let mut state: OpenSet<&str> = OpenSet::bottom();
        state.open_at("X", s1);
        state.open_at("X", s2);
        assert_eq!(state.as_map().get("X").unwrap().len(), 2);

        state.close(&"X");
        assert!(state.is_empty(), "close must drop the entry, not just shrink it");
    }

    #[test]
    fn open_set_lattice_idempotence_and_commutativity() {
        let s1 = raw(1);
        let s2 = raw(2);

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
        let s1 = raw(1);
        let s2 = raw(2);
        let s3 = raw(3);

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
