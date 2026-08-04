//! Daemon-owned filesystem change hub.
//!
//! One [`notify`] watcher covers the workspace root recursively and folds raw
//! events into a bounded, typed accumulator keyed by canonical path. Consumers
//! (search today; diagnostics and graph later) each register a cursor and pull
//! the drift they have not yet seen, so a slow or restarting sink never
//! blast-radiuses the others.
//!
//! Reclamation is driven by the cursors: an entry is dropped once every live
//! cursor has advanced past it, so the capacity bounds only *undrained* in-flight
//! paths rather than growing for the daemon's whole lifetime. If more than the
//! cap pile up undrained (a mass change like a branch switch), or the event
//! stream is lossy, the accumulator is cleared and every live cursor is told —
//! exactly once — to reconcile with a full scan; health returns to `Healthy` once
//! all cursors have acknowledged, and accumulation continues.

use notify::{
    Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};
use walkdir::WalkDir;

/// Default capacity, counted in *undrained* in-flight distinct paths. Beyond this
/// the hub drops detail and asks consumers to reconcile with a full scan — the
/// same cost they would pay on a cold start, so correctness is preserved.
const DEFAULT_CAPACITY: usize = 8192;

/// Bound on the notify-callback → hub-thread channel. `try_send` past this never
/// blocks the notify callback; the overflow is folded into the same rescan path
/// as an accumulator overflow, so a storm degrades gracefully instead of spiking
/// memory.
const CHANNEL_BOUND: usize = 65536;

/// What is known to have happened to a path within a drain window. The kind is
/// re-derived from on-disk state at event time (stats are truth), so a
/// create-then-delete or delete-then-create burst settles on the final reality
/// rather than misclassifying on the first event seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChangeKind {
    /// The path exists on disk; its content may have changed.
    MaybeChanged,
    /// A file path is gone; consumers should tombstone it.
    MaybeRemoved,
    /// An extension-less path under the root vanished — most likely a removed
    /// directory whose descendants must be expanded by the consumer.
    SubtreeRemoved,
}

/// Why the hub is asking consumers to reconcile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DegradeReason {
    /// The watcher could not be created or could not watch the root (permanent).
    WatcherSetup,
    /// The watcher delivered a runtime error through its callback.
    RuntimeError,
    /// An event kind outside Create/Modify/Remove/Access arrived; rather than
    /// silently dropping it, the hub assumes it may have missed real drift.
    UnknownEvent,
    /// Extending the recursive watch to a newly-created subtree failed, so that
    /// subtree may be blind to further changes until a reconcile re-covers it.
    RewatchFailed,
    /// A consumer's periodic reconcile scan found drift the event stream never
    /// delivered — evidence the backend is lossy, so fall back to scanning.
    ReconcileMiss,
    /// More than the cap piled up undrained, or the callback channel overflowed:
    /// detail was dropped and a full reconcile scan is needed.
    Overflow,
    /// The watched root set was re-pointed (an extension topology reload). State a
    /// consumer derived under the old set predates the new roots' coverage, so
    /// each must rescan once before trusting the stream again.
    Rearmed,
}

/// Observable health of the hub. `WatcherSetup` is permanent; every other
/// degradation is transient and clears back to `Healthy` once all live cursors
/// have acknowledged the reconcile request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Health {
    Healthy,
    Degraded(DegradeReason),
}

impl Health {
    /// A stable label for status reporting.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Health::Healthy => "healthy",
            Health::Degraded(DegradeReason::WatcherSetup) => "degraded:watcher-setup",
            Health::Degraded(DegradeReason::RuntimeError) => "degraded:runtime-error",
            Health::Degraded(DegradeReason::UnknownEvent) => "degraded:unknown-event",
            Health::Degraded(DegradeReason::RewatchFailed) => "degraded:rewatch-failed",
            Health::Degraded(DegradeReason::ReconcileMiss) => "degraded:reconcile-miss",
            Health::Degraded(DegradeReason::Overflow) => "degraded:overflow",
            Health::Degraded(DegradeReason::Rearmed) => "degraded:rearmed",
        }
    }
}

/// One accumulated change. Carries both the canonical key (matching the scan
/// universe used by drift detection) and the raw path as the watcher reported
/// it — consumers that strip a non-canonical root (search strips the configured
/// source root) need the raw spelling, or a symlinked root would fail to match.
///
/// `canonical` and `kind` are the drift-consumption contract: the shared
/// classifier re-stats `canonical` (stats are truth) and branches on `kind` for a
/// subtree removal. `raw` is the watcher spelling search strips its source root
/// against.
#[derive(Debug, Clone)]
pub(crate) struct ChangeEntry {
    pub(crate) canonical: PathBuf,
    pub(crate) raw: PathBuf,
    pub(crate) kind: ChangeKind,
    pub(crate) seq: u64,
}

/// A consumer's handle into the change stream. Opaque; the cursor's position and
/// pending-rescan flag live inside the hub, keyed by this id, so cursors are
/// independent and reclamation can track the slowest one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SinkCursor {
    id: u64,
}

/// The result of draining a cursor: the entries newer than the cursor's last
/// position, the cursor to reuse, and whether this cursor must reconcile with a
/// full scan (delivered exactly once per overflow).
#[derive(Debug, Clone)]
pub(crate) struct DrainBatch {
    pub(crate) entries: Vec<ChangeEntry>,
    pub(crate) cursor: SinkCursor,
    pub(crate) rescan_required: bool,
}

/// Per-cursor state held by the accumulator.
struct CursorState {
    /// The last sequence number this cursor has drained through.
    pos: u64,
    /// Set when the hub overflowed; delivered once on the next drain, then cleared.
    pending_rescan: bool,
}

/// Bounded, seq-tagged accumulator. Entries coalesce by canonical path so the map
/// is bounded by the number of distinct *undrained* dirty paths, not the event
/// rate or the daemon's lifetime.
struct Accumulator {
    entries: HashMap<PathBuf, ChangeEntry>,
    cursors: HashMap<u64, CursorState>,
    next_cursor_id: u64,
    cap: usize,
    /// Next sequence number to assign. Monotonic across the hub's lifetime; a
    /// path's seq moves forward on every update so a lagging cursor still sees
    /// its latest state.
    next_seq: u64,
    /// Bumped when there is new work for sinks to drain (a recorded change, an
    /// entered-rescan transition, or setup completion), so sink threads sleep on
    /// the condvar and wake only when needed — never per dropped overflow event.
    generation: u64,
    /// Monotonic count of raw watcher events observed, for observability.
    events_seen: u64,
    /// The active reconcile reason, or `None` when healthy. Set when the hub
    /// enters a rescan; cleared once every live cursor has acknowledged.
    degrade_reason: Option<DegradeReason>,
    /// Set once if the watcher could not be set up. Permanent for the hub's life.
    setup_failed: bool,
    /// Every reconcile REQUEST, not every reconcile a consumer sees. `enter_rescan`
    /// collapses repeats inside an open window, and `drain` closes that window, so
    /// external state cannot tell one request per tick from one per target — while
    /// the cost is real: a consumer answers each with a full tree walk.
    rescans_requested: u64,
}

impl Accumulator {
    fn new(cap: usize) -> Self {
        Self {
            entries: HashMap::new(),
            cursors: HashMap::new(),
            next_cursor_id: 1,
            cap,
            next_seq: 1,
            generation: 0,
            events_seen: 0,
            degrade_reason: None,
            setup_failed: false,
            rescans_requested: 0,
        }
    }

    fn max_seq(&self) -> u64 {
        self.next_seq - 1
    }

    fn subscribe(&mut self) -> u64 {
        let id = self.next_cursor_id;
        self.next_cursor_id += 1;
        // A cursor born during an active rescan window must still be told to
        // reconcile; one born while healthy starts clean.
        let pending_rescan = self.degrade_reason.is_some();
        self.cursors.insert(id, CursorState { pos: self.max_seq(), pending_rescan });
        id
    }

    fn unsubscribe(&mut self, id: u64) {
        self.cursors.remove(&id);
        self.reclaim();
    }

    fn record(&mut self, canonical: PathBuf, raw: PathBuf, kind: ChangeKind) {
        // A brand-new key past the cap means more than `cap` distinct paths are
        // waiting undrained: detail is no longer trustworthy, so drop it all and
        // ask consumers to reconcile. Already-tracked keys still refresh below.
        if !self.entries.contains_key(&canonical) && self.entries.len() >= self.cap {
            self.enter_rescan(true, DegradeReason::Overflow);
            return;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.entries.insert(canonical.clone(), ChangeEntry { canonical, raw, kind, seq });
        self.generation += 1;
    }

    /// Enter a reconcile window: optionally clear the (now-untrusted) entries, flag
    /// every live cursor to reconcile once, and record the reason. Idempotent while
    /// a window is already open — repeated overflow events neither re-log nor
    /// re-wake sinks, so a storm does not thrash.
    fn enter_rescan(&mut self, clear_entries: bool, reason: DegradeReason) {
        self.rescans_requested += 1;
        let newly = self.degrade_reason.is_none();
        if clear_entries {
            self.entries.clear();
        }
        let mut changed = newly;
        for cursor in self.cursors.values_mut() {
            if !cursor.pending_rescan {
                cursor.pending_rescan = true;
                changed = true;
            }
        }
        self.degrade_reason = Some(reason.clone());
        if changed {
            self.generation += 1;
        }
        if newly {
            tracing::warn!(
                ?reason,
                "workspace change hub entering reconcile; consumers will rescan"
            );
        }
    }

    fn drain(&mut self, id: u64) -> DrainBatch {
        let max = self.max_seq();
        let pos = self.cursors.get(&id).map(|c| c.pos).unwrap_or(max);
        let mut entries: Vec<ChangeEntry> =
            self.entries.values().filter(|e| e.seq > pos).cloned().collect();
        entries.sort_by_key(|e| e.seq);

        let rescan_required = match self.cursors.get_mut(&id) {
            Some(cursor) => {
                cursor.pos = max;
                std::mem::replace(&mut cursor.pending_rescan, false)
            }
            None => false,
        };

        // Recover once no live cursor is still owed a reconcile.
        if self.degrade_reason.is_some() && !self.cursors.values().any(|c| c.pending_rescan) {
            self.degrade_reason = None;
        }
        self.reclaim();

        DrainBatch { entries, cursor: SinkCursor { id }, rescan_required }
    }

    /// Drop entries every live cursor has already advanced past. With no cursors
    /// nothing is being observed, so the map is emptied.
    fn reclaim(&mut self) {
        match self.cursors.values().map(|c| c.pos).min() {
            Some(min_pos) => self.entries.retain(|_, e| e.seq > min_pos),
            None => self.entries.clear(),
        }
    }

    fn health(&self) -> Health {
        if self.setup_failed {
            return Health::Degraded(DegradeReason::WatcherSetup);
        }
        match &self.degrade_reason {
            Some(reason) => Health::Degraded(reason.clone()),
            None => Health::Healthy,
        }
    }
}

/// What the hub takes into work, derived from the watch targets themselves.
///
/// Two permissions, deliberately separate. A path under a SCAN ROOT may be
/// recorded, walked and re-watched. A project-config file directly in a config
/// directory may only be recorded — the name grants no right to walk, or a
/// directory that merely carries that name would be taken under recursive watch.
/// The permissions add up: in a flat project the workspace IS the scan root, so a
/// config-named directory there is walked on the ordinary rule.
///
/// Both spellings of every directory are kept. `dedup_targets` decides by the
/// canonical path while `watcher.watch` receives the raw one, so events arrive
/// spelled either way and a predicate holding one spelling would discard a whole
/// tree declared through a symlink.
#[derive(Debug, Default, Clone)]
struct Scope {
    scan_roots: Vec<Spellings>,
    config_dirs: Vec<Spellings>,
}

/// Every spelling one watched directory can appear under in an event path.
///
/// Two, because a scope is only ever built from [`ResolvedTargets`], whose paths
/// are already absolute: `declared` is what `watcher.watch` receives and what the
/// backend therefore reports, `canonical` is what topology decisions rank by. The
/// two part company as soon as the path crosses a symlink, and a predicate holding
/// one of them would discard a whole tree declared through the other.
#[derive(Debug, Clone)]
struct Spellings {
    declared: PathBuf,
    canonical: PathBuf,
}

impl Spellings {
    fn of(path: &Path) -> Self {
        Self {
            declared: path.to_path_buf(),
            canonical: path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
        }
    }

    fn covers(&self, path: &Path) -> bool {
        path.starts_with(&self.declared) || path.starts_with(&self.canonical)
    }

    fn is(&self, path: &Path) -> bool {
        path == self.declared || path == self.canonical
    }
}

/// Watch targets whose relative paths have been resolved against the current
/// directory exactly ONCE, before anything is armed or compared.
///
/// A newtype rather than a convention, because forgetting the call is invisible:
/// the current directory would then be read twice from process-wide state — by the
/// scope and, later, by the backend inside `watch` — and a change in between would
/// leave the watcher armed on one tree while the scope describes another, with
/// every event from the armed tree filtered out in silence. Nothing observable
/// fails, so no test catches it; the type does. Handing the backend an
/// already-absolute path also removes that second read entirely: `watch_inner`
/// takes an absolute path as given.
/// In its own module so the fields are out of reach even here: a tuple constructor
/// visible to the rest of the file would let the resolution be skipped by writing
/// `ResolvedTargets(targets)`, which is precisely the mistake the type exists to
/// make impossible.
mod resolved {
    use std::path::Path;

    use super::WatchTarget;

    pub(super) struct ResolvedTargets {
        targets: Vec<WatchTarget>,
        complete: bool,
    }

    impl ResolvedTargets {
        /// Resolve against ONE snapshot of the current directory, taken by the
        /// caller for the whole set. Per-target reads would let a set spanning a
        /// scan root and a config directory land in two different workspaces.
        ///
        /// The join is exactly what a notify backend does to a relative target:
        /// prepend the current directory and leave the components alone.
        /// `std::path::absolute` is NOT a substitute — on Windows it goes through
        /// `GetFullPathNameW`, which resolves `..` away (`C:\foo\..\bar.rs` becomes
        /// `C:\foo\bar.rs`), while the backend keeps the component, so the watched
        /// path and the reported one would stop matching and the whole tree would go
        /// silent. On Unix it also drops `.` components, which the backend keeps.
        ///
        /// Without a snapshot (`cwd` is `None` — a deleted or unreadable current
        /// directory) a relative target is DROPPED rather than carried through
        /// relative: keeping it would hand the backend a path it resolves against
        /// its own later read of the same process-wide state, which is the very
        /// disagreement this type exists to prevent. The set then reports itself
        /// incomplete, and the caller degrades instead of claiming coverage.
        pub(super) fn resolve(targets: Vec<WatchTarget>, cwd: Option<&Path>) -> Self {
            let mut complete = true;
            let targets = targets
                .into_iter()
                .filter_map(|target| {
                    if target.path.is_absolute() {
                        return Some(target);
                    }
                    let placed = cwd.map(|cwd| cwd.join(&target.path));
                    // The join is checked, not trusted. On Windows a drive-relative
                    // target (`C:src`) carries a prefix without a root, and `join`
                    // REPLACES the base with it, so the result is still relative and
                    // the backend would resolve it against its own later read of the
                    // per-drive current directory — the very race being removed here.
                    match placed.filter(|path| path.is_absolute()) {
                        Some(path) => Some(WatchTarget { path, recursive: target.recursive }),
                        None => {
                            tracing::warn!(
                                root = ?target.path,
                                "workspace change hub cannot place a relative watch root"
                            );
                            complete = false;
                            None
                        }
                    }
                })
                .collect();
            Self { targets, complete }
        }

        pub(super) fn here(targets: Vec<WatchTarget>) -> Self {
            Self::resolve(targets, std::env::current_dir().ok().as_deref())
        }

        /// Whether every requested target survived resolution. `false` means the
        /// watch cannot cover what was asked for, whatever the backend then says.
        pub(super) fn is_complete(&self) -> bool {
            self.complete
        }

        pub(super) fn as_slice(&self) -> &[WatchTarget] {
            &self.targets
        }

        pub(super) fn into_inner(self) -> Vec<WatchTarget> {
            self.targets
        }
    }
}

use resolved::ResolvedTargets;

impl Scope {
    /// Recursive targets are the scan roots; a non-recursive target is there for
    /// the project-config files that live directly in it (see
    /// [`watch_targets_for`]). Built from the DESIRED targets, not the armed ones:
    /// a root that failed to arm is still part of the scope, and its events —
    /// arriving through a covering target — must not be dropped.
    fn from_targets(targets: &ResolvedTargets) -> Self {
        let spellings = |t: &WatchTarget| Spellings::of(&t.path);
        let targets = targets.as_slice();
        Self {
            scan_roots: targets.iter().filter(|t| t.recursive).map(spellings).collect(),
            config_dirs: targets.iter().filter(|t| !t.recursive).map(spellings).collect(),
        }
    }

    /// Whether a change to `path` may be walked and taken under recursive watch.
    fn may_walk(&self, path: &Path) -> bool {
        self.scan_roots.iter().any(|root| root.covers(path))
    }

    /// Whether a change to `path` may be recorded for consumers.
    fn may_record(&self, path: &Path) -> bool {
        self.may_walk(path) || self.is_project_config(path)
    }

    /// A project-config file sitting DIRECTLY in a config directory. Decided from
    /// the name and the parent alone, never from the disk: a deleted config shapes
    /// the topology just as much as an edited one, and a predicate gated on "the
    /// file exists" would drop the removal.
    fn is_project_config(&self, path: &Path) -> bool {
        let named_like_a_config = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| project_model::CONFIG_FILE_NAMES.contains(&n));
        if !named_like_a_config {
            return false;
        }
        let Some(parent) = path.parent() else { return false };
        self.config_dirs.iter().any(|dir| dir.is(parent))
    }
}

struct HubInner {
    acc: Mutex<Accumulator>,
    /// Signalled when there is new work to drain, or setup has settled.
    wake: Condvar,
    /// Set once the recursive watch is armed; false until the hub thread finishes
    /// setup (or forever, if setup failed).
    watching: AtomicBool,
    /// Set by the notify callback when the bounded channel is full: events were
    /// dropped, so the hub thread must trigger a reconcile. Non-locking, so the
    /// callback never blocks.
    channel_overflow: AtomicBool,
    /// `(canonical path, recursive)` of the currently-armed watch targets,
    /// published by the hub thread at setup and after every re-arm, so a consumer
    /// can compare a project snapshot's targets against the live set without a
    /// control roundtrip.
    watched_roots: Mutex<Vec<(PathBuf, bool)>>,
    /// What the hub takes into work. Set before the thread starts (events may
    /// arrive before setup finishes) and re-derived on every re-arm.
    scope: Mutex<Scope>,
    /// How often the hub re-checks that its declared coverage is still live. A field,
    /// not a constant: a test that waited the production interval would be unusable,
    /// and a globally swappable constant would be shared state between parallel tests.
    tick_period: Duration,
    /// Coverage ticks that ran, and re-arms they caused. Both are needed: a tick that
    /// merely happens proves nothing, and a re-arm that never happens is exactly the
    /// failure this node exists to prevent.
    ticks: AtomicU64,
    rearms: AtomicU64,
    /// The declared set last handed to the thread, so `ensure_roots` can tell a genuine
    /// declaration change from a repeat. Compared by raw spelling AND mode: an alias
    /// swap keeps the canonical path and would otherwise pass unnoticed.
    declared_published: Mutex<Vec<WatchTarget>>,
}

impl HubInner {
    /// Publish the armed targets' `(canonical-at-arm, recursive)` pairs for cheap
    /// comparisons by [`WorkspaceChangeHub::ensure_roots`]. Targets whose
    /// `watch()` failed are not included, so a retry re-arms them.
    fn publish_watched_roots(&self, armed: &[(WatchTarget, PathBuf)]) {
        let pairs: Vec<(PathBuf, bool)> =
            armed.iter().map(|(t, canonical)| (canonical.clone(), t.recursive)).collect();
        *self.watched_roots.lock().unwrap_or_else(PoisonError::into_inner) = pairs;
    }

    fn lock_acc(&self) -> std::sync::MutexGuard<'_, Accumulator> {
        self.acc.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn scope(&self) -> Scope {
        self.scope.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn set_scope(&self, scope: Scope) {
        *self.scope.lock().unwrap_or_else(PoisonError::into_inner) = scope;
    }

    /// Fold one raw watcher result into the accumulator and return the directories
    /// that must be (re-)watched recursively. The caller (which owns the watcher)
    /// applies the re-watch off the notify callback thread, so `notify::watch` is
    /// never re-entered from inside a notify callback. The accumulator mutex is
    /// never held across filesystem I/O: every path is stat'd (and new subtrees
    /// walked) before the lock is taken.
    fn ingest_event(&self, res: Result<Event, notify::Error>) -> Vec<PathBuf> {
        let event = match res {
            Ok(event) => event,
            Err(error) => {
                tracing::warn!("workspace watch event error: {error}");
                self.lock_acc().enter_rescan(false, DegradeReason::RuntimeError);
                self.wake.notify_all();
                return Vec::new();
            }
        };

        {
            let mut acc = self.lock_acc();
            acc.events_seen += 1;
        }

        // The scope filter runs BEFORE the branch on event kind, and per PATH
        // rather than per event. Before, because an unknown kind degrades on the
        // spot, and one foreign file would otherwise drag every consumer into a
        // rescan. Per path, because `Modify(Name(Both))` carries the vanished and
        // the arrived path in one event, and a rename out of a scan root puts them
        // on opposite sides of the boundary. An event with NO paths is left alone:
        // an absent path is not evidence that the lost change was out of scope.
        //
        // A rescan notice is handled before any of that: it does not report a change
        // to the path it names, it reports that the stream lapsed and nothing
        // received so far can be trusted. Scope says nothing about what was lost, so
        // neither the filter nor the kind may swallow it — the flag is an attribute
        // in its own right, and nothing in notify's contract ties it to one kind.
        // Inotify raises it without a path; FSEvents attaches one, commonly the
        // workspace directory, which in a nested layout lies outside every scan root.
        if event.need_rescan() {
            self.lock_acc().enter_rescan(false, DegradeReason::UnknownEvent);
        }

        let scope = self.scope();
        let paths: Vec<PathBuf> =
            event.paths.iter().filter(|path| scope.may_record(path)).cloned().collect();
        if !event.paths.is_empty() && paths.is_empty() {
            self.wake.notify_all();
            return Vec::new();
        }

        let mut rewatch: Vec<PathBuf> = Vec::new();
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                let mut records: Vec<(PathBuf, PathBuf, ChangeKind)> = Vec::new();
                for path in &paths {
                    // A directory that just appeared needs two things bare recursive
                    // watching does not give reliably on Linux: files written into
                    // it before the OS watch arms are lost, and a deep subtree
                    // created in one burst may never be watched. Walking the new
                    // subtree records whatever already exists (stats are truth),
                    // and re-arming a recursive watch covers everything created
                    // afterwards.
                    //
                    // Appearing is not only `Create`: a directory MOVED into the
                    // tree arrives as `Modify(Name(To))`, and the files that rode
                    // along with it fire no events of their own — their path
                    // changed, they did not. `Name` and not any `Modify`, though: a
                    // chmod on a large directory would walk it for nothing.
                    //
                    // Walking is the narrower permission: a config file is in scope
                    // by name, but a DIRECTORY carrying that name is still foreign,
                    // and walking it is the very cost this boundary exists to avoid.
                    let may_have_appeared = matches!(
                        event.kind,
                        EventKind::Create(_)
                            | EventKind::Modify(notify::event::ModifyKind::Name(_))
                    );
                    if may_have_appeared && scope.may_walk(path) {
                        if let Ok(meta) = std::fs::metadata(path) {
                            if meta.is_dir() {
                                rewatch.push(path.clone());
                                collect_subtree(path, &mut records);
                                continue;
                            }
                        }
                    }
                    if let Some((canonical, kind)) = classify_path(path) {
                        records.push((canonical, path.clone(), kind));
                    }
                }
                if !records.is_empty() {
                    let mut acc = self.lock_acc();
                    for (canonical, raw, kind) in records {
                        acc.record(canonical, raw, kind);
                    }
                }
            }
            // Reads/opens/closes are understood and irrelevant to drift; ignore
            // them without degrading. Anything else (`Any`/`Other`) is a kind we
            // do not model, so assume the stream may be incomplete and ask
            // consumers to reconcile — the scan then covers whatever was missed.
            EventKind::Access(_) => {}
            _ => self.lock_acc().enter_rescan(false, DegradeReason::UnknownEvent),
        }

        self.wake.notify_all();
        rewatch
    }

    /// If the notify callback reported a dropped-event overflow, fold it into the
    /// reconcile path once.
    fn drain_channel_overflow(&self) {
        if self.channel_overflow.swap(false, Ordering::Relaxed) {
            self.lock_acc().enter_rescan(true, DegradeReason::Overflow);
            self.wake.notify_all();
        }
    }

    /// A newly-created subtree could not be added to the recursive watch, so it
    /// may miss further changes. Ask consumers to reconcile (recoverable, like a
    /// runtime error) rather than silently going blind. Entries stay: what is
    /// already tracked is still valid; only the un-watched subtree is at risk.
    fn note_rewatch_failed(&self, dir: &Path, error: &notify::Error) {
        tracing::warn!(
            ?dir,
            "change hub could not extend watch to new subtree; drift there may be missed: {error}"
        );
        self.lock_acc().enter_rescan(false, DegradeReason::RewatchFailed);
        self.wake.notify_all();
    }

    /// A target that could not be placed leaves a subtree unwatched under a path
    /// nobody downstream can even name, unlike a root that merely failed to arm —
    /// there the path is known and a later re-arm retries it.
    ///
    /// What this buys is ONE forced reconcile, not standing ill health: `drain`
    /// clears the reason once every cursor has acknowledged that round, while the
    /// dropped target stays dropped. Standing ill health has exactly one carrier
    /// here, `setup_failed`, and it means the hub is unusable — spending it on a
    /// partial drop would cost every consumer the event stream it still has, to
    /// describe a subtree the periodic reconcile already covers. So: nothing
    /// derived before the drop is trusted, and coverage afterwards is the
    /// reconciler's, the same bargain an unarmable root gets.
    fn note_unplaced_targets(&self) {
        self.lock_acc().enter_rescan(false, DegradeReason::WatcherSetup);
        self.wake.notify_all();
    }

    fn mark_setup_failed(&self) {
        self.lock_acc().setup_failed = true;
        self.wake.notify_all();
    }

    fn mark_watching(&self) {
        self.watching.store(true, Ordering::SeqCst);
        // Bump generation under the lock so `wait_until_watching` wakers re-check.
        self.lock_acc().generation += 1;
        self.wake.notify_all();
    }
}

/// The canonical key for a path known to be *missing* (a removal): its own
/// `canonicalize` fails, so canonicalize the parent (usually still present) and
/// re-join the file name. This lets a create (keyed by the file's own
/// canonicalization) and its later remove coalesce onto the same key even under a
/// symlinked root. Falls back to the raw path when the parent is unavailable.
fn canonical_for_missing(path: &Path) -> PathBuf {
    // Edge case: if the parent was ALSO removed in the same burst its canonicalize
    // fails too, so we fall back to the raw path and a create/remove pair may land
    // on different keys. That is not a leak — the stray entry is released by cursor
    // reclamation once every cursor advances past it.
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => {
            parent.canonicalize().map(|p| p.join(name)).unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    }
}

// Thread-local on purpose: tests run in parallel and a process-global counter
// would let one case observe another's walks.
#[cfg(test)]
thread_local! {
    static SUBTREE_WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Walk a freshly-created directory and record every file already inside it,
/// canonicalizing the directory once and joining each file's relative path rather
/// than canonicalizing per file.
fn collect_subtree(dir: &Path, records: &mut Vec<(PathBuf, PathBuf, ChangeKind)>) {
    #[cfg(test)]
    SUBTREE_WALKS.with(|walks| walks.set(walks.get() + 1));
    let base_canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    for entry in WalkDir::new(dir).follow_links(true) {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        let file = entry.path();
        let canonical = match file.strip_prefix(dir) {
            Ok(rel) => base_canonical.join(rel),
            Err(_) => file.canonicalize().unwrap_or_else(|_| file.to_path_buf()),
        };
        records.push((canonical, file.to_path_buf(), ChangeKind::MaybeChanged));
    }
}

/// Re-derive what happened to `path` from its current on-disk state. Returns the
/// canonical key and the change kind, or `None` for events that carry no drift
/// (a bare directory whose children arrive as their own events, a transient stat
/// error that must not be mistaken for a removal).
fn classify_path(path: &Path) -> Option<(PathBuf, ChangeKind)> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => None,
        Ok(meta) if meta.is_file() => {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            Some((canonical, ChangeKind::MaybeChanged))
        }
        Ok(_) => None,
        // Only an actual absence is a removal. Any other stat error (permissions,
        // interruption, a momentary race) must not tombstone a live file.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let canonical = canonical_for_missing(path);
            if path.extension().is_none() {
                Some((canonical, ChangeKind::SubtreeRemoved))
            } else {
                Some((canonical, ChangeKind::MaybeRemoved))
            }
        }
        Err(_) => None,
    }
}

/// Messages the hub thread processes. Control travels the SAME channel as watcher
/// events, so a re-arm is ordered relative to the event stream and executed by the
/// one thread that owns the watcher — no cross-thread watcher mutation, no second
/// hub identity for the (many, clonable) handle holders to migrate to.
enum HubMsg {
    Event(Result<Event, notify::Error>),
    /// Re-point the watch set (see [`WorkspaceChangeHub::rearm`]). `ack` fires
    /// once the new set is applied and every cursor is flagged to rescan; it
    /// carries whether EVERY desired target is actually armed (partial coverage
    /// must surface to the caller, not read as success).
    Rearm {
        targets: Vec<WatchTarget>,
        ack: std::sync::mpsc::SyncSender<bool>,
    },
    /// A new declared set that the caller found coverage-equivalent. The thread still
    /// decides for itself whether to re-arm (see [`apply_declaration`]).
    Declare(Vec<WatchTarget>),
    /// Run one coverage tick now. A test seam: production drives ticks by the
    /// deadline, and both paths call the same function so a broken periodic path
    /// cannot hide behind a working commanded one.
    #[cfg(test)]
    Tick,
    /// Exit the hub thread. Cursors keep draining the frozen stream.
    #[allow(
        dead_code,
        reason = "constructed only by the test-facing shutdown seam; production daemons exit the process"
    )]
    Shutdown,
}

/// Daemon-owned hub over one recursive workspace watcher. Cheap to clone
/// (`Arc`-backed); every clone observes the same accumulator and health.
#[derive(Clone)]
pub(crate) struct WorkspaceChangeHub {
    inner: Arc<HubInner>,
    /// Producer side of the hub thread's channel, for control messages. The
    /// watcher callback holds its own clone for events.
    control: std::sync::mpsc::SyncSender<HubMsg>,
    /// The hub thread's handle, joined once by [`Self::shutdown`].
    #[allow(
        dead_code,
        reason = "read only by the test-facing shutdown seam; production daemons exit the process"
    )]
    thread: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl WorkspaceChangeHub {
    /// Spawn the hub over one or more roots (the drift-scan universe: the config source
    /// root plus each extension root). Returns immediately — each root is watched
    /// recursively on the hub thread (walking large trees must not block daemon startup).
    /// Nested roots are de-duplicated so a subtree is not double-watched. Use
    /// [`Self::wait_until_watching`] / [`Self::is_watching`] to observe setup completion;
    /// [`Self::health`] reports `Degraded(WatcherSetup)` if no root could be watched.
    /// Test constructor: every root recursive. Production spawns via
    /// [`Self::start_targets`] so the workspace root rides along non-recursively.
    #[cfg(test)]
    pub(crate) fn start(roots: Vec<PathBuf>) -> Self {
        Self::start_targets(roots.into_iter().map(WatchTarget::recursive).collect())
    }

    /// [`Self::start`] with explicit per-target modes (see [`watch_targets_for`]).
    pub(crate) fn start_targets(targets: Vec<WatchTarget>) -> Self {
        Self::start_with_capacity(targets, DEFAULT_CAPACITY, COVERAGE_TICK_PERIOD)
    }

    /// [`Self::start_targets`] with a tick interval a test can actually wait for.
    #[cfg(test)]
    pub(crate) fn start_targets_with_period(targets: Vec<WatchTarget>, period: Duration) -> Self {
        Self::start_with_capacity(targets, DEFAULT_CAPACITY, period)
    }

    fn start_with_capacity(targets: Vec<WatchTarget>, cap: usize, tick_period: Duration) -> Self {
        let inner = Arc::new(HubInner {
            acc: Mutex::new(Accumulator::new(cap)),
            wake: Condvar::new(),
            watching: AtomicBool::new(false),
            channel_overflow: AtomicBool::new(false),
            watched_roots: Mutex::new(Vec::new()),
            // A starting value only; the hub thread re-derives it right before
            // arming, so the relative spellings are resolved against the same
            // current directory the backend will use.
            scope: Mutex::new(Scope::from_targets(&ResolvedTargets::here(targets.clone()))),
            tick_period,
            ticks: AtomicU64::new(0),
            rearms: AtomicU64::new(0),
            declared_published: Mutex::new(targets.clone()),
        });
        let (tx, rx) = std::sync::mpsc::sync_channel::<HubMsg>(CHANNEL_BOUND);

        let thread_inner = Arc::clone(&inner);
        let event_tx = tx.clone();
        let thread = std::thread::Builder::new()
            .name("bsl-workspace-change-hub".to_owned())
            .spawn(move || run_hub_thread(thread_inner, targets, event_tx, rx))
            .ok();

        Self { inner, control: tx, thread: Arc::new(Mutex::new(thread)) }
    }

    /// Ask the hub thread to re-point the watch set at `targets`, blocking until it
    /// acknowledges or `timeout` elapses. The hub identity is stable across a
    /// re-arm: cursors, health and all clonable handles keep working — only the
    /// covered subtrees change. `timeout` bounds the WHOLE call: the enqueue onto
    /// a possibly-full channel and the wait for the acknowledgement share one
    /// deadline. Returns whether every desired target is actually armed; `false`
    /// for a timeout, a dead hub thread, or partial coverage (an unwatchable
    /// target) — the caller must not treat any of those as covered.
    pub(crate) fn rearm(&self, targets: Vec<WatchTarget>, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(1);
        let mut msg = HubMsg::Rearm { targets, ack: ack_tx };
        loop {
            match self.control.try_send(msg) {
                Ok(()) => break,
                Err(std::sync::mpsc::TrySendError::Full(back)) => {
                    if Instant::now() >= deadline {
                        return false;
                    }
                    msg = back;
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return false,
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        ack_rx.recv_timeout(remaining).unwrap_or(false)
    }

    /// Re-arm onto `targets` only when they differ from the live watch set
    /// (canonical + mode comparison), so a rebuild whose topology did not move
    /// never costs consumers a rescan round. Returns whether the hub covers
    /// `targets` — `false` when a needed re-arm was not acknowledged or left a
    /// target unarmed.
    pub(crate) fn ensure_roots(&self, targets: &[WatchTarget]) -> bool {
        // Resolve before comparing, exactly as the hub thread will: the live set was
        // published from placed targets, so comparing raw spellings against it would
        // measure two different languages.
        let resolved = ResolvedTargets::here(targets.to_vec());
        let placed = resolved.is_complete();
        let mut desired: Vec<(PathBuf, bool)> = dedup_targets(resolved.into_inner())
            .into_iter()
            .map(|(t, canonical)| (canonical, t.recursive))
            .collect();
        desired.sort();
        let mut current =
            self.inner.watched_roots.lock().unwrap_or_else(PoisonError::into_inner).clone();
        current.sort();
        if placed && current == desired {
            // Coverage is unchanged, but the DECLARATION may not be: a target absorbed
            // by a recursive ancestor never reaches `watched_roots`, so its removal is
            // invisible here, and an alias swap keeps the canonical path while changing
            // the only spelling the watch actually holds. Telling the thread costs a
            // message; not telling it leaves the tick re-arming targets the topology
            // dropped long ago.
            self.publish_declaration(targets);
            return true;
        }
        tracing::info!(?targets, "workspace change hub re-arming onto new scan roots");
        self.note_declaration(targets);
        self.rearm(targets.to_vec(), REARM_ACK_TIMEOUT)
    }

    /// Record the declaration the thread is about to receive, so a repeat of the same
    /// set costs nothing.
    fn note_declaration(&self, targets: &[WatchTarget]) {
        *self.inner.declared_published.lock().unwrap_or_else(PoisonError::into_inner) =
            targets.to_vec();
    }

    /// Hand a coverage-equivalent declaration to the thread, unless it already has it.
    fn publish_declaration(&self, targets: &[WatchTarget]) {
        {
            let mut published =
                self.inner.declared_published.lock().unwrap_or_else(PoisonError::into_inner);
            if *published == targets {
                return;
            }
            *published = targets.to_vec();
        }
        // `try_send`: a full control channel means the thread is busy with work that
        // will re-derive coverage anyway, and blocking a build thread here would be
        // worse than a late declaration.
        let _ = self.control.try_send(HubMsg::Declare(targets.to_vec()));
    }

    /// Terminate the hub thread and join it. Cursors keep draining whatever was
    /// accumulated; no further events arrive. Idempotent, and bounded: if the
    /// control channel stays full past the enqueue deadline the hub is left
    /// running (never a hang). A test seam today: production daemons exit the
    /// process, but tests must be able to prove the thread terminates instead of
    /// leaking a watcher per case.
    #[allow(dead_code, reason = "exercised by tests; no production teardown path needs it yet")]
    pub(crate) fn shutdown(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut msg = HubMsg::Shutdown;
        let sent = loop {
            match self.control.try_send(msg) {
                Ok(()) => break true,
                Err(std::sync::mpsc::TrySendError::Full(back)) => {
                    if Instant::now() >= deadline {
                        break false;
                    }
                    msg = back;
                    std::thread::sleep(Duration::from_millis(10));
                }
                // Already gone: joining below is safe and immediate.
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break true,
            }
        };
        if !sent {
            tracing::warn!("change hub shutdown could not be enqueued; leaving the thread running");
            return;
        }
        let handle = self.thread.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    /// Register a cursor positioned at "everything up to now already seen": a fresh
    /// subscriber only receives changes that land after it subscribes (plus a
    /// pending reconcile flag if it subscribes during an open rescan window).
    pub(crate) fn subscribe(&self) -> SinkCursor {
        SinkCursor { id: self.inner.lock_acc().subscribe() }
    }

    /// Drop a cursor and reclaim any entries it was the last to hold back.
    #[allow(dead_code)]
    pub(crate) fn unsubscribe(&self, cursor: SinkCursor) {
        self.inner.lock_acc().unsubscribe(cursor.id);
    }

    /// Return the changes newer than `cursor`'s last position and advance it.
    /// Cursors are independent: draining one never affects another's view.
    pub(crate) fn drain(&self, cursor: SinkCursor) -> DrainBatch {
        self.inner.lock_acc().drain(cursor.id)
    }

    pub(crate) fn health(&self) -> Health {
        self.inner.lock_acc().health()
    }

    pub(crate) fn events_seen(&self) -> u64 {
        self.inner.lock_acc().events_seen
    }

    /// Ask every live cursor to reconcile: used by a consumer whose periodic scan
    /// found drift the event stream never delivered (a lossy backend). Recoverable
    /// like any other transient miss — health clears once all cursors acknowledge.
    /// Entries are kept; the caller applies the drift it already found.
    pub(crate) fn degrade_external(&self) {
        self.inner.lock_acc().enter_rescan(false, DegradeReason::ReconcileMiss);
        self.inner.wake.notify_all();
    }

    /// Whether the watch is armed. False means setup is still in flight or failed.
    /// Sinks gate on [`Self::wait_until_watching`] instead; this is the
    /// point-in-time form for status reporting.
    #[allow(dead_code)]
    pub(crate) fn is_watching(&self) -> bool {
        self.inner.watching.load(Ordering::SeqCst)
    }

    /// Block until setup settles (watch armed or failed) or `timeout` elapses.
    /// Returns whether the watch is armed. Sinks call this instead of a bare
    /// `is_watching` check so they do not race the asynchronous setup.
    pub(crate) fn wait_until_watching(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut acc = self.inner.lock_acc();
        loop {
            if self.inner.watching.load(Ordering::SeqCst) {
                return true;
            }
            if acc.setup_failed {
                return false;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return self.inner.watching.load(Ordering::SeqCst);
            }
            let (guard, _) = self
                .inner
                .wake
                .wait_timeout(acc, remaining)
                .unwrap_or_else(|poison| poison.into_inner());
            acc = guard;
        }
    }

    /// Block until the accumulator advances past `since` or `timeout` elapses,
    /// then return the current generation. Sink threads pass the generation they
    /// last observed to sleep until there is new work.
    pub(crate) fn wait_for_change(&self, since: u64, timeout: Duration) -> u64 {
        let acc = self.inner.lock_acc();
        if acc.generation > since {
            return acc.generation;
        }
        let (acc, _) =
            self.inner.wake.wait_timeout(acc, timeout).unwrap_or_else(|poison| poison.into_inner());
        acc.generation
    }

    #[cfg(test)]
    fn ingest_for_test(&self, res: Result<Event, notify::Error>) {
        // Re-watch requests are the caller's job; tests that need real subtree
        // watching drive the live watcher through `start` instead.
        let _rewatch = self.inner.ingest_event(res);
    }

    /// Number of registered cursors. Used by tests to wait deterministically for a
    /// sink to subscribe instead of sleeping a guessed interval.
    #[cfg(test)]
    pub(crate) fn active_cursor_count(&self) -> usize {
        self.inner.lock_acc().cursors.len()
    }

    /// Coverage ticks that ran on this hub.
    #[cfg(test)]
    pub(crate) fn tick_count(&self) -> u64 {
        self.inner.ticks.load(Ordering::Relaxed)
    }

    /// Re-arms this hub decided on its own — by tick or by declaration, never by an
    /// explicit `rearm` from a caller.
    #[cfg(test)]
    pub(crate) fn self_rearm_count(&self) -> u64 {
        self.inner.rearms.load(Ordering::Relaxed)
    }

    /// Reconcile REQUESTS, including those a consumer never distinguishes: `drain`
    /// closes the idempotence window, so repeats cost a full walk each.
    #[cfg(test)]
    pub(crate) fn rescan_request_count(&self) -> u64 {
        self.inner.lock_acc().rescans_requested
    }

    /// Run one coverage tick and wait for it to finish, without waiting out the
    /// period. Returns false if the hub thread is gone.
    #[cfg(test)]
    pub(crate) fn tick_now(&self, timeout: Duration) -> bool {
        let before = self.tick_count();
        if self.control.send(HubMsg::Tick).is_err() {
            return false;
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.tick_count() > before {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// Drive the exact transition the re-watch error branch takes. A real
    /// `watcher.watch` failure cannot be injected without a mock backend, so tests
    /// exercise the state transition directly.
    #[cfg(test)]
    fn trigger_rewatch_failure_for_test(&self) {
        self.inner.note_rewatch_failed(Path::new("/unwatchable"), &notify::Error::generic("test"));
    }
}

/// One watch target: a directory watched recursively (a scan root) or
/// non-recursively (the workspace root, for the analyzer config files that live
/// directly in it). Watching the DIRECTORY — never the config files themselves —
/// is load-bearing twice over: an editor's atomic save replaces the file's inode
/// (killing a file watch while the canonical set looks unchanged), and a config
/// file that does not exist yet cannot be watched at all, so its creation would
/// go unseen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WatchTarget {
    pub(crate) path: PathBuf,
    pub(crate) recursive: bool,
}

impl WatchTarget {
    pub(crate) fn recursive(path: PathBuf) -> Self {
        Self { path, recursive: true }
    }

    fn mode(&self) -> RecursiveMode {
        if self.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        }
    }
}

/// The full watch-target set for a workspace: the drift-scan roots (recursive)
/// plus the workspace root itself, non-recursively, so config-file
/// creation/edit/atomic-replace is event-delivered even in a nested layout
/// where the workspace root is NOT a scan root. A non-recursive root already
/// covered by a recursive scan root is deduplicated at arm time.
pub(crate) fn watch_targets_for(workspace_root: &Path, scan_roots: &[PathBuf]) -> Vec<WatchTarget> {
    let mut targets: Vec<WatchTarget> =
        scan_roots.iter().cloned().map(WatchTarget::recursive).collect();
    targets.push(WatchTarget { path: workspace_root.to_path_buf(), recursive: false });
    targets
}

/// What one `stat` of a watch target says about it, at three levels rather than two.
///
/// `Absent` is a PROVEN absence and nothing weaker: the same allow-list the workspace
/// walker uses (`project-model/src/workspace_walk.rs`), because a denied or interrupted
/// call describes a path that is still there, and calling it gone would re-arm the whole
/// tree twice — once on the failure, once on the recovery.
///
/// `Unknown` is that weaker case. It equals itself, so a persistent failure never reads
/// as movement, and it carries no canonical path, so such a target stays out of the
/// cover until a `stat` succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Fingerprint {
    Absent,
    Unknown,
    Present { canonical: PathBuf, created: Option<SystemTime> },
}

/// A link cycle belongs in `Absent` by the same argument as `NotADirectory`, but it
/// cannot be named: `ErrorKind::FilesystemLoop` is unstable on the toolchain this crate
/// builds with, and matching a raw errno would differ per platform. It therefore lands
/// in `Unknown`, which errs toward keeping the watch rather than dropping a live tree.
fn target_cannot_exist(kind: std::io::ErrorKind) -> bool {
    matches!(kind, std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory)
}

/// Read one target's fingerprint. `previous` is consulted only for the `Unknown` case:
/// a target already described keeps its description, so a transient failure costs
/// nothing, while a target seen for the first time records `Unknown` as itself.
fn fingerprint_of(path: &Path, previous: Option<&Fingerprint>) -> Fingerprint {
    match std::fs::metadata(path) {
        Ok(meta) => match path.canonicalize() {
            Ok(canonical) => Fingerprint::Present { canonical, created: meta.created().ok() },
            Err(error) if target_cannot_exist(error.kind()) => Fingerprint::Absent,
            Err(_) => previous.cloned().unwrap_or(Fingerprint::Unknown),
        },
        Err(error) if target_cannot_exist(error.kind()) => Fingerprint::Absent,
        Err(_) => previous.cloned().unwrap_or(Fingerprint::Unknown),
    }
}

/// Fingerprints of every DECLARED target, keyed by its declared spelling.
type Snapshot = HashMap<PathBuf, Fingerprint>;

/// Take a fresh snapshot, carrying each target's previous description into a failed
/// `stat` (see [`Fingerprint::Unknown`]). Targets that left the declared set are dropped;
/// new ones are described as they are now.
fn snapshot_of(declared: &[WatchTarget], previous: &Snapshot) -> Snapshot {
    declared
        .iter()
        .map(|target| {
            let fingerprint = fingerprint_of(&target.path, previous.get(&target.path));
            (target.path.clone(), fingerprint)
        })
        .collect()
}

/// The minimal cover derived FROM A SNAPSHOT — never by canonicalizing again.
///
/// [`dedup_targets`] re-reads the filesystem and collapses every error to the raw path,
/// so a denied `stat` on a symlink's parent would swap that target's canonical path for
/// its declared one and read as a composition change: a full re-arm every tick for as
/// long as the failure lasts. The snapshot already holds the canonical paths, and a
/// target without one (absent or unknown) is simply not part of what can be watched.
fn cover_from_snapshot(
    declared: &[WatchTarget],
    snapshot: &Snapshot,
) -> Vec<(WatchTarget, PathBuf)> {
    let mut pairs: Vec<(PathBuf, WatchTarget)> = declared
        .iter()
        .filter_map(|target| match snapshot.get(&target.path) {
            Some(Fingerprint::Present { canonical, .. }) => {
                Some((canonical.clone(), target.clone()))
            }
            _ => None,
        })
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.recursive.cmp(&a.1.recursive)));
    pairs.dedup_by(|a, b| a.0 == b.0);

    let mut kept: Vec<(WatchTarget, PathBuf)> = Vec::new();
    for (canonical, target) in pairs {
        if kept.iter().any(|(k, kc)| k.recursive && canonical.starts_with(kc)) {
            continue;
        }
        kept.push((target, canonical));
    }
    kept
}

/// Has the watched world moved since `previous` was taken?
///
/// Two things count, and nothing else. The cover's MEMBERSHIP — compared by declared
/// spelling too, because collapsing canonical duplicates keeps one target and the winner
/// may swap from one alias to another with every fingerprint equal, leaving the watch on
/// a spelling the scope no longer knows. And a fingerprint change on a target that is IN
/// the cover: a target absorbed by a recursive ancestor does not reach `apply_rearm` at
/// all, so paying a full walk for its re-creation would buy nothing — notify re-arms
/// subdirectories of a recursive watch by itself.
fn coverage_moved(declared: &[WatchTarget], previous: &Snapshot, current: &Snapshot) -> bool {
    let was = cover_from_snapshot(declared, previous);
    let now = cover_from_snapshot(declared, current);
    let key = |cover: &[(WatchTarget, PathBuf)]| -> Vec<(PathBuf, bool, PathBuf)> {
        let mut keys: Vec<(PathBuf, bool, PathBuf)> = cover
            .iter()
            .map(|(t, canonical)| (t.path.clone(), t.recursive, canonical.clone()))
            .collect();
        keys.sort();
        keys
    };
    if key(&was) != key(&now) {
        return true;
    }
    now.iter().any(|(target, _)| previous.get(&target.path) != current.get(&target.path))
}

/// Reduce a set of watch targets to the minimal cover: drop any target nested under
/// a RECURSIVE target (a non-recursive ancestor covers only its direct children, so
/// it absorbs nothing), and collapse exact duplicates — a recursive duplicate wins
/// over a non-recursive one. Comparison is by canonical path; the RAW path is what
/// gets watched, so event paths keep the spelling consumers strip against (the
/// search sink strips the non-canonical source root). Returns each kept target with
/// the canonical path used for the decision.
/// How often the hub re-checks that its declared coverage is still live.
///
/// A symlinked root retargeted in place emits no event at all, so nothing but this
/// interval bounds how long a daemon can watch a tree nobody declared any more. Thirty
/// seconds sits alongside the consumers' own reconcile cadence, and the check itself is
/// a handful of `stat` calls over a handful of targets.
const COVERAGE_TICK_PERIOD: Duration = Duration::from_secs(30);

fn dedup_targets(targets: Vec<WatchTarget>) -> Vec<(WatchTarget, PathBuf)> {
    let mut pairs: Vec<(PathBuf, WatchTarget)> = targets
        .into_iter()
        .map(|t| (t.path.canonicalize().unwrap_or_else(|_| t.path.clone()), t))
        .collect();
    // Parents sort before descendants; among equal canonicals the recursive one first.
    pairs.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.recursive.cmp(&a.1.recursive)));
    pairs.dedup_by(|a, b| a.0 == b.0);

    let mut kept: Vec<(WatchTarget, PathBuf)> = Vec::new();
    for (canonical, target) in pairs {
        if kept.iter().any(|(k, kc)| k.recursive && canonical.starts_with(kc)) {
            continue;
        }
        kept.push((target, canonical));
    }
    kept
}

/// How long a re-arm caller waits for the hub thread's acknowledgement before
/// reporting failure. The thread only pumps events, so this is generous.
const REARM_ACK_TIMEOUT: Duration = Duration::from_secs(30);

/// Arm the watch over every target and pump events (and control messages) until
/// shutdown. Runs on its own thread so `start` returns without blocking on the
/// initial (potentially huge) directory walks.
fn run_hub_thread(
    inner: Arc<HubInner>,
    targets: Vec<WatchTarget>,
    event_tx: std::sync::mpsc::SyncSender<HubMsg>,
    rx: std::sync::mpsc::Receiver<HubMsg>,
) {
    let callback_inner = Arc::clone(&inner);
    let watcher = RecommendedWatcher::new(
        move |res| {
            // Never block the notify thread: drop-and-flag on a full channel and
            // let the hub thread fold that into a reconcile.
            if event_tx.try_send(HubMsg::Event(res)).is_err() {
                callback_inner.channel_overflow.store(true, Ordering::Relaxed);
            }
        },
        NotifyConfig::default(),
    );

    let mut watcher = match watcher {
        Ok(watcher) => watcher,
        Err(error) => {
            tracing::warn!("workspace change hub failed to create watcher: {error}");
            inner.mark_setup_failed();
            return;
        }
    };

    // `armed` holds each successfully-watched target with its canonical path
    // CAPTURED AT WATCH TIME — later comparisons must never re-canonicalize a raw
    // spelling (a retargeted symlink would then claim coverage it lost). A target
    // whose `watch()` failed is deliberately NOT recorded, so a later re-arm onto
    // the same set retries it instead of assuming coverage.
    // Re-derive the scope here, on the thread that is about to arm the watch, from
    // targets whose relative spellings are already placed: the backend receives an
    // absolute path and never reads the process-wide current directory again, so
    // the scope cannot end up describing a different tree than the one armed.
    let mut armed: Vec<(WatchTarget, PathBuf)> = Vec::new();
    let targets = ResolvedTargets::here(targets);
    if !targets.is_complete() {
        inner.note_unplaced_targets();
    }
    inner.set_scope(Scope::from_targets(&targets));
    // The declared set, kept for the life of the thread: `dedup_targets` below drops
    // whatever a recursive ancestor absorbs or a canonical duplicate collapses, and
    // either can become a target in its own right when a link is retargeted. A set
    // rebuilt from the survivors could never bring those back.
    let mut declared = targets.as_slice().to_vec();
    // Taken BEFORE anything is armed. A snapshot taken afterwards would describe the
    // tree the watcher ended up on, so a retarget racing the arming pass would read as
    // agreement forever; taken before, the same race costs one extra re-arm.
    let mut snapshot = snapshot_of(&declared, &Snapshot::new());
    for (target, canonical) in dedup_targets(targets.into_inner()) {
        match watcher.watch(&target.path, target.mode()) {
            Ok(()) => {
                tracing::info!(root = ?target.path, recursive = target.recursive, "workspace change hub watching root");
                armed.push((target, canonical));
            }
            // A single unwatchable root (a missing extension dir, an inotify-limit) leaves
            // that subtree to the reconciler rather than failing the whole hub.
            Err(error) => {
                tracing::warn!(root = ?target.path, "workspace change hub failed to watch root: {error}")
            }
        }
    }
    if armed.is_empty() {
        inner.mark_setup_failed();
        return;
    }
    inner.publish_watched_roots(&armed);
    inner.mark_watching();

    // The deadline is checked BEFORE reading a message, not derived from a receive
    // timeout: `recv_timeout` hands over whatever is already queued regardless of how
    // long the deadline has been past, so a storm of events would starve the tick
    // indefinitely — on exactly the tree where losing coverage costs the most.
    let mut due = Instant::now() + inner.tick_period;
    loop {
        inner.drain_channel_overflow();
        let now = Instant::now();
        if now >= due {
            coverage_tick(&inner, &mut watcher, &mut armed, &declared, &mut snapshot);
            due = Instant::now() + inner.tick_period;
            continue;
        }
        let msg = match rx.recv_timeout(due - now) {
            Ok(msg) => msg,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        };
        match msg {
            HubMsg::Event(res) => {
                for dir in inner.ingest_event(res) {
                    if let Err(error) = watcher.watch(&dir, RecursiveMode::Recursive) {
                        inner.note_rewatch_failed(&dir, &error);
                    }
                }
            }
            HubMsg::Rearm { targets, ack } => {
                declared = ResolvedTargets::here(targets.clone()).into_inner();
                snapshot = snapshot_of(&declared, &snapshot);
                let full_coverage = apply_rearm(&inner, &mut watcher, &mut armed, targets);
                // `try_send`, not `send`: the requester may have timed out and
                // dropped its receiver; the hub thread must never block on it.
                let _ = ack.try_send(full_coverage);
            }
            HubMsg::Declare(targets) => {
                apply_declaration(
                    &inner,
                    &mut watcher,
                    &mut armed,
                    &mut declared,
                    &mut snapshot,
                    targets,
                );
            }
            #[cfg(test)]
            HubMsg::Tick => {
                coverage_tick(&inner, &mut watcher, &mut armed, &declared, &mut snapshot);
                due = Instant::now() + inner.tick_period;
            }
            HubMsg::Shutdown => return,
        }
    }
}

/// Re-check that the declared coverage is still the coverage in force, and re-arm the
/// whole set when it is not.
///
/// The tick decides, `apply_rearm` acts. Splitting it the other way — unwatching and
/// watching target by target — was tried and abandoned: a recursive `unwatch` takes
/// descendant registrations with it, and a nested target's removal punches a hole in a
/// kept ancestor, so any per-target sequence has to rebuild the guarantees
/// `apply_rearm` already provides by arming additions before removals and defensively
/// re-watching everything it keeps.
fn coverage_tick(
    inner: &Arc<HubInner>,
    watcher: &mut RecommendedWatcher,
    armed: &mut Vec<(WatchTarget, PathBuf)>,
    declared: &[WatchTarget],
    snapshot: &mut Snapshot,
) {
    inner.ticks.fetch_add(1, Ordering::Relaxed);
    let current = snapshot_of(declared, snapshot);
    if !coverage_moved(declared, snapshot, &current) {
        return;
    }
    *snapshot = current;
    inner.rearms.fetch_add(1, Ordering::Relaxed);
    apply_rearm(inner, watcher, armed, declared.to_vec());
}

/// Take a new declared set that the caller believes needs no re-arming.
///
/// The belief is not trusted: the caller compared coverage on ITS thread, and a target
/// absorbed by an ancestor at that moment can be standing on its own by the time this
/// runs. The snapshot is therefore taken FIRST — before the cover is recomputed — so a
/// retarget inside that window reads as movement instead of being recorded as the
/// starting state, and the decision to arm uses the same rule the tick uses.
fn apply_declaration(
    inner: &Arc<HubInner>,
    watcher: &mut RecommendedWatcher,
    armed: &mut Vec<(WatchTarget, PathBuf)>,
    declared: &mut Vec<WatchTarget>,
    snapshot: &mut Snapshot,
    targets: Vec<WatchTarget>,
) {
    let resolved = ResolvedTargets::here(targets);
    let next = resolved.as_slice().to_vec();
    // Merged, not replaced: a drift that already happened to a surviving target must
    // survive this update, and a target seen for the first time gets described now.
    let current = snapshot_of(&next, snapshot);
    let moved =
        coverage_moved(declared, snapshot, &current) || coverage_moved(&next, snapshot, &current);
    *snapshot = current;
    *declared = next;
    inner.set_scope(Scope::from_targets(&resolved));
    if moved {
        inner.rearms.fetch_add(1, Ordering::Relaxed);
        apply_rearm(inner, watcher, armed, declared.clone());
    }
}

/// Re-point the watch set at `new_targets`, on the hub thread. Additions are armed
/// BEFORE obsolete targets are unwatched, so a subtree present in both sets has no
/// unwatched window; every surviving target is then defensively re-armed, because a
/// recursive `unwatch` of an overlapping old root can deregister a kept target's
/// descendants on inotify. Comparison uses each armed target's canonical path
/// CAPTURED AT WATCH TIME — a symlink retargeted since then must read as "not
/// covered" and be re-armed, not silently claimed. Every cursor is then flagged to
/// rescan once: anything a consumer derived under the old set predates the new
/// targets' coverage, and events inside a newly-added root from before its arm were
/// never delivered. Returns whether EVERY desired target is armed afterwards.
fn apply_rearm(
    inner: &HubInner,
    watcher: &mut RecommendedWatcher,
    armed: &mut Vec<(WatchTarget, PathBuf)>,
    new_targets: Vec<WatchTarget>,
) -> bool {
    // Scope follows the DESIRED set, before de-duplication: a target absorbed by a
    // recursive ancestor is still part of what the hub watches for.
    let new_targets = ResolvedTargets::here(new_targets);
    // A target that could not be placed is absent from the desired set, so the
    // arming loop below has nothing to fail on: coverage has to be denied here or
    // the caller would read a silent drop as success.
    let mut full_coverage = new_targets.is_complete();
    if !full_coverage {
        inner.note_unplaced_targets();
    }
    inner.set_scope(Scope::from_targets(&new_targets));
    let desired = dedup_targets(new_targets.into_inner());
    let is_armed = |list: &[(WatchTarget, PathBuf)], t: &WatchTarget, c: &PathBuf| {
        list.iter().any(|(at, ac)| at.recursive == t.recursive && ac == c)
    };

    let mut next_armed: Vec<(WatchTarget, PathBuf)> = Vec::new();
    for (target, canonical) in &desired {
        if is_armed(armed, target, canonical) {
            next_armed.push((target.clone(), canonical.clone()));
            continue;
        }
        match watcher.watch(&target.path, target.mode()) {
            Ok(()) => {
                tracing::info!(root = ?target.path, recursive = target.recursive, "workspace change hub watching root (re-arm)");
                next_armed.push((target.clone(), canonical.clone()));
            }
            Err(error) => {
                tracing::warn!(root = ?target.path, "workspace change hub failed to watch new root: {error}");
                inner.note_rewatch_failed(&target.path, &error);
                full_coverage = false;
            }
        }
    }
    for (target, canonical) in armed.iter() {
        if !is_armed(&next_armed, target, canonical) {
            if let Err(error) = watcher.unwatch(&target.path) {
                tracing::debug!(root = ?target.path, "workspace change hub unwatch on re-arm: {error}");
            }
        }
    }
    // Defensive re-arm of every kept target: on inotify a recursive unwatch of an
    // overlapping obsolete root strips descendant registrations, including a kept
    // target's. Re-watching an already-watched path is idempotent.
    for (target, _) in &next_armed {
        if let Err(error) = watcher.watch(&target.path, target.mode()) {
            inner.note_rewatch_failed(&target.path, &error);
            full_coverage = false;
        }
    }
    *armed = next_armed;
    inner.publish_watched_roots(armed);

    let mut acc = inner.lock_acc();
    acc.enter_rescan(false, DegradeReason::Rearmed);
    drop(acc);
    inner.wake.notify_all();
    full_coverage
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, EventKind, ModifyKind, RemoveKind};
    use tempfile::tempdir;

    fn change_event(kind: EventKind, path: PathBuf) -> Result<Event, notify::Error> {
        Ok(Event { kind, paths: vec![path], attrs: Default::default() })
    }

    fn event_with_paths(kind: EventKind, paths: Vec<PathBuf>) -> Result<Event, notify::Error> {
        Ok(Event { kind, paths, attrs: Default::default() })
    }

    /// A nested project: the workspace holds the config files, the scan root sits
    /// one level down. This is the layout the scope boundary exists for — a flat
    /// project, where the workspace IS the scan root, cannot show the difference.
    struct NestedProject {
        _dir: tempfile::TempDir,
        workspace: PathBuf,
        scan_root: PathBuf,
    }

    fn nested_project() -> NestedProject {
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let scan_root = workspace.join("src");
        std::fs::create_dir_all(&scan_root).unwrap();
        NestedProject { _dir: dir, workspace, scan_root }
    }

    impl NestedProject {
        fn hub(&self) -> WorkspaceChangeHub {
            let hub = WorkspaceChangeHub::start_targets(watch_targets_for(
                &self.workspace,
                std::slice::from_ref(&self.scan_root),
            ));
            assert!(hub.wait_until_watching(Duration::from_secs(5)));
            hub
        }

        /// A ready-made directory with one `.bsl` inside, built OUTSIDE the watched
        /// tree so a later rename into it carries content that never fired an event.
        fn staged_dir(&self, name: &str) -> PathBuf {
            let staged = self.workspace.join(format!(".staging-{name}"));
            std::fs::create_dir_all(&staged).unwrap();
            std::fs::write(staged.join("Module.bsl"), "Процедура П() КонецПроцедуры").unwrap();
            staged
        }
    }

    fn subtree_walks() -> usize {
        SUBTREE_WALKS.with(|walks| walks.get())
    }

    fn entry_names(batch: &DrainBatch) -> Vec<String> {
        batch.entries.iter().map(|e| e.raw.to_string_lossy().into_owned()).collect()
    }

    /// A workspace whose scan root is reached through a symlink, so the root can be
    /// retargeted in place — the move that produces no filesystem event at all.
    struct LinkedRoot {
        _dir: tempfile::TempDir,
        workspace: PathBuf,
        link: PathBuf,
        first: PathBuf,
        second: PathBuf,
    }

    #[cfg(unix)]
    fn linked_root() -> LinkedRoot {
        let dir = tempdir().unwrap();
        let workspace = dir.path().canonicalize().unwrap();
        let first = workspace.join("first");
        let second = workspace.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let link = workspace.join("root");
        std::os::unix::fs::symlink(&first, &link).unwrap();
        LinkedRoot { _dir: dir, workspace, link, first, second }
    }

    #[cfg(unix)]
    impl LinkedRoot {
        fn hub(&self, period: Duration) -> WorkspaceChangeHub {
            let hub = WorkspaceChangeHub::start_targets_with_period(
                watch_targets_for(&self.workspace, std::slice::from_ref(&self.link)),
                period,
            );
            assert!(hub.wait_until_watching(Duration::from_secs(5)), "the hub must arm");
            hub
        }

        fn retarget(&self, to: &Path) {
            std::fs::remove_file(&self.link).unwrap();
            std::os::unix::fs::symlink(to, &self.link).unwrap();
        }
    }

    /// Wait until `f` holds, polling; returns whether it ever did.
    fn eventually(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        f()
    }

    /// The move this whole node exists for, and the one nothing else can catch: a
    /// symlinked scan root retargeted in place emits NO event, so an idle hub has
    /// nothing to react to. Only the periodic check notices, and a hub that ran its
    /// detector solely on the arrival of some other message would stay pointed at a
    /// tree nobody declared any more — for as long as the daemon lives.
    #[cfg(unix)]
    #[test]
    fn an_idle_hub_notices_a_root_retargeted_in_place() {
        let project = linked_root();
        let hub = project.hub(Duration::from_millis(50));
        let cursor = hub.subscribe();
        let _ = hub.drain(cursor);

        project.retarget(&project.second);
        assert!(
            eventually(Duration::from_secs(10), || hub.self_rearm_count() > 0),
            "an idle hub must notice a retarget nothing reports"
        );

        std::fs::write(project.second.join("Module.bsl"), "x").unwrap();
        assert!(
            eventually(Duration::from_secs(10), || { !entry_names(&hub.drain(cursor)).is_empty() }),
            "the new target must be delivered once coverage follows it"
        );
    }

    /// A busy hub is the one that can least afford blind coverage, and it is exactly
    /// where a deadline read off a receive timeout never fires: `recv_timeout` returns
    /// whatever is already queued, however long the deadline has been past.
    #[cfg(unix)]
    #[test]
    fn a_tick_still_runs_while_the_queue_never_empties() {
        let project = linked_root();
        let hub = project.hub(Duration::from_millis(50));
        let stop = Arc::new(AtomicBool::new(false));
        let noise = {
            let stop = Arc::clone(&stop);
            let dir = project.first.clone();
            std::thread::spawn(move || {
                let mut n = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    let _ = std::fs::write(dir.join(format!("Noise{n}.bsl")), "x");
                    n += 1;
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        };

        project.retarget(&project.second);
        let noticed = eventually(Duration::from_secs(10), || hub.self_rearm_count() > 0);
        stop.store(true, Ordering::Relaxed);
        noise.join().unwrap();
        assert!(noticed, "a busy queue must not starve the coverage tick");
    }

    /// One tick is not a tick: a deadline armed once would pass every check that moves
    /// the target before the first firing, and leave the hub blind from then on.
    #[cfg(unix)]
    #[test]
    fn the_tick_keeps_firing_after_the_first_one() {
        let project = linked_root();
        let hub = project.hub(Duration::from_millis(50));
        assert!(
            eventually(Duration::from_secs(10), || hub.tick_count() >= 1),
            "the first tick must happen"
        );
        let after_first = hub.tick_count();

        project.retarget(&project.second);
        assert!(
            eventually(Duration::from_secs(10), || hub.self_rearm_count() > 0),
            "a retarget after the first tick must still be noticed"
        );
        assert!(hub.tick_count() > after_first);
    }

    /// A target that is simply not there costs nothing at all. It stays in the declared
    /// set and out of the cover, tick after tick, so neither a full re-arm nor a
    /// reconcile — which every consumer answers with a complete tree walk — may be
    /// spent on it. The live neighbour is the positive control: without it the thread
    /// would give up before the loop and both counters would read zero for the wrong
    /// reason.
    #[test]
    fn a_target_that_stays_missing_costs_no_rearm_and_no_reconcile() {
        let dir = tempdir().unwrap();
        let live = dir.path().join("live");
        std::fs::create_dir_all(&live).unwrap();
        let hub = WorkspaceChangeHub::start_targets_with_period(
            vec![
                WatchTarget::recursive(live.clone()),
                WatchTarget::recursive(dir.path().join("absent")),
            ],
            Duration::from_millis(20),
        );
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let cursor = hub.subscribe();
        let _ = hub.drain(cursor);
        let rescans = hub.rescan_request_count();

        assert!(eventually(Duration::from_secs(5), || hub.tick_count() >= 3), "ticks must run");
        assert_eq!(hub.self_rearm_count(), 0, "a stable absence is not movement");
        assert_eq!(hub.rescan_request_count(), rescans, "and it must not cost a reconcile");
    }

    /// A target that disappears has moved once, not once per tick. The first tick
    /// after the removal is entitled to a re-arm; every later one sees the same
    /// absence and must stay silent, or a deleted extension root would put every
    /// consumer through a full walk every period for the life of the daemon.
    #[test]
    fn a_removed_target_is_noticed_once_and_not_again() {
        let dir = tempdir().unwrap();
        let live = dir.path().join("live");
        let doomed = dir.path().join("doomed");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::create_dir_all(&doomed).unwrap();
        let hub = WorkspaceChangeHub::start_targets_with_period(
            vec![WatchTarget::recursive(live), WatchTarget::recursive(doomed.clone())],
            Duration::from_secs(3600),
        );
        assert!(hub.wait_until_watching(Duration::from_secs(5)));

        std::fs::remove_dir_all(&doomed).unwrap();
        assert!(hub.tick_now(Duration::from_secs(5)));
        assert_eq!(hub.self_rearm_count(), 1, "the removal itself is movement");

        let after = hub.self_rearm_count();
        let rescans = hub.rescan_request_count();
        assert!(hub.tick_now(Duration::from_secs(5)));
        assert!(hub.tick_now(Duration::from_secs(5)));
        assert_eq!(hub.self_rearm_count(), after, "the same absence is not movement again");
        assert_eq!(hub.rescan_request_count(), rescans);
    }

    /// The flat layout is the one that breaks a naive detector: `watch_targets_for`
    /// declares the workspace both recursively and non-recursively, so a rule that
    /// compared the declared set against what is armed would find a target missing
    /// forever and re-walk the whole tree every period.
    #[test]
    fn a_flat_workspace_that_does_not_move_costs_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let hub = WorkspaceChangeHub::start_targets_with_period(
            watch_targets_for(&root, std::slice::from_ref(&root)),
            Duration::from_millis(20),
        );
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let cursor = hub.subscribe();
        let _ = hub.drain(cursor);
        let rescans = hub.rescan_request_count();

        assert!(eventually(Duration::from_secs(5), || hub.tick_count() >= 5), "ticks must run");
        assert_eq!(hub.self_rearm_count(), 0, "a still tree is not movement");
        assert_eq!(hub.rescan_request_count(), rescans);
    }

    /// The production interval is part of the contract, not an implementation detail:
    /// every other test either shortens it or drives the tick by hand, so a hub built
    /// the ordinary way could carry an interval of a day and still pass them all.
    #[test]
    fn the_production_hub_ticks_every_thirty_seconds() {
        assert_eq!(COVERAGE_TICK_PERIOD, Duration::from_secs(30));
        let dir = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(vec![dir.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        assert_eq!(hub.inner.tick_period, Duration::from_secs(30));
    }

    /// The hub takes a path into work only when it belongs to the observed scope.
    /// A directory OUTSIDE every scan root — the build output or a vendored clone
    /// that lands next to the sources — must not be walked: `collect_subtree`
    /// records one entry per file it finds, and a foreign tree larger than the
    /// accumulator's capacity would push every consumer into a full rescan.
    #[test]
    fn a_foreign_directory_is_not_walked() {
        let project = nested_project();
        let hub = project.hub();
        let cursor = hub.subscribe();

        let foreign = project.workspace.join("node_modules");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("index.js"), "x").unwrap();
        std::fs::write(foreign.join("Module.bsl"), "Процедура П() КонецПроцедуры").unwrap();

        let walks_before = subtree_walks();
        let rewatch =
            hub.inner.ingest_event(change_event(EventKind::Create(CreateKind::Folder), foreign));

        assert_eq!(subtree_walks(), walks_before, "a foreign directory is not walked");
        assert!(rewatch.is_empty(), "and is not handed back for a recursive re-watch");
        assert!(hub.drain(cursor).entries.is_empty(), "so none of its files reach the accumulator");
    }

    /// The positive control for the case above: the very same shape INSIDE a scan
    /// root is walked, re-watched and recorded. Without it, a predicate that
    /// filtered everything would pass the negative test.
    #[test]
    fn a_directory_inside_a_scan_root_is_walked() {
        let project = nested_project();
        let hub = project.hub();
        let cursor = hub.subscribe();

        let owned = project.scan_root.join("CommonModules");
        std::fs::create_dir_all(&owned).unwrap();
        std::fs::write(owned.join("Module.bsl"), "Процедура П() КонецПроцедуры").unwrap();

        let walks_before = subtree_walks();
        let rewatch = hub
            .inner
            .ingest_event(change_event(EventKind::Create(CreateKind::Folder), owned.clone()));

        assert_eq!(subtree_walks(), walks_before + 1, "a directory in scope is walked");
        assert_eq!(rewatch, vec![owned], "and is handed back for a recursive re-watch");
        assert!(
            entry_names(&hub.drain(cursor)).iter().any(|p| p.ends_with("Module.bsl")),
            "its files reach the accumulator"
        );
    }

    /// A vanished path outside the scope must not reach the accumulator either.
    /// `classify_path` returns `None` only for a directory that still EXISTS; a
    /// gone extension-less path becomes `SubtreeRemoved`, which every consumer
    /// reads as "reconsider the whole tree".
    #[test]
    fn a_foreign_directory_removal_asks_for_no_rescan() {
        let project = nested_project();
        let hub = project.hub();
        let cursor = hub.subscribe();

        let foreign = project.workspace.join("vendor");
        hub.inner.ingest_event(change_event(EventKind::Remove(RemoveKind::Folder), foreign));

        let batch = hub.drain(cursor);
        assert!(batch.entries.is_empty(), "a removal outside the scope is not recorded");
        assert_eq!(hub.health(), Health::Healthy, "and does not degrade the hub");
    }

    /// Positive control: the same removal INSIDE a scan root still asks consumers
    /// to reconsider the subtree.
    #[test]
    fn a_scan_root_directory_removal_is_recorded() {
        let project = nested_project();
        let hub = project.hub();
        let cursor = hub.subscribe();

        let gone = project.scan_root.join("Catalogs");
        hub.inner.ingest_event(change_event(EventKind::Remove(RemoveKind::Folder), gone));

        let batch = hub.drain(cursor);
        assert_eq!(batch.entries.len(), 1, "a removal in scope is recorded");
        assert_eq!(batch.entries[0].kind, ChangeKind::SubtreeRemoved);
    }

    /// An unknown event kind is a "we may have missed something" signal, so it
    /// degrades. But a path outside the scope carries nothing we were watching for,
    /// and degrading on it lets one foreign file drag every consumer into a scan.
    #[test]
    fn an_unknown_event_outside_the_scope_does_not_degrade() {
        let project = nested_project();
        let hub = project.hub();

        hub.ingest_for_test(change_event(EventKind::Other, project.workspace.join("stray.tmp")));

        assert_eq!(hub.health(), Health::Healthy);
    }

    /// Positive control: an unknown event about a path we ARE watching still
    /// degrades, and so does one that carries no path at all — an absent path is
    /// not evidence that the lost change was out of scope.
    #[test]
    fn an_unknown_event_in_scope_or_without_paths_still_degrades() {
        let project = nested_project();
        let hub = project.hub();
        hub.ingest_for_test(change_event(EventKind::Other, project.scan_root.join("x.bsl")));
        assert_eq!(hub.health(), Health::Degraded(DegradeReason::UnknownEvent));

        let project = nested_project();
        let hub = project.hub();
        hub.ingest_for_test(event_with_paths(EventKind::Other, Vec::new()));
        assert_eq!(
            hub.health(),
            Health::Degraded(DegradeReason::UnknownEvent),
            "an event with no path proves nothing about scope"
        );
    }

    /// Every notify backend makes a relative watch target absolute before arming
    /// it, so events come back spelled from the current directory. Keeping only the
    /// declared (relative) and canonical spellings loses the whole tree whenever
    /// those two differ from the reported one — a `..` component, or a symlink on
    /// the way. A plain relative root hides this: its canonical spelling already
    /// matches what the watcher reports.
    ///
    /// This case holds the behaviour but cannot, on Unix, distinguish the two ways
    /// of building that third spelling: `std::path::absolute` differs from the
    /// backend's plain join only in dropping `.` (which component comparison
    /// ignores anyway) and in resolving `..` — and the latter it does on Windows
    /// alone. So the mutation that swaps one for the other is only observable on a
    /// Windows host; here the reasoning rests on the std docs and the backend
    /// source, not on a red test.
    #[test]
    fn a_relative_target_stays_in_scope_when_events_arrive_absolute() {
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir_in(&cwd).unwrap();
        std::fs::create_dir(dir.path().join("prefix")).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();

        let base = dir.path().strip_prefix(&cwd).unwrap();
        // `.` alongside `..`: the backend keeps both, `std::path::absolute` drops
        // the first everywhere and resolves the second on Windows. Without them the
        // test cannot tell the two ways of building the spelling apart.
        let declared = base.join("prefix").join("..").join(".").join("src");
        let scope = Scope::from_targets(&ResolvedTargets::here(vec![WatchTarget::recursive(
            declared.clone(),
        )]));

        // The reference is computed the way the backend computes it — joining to the
        // current directory, components untouched.
        let reported = cwd.join(&declared);
        assert!(
            scope.may_record(&reported.join("Module.bsl")),
            "the spelling the watcher actually reports is in scope"
        );
        assert!(scope.may_walk(&reported.join("CommonModules")));
    }

    /// Reading the current directory twice — once for the scope, once inside the
    /// backend's `watch` — is a race on process-wide state: a change in between
    /// arms the watcher on one tree while the scope describes another, and every
    /// event from the armed tree is then filtered out in silence. Resolving the
    /// targets once, before anything is armed or compared, removes the second read:
    /// the backend takes an absolute path as given.
    #[test]
    fn targets_are_resolved_once_so_the_watcher_and_the_scope_cannot_disagree() {
        let resolved = ResolvedTargets::here(vec![
            WatchTarget::recursive(PathBuf::from("src")),
            WatchTarget { path: PathBuf::from("."), recursive: false },
        ]);
        assert!(resolved.is_complete());
        let resolved = resolved.as_slice();

        assert!(
            resolved.iter().all(|t| t.path.is_absolute()),
            "nothing relative reaches the watcher, so it never re-reads the directory"
        );
        // Modes must survive the rewrite: the non-recursive one is what carries the
        // project-config files.
        assert_eq!(resolved.iter().filter(|t| t.recursive).count(), 1);
        assert_eq!(resolved.iter().filter(|t| !t.recursive).count(), 1);

        let absolute = std::env::current_dir().unwrap().join("src");
        assert!(
            ResolvedTargets::here(vec![WatchTarget::recursive(absolute.clone())]).as_slice()[0]
                .path
                == absolute,
            "an already-absolute target is left exactly as it was"
        );
    }

    /// One directory snapshot for the whole set, not one per target. A set spanning
    /// a scan root and the workspace config directory that were placed against
    /// different directories would watch the sources of one project and the
    /// configuration of another, and nothing downstream could tell.
    ///
    /// The single read is a property of the signature — `resolve` cannot consult
    /// process state at all — so this asserts the consequence: every relative target
    /// lands under the one directory handed in, and absolute ones are left alone.
    #[test]
    fn every_relative_target_is_placed_against_the_one_directory_given() {
        // Not a literal: `/base` carries no drive prefix, so Windows reads it as
        // RELATIVE and the resolver would rightly drop it.
        let base = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        // Canonicalized, not merely temporary: the system temp directory is
        // whatever `TMPDIR` says, and a test that assumes it absolute would be
        // testing the environment instead of the resolver.
        let base = base.path().canonicalize().unwrap();
        let elsewhere = elsewhere.path().canonicalize().unwrap().join("extension");
        let resolved = ResolvedTargets::resolve(
            vec![
                WatchTarget::recursive(PathBuf::from("src")),
                WatchTarget { path: PathBuf::from("."), recursive: false },
                WatchTarget::recursive(elsewhere.clone()),
            ],
            Some(&base),
        );

        assert!(resolved.is_complete());
        let placed: Vec<&PathBuf> = resolved.as_slice().iter().map(|t| &t.path).collect();
        assert_eq!(placed, vec![&base.join("src"), &base.join("."), &elsewhere]);
    }

    /// Placing a target is a claim that the backend will not resolve the path
    /// again, and only an absolute path makes that claim true. A base that is
    /// itself relative cannot produce one — nor can a Windows drive-relative
    /// target, whose prefix REPLACES the base on join and leaves the result
    /// relative to the per-drive current directory. The join is therefore checked
    /// rather than assumed.
    #[test]
    fn a_target_that_stays_relative_after_the_join_is_dropped() {
        let resolved = ResolvedTargets::resolve(
            vec![WatchTarget::recursive(PathBuf::from("src"))],
            Some(Path::new("relative/base")),
        );

        assert!(!resolved.is_complete());
        assert!(resolved.as_slice().is_empty());
    }

    /// Without a readable current directory a relative target cannot be placed at
    /// all. Carrying it through relative would hand the backend a path it resolves
    /// against its own later read of the same process-wide state — the disagreement
    /// between the armed tree and the described one that this whole boundary exists
    /// to prevent. Dropping it is only safe if the set says so: a caller that read
    /// the drop as success would report full coverage over a subtree nobody watches.
    #[test]
    fn a_relative_target_without_a_directory_is_dropped_and_the_set_says_so() {
        // A real temporary directory, not a `/base/src` literal: on Windows a path
        // without a drive prefix is relative, and the assertion would invert.
        let dir = tempfile::tempdir().unwrap();
        let absolute = dir.path().canonicalize().unwrap().join("src");
        let resolved = ResolvedTargets::resolve(
            vec![
                WatchTarget::recursive(PathBuf::from("src")),
                WatchTarget::recursive(absolute.clone()),
            ],
            None,
        );

        assert!(!resolved.is_complete(), "coverage cannot be claimed over a dropped target");
        let placed: Vec<&PathBuf> = resolved.as_slice().iter().map(|t| &t.path).collect();
        assert_eq!(placed, vec![&absolute], "what could be placed is still watched");
    }

    /// A rescan notice is not a change to the path it names — it says the event
    /// stream itself lapsed, so nothing received so far can be trusted. Scope tells
    /// nothing about what was lost, and dropping the notice would leave consumers
    /// serving stale results with the hub reporting good health. Inotify raises it
    /// without a path, FSEvents attaches one (the workspace directory, quite
    /// possibly outside every scan root) — which is exactly where a scope filter
    /// would swallow it.
    #[test]
    fn a_rescan_notice_outside_the_scope_still_degrades() {
        // The flag is an attribute in its own right: nothing in the contract ties it
        // to one kind, so every kind a backend may pair it with must degrade. Each
        // kind gets its own hub — health does not reset between notices.
        for kind in [
            EventKind::Other,
            EventKind::Create(CreateKind::File),
            EventKind::Modify(ModifyKind::Any),
            EventKind::Remove(RemoveKind::File),
            EventKind::Access(notify::event::AccessKind::Close(notify::event::AccessMode::Write)),
        ] {
            let project = nested_project();
            let hub = project.hub();
            let notice = Event::new(kind)
                .add_path(project.workspace.join("vendor"))
                .set_flag(notify::event::Flag::Rescan);
            hub.ingest_for_test(Ok(notice));

            assert_eq!(
                hub.health(),
                Health::Degraded(DegradeReason::UnknownEvent),
                "a rescan notice carrying {kind:?} must still degrade"
            );
        }
    }

    /// An ordinary file outside every scan root is not a config file and not a
    /// directory — the plainest way to be out of scope, and the one a
    /// directory-only filter would miss.
    #[test]
    fn an_ordinary_file_outside_the_scope_is_not_recorded() {
        let project = nested_project();
        let hub = project.hub();
        let cursor = hub.subscribe();

        let stray = project.workspace.join("notes.tmp");
        std::fs::write(&stray, "x").unwrap();
        hub.ingest_for_test(change_event(EventKind::Modify(ModifyKind::Any), stray));

        assert!(hub.drain(cursor).entries.is_empty());
    }

    /// Every project-config name — not just the TOML one — reaches consumers from
    /// the workspace directory, even though that directory is not a scan root.
    /// This is what the non-recursive workspace target exists for.
    #[test]
    fn every_config_file_name_in_the_workspace_is_recorded() {
        for name in project_model::CONFIG_FILE_NAMES {
            let project = nested_project();
            let hub = project.hub();
            let cursor = hub.subscribe();

            let config = project.workspace.join(name);
            std::fs::write(&config, "{}").unwrap();
            hub.ingest_for_test(change_event(EventKind::Modify(ModifyKind::Any), config));

            assert!(
                !hub.drain(cursor).entries.is_empty(),
                "a change to {name} must reach consumers"
            );
        }
    }

    /// The config predicate reads the NAME and the PARENT, never the disk: a
    /// deleted config is exactly as topology-shaping as an edited one, and a
    /// predicate gated on "the file exists" would drop it.
    #[test]
    fn every_config_file_removal_in_the_workspace_is_recorded() {
        for name in project_model::CONFIG_FILE_NAMES {
            let project = nested_project();
            let hub = project.hub();
            let cursor = hub.subscribe();

            let config = project.workspace.join(name);
            hub.ingest_for_test(change_event(EventKind::Remove(RemoveKind::File), config));

            let batch = hub.drain(cursor);
            assert_eq!(batch.entries.len(), 1, "the removal of {name} must reach consumers");
            assert_eq!(batch.entries[0].kind, ChangeKind::MaybeRemoved);
        }
    }

    /// The config exception is anchored to the workspace directory. The same name
    /// deeper down, outside every scan root, is somebody else's file.
    #[test]
    fn a_config_name_outside_the_workspace_directory_is_not_recorded() {
        let project = nested_project();
        let hub = project.hub();
        let cursor = hub.subscribe();

        let nested = project.workspace.join("vendor").join(project_model::CONFIG_FILE_NAMES[0]);
        std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        std::fs::write(&nested, "{}").unwrap();
        hub.ingest_for_test(change_event(EventKind::Modify(ModifyKind::Any), nested));

        assert!(hub.drain(cursor).entries.is_empty());
    }

    /// The two permissions add up rather than cancel: a config NAME grants the
    /// right to record, never the right to walk. A directory carrying that name is
    /// still a foreign directory.
    #[test]
    fn a_directory_named_like_a_config_is_not_walked() {
        let project = nested_project();
        let hub = project.hub();

        let trap = project.workspace.join(project_model::CONFIG_FILE_NAMES[0]);
        std::fs::create_dir_all(&trap).unwrap();
        std::fs::write(trap.join("payload.tmp"), "x").unwrap();

        let walks_before = subtree_walks();
        let rewatch =
            hub.inner.ingest_event(change_event(EventKind::Create(CreateKind::Folder), trap));

        assert_eq!(subtree_walks(), walks_before, "a config-named directory is not walked");
        assert!(rewatch.is_empty(), "and is not taken under recursive watch");
    }

    /// Positive control for the case above, and the reason it must be worded
    /// carefully: in a FLAT project the workspace IS the scan root, so a
    /// config-named directory sits under a scan root and is walked on the ordinary
    /// rule. Forbidding it outright would silently drop the contents of a moved
    /// directory.
    #[test]
    fn a_config_named_directory_under_a_scan_root_is_walked() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let hub = WorkspaceChangeHub::start_targets(watch_targets_for(
            &root,
            std::slice::from_ref(&root),
        ));
        assert!(hub.wait_until_watching(Duration::from_secs(5)));

        let trap = root.join(project_model::CONFIG_FILE_NAMES[0]);
        std::fs::create_dir_all(&trap).unwrap();
        std::fs::write(trap.join("Module.bsl"), "Процедура П() КонецПроцедуры").unwrap();

        let walks_before = subtree_walks();
        let rewatch = hub
            .inner
            .ingest_event(change_event(EventKind::Create(CreateKind::Folder), trap.clone()));

        assert_eq!(subtree_walks(), walks_before + 1, "under a scan root it is walked");
        assert_eq!(rewatch, vec![trap]);
    }

    /// A scan root declared through a symlink sends events spelled the DECLARED
    /// way: `dedup_targets` decides by canonical path but hands `watcher.watch`
    /// the raw spelling. A predicate comparing against one spelling alone would
    /// throw the whole tree away.
    #[cfg(unix)]
    #[test]
    fn both_spellings_of_a_scan_root_are_in_scope() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let real = workspace.join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = workspace.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let hub = WorkspaceChangeHub::start_targets(watch_targets_for(
            &workspace,
            std::slice::from_ref(&link),
        ));
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let cursor = hub.subscribe();

        // Two DIFFERENT files, one fed by each spelling: feeding one file twice
        // would prove nothing, since both spellings coalesce onto a single key and
        // dropping either would leave the count unchanged.
        std::fs::write(real.join("Declared.bsl"), "Процедура П() КонецПроцедуры").unwrap();
        std::fs::write(real.join("Canonical.bsl"), "Процедура П() КонецПроцедуры").unwrap();
        hub.ingest_for_test(change_event(
            EventKind::Modify(ModifyKind::Any),
            link.join("Declared.bsl"),
        ));
        hub.ingest_for_test(change_event(
            EventKind::Modify(ModifyKind::Any),
            real.join("Canonical.bsl"),
        ));

        let names = entry_names(&hub.drain(cursor));
        assert!(
            names.iter().any(|p| p.ends_with("Declared.bsl")),
            "the declared spelling — what the watcher actually reports — is in scope"
        );
        assert!(names.iter().any(|p| p.ends_with("Canonical.bsl")), "and so is the canonical one");
    }

    /// A directory MOVED into the tree arrives as `Modify(Name(To))`, never as a
    /// `Create`. Its files did not change — their path did — so they fire no events
    /// of their own: unless the arrival itself is walked, nothing in it is ever
    /// indexed until a full reconcile.
    #[test]
    fn a_directory_moved_into_a_scan_root_is_walked() {
        let project = nested_project();
        let hub = project.hub();
        let cursor = hub.subscribe();

        let staged = project.staged_dir("moved");
        let landed = project.scan_root.join("CommonModules");
        std::fs::rename(&staged, &landed).unwrap();

        let walks_before = subtree_walks();
        let rewatch = hub.inner.ingest_event(change_event(
            EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::To)),
            landed.clone(),
        ));

        assert_eq!(subtree_walks(), walks_before + 1, "the arrived directory is walked");
        assert_eq!(rewatch, vec![landed], "and taken under recursive watch");
        assert!(
            entry_names(&hub.drain(cursor)).iter().any(|p| p.ends_with("Module.bsl")),
            "the content that rode along with it reaches the accumulator"
        );
    }

    /// Only `Name` widens the branch. A `chmod` on a directory is a `Modify` too,
    /// and walking a large tree for it would be pure waste — the plainest wrong
    /// widening (to any `Modify`) is exactly what this guards against.
    #[test]
    fn a_non_rename_modify_of_a_directory_is_not_walked() {
        let project = nested_project();
        let hub = project.hub();

        let dir = project.scan_root.join("CommonModules");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Module.bsl"), "Процедура П() КонецПроцедуры").unwrap();

        for kind in [
            ModifyKind::Metadata(notify::event::MetadataKind::Permissions),
            ModifyKind::Data(notify::event::DataChange::Any),
            ModifyKind::Any,
            ModifyKind::Other,
        ] {
            let walks_before = subtree_walks();
            let rewatch =
                hub.inner.ingest_event(change_event(EventKind::Modify(kind), dir.clone()));
            assert_eq!(subtree_walks(), walks_before, "a {kind:?} on a directory is not walked");
            assert!(rewatch.is_empty(), "and does not take it under recursive watch");
        }
    }

    /// A mixed `Name(Both)` carries the vanished path and the arrived one in a
    /// single event, and a rename out of a scan root puts them on opposite sides
    /// of the boundary. Filtering per EVENT would let the foreign directory
    /// through; the filter is per PATH.
    #[test]
    fn a_rename_across_the_boundary_filters_each_path() {
        let project = nested_project();
        let hub = project.hub();
        let cursor = hub.subscribe();

        let gone = project.scan_root.join("Catalogs");
        let landed = project.workspace.join("vendor");
        std::fs::create_dir_all(&landed).unwrap();
        std::fs::write(landed.join("Module.bsl"), "Процедура П() КонецПроцедуры").unwrap();

        let walks_before = subtree_walks();
        let rewatch = hub.inner.ingest_event(event_with_paths(
            EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Both)),
            vec![gone.clone(), landed],
        ));

        assert_eq!(subtree_walks(), walks_before, "the arrived foreign directory is not walked");
        assert!(rewatch.is_empty());
        let batch = hub.drain(cursor);
        assert_eq!(batch.entries.len(), 1, "only the in-scope path is recorded");
        assert_eq!(batch.entries[0].raw, gone);
    }

    #[test]
    fn create_then_delete_in_one_window_settles_on_removal() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("Module.bsl");
        std::fs::write(&file, "Процедура П()\nКонецПроцедуры").unwrap();

        let mut acc = Accumulator::new(64);
        let cursor = acc.subscribe();
        // First event fires while the file exists.
        let (canonical, kind) = classify_path(&file).unwrap();
        assert_eq!(kind, ChangeKind::MaybeChanged);
        acc.record(canonical, file.clone(), kind);

        // The file is deleted before the next event is classified: on-disk truth
        // now says removed, and that is what the coalesced entry must reflect. The
        // create and remove must land on the SAME canonical key so they coalesce.
        std::fs::remove_file(&file).unwrap();
        let (canonical, kind) = classify_path(&file).unwrap();
        assert_eq!(kind, ChangeKind::MaybeRemoved);
        acc.record(canonical, file.clone(), kind);

        let batch = acc.drain(cursor);
        assert_eq!(batch.entries.len(), 1, "the path coalesced to a single entry");
        assert_eq!(batch.entries[0].kind, ChangeKind::MaybeRemoved);
    }

    #[test]
    fn create_then_remove_coalesce_under_symlinked_root() {
        // A symlinked directory component makes the raw and canonical spellings
        // differ. The removal key must still match the create key (via the parent's
        // canonicalization) so the two coalesce instead of leaving a ghost entry.
        let dir = tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(not(unix))]
        return;

        #[cfg(unix)]
        {
            let via_link = link.join("Module.bsl");
            std::fs::write(&via_link, "x").unwrap();

            let mut acc = Accumulator::new(64);
            let cursor = acc.subscribe();
            let (create_key, _) = classify_path(&via_link).unwrap();
            acc.record(create_key, via_link.clone(), ChangeKind::MaybeChanged);

            std::fs::remove_file(&via_link).unwrap();
            let (remove_key, kind) = classify_path(&via_link).unwrap();
            assert_eq!(kind, ChangeKind::MaybeRemoved);
            acc.record(remove_key, via_link.clone(), kind);

            let batch = acc.drain(cursor);
            assert_eq!(batch.entries.len(), 1, "create+remove coalesced under the symlink");
            assert_eq!(batch.entries[0].kind, ChangeKind::MaybeRemoved);
        }
    }

    #[test]
    fn removed_extensionless_path_is_a_subtree_removal() {
        let dir = tempdir().unwrap();
        let gone = dir.path().join("Catalogs");
        let (_canonical, kind) = classify_path(&gone).unwrap();
        assert_eq!(kind, ChangeKind::SubtreeRemoved);
    }

    #[test]
    fn reclamation_releases_entries_once_all_cursors_advance() {
        let mut acc = Accumulator::new(64);
        let a = acc.subscribe();
        let b = acc.subscribe();

        for i in 0..10 {
            let p = PathBuf::from(format!("/f{i}.bsl"));
            acc.record(p.clone(), p, ChangeKind::MaybeChanged);
        }
        assert_eq!(acc.entries.len(), 10);

        // A drains; B has not, so nothing is reclaimed yet (B still needs them).
        let _ = acc.drain(a);
        assert_eq!(acc.entries.len(), 10, "the slower cursor still holds the entries");

        // B drains; now the slowest cursor has advanced and the map empties.
        let _ = acc.drain(b);
        assert_eq!(acc.entries.len(), 0, "entries release once every cursor passed them");

        // Cap counts undrained in-flight paths, so post-drain recording keeps going
        // without ever tripping overflow.
        for i in 0..200 {
            let p = PathBuf::from(format!("/g{i}.bsl"));
            acc.record(p.clone(), p, ChangeKind::MaybeChanged);
            let _ = acc.drain(a);
            let _ = acc.drain(b);
        }
        assert_eq!(acc.health(), Health::Healthy, "steady drain never overflows");
        assert!(acc.entries.is_empty());
    }

    #[test]
    fn overflow_clears_flags_per_cursor_then_recovers() {
        let mut acc = Accumulator::new(2);
        let a = acc.subscribe();
        let b = acc.subscribe();

        // Two distinct undrained keys fill the cap; a third trips overflow.
        acc.record(PathBuf::from("/a.bsl"), PathBuf::from("/a.bsl"), ChangeKind::MaybeChanged);
        acc.record(PathBuf::from("/b.bsl"), PathBuf::from("/b.bsl"), ChangeKind::MaybeChanged);
        acc.record(PathBuf::from("/c.bsl"), PathBuf::from("/c.bsl"), ChangeKind::MaybeChanged);

        assert_eq!(acc.health(), Health::Degraded(DegradeReason::Overflow));
        assert!(acc.entries.is_empty(), "overflow drops the untrusted detail");

        // Accumulation continues after overflow (the old bug dropped new keys).
        acc.record(PathBuf::from("/d.bsl"), PathBuf::from("/d.bsl"), ChangeKind::MaybeChanged);
        assert_eq!(acc.entries.len(), 1, "new changes after overflow are captured");

        // Each cursor is told to reconcile exactly once.
        let batch_a = acc.drain(a);
        assert!(batch_a.rescan_required);
        let batch_a2 = acc.drain(a);
        assert!(!batch_a2.rescan_required, "the flag is delivered only once");
        // A alone acknowledging is not enough; B still owes a reconcile.
        assert_eq!(acc.health(), Health::Degraded(DegradeReason::Overflow));

        let batch_b = acc.drain(b);
        assert!(batch_b.rescan_required);
        assert_eq!(acc.health(), Health::Healthy, "recovers once all cursors confirm");
    }

    #[test]
    fn late_subscriber_during_rescan_window_sees_the_flag() {
        let mut acc = Accumulator::new(2);
        let a = acc.subscribe();
        acc.record(PathBuf::from("/a.bsl"), PathBuf::from("/a.bsl"), ChangeKind::MaybeChanged);
        acc.record(PathBuf::from("/b.bsl"), PathBuf::from("/b.bsl"), ChangeKind::MaybeChanged);
        acc.record(PathBuf::from("/c.bsl"), PathBuf::from("/c.bsl"), ChangeKind::MaybeChanged);
        assert_eq!(acc.health(), Health::Degraded(DegradeReason::Overflow));

        // Subscribing while A has not yet acknowledged: the newcomer must reconcile.
        let late = acc.subscribe();
        assert!(acc.drain(late).rescan_required, "a cursor born mid-window reconciles");

        // A newcomer after everyone has acknowledged starts clean.
        let _ = acc.drain(a);
        let _ = acc.drain(late);
        assert_eq!(acc.health(), Health::Healthy);
        let fresh = acc.subscribe();
        assert!(!acc.drain(fresh).rescan_required, "a cursor born healthy does not reconcile");
    }

    #[test]
    fn independent_cursors_drain_their_own_deltas() {
        let mut acc = Accumulator::new(64);
        let x = acc.subscribe();
        acc.record(PathBuf::from("/a.bsl"), PathBuf::from("/a.bsl"), ChangeKind::MaybeChanged);
        acc.record(PathBuf::from("/b.bsl"), PathBuf::from("/b.bsl"), ChangeKind::MaybeChanged);

        let batch_x = acc.drain(x);
        assert_eq!(batch_x.entries.len(), 2);

        // Y subscribes now — it must not replay the earlier changes.
        let y = acc.subscribe();
        acc.record(PathBuf::from("/c.bsl"), PathBuf::from("/c.bsl"), ChangeKind::MaybeChanged);

        let batch_y = acc.drain(y);
        assert_eq!(batch_y.entries.len(), 1, "Y sees only the change after it subscribed");
        assert_eq!(batch_y.entries[0].raw, Path::new("/c.bsl"));

        let batch_x = acc.drain(x);
        assert_eq!(batch_x.entries.len(), 1, "X sees only its own delta");
        assert_eq!(batch_x.entries[0].raw, Path::new("/c.bsl"));
    }

    #[test]
    fn health_flips_on_setup_error_for_invalid_root() {
        let hub =
            WorkspaceChangeHub::start(vec![PathBuf::from("/definitely/not/a/real/path/xyzzy")]);
        assert!(!hub.wait_until_watching(Duration::from_secs(5)));
        assert_eq!(hub.health(), Health::Degraded(DegradeReason::WatcherSetup));
        assert!(!hub.is_watching());
    }

    #[test]
    fn start_arms_the_watch_asynchronously() {
        let dir = tempdir().unwrap();
        // `start` returns immediately; the watch arms on the hub thread. Poll for it
        // rather than asserting synchronously.
        let hub = WorkspaceChangeHub::start(vec![dir.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)), "setup settles to watching");
        assert!(hub.is_watching());
        assert_eq!(hub.health(), Health::Healthy);
    }

    #[test]
    fn health_flips_on_unknown_event_kind() {
        let dir = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(vec![dir.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        assert_eq!(hub.health(), Health::Healthy);
        hub.ingest_for_test(change_event(EventKind::Other, dir.path().join("x")));
        assert_eq!(hub.health(), Health::Degraded(DegradeReason::UnknownEvent));
    }

    #[test]
    fn runtime_error_flips_health() {
        let dir = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(vec![dir.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        hub.ingest_for_test(Err(notify::Error::generic("boom")));
        assert_eq!(hub.health(), Health::Degraded(DegradeReason::RuntimeError));
    }

    #[test]
    fn rewatch_failure_degrades_and_recovers_through_rescan() {
        let dir = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(vec![dir.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let cursor = hub.subscribe();

        // A failure to extend the watch to a new subtree must not stay silent: it
        // degrades health and asks the consumer to reconcile, recoverable like any
        // other transient miss once the cursor acknowledges.
        hub.trigger_rewatch_failure_for_test();
        assert_eq!(hub.health(), Health::Degraded(DegradeReason::RewatchFailed));

        let batch = hub.drain(cursor);
        assert!(batch.rescan_required, "the sink is told to reconcile the possibly-blind subtree");
        assert_eq!(hub.health(), Health::Healthy, "recovers once the cursor acknowledges");
    }

    #[test]
    fn access_events_are_ignored_without_degrading() {
        use notify::event::AccessKind;
        let dir = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(vec![dir.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        hub.ingest_for_test(change_event(
            EventKind::Access(AccessKind::Read),
            dir.path().join("x.bsl"),
        ));
        assert_eq!(hub.health(), Health::Healthy, "a read is not drift");
    }

    #[test]
    fn config_file_paths_are_accumulated_not_filtered() {
        let dir = tempdir().unwrap();
        let toml = dir.path().join("bsl-analyzer.toml");
        std::fs::write(&toml, "[source]\nroot = \".\"\n").unwrap();

        let hub = WorkspaceChangeHub::start(vec![dir.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let cursor = hub.subscribe();
        hub.ingest_for_test(change_event(EventKind::Modify(ModifyKind::Any), toml.clone()));

        let batch = hub.drain(cursor);
        assert!(
            batch.entries.iter().any(|e| e.raw == toml),
            "the hub accepts any path; kind-filtering is the consumer's job",
        );
    }

    #[test]
    fn events_seen_counts_every_raw_event() {
        let dir = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(vec![dir.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        assert_eq!(hub.events_seen(), 0);
        hub.ingest_for_test(change_event(
            EventKind::Create(CreateKind::Any),
            dir.path().join("a.bsl"),
        ));
        hub.ingest_for_test(change_event(
            EventKind::Remove(RemoveKind::Any),
            dir.path().join("a.bsl"),
        ));
        assert_eq!(hub.events_seen(), 2);
    }

    /// Empirical check that a file created under a directory that did not exist
    /// when the watcher started is still observed. Bare `RecursiveMode::Recursive`
    /// races the OS watch arming; the hub closes that race by walking a
    /// freshly-created subtree on its create event.
    #[test]
    fn nested_directory_creation_is_observed() {
        let dir = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(vec![dir.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let cursor = hub.subscribe();

        let nested = dir.path().join("deep").join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("Module.bsl");
        std::fs::write(&file, "Процедура П()\nКонецПроцедуры").unwrap();

        let canonical = file.canonicalize().unwrap_or_else(|_| file.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = false;
        while Instant::now() < deadline {
            let batch = hub.drain(cursor);
            if batch.entries.iter().any(|e| e.canonical == canonical || e.raw == file) {
                seen = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(seen, "a file under a freshly-created subdirectory must be captured");
    }

    /// The hub watches EVERY root it is given (the config source root plus each extension
    /// root), so drift in a disjoint extension tree is event-delivered, not left to a scan.
    #[test]
    fn watches_all_roots() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(vec![a.path().to_path_buf(), b.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let cursor = hub.subscribe();

        std::thread::sleep(Duration::from_millis(100));
        // A change in the SECOND root must be observed.
        let file = b.path().join("Ext.bsl");
        std::fs::write(&file, "Процедура П()\nКонецПроцедуры").unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = false;
        while Instant::now() < deadline {
            let batch = hub.drain(cursor);
            if batch.entries.iter().any(|e| e.raw == file) {
                seen = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(seen, "a change in a secondary watch root must be captured");
    }

    /// In a nested layout the analyzer config sits ABOVE every scan root; the
    /// watch-target set must cover it as an individual file target, or a
    /// `dependsOn` edit would never be event-delivered to any consumer.
    #[test]
    fn watch_targets_cover_config_files_above_the_scan_roots() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let source = root.join("src/cf");
        std::fs::create_dir_all(&source).unwrap();
        let toml = root.join("bsl-analyzer.toml");
        std::fs::write(&toml, "[source]\nroot = \"src/cf\"\n").unwrap();

        let hub = WorkspaceChangeHub::start_targets(watch_targets_for(root, &[source]));
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let mut cursor = hub.subscribe();

        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(&toml, "[source]\nroot = \"src/cf\"\nextensions = []\n").unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = false;
        while Instant::now() < deadline {
            let batch = hub.drain(cursor);
            cursor = batch.cursor;
            if batch.entries.iter().any(|e| e.raw == toml || e.canonical == toml) {
                seen = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(seen, "an edit to the config file above the scan roots must be delivered");
    }

    /// The workspace-root dir watch must deliver a config file that did NOT exist
    /// at arm time (absent -> create) and keep delivering across editor-style
    /// atomic saves (write temp + rename over), which replace the inode and would
    /// permanently kill a watch on the file itself.
    #[test]
    fn config_creation_and_atomic_replace_are_delivered_via_the_root_watch() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let source = root.join("src/cf");
        std::fs::create_dir_all(&source).unwrap();
        // NO config file exists yet.
        let hub = WorkspaceChangeHub::start_targets(watch_targets_for(root, &[source]));
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let mut cursor = hub.subscribe();
        std::thread::sleep(Duration::from_millis(100));

        let toml = root.join("bsl-analyzer.toml");
        let expect_delivery = |cursor: &mut SinkCursor, what: &str| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let batch = hub.drain(*cursor);
                *cursor = batch.cursor;
                if batch.entries.iter().any(|e| e.raw == toml || e.canonical == toml) {
                    break;
                }
                assert!(Instant::now() < deadline, "config {what} must be delivered");
                std::thread::sleep(Duration::from_millis(50));
            }
        };

        std::fs::write(&toml, "[source]\nroot = \"src/cf\"\n").unwrap();
        expect_delivery(&mut cursor, "creation");

        for round in 0..2 {
            let tmp = root.join(format!(".bsl-analyzer.toml.tmp{round}"));
            std::fs::write(&tmp, format!("[source]\nroot = \"src/cf\"\n# v{round}\n")).unwrap();
            std::fs::rename(&tmp, &toml).unwrap();
            expect_delivery(&mut cursor, "atomic replace");
        }
    }

    /// A re-arm extends coverage to the new root without a hub restart: the cursor
    /// survives (same id, one rescan flag), and a change in the NEWLY-added root is
    /// event-delivered afterwards.
    #[test]
    fn rearm_extends_coverage_and_flags_cursors_to_rescan() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(vec![a.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let cursor = hub.subscribe();

        assert!(
            hub.rearm(
                vec![
                    WatchTarget::recursive(a.path().to_path_buf()),
                    WatchTarget::recursive(b.path().to_path_buf()),
                ],
                Duration::from_secs(10)
            ),
            "the hub thread acknowledges the re-arm with full coverage"
        );
        let batch = hub.drain(cursor);
        assert!(batch.rescan_required, "a re-arm owes every cursor exactly one rescan");
        let cursor = batch.cursor;
        assert_eq!(hub.health(), Health::Healthy, "health recovers once cursors acknowledge");

        std::thread::sleep(Duration::from_millis(100));
        let file = b.path().join("Новый.bsl");
        std::fs::write(&file, "Процедура П()\nКонецПроцедуры").unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = false;
        while Instant::now() < deadline {
            let batch = hub.drain(cursor);
            if batch.entries.iter().any(|e| e.raw == file) {
                seen = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(seen, "a change in the newly-armed root must be captured after the re-arm");
    }

    /// `ensure_roots` with the live set is free: no rescan round, no health blip —
    /// so calling it after EVERY rebuild is safe.
    #[test]
    fn ensure_roots_is_a_no_op_for_the_same_set() {
        let a = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(vec![a.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let cursor = hub.subscribe();

        assert!(hub.ensure_roots(&[WatchTarget::recursive(a.path().to_path_buf())]));
        let batch = hub.drain(cursor);
        assert!(!batch.rescan_required, "an unchanged root set must not force a rescan");
        assert_eq!(hub.health(), Health::Healthy);
    }

    /// A re-arm that cannot watch one of the new roots reports partial coverage (the
    /// armable subset is covered) and degrades health so consumers scan, instead of
    /// silently pretending the missing subtree is watched.
    #[test]
    fn rearm_onto_a_missing_root_degrades_but_still_acks() {
        let a = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(vec![a.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let cursor = hub.subscribe();

        // Disjoint from `a`: a missing path nested under a watched recursive root
        // is legitimately covered (the recursive watch sees it once created).
        let elsewhere = tempdir().unwrap();
        let missing = elsewhere.path().join("нет-такого-каталога");
        assert!(
            !hub.rearm(
                vec![
                    WatchTarget::recursive(a.path().to_path_buf()),
                    WatchTarget::recursive(missing),
                ],
                Duration::from_secs(10)
            ),
            "a re-arm that leaves a root unarmed must NOT read as covered"
        );
        assert!(matches!(hub.health(), Health::Degraded(_)), "an unwatchable root degrades health");
        let batch = hub.drain(cursor);
        assert!(batch.rescan_required, "the armable subset still owes a rescan");
    }

    /// `shutdown` terminates and joins the hub thread; later control requests fail
    /// fast and cursors keep draining the frozen stream.
    #[test]
    fn shutdown_joins_the_thread_and_freezes_the_stream() {
        let a = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(vec![a.path().to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));

        hub.shutdown();
        assert!(
            !hub.rearm(
                vec![WatchTarget::recursive(a.path().to_path_buf())],
                Duration::from_millis(100)
            ),
            "a re-arm after shutdown must report failure, not hang"
        );
        let cursor = hub.subscribe();
        let batch = hub.drain(cursor);
        assert!(batch.entries.is_empty(), "the frozen stream still drains cleanly");
        hub.shutdown();
    }

    /// Nested targets collapse under a recursive ancestor so a subtree is never
    /// double-watched (which some backends would report as duplicate events),
    /// regardless of input order — while a NON-recursive ancestor absorbs nothing
    /// (it covers only direct children), and a recursive duplicate of the same
    /// path wins over a non-recursive one.
    #[test]
    fn dedup_targets_drops_subtrees_of_recursive_ancestors_only() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("parent");
        let child = parent.join("sub");
        let sibling = dir.path().join("sibling");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let r = |p: &PathBuf| WatchTarget::recursive(p.clone());
        let nr = |p: &PathBuf| WatchTarget { path: p.clone(), recursive: false };
        let kept_paths = |targets: Vec<WatchTarget>| -> Vec<(PathBuf, bool)> {
            dedup_targets(targets).into_iter().map(|(t, _)| (t.path, t.recursive)).collect()
        };

        let kept = kept_paths(vec![r(&parent), r(&child), r(&sibling)]);
        assert!(kept.contains(&(parent.clone(), true)), "the ancestor is kept");
        assert!(kept.contains(&(sibling.clone(), true)), "a disjoint root is kept");
        assert!(!kept.iter().any(|(p, _)| p == &child), "a nested root is dropped");

        // Order-independent: the child listed first is still dropped.
        assert_eq!(kept_paths(vec![r(&child), r(&parent)]), vec![(parent.clone(), true)]);

        // A non-recursive ancestor does not absorb a recursive descendant.
        let kept = kept_paths(vec![nr(&parent), r(&child)]);
        assert!(kept.contains(&(parent.clone(), false)));
        assert!(kept.contains(&(child.clone(), true)), "non-recursive parent covers no subtree");

        // Same path, both modes: the recursive registration wins.
        assert_eq!(kept_paths(vec![nr(&parent), r(&parent)]), vec![(parent.clone(), true)]);
    }
}
