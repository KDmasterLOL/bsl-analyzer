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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};
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
    /// More than the cap piled up undrained, or the callback channel overflowed:
    /// detail was dropped and a full reconcile scan is needed.
    Overflow,
}

/// Observable health of the hub. `WatcherSetup` is permanent; every other
/// degradation is transient and clears back to `Healthy` once all live cursors
/// have acknowledged the reconcile request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Health {
    Healthy,
    Degraded(DegradeReason),
}

/// One accumulated change. Carries both the canonical key (matching the scan
/// universe used by drift detection) and the raw path as the watcher reported
/// it — consumers that strip a non-canonical root (search strips the configured
/// source root) need the raw spelling, or a symlinked root would fail to match.
///
/// `canonical` and `kind` are the drift-consumption contract: the diagnostics
/// sink re-stats `canonical` and branches on `kind`. The only sink today
/// (search) needs just `raw`, so those two read only from tests until that sink
/// lands.
#[derive(Debug, Clone)]
pub(crate) struct ChangeEntry {
    #[allow(dead_code)]
    pub(crate) canonical: PathBuf,
    pub(crate) raw: PathBuf,
    #[allow(dead_code)]
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
}

impl HubInner {
    fn lock_acc(&self) -> std::sync::MutexGuard<'_, Accumulator> {
        self.acc.lock().unwrap_or_else(PoisonError::into_inner)
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

        let mut rewatch: Vec<PathBuf> = Vec::new();
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                let mut records: Vec<(PathBuf, PathBuf, ChangeKind)> = Vec::new();
                for path in &event.paths {
                    // A newly created directory needs two things bare recursive
                    // watching does not give reliably on Linux: files written into
                    // it before the OS watch arms are lost, and a deep subtree
                    // created in one burst may never be watched. Walking the new
                    // subtree records whatever already exists (stats are truth),
                    // and re-arming a recursive watch covers everything created
                    // afterwards.
                    if matches!(event.kind, EventKind::Create(_)) {
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

/// Walk a freshly-created directory and record every file already inside it,
/// canonicalizing the directory once and joining each file's relative path rather
/// than canonicalizing per file.
fn collect_subtree(dir: &Path, records: &mut Vec<(PathBuf, PathBuf, ChangeKind)>) {
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

/// Daemon-owned hub over one recursive workspace watcher. Cheap to clone
/// (`Arc`-backed); every clone observes the same accumulator and health.
#[derive(Clone)]
pub(crate) struct WorkspaceChangeHub {
    inner: Arc<HubInner>,
}

impl WorkspaceChangeHub {
    /// Spawn the hub. Returns immediately: the recursive watch is armed on the hub
    /// thread (walking a large tree must not block daemon startup). Use
    /// [`Self::wait_until_watching`] or [`Self::is_watching`] to observe setup
    /// completion; [`Self::health`] reports `Degraded(WatcherSetup)` if it failed.
    pub(crate) fn start(root: PathBuf) -> Self {
        Self::start_with_capacity(root, DEFAULT_CAPACITY)
    }

    fn start_with_capacity(root: PathBuf, cap: usize) -> Self {
        let inner = Arc::new(HubInner {
            acc: Mutex::new(Accumulator::new(cap)),
            wake: Condvar::new(),
            watching: AtomicBool::new(false),
            channel_overflow: AtomicBool::new(false),
        });

        let thread_inner = Arc::clone(&inner);
        std::thread::Builder::new()
            .name("bsl-workspace-change-hub".to_owned())
            .spawn(move || run_hub_thread(thread_inner, root))
            .ok();

        Self { inner }
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

    // Health and the events-seen counter are the hub's observability surface. They
    // are consumed by `status_report` once the diagnostics sink lands; exposing
    // them now keeps the hub's contract complete and unit-testable in isolation.
    #[allow(dead_code)]
    pub(crate) fn health(&self) -> Health {
        self.inner.lock_acc().health()
    }

    #[allow(dead_code)]
    pub(crate) fn events_seen(&self) -> u64 {
        self.inner.lock_acc().events_seen
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

    /// Drive the exact transition the re-watch error branch takes. A real
    /// `watcher.watch` failure cannot be injected without a mock backend, so tests
    /// exercise the state transition directly.
    #[cfg(test)]
    fn trigger_rewatch_failure_for_test(&self) {
        self.inner.note_rewatch_failed(Path::new("/unwatchable"), &notify::Error::generic("test"));
    }
}

/// Arm the recursive watch and pump events until the daemon exits. Runs on its own
/// thread so `start` returns without blocking on the initial (potentially huge)
/// directory walk.
fn run_hub_thread(inner: Arc<HubInner>, root: PathBuf) {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<Event, notify::Error>>(CHANNEL_BOUND);

    let callback_inner = Arc::clone(&inner);
    let watcher = RecommendedWatcher::new(
        move |res| {
            // Never block the notify thread: drop-and-flag on a full channel and
            // let the hub thread fold that into a reconcile.
            if tx.try_send(res).is_err() {
                callback_inner.channel_overflow.store(true, Ordering::Relaxed);
            }
        },
        NotifyConfig::default(),
    );

    let mut watcher = match watcher {
        Ok(watcher) => watcher,
        Err(error) => {
            tracing::warn!(?root, "workspace change hub failed to create watcher: {error}");
            inner.mark_setup_failed();
            return;
        }
    };

    if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
        tracing::warn!(?root, "workspace change hub failed to watch root: {error}");
        inner.mark_setup_failed();
        return;
    }
    inner.mark_watching();
    tracing::info!(?root, "workspace change hub watching");

    for res in rx {
        inner.drain_channel_overflow();
        for dir in inner.ingest_event(res) {
            if let Err(error) = watcher.watch(&dir, RecursiveMode::Recursive) {
                inner.note_rewatch_failed(&dir, &error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, EventKind, ModifyKind, RemoveKind};
    use tempfile::tempdir;

    fn change_event(kind: EventKind, path: PathBuf) -> Result<Event, notify::Error> {
        Ok(Event { kind, paths: vec![path], attrs: Default::default() })
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
        let hub = WorkspaceChangeHub::start(PathBuf::from("/definitely/not/a/real/path/xyzzy"));
        assert!(!hub.wait_until_watching(Duration::from_secs(5)));
        assert_eq!(hub.health(), Health::Degraded(DegradeReason::WatcherSetup));
        assert!(!hub.is_watching());
    }

    #[test]
    fn start_arms_the_watch_asynchronously() {
        let dir = tempdir().unwrap();
        // `start` returns immediately; the watch arms on the hub thread. Poll for it
        // rather than asserting synchronously.
        let hub = WorkspaceChangeHub::start(dir.path().to_path_buf());
        assert!(hub.wait_until_watching(Duration::from_secs(5)), "setup settles to watching");
        assert!(hub.is_watching());
        assert_eq!(hub.health(), Health::Healthy);
    }

    #[test]
    fn health_flips_on_unknown_event_kind() {
        let dir = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(dir.path().to_path_buf());
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        assert_eq!(hub.health(), Health::Healthy);
        hub.ingest_for_test(change_event(EventKind::Other, dir.path().join("x")));
        assert_eq!(hub.health(), Health::Degraded(DegradeReason::UnknownEvent));
    }

    #[test]
    fn runtime_error_flips_health() {
        let dir = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(dir.path().to_path_buf());
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        hub.ingest_for_test(Err(notify::Error::generic("boom")));
        assert_eq!(hub.health(), Health::Degraded(DegradeReason::RuntimeError));
    }

    #[test]
    fn rewatch_failure_degrades_and_recovers_through_rescan() {
        let dir = tempdir().unwrap();
        let hub = WorkspaceChangeHub::start(dir.path().to_path_buf());
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
        let hub = WorkspaceChangeHub::start(dir.path().to_path_buf());
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

        let hub = WorkspaceChangeHub::start(dir.path().to_path_buf());
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
        let hub = WorkspaceChangeHub::start(dir.path().to_path_buf());
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
        let hub = WorkspaceChangeHub::start(dir.path().to_path_buf());
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
}
