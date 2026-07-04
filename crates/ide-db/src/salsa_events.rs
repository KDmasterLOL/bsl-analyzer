//! Opt-in salsa runtime observability (`BSL_SALSA_EVENTS=1`).
//!
//! Aggregates per-ingredient counters from salsa's event stream: how many query
//! instances actually executed (cache miss / invalidation) versus were revalidated
//! from cache, plus intern and discard activity. This is the dynamic complement to
//! [`crate::database::RootDatabaseImpl::memory_report`], which is only a static
//! snapshot of live counts. The counters feed the LRU/memory analysis: a rising
//! `execute` count after an `enforce_lru` trim reveals memos that were evicted and
//! had to be recomputed.
//!
//! The callback runs on salsa's hot path, so [`SalsaEventStats::record`] does only
//! atomic increments — no allocation, no database access, no name resolution, no
//! logging. Ingredient names are resolved once, at report time, via
//! `salsa::Database::ingredient_debug_name`, which needs the database the callback
//! does not carry.

use std::hash::BuildHasherDefault;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use rustc_hash::FxHasher;
use salsa::{DatabaseKeyIndex, Event, EventKind, IngredientIndex};

type FxDashMap<K, V> = DashMap<K, V, BuildHasherDefault<FxHasher>>;

/// Per-ingredient salsa event counters. Monotonic since the owning
/// [`SalsaEventStats`] was created.
#[derive(Default)]
struct IngredientCounters {
    /// `WillExecute` — the query body actually ran (cache miss or invalidation).
    execute: AtomicU64,
    /// `DidValidateMemoizedValue` — inputs were up to date, memo reused without running.
    validate: AtomicU64,
    /// `DidDiscard` — a tracked-struct or memo value was freed.
    did_discard: AtomicU64,
    /// `WillDiscardStaleOutput` — attributed to the executing (`execute_key`) query.
    discard_stale: AtomicU64,
    /// `DidInternValue` — a value was newly interned.
    intern_new: AtomicU64,
    /// `DidReuseInternedValue` — an interned slot was reused for a new value.
    intern_reuse: AtomicU64,
    /// `DidValidateInternedValue` — a previously interned value was read in a new revision.
    intern_validate: AtomicU64,
    /// `WillBlockOn` — this query was found running on another thread; we blocked.
    block_on: AtomicU64,
}

/// Per-*key* churn counters — how often one concrete query instance (a specific
/// file or method, not just the query type) re-executed or had a stale output
/// discarded. This is the incremental-attribution signal the per-ingredient
/// counters cannot give: "which 12 modules caused 80% of the re-execution".
#[derive(Default)]
struct KeyCounters {
    /// `WillExecute` for this exact `DatabaseKeyIndex` — the instance recomputed.
    execute: AtomicU64,
    /// `WillDiscardStaleOutput` attributed to this executing key.
    discard_stale: AtomicU64,
}

/// Lock-free aggregation of the salsa event stream. Shared (as `Arc`) across all
/// cloned database handles and rayon worker snapshots — a salsa `event_callback`
/// is one per `Zalsa`, so the counters are process-global for one database tree.
#[derive(Default)]
pub struct SalsaEventStats {
    per_ingredient: FxDashMap<IngredientIndex, IngredientCounters>,
    /// Per-key churn (`execute` / `discard_stale`), keyed by the full
    /// [`DatabaseKeyIndex`] the event carries. Grows with the number of *distinct*
    /// keys that executed in the observed run — bounded by the workload, and only
    /// populated under the `BSL_SALSA_EVENTS=1` gate. The hot path only inserts
    /// `Copy` keys and bumps atomics; top-K selection and name resolution happen at
    /// report time (see [`Self::top_keys`]), never in the callback.
    per_key: FxDashMap<DatabaseKeyIndex, KeyCounters>,
    /// `WillCheckCancellation` — no ingredient key; counted globally. Fires very often.
    check_cancellation: AtomicU64,
    /// `DidSetCancellationFlag` — a handle set the cancellation flag.
    set_cancellation: AtomicU64,
    /// `DidDiscardAccumulated` — accumulator discards; no single owning ingredient.
    discard_accumulated: AtomicU64,
}

impl SalsaEventStats {
    fn bump(&self, idx: IngredientIndex, pick: impl Fn(&IngredientCounters) -> &AtomicU64) {
        // Steady state hits the read-locked `get` fast path (shared across threads);
        // the write-locked `entry` insert runs at most once per ingredient.
        if let Some(ctr) = self.per_ingredient.get(&idx) {
            pick(ctr.value()).fetch_add(1, Ordering::Relaxed);
            return;
        }
        let ctr = self.per_ingredient.entry(idx).or_default();
        pick(ctr.value()).fetch_add(1, Ordering::Relaxed);
    }

    fn bump_key(&self, key: DatabaseKeyIndex, pick: impl Fn(&KeyCounters) -> &AtomicU64) {
        if let Some(ctr) = self.per_key.get(&key) {
            pick(ctr.value()).fetch_add(1, Ordering::Relaxed);
            return;
        }
        let ctr = self.per_key.entry(key).or_default();
        pick(ctr.value()).fetch_add(1, Ordering::Relaxed);
    }

    /// Record one salsa event. Runs on the hot path — panic-free, no allocation-heavy
    /// work, no formatting, and no database access (a re-entrant salsa call here could
    /// deadlock against the memo or intern-table locks held at the event's emission
    /// point). Steady state is a read-locked map lookup plus a relaxed atomic add; the
    /// first sighting of an ingredient or key takes a sharded write lock to insert its
    /// counter once. The per-key map is higher-cardinality than the per-ingredient
    /// one, so it sees more first-sight inserts, but only ever `Copy` keys and atomics.
    pub fn record(&self, event: &Event) {
        match &event.kind {
            EventKind::WillExecute { database_key } => {
                self.bump(database_key.ingredient_index(), |c| &c.execute);
                self.bump_key(*database_key, |c| &c.execute);
            }
            EventKind::DidValidateMemoizedValue { database_key } => {
                self.bump(database_key.ingredient_index(), |c| &c.validate)
            }
            EventKind::DidDiscard { key } => self.bump(key.ingredient_index(), |c| &c.did_discard),
            EventKind::WillDiscardStaleOutput { execute_key, .. } => {
                self.bump(execute_key.ingredient_index(), |c| &c.discard_stale);
                self.bump_key(*execute_key, |c| &c.discard_stale);
            }
            EventKind::DidInternValue { key, .. } => {
                self.bump(key.ingredient_index(), |c| &c.intern_new)
            }
            EventKind::DidReuseInternedValue { key, .. } => {
                self.bump(key.ingredient_index(), |c| &c.intern_reuse)
            }
            EventKind::DidValidateInternedValue { key, .. } => {
                self.bump(key.ingredient_index(), |c| &c.intern_validate)
            }
            EventKind::WillBlockOn { database_key, .. } => {
                self.bump(database_key.ingredient_index(), |c| &c.block_on)
            }
            EventKind::WillCheckCancellation => {
                self.check_cancellation.fetch_add(1, Ordering::Relaxed);
            }
            EventKind::DidSetCancellationFlag => {
                self.set_cancellation.fetch_add(1, Ordering::Relaxed);
            }
            EventKind::DidDiscardAccumulated { .. } => {
                self.discard_accumulated.fetch_add(1, Ordering::Relaxed);
            }
            // Fixpoint-cycle iteration bookkeeping — not a memory/incrementality signal.
            EventKind::WillIterateCycle { .. } | EventKind::DidFinalizeCycle { .. } => {}
        }
    }

    /// Global (keyless) counters: `(check_cancellation, set_cancellation, discard_accumulated)`.
    pub fn global_counts(&self) -> GlobalCounts {
        GlobalCounts {
            check_cancellation: self.check_cancellation.load(Ordering::Relaxed),
            set_cancellation: self.set_cancellation.load(Ordering::Relaxed),
            discard_accumulated: self.discard_accumulated.load(Ordering::Relaxed),
        }
    }

    /// Per-ingredient rows resolved to names via `resolve`, sorted by descending
    /// `(execute, validate)`. `resolve` maps an [`IngredientIndex`] to its debug
    /// name — pass a closure over `salsa::Database::ingredient_debug_name` (the
    /// database is available at the report call site but not inside the callback).
    pub fn rows(&self, resolve: impl Fn(IngredientIndex) -> String) -> Vec<SalsaEventRow> {
        let mut rows: Vec<SalsaEventRow> = self
            .per_ingredient
            .iter()
            .map(|e| {
                let c = e.value();
                SalsaEventRow {
                    name: resolve(*e.key()),
                    execute: c.execute.load(Ordering::Relaxed),
                    validate: c.validate.load(Ordering::Relaxed),
                    did_discard: c.did_discard.load(Ordering::Relaxed),
                    discard_stale: c.discard_stale.load(Ordering::Relaxed),
                    intern_new: c.intern_new.load(Ordering::Relaxed),
                    intern_reuse: c.intern_reuse.load(Ordering::Relaxed),
                    intern_validate: c.intern_validate.load(Ordering::Relaxed),
                    block_on: c.block_on.load(Ordering::Relaxed),
                }
            })
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse((r.execute, r.validate)));
        rows
    }

    /// The `k` hottest keys by `(execute, discard_stale)`, as raw
    /// [`DatabaseKeyIndex`] values with their counts. Names are deliberately *not*
    /// resolved here: turning a key into a readable file/method requires the
    /// database (interned-value lookup + `salsa::attach`), which the report call
    /// site has but this crate-internal aggregator must not touch. See
    /// `RootDatabaseImpl::salsa_key_event_report` for the resolving wrapper.
    pub fn top_keys(&self, k: usize) -> Vec<KeyEventCount> {
        let mut rows: Vec<KeyEventCount> = self
            .per_key
            .iter()
            .map(|e| {
                let c = e.value();
                KeyEventCount {
                    key: *e.key(),
                    execute: c.execute.load(Ordering::Relaxed),
                    discard_stale: c.discard_stale.load(Ordering::Relaxed),
                }
            })
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse((r.execute, r.discard_stale)));
        rows.truncate(k);
        rows
    }
}

/// Global keyless event counters; see [`SalsaEventStats::global_counts`].
#[derive(Clone, Copy)]
pub struct GlobalCounts {
    pub check_cancellation: u64,
    pub set_cancellation: u64,
    pub discard_accumulated: u64,
}

/// One hot key with raw counts, before name resolution; see
/// [`SalsaEventStats::top_keys`].
#[derive(Clone, Copy)]
pub struct KeyEventCount {
    pub key: DatabaseKeyIndex,
    pub execute: u64,
    pub discard_stale: u64,
}

/// One hot key resolved to a human-readable name (`query(<module/method>)` where
/// the key type is known, else `query(Id(..))`); see
/// `RootDatabaseImpl::salsa_key_event_report`.
pub struct KeyEventRow {
    pub name: String,
    pub execute: u64,
    pub discard_stale: u64,
}

/// One ingredient's resolved event row; see [`SalsaEventStats::rows`].
pub struct SalsaEventRow {
    pub name: String,
    pub execute: u64,
    pub validate: u64,
    pub did_discard: u64,
    pub discard_stale: u64,
    pub intern_new: u64,
    pub intern_reuse: u64,
    pub intern_validate: u64,
    pub block_on: u64,
}
