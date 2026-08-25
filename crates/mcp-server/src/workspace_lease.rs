//! Which daemon owns a workspace's derived caches.
//!
//! A workspace's `.build` directory holds caches derived from the same sources — the call
//! graph and the code-search index — but the daemon that maintains them is not unique.
//! [`BackendKey`](crate::broker::BackendKey) forks a fresh backend on a binary upgrade, an
//! embedding-config change, or an extension-topology edit, and the superseded daemon lives on
//! until its idle TTL (indefinitely, while a client stays connected). Both processes then
//! rebuild and atomically rename the same graph database, and both re-render the same search
//! contexts: convergent, but wasteful, and each publish flickers the generation the other's
//! clients see.
//!
//! The lease makes that ownership explicit and single. Every daemon claims it at startup under
//! a file lock, taking a generation one above whatever it found, so the newest process owns the
//! workspace and the ones it superseded stop writing derived caches — the right way round,
//! since a client whose config or binary just changed is served by the newest daemon while the
//! older ones drain. Ownership by claim order is enough: a daemon that outlives a config edit
//! rebuilds against the live configuration like any other, so the generations do not disagree
//! about what the workspace *is*, only about who writes it down.
//!
//! Losing the lease is not a failure. A superseded daemon keeps serving everything it already
//! holds — its resident host, its published graph snapshot, its search index — and simply
//! stops producing new derived state. Once its last session leaves it exits immediately
//! instead of idling out, because a warm backend that may not write is worth little.
//!
//! What the lease deliberately does NOT gate is the search index's lexical side: chunks and
//! FTS text. Both generations derive those from the same files, SQLite serializes the writes
//! under WAL, and mark stamps come from the database itself (`bsl_search::Store`), so
//! duplicating them costs work rather than correctness — the one field whose meaning depends on
//! the graph, a chunk's rendered context, is covered by the topology check every graph reader
//! applies before adopting a file it did not write. Embeddings ARE gated: a vector is stored as
//! a bare blob against a chunk id, with no record of the model behind it, and the embedding
//! configuration is one of the axes that forks a generation in the first place.

use std::fs::{File, OpenOptions};
use std::io;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Lease record file name, next to the caches it governs.
#[cfg(test)]
const LEASE_FILE: &str = "writer.lease";
/// The file locked for the read-modify-write of a claim. Separate from the record so the
/// record itself is only ever replaced by an atomic rename and readers need no lock.
use crate::cache::LEASE_LOCK_FILE;

/// How often the owner restamps its record so the others can tell it is still alive.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
/// A record older than this is treated as abandoned, and a daemon that has not observed a live
/// foreign owner may take the workspace. Comfortably above [`HEARTBEAT_INTERVAL`] so a loaded
/// machine cannot make a live owner look dead.
const STALE_AFTER: Duration = Duration::from_secs(60);
/// How long a cached ownership verdict is reused before the record is read again. Every gated
/// write path consults the lease, so this keeps the check off the syscall path without letting
/// a demotion go unnoticed for long.
pub(crate) const VERDICT_TTL: Duration = Duration::from_secs(2);
/// How long a claim waits for the lock file. The critical section is a read and one small
/// write, so anything beyond this means a peer wedged holding the lock — give up on this
/// attempt (the caller retries at its next check) rather than block a daemon's startup on it.
const LOCK_WAIT: Duration = Duration::from_secs(2);

/// Generation of a lease that has not (yet) written a record: it owns nothing, and every
/// ownership check retries the claim.
const UNCLAIMED: u64 = 0;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LeaseOutcome<T> {
    Applied(T),
    TransientRefusal,
    Terminal,
}

/// The on-disk record: who owns the workspace's derived caches, and since when.
#[derive(Serialize, Deserialize)]
struct LeaseRecord {
    /// Which claim this is. Ordering only — it decides who outbids whom, never who a record
    /// belongs to: generations restart from 1 whenever the record is deleted, so the same
    /// number can name two different daemons.
    generation: u64,
    /// WHO holds the workspace. Unique per claim (see [`new_token`]), which is what makes the
    /// identity check sound where the generation is not: after a `.build` wipe a daemon
    /// reclaiming as generation 1 must not be mistaken for the live generation-1 daemon it
    /// superseded, or both would own the workspace for good.
    token: u64,
    /// The owner's process id. Diagnostics only — liveness is decided by the heartbeat, which
    /// needs no cross-platform process introspection and is immune to pid reuse.
    pid: u32,
    /// Unix seconds of the owner's last heartbeat.
    heartbeat_secs: u64,
}

/// An identity for one claim: a 64-bit digest of this process's id, the instant of the claim,
/// and a per-process counter — so two claims differ even within one daemon in one nanosecond.
/// Distinctness is probabilistic in the digest, which at a handful of claims per workspace is a
/// collision chance no one will meet; what it must not be is DERIVABLE, as the generation is,
/// since that is what let two daemons read one record as both of theirs.
fn new_token() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos =
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) as u64;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&std::process::id().to_le_bytes());
    hasher.update(&nanos.to_le_bytes());
    hasher.update(&NEXT.fetch_add(1, Ordering::SeqCst).to_le_bytes());
    u64::from_le_bytes(hasher.finalize().as_bytes()[..8].try_into().expect("blake3 yields 32"))
}

/// A daemon's claim on one workspace's derived caches. Cheap to clone (every holder shares one
/// verdict cache), and safe to consult from any thread.
#[derive(Clone)]
pub(crate) struct WorkspaceLease {
    inner: Arc<Inner>,
}

struct Inner {
    /// `None` for a lease that governs nothing (no workspace, or the claim could not be
    /// written). Such a lease always reports ownership: coordination is best-effort, and
    /// failing closed would silently stop a lone daemon from maintaining its own caches.
    path: Option<PathBuf>,
    generation: AtomicU64,
    /// The token this daemon last wrote into the record; `0` while unclaimed.
    token: AtomicU64,
    owns: AtomicBool,
    /// Set permanently after this lease, having owned the workspace, observes a live foreign
    /// token. All clones share the verdict and never attempt to reclaim after it is set.
    superseded: AtomicBool,
    /// Set by [`WorkspaceLease::release`] and never cleared: this process is going away, so it
    /// must not take the workspace back — a background pass still finishing during shutdown
    /// would otherwise see the record it just removed as "nobody owns this" and re-claim it.
    released: AtomicBool,
    checked_at: Mutex<Option<Instant>>,
}

impl WorkspaceLease {
    /// Claim `workspace_root`'s derived caches for this process, taking the generation above
    /// whatever the last owner recorded. An unwritable or locked-out `.build` yields an
    /// unmanaged lease (see [`Inner::path`]) rather than an error: the daemon still works, it
    /// just cannot coordinate with a peer.
    #[cfg(test)]
    pub(crate) fn claim(workspace_root: &Path) -> Self {
        let cache = crate::cache::WorkspaceCacheLayout::for_workspace(workspace_root);
        Self::claim_cache(&cache)
    }

    #[cfg(test)]
    pub(crate) fn while_cache_lock_held<T>(
        cache: &crate::cache::WorkspaceCacheLayout,
        run: impl FnOnce() -> T,
    ) -> T {
        cache.ensure().unwrap();
        let _guard = LockGuard::acquire(&cache.lease_lock_path(), LOCK_WAIT).unwrap();
        run()
    }

    #[cfg(test)]
    pub(crate) fn hold_cache_lock_for(
        cache: &crate::cache::WorkspaceCacheLayout,
        duration: Duration,
    ) -> std::thread::JoinHandle<()> {
        cache.ensure().unwrap();
        let path = cache.lease_lock_path();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let _guard = LockGuard::acquire(&path, LOCK_WAIT).unwrap();
            ready_tx.send(()).unwrap();
            std::thread::sleep(duration);
        });
        ready_rx.recv().unwrap();
        handle
    }

    /// Claim the derived caches rooted at `cache` for this process.
    pub(crate) fn claim_cache(cache: &crate::cache::WorkspaceCacheLayout) -> Self {
        match Self::try_claim_cache(cache) {
            Ok(lease) => lease,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    root = %cache.root().display(),
                    "could not claim the workspace cache lease; this daemon will not coordinate \
                     with another generation over the same caches"
                );
                Self::unmanaged()
            }
        }
    }

    /// A lease over nothing: reference profiles, tests, and every path with no workspace
    /// directory to coordinate through.
    pub(crate) fn unmanaged() -> Self {
        Self {
            inner: Arc::new(Inner {
                path: None,
                generation: AtomicU64::new(0),
                token: AtomicU64::new(0),
                owns: AtomicBool::new(true),
                superseded: AtomicBool::new(false),
                released: AtomicBool::new(false),
                checked_at: Mutex::new(None),
            }),
        }
    }

    fn try_claim_cache(cache: &crate::cache::WorkspaceCacheLayout) -> io::Result<Self> {
        // The directory is the one thing a lease cannot do without. Everything past it — the
        // lock, the record write — is retried later by `owns_caches`, so a moment's contention
        // does not cost this daemon its place in the coordination for good.
        cache.ensure()?;
        let path = cache.lease_path();
        let inner = Arc::new(Inner {
            path: Some(path),
            generation: AtomicU64::new(UNCLAIMED),
            token: AtomicU64::new(0),
            owns: AtomicBool::new(false),
            superseded: AtomicBool::new(false),
            released: AtomicBool::new(false),
            checked_at: Mutex::new(None),
        });
        let lease = Self { inner };
        // A starting daemon outbids whatever it finds — newest wins is the whole rule.
        if !lease.take_generation(|_| true) {
            tracing::warn!(
                root = %cache.root().display(),
                "workspace cache lease is locked by a peer; retrying on the next check"
            );
        }
        spawn_heartbeat(Arc::downgrade(&lease.inner));
        Ok(lease)
    }

    /// Take the generation above whatever the record holds, under the lock — but only when
    /// `claimable` accepts the record found THERE, not the one that was read outside it. A
    /// startup claim accepts anything; a reclaim accepts only a workspace that is still free
    /// (no record, or one whose owner stopped reporting), so a daemon that claimed while we
    /// waited for the lock is not outbid on the strength of an observation that has expired.
    ///
    /// `false` when the lock, the predicate, or the write did not go through — the caller stays
    /// as it was (an unclaimed lease owns nothing) and tries again at its next check.
    fn take_generation(&self, claimable: impl Fn(Option<&LeaseRecord>) -> bool) -> bool {
        let mut checked_at = lock_recover(&self.inner.checked_at);
        self.take_generation_locked(&mut checked_at, claimable)
    }

    /// [`Self::take_generation`] with the process-local lifecycle lock already held.
    fn take_generation_locked(
        &self,
        checked_at: &mut Option<Instant>,
        claimable: impl Fn(Option<&LeaseRecord>) -> bool,
    ) -> bool {
        if self.inner.released.load(Ordering::SeqCst)
            || self.inner.superseded.load(Ordering::SeqCst)
        {
            return false;
        }
        let Some(path) = self.inner.path.as_deref() else {
            return false;
        };
        let Some(dir) = path.parent() else { return false };
        // Recreated, not just used: `.build` is a cache directory users are told they may
        // delete, and a daemon that could not put the lock file back would report non-ownership
        // for the rest of its life — every live daemon stuck read-only over a workspace nobody
        // owns, with no way to recreate what they are all waiting for.
        if std::fs::create_dir_all(dir).is_err() {
            return false;
        }
        let Ok(_guard) = LockGuard::acquire(&dir.join(LEASE_LOCK_FILE), LOCK_WAIT) else {
            return false;
        };
        // Re-read UNDER the lock, where `release` also runs: a claim that started before this
        // process decided to leave must not complete afterwards. It would put a record on disk
        // that this daemon will never heartbeat (the release stops that) and never remove (the
        // release already ran) — a live-looking claim on a workspace nobody is maintaining,
        // blocking every other daemon until it goes stale.
        if self.inner.released.load(Ordering::SeqCst) {
            return false;
        }
        let found = read_record(path);
        if !claimable(found.as_ref()) {
            return false;
        }
        let generation = found.map(|r| r.generation).unwrap_or(0) + 1;
        let token = new_token();
        if write_record(path, generation, token).is_err() {
            return false;
        }
        self.inner.generation.store(generation, Ordering::SeqCst);
        self.inner.token.store(token, Ordering::SeqCst);
        self.inner.owns.store(true, Ordering::SeqCst);
        *checked_at = Some(Instant::now());
        tracing::info!(
            generation,
            path = %path.display(),
            "claimed the workspace derived-cache lease"
        );
        true
    }

    /// Whether this daemon may write the workspace's derived caches. The verdict is cached for
    /// [`VERDICT_TTL`], so gating a write path on it costs an atomic load in the common case.
    pub(crate) fn owns_caches(&self) -> bool {
        if self.inner.released.load(Ordering::SeqCst)
            || self.inner.superseded.load(Ordering::SeqCst)
        {
            return false;
        }
        let Some(path) = self.inner.path.as_deref() else {
            return true;
        };
        let mut checked_at = lock_recover(&self.inner.checked_at);
        if self.inner.released.load(Ordering::SeqCst)
            || self.inner.superseded.load(Ordering::SeqCst)
        {
            return false;
        }
        match *checked_at {
            Some(at) if at.elapsed() < VERDICT_TTL => {
                return self.inner.owns.load(Ordering::SeqCst)
            }
            _ => *checked_at = Some(Instant::now()),
        }
        // A claim that could not be written at startup is retried here rather than leaving this
        // daemon permanently outside the coordination — which, since an unclaimed lease never
        // owns anything, would otherwise mean it never maintains the caches at all.
        if self.inner.generation.load(Ordering::SeqCst) == UNCLAIMED {
            return self.take_generation_locked(&mut checked_at, |_| true);
        }
        let owns = self.recheck_locked(path, &mut checked_at);
        self.inner.owns.store(owns, Ordering::SeqCst);
        owns
    }

    /// Ownership as of NOW, bypassing the cached verdict.
    ///
    /// For a caller whose next act writes something a takeover would poison, where up to
    /// [`VERDICT_TTL`] of stale "yes" is too generous — the embedding pass, which persists a
    /// vector per batch. It costs one small read, so it belongs on paths that run per batch,
    /// not per query. This narrows the window; it does not fence it (only
    /// [`Self::with_ownership`] does), which is the right trade where the write is a vector
    /// that a re-embed can replace rather than a rename that destroys another daemon's build.
    pub(crate) fn owns_caches_now(&self) -> bool {
        if self.inner.released.load(Ordering::SeqCst)
            || self.inner.superseded.load(Ordering::SeqCst)
        {
            return false;
        }
        let Some(path) = self.inner.path.as_deref() else {
            return true;
        };
        let mut checked_at = lock_recover(&self.inner.checked_at);
        if self.inner.released.load(Ordering::SeqCst)
            || self.inner.superseded.load(Ordering::SeqCst)
        {
            return false;
        }
        *checked_at = Some(Instant::now());
        if self.inner.generation.load(Ordering::SeqCst) == UNCLAIMED {
            return self.take_generation_locked(&mut checked_at, |_| true);
        }
        let owns = self.recheck_locked(path, &mut checked_at);
        self.inner.owns.store(owns, Ordering::SeqCst);
        owns
    }

    /// Run `write` with ownership held for its whole duration and preserve why admission failed.
    ///
    /// A cached verdict (or even a fresh read) only says we owned the workspace an instant ago;
    /// a claim landing between the check and the write would leave two daemons publishing. This
    /// holds the same lock a claim takes, so a peer's claim either completes before this write
    /// begins or waits until it is done — the fence a rename into the shared path needs.
    pub(crate) fn with_ownership_outcome<T>(&self, write: impl FnOnce() -> T) -> LeaseOutcome<T> {
        self.with_ownership_checkpointed(|_| ControlFlow::Continue(write()))
    }

    /// The same fence with a heartbeat/termination checkpoint for an indivisible transaction.
    /// A callback that observes `Break` rolls its work back and returns `Break`; the primitive
    /// then classifies that stop as terminal while the fence still holds.
    pub(crate) fn with_ownership_checkpointed<T>(
        &self,
        write: impl FnOnce(&mut dyn FnMut() -> ControlFlow<()>) -> ControlFlow<(), T>,
    ) -> LeaseOutcome<T> {
        if self.inner.released.load(Ordering::SeqCst)
            || self.inner.superseded.load(Ordering::SeqCst)
        {
            return LeaseOutcome::Terminal;
        }
        let Some(path) = self.inner.path.as_deref() else {
            let mut checkpoint = || {
                if self.inner.released.load(Ordering::SeqCst) {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            };
            return match write(&mut checkpoint) {
                ControlFlow::Continue(value) => LeaseOutcome::Applied(value),
                ControlFlow::Break(()) => LeaseOutcome::Terminal,
            };
        };
        let mut checked_at = lock_recover(&self.inner.checked_at);
        if self.inner.released.load(Ordering::SeqCst)
            || self.inner.superseded.load(Ordering::SeqCst)
        {
            return LeaseOutcome::Terminal;
        }
        let Some(dir) = path.parent() else { return LeaseOutcome::TransientRefusal };
        let Ok(_guard) = LockGuard::acquire(&dir.join(LEASE_LOCK_FILE), LOCK_WAIT) else {
            return LeaseOutcome::TransientRefusal;
        };
        if self.inner.generation.load(Ordering::SeqCst) == UNCLAIMED {
            return LeaseOutcome::TransientRefusal;
        }
        let mine = self.inner.token.load(Ordering::SeqCst);
        // Exactly ours, or not ours: a missing record is not an invitation to write. Restoring
        // our own claim here would let a superseded daemon publish over the owner's build
        // whenever `.build` was cleared. Re-claiming an unowned workspace is [`Self::recheck`]'s
        // job, where the loser of that race learns it lost.
        match read_record(path) {
            Some(record) if record.token == mine => {
                let mut checkpoint = || {
                    if self.inner.released.load(Ordering::SeqCst)
                        || self.inner.superseded.load(Ordering::SeqCst)
                    {
                        return ControlFlow::Break(());
                    }
                    let _ = write_record(
                        path,
                        self.inner.generation.load(Ordering::SeqCst),
                        self.inner.token.load(Ordering::SeqCst),
                    );
                    ControlFlow::Continue(())
                };
                match write(&mut checkpoint) {
                    ControlFlow::Continue(value) => LeaseOutcome::Applied(value),
                    ControlFlow::Break(()) => LeaseOutcome::Terminal,
                }
            }
            Some(record) if !is_stale(&record) => {
                self.latch_superseded(&record);
                self.inner.owns.store(false, Ordering::SeqCst);
                *checked_at = Some(Instant::now());
                LeaseOutcome::Terminal
            }
            _ => {
                self.inner.owns.store(false, Ordering::SeqCst);
                *checked_at = Some(Instant::now());
                LeaseOutcome::TransientRefusal
            }
        }
    }

    pub(crate) fn is_superseded(&self) -> bool {
        self.inner.superseded.load(Ordering::SeqCst)
    }

    pub(crate) fn is_released(&self) -> bool {
        self.inner.released.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn hold_file_lock_for_test(&self) -> impl Send {
        let path = self.inner.path.as_deref().expect("test lease is managed");
        LockGuard::acquire(
            &path.parent().expect("lease path has a parent").join(LEASE_LOCK_FILE),
            LOCK_WAIT,
        )
        .expect("test acquires lease file lock")
    }

    #[cfg(test)]
    pub(crate) fn invalidate_verdict_for_test(&self) {
        *lock_recover(&self.inner.checked_at) = None;
    }

    /// Release this process's record on a clean exit.
    ///
    /// Ownership survives a crash by design — the heartbeat is what tells the others the owner
    /// is gone, and it takes [`STALE_AFTER`] to conclude that. A process that exits on purpose
    /// knows better and says so, so a fresh process can claim without waiting that window out.
    /// A previously superseded lease remains terminal. Only OUR record is removed: a generation
    /// that took the workspace over in the meantime keeps it.
    pub(crate) fn release(&self) {
        self.inner.released.store(true, Ordering::SeqCst);
        let mut checked_at = lock_recover(&self.inner.checked_at);
        self.inner.owns.store(false, Ordering::SeqCst);
        *checked_at = Some(Instant::now());
        let Some(path) = self.inner.path.as_deref() else {
            return;
        };
        // Under the lock from the start, because a claim may be in flight on another thread and
        // the two must not interleave: taking the lock first means either the claim completes
        // and this removes the record it just wrote, or this runs first and the claim finds the
        // `released` flag and abandons. Either way no record is left behind that nobody owns.
        // The flag itself is set unconditionally — "this process is leaving" holds whether or
        // not the workspace was still ours, and a demoted lease that skipped it could reclaim
        // an abandoned workspace during shutdown and start writing caches on the way out.
        let guard = path
            .parent()
            .and_then(|dir| LockGuard::acquire(&dir.join(LEASE_LOCK_FILE), LOCK_WAIT).ok());
        let Some(_guard) = guard else { return };
        let mine = self.inner.token.load(Ordering::SeqCst);
        if read_record(path).is_some_and(|record| record.token == mine) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// This daemon's ownership generation; `None` when unmanaged. The generation is a
    /// coordination detail rather than an agent-facing fact — what a client needs to know is
    /// whether the backend still maintains the caches, which `owns_caches` answers — so it is
    /// carried in the claim log line and asserted here.
    #[cfg(test)]
    fn generation(&self) -> Option<u64> {
        self.inner.path.as_ref().map(|_| self.inner.generation.load(Ordering::SeqCst))
    }

    /// Re-read the record and decide ownership.
    ///
    /// The record IS the ownership: this daemon owns the workspace exactly while the record
    /// names its generation. Any other live record — higher OR lower — belongs to somebody
    /// else, and comparing numbers instead would break the moment `.build` is cleared: an
    /// older daemon would restore its own lower generation, the newer one would read that as
    /// "below mine, so still mine", and both would write the caches. A record whose owner
    /// stopped reporting, or none at all, means the workspace is free: claim it afresh under
    /// the lock, where two daemons doing the same thing get distinct generations and the loser
    /// demotes at its next check.
    fn recheck_locked(&self, path: &Path, checked_at: &mut Option<Instant>) -> bool {
        let mine = self.inner.token.load(Ordering::SeqCst);
        match read_record(path) {
            Some(record) if record.token == mine => true,
            Some(record) if !is_stale(&record) => {
                self.latch_superseded(&record);
                false
            }
            found => {
                let abandoned = found.map(|r| r.generation);
                let claimed = self.take_generation_locked(checked_at, |under_lock| {
                    under_lock.is_none_or(is_stale) // still free once we hold the lock
                });
                if claimed {
                    tracing::info!(
                        generation = self.inner.generation.load(Ordering::SeqCst),
                        abandoned,
                        "this workspace's derived caches were left unowned; claiming them"
                    );
                }
                claimed
            }
        }
    }

    fn latch_superseded(&self, owner: &LeaseRecord) {
        if self.inner.generation.load(Ordering::SeqCst) == UNCLAIMED {
            return;
        }
        if !self.inner.superseded.swap(true, Ordering::SeqCst) {
            tracing::info!(
                mine = self.inner.generation.load(Ordering::SeqCst),
                owner = owner.generation,
                owner_pid = owner.pid,
                "another daemon generation now owns this workspace's derived caches; this one \
                 is permanently superseded"
            );
        }
    }
}

/// Restamp the owner's record for as long as this process holds the lease. Ownership must not
/// depend on a daemon happening to write something, so this runs on its own thread rather than
/// off the gated paths; it holds a [`Weak`], so the thread ends with the state that owns the
/// lease.
fn spawn_heartbeat(inner: Weak<Inner>) {
    let spawned =
        std::thread::Builder::new().name("bsl-cache-lease".to_owned()).spawn(move || loop {
            // Sleep in slices so the thread notices a dropped lease promptly instead of
            // outliving a test's temporary directory by a whole interval.
            let mut waited = Duration::ZERO;
            while waited < HEARTBEAT_INTERVAL {
                std::thread::sleep(Duration::from_secs(1));
                waited += Duration::from_secs(1);
                if inner.upgrade().is_none() {
                    return;
                }
            }
            let Some(inner) = inner.upgrade() else { return };
            if inner.released.load(Ordering::SeqCst) {
                return;
            }
            let lease = WorkspaceLease { inner };
            // Re-deciding ownership here, not just when a write path asks, promptly latches a
            // live foreign owner even while this daemon only serves reads.
            if !lease.owns_caches() {
                continue;
            }
            let Some(path) = lease.inner.path.as_deref() else { continue };
            // Restamped under the lock, and only while the record is still OURS. An
            // unconditional write would put our generation back over a newer daemon's claim,
            // and both processes would then read the record as their own — two owners, each
            // convinced it is the only one.
            // Read INSIDE the fence, never captured before it: no claim can run while the
            // fence holds the lock, so what the closure reads is exactly the identity the
            // fence validated. Values captured beforehand could be a generation a concurrent
            // reclaim has already replaced — stamping them back would leave a record on disk
            // that no live lease recognises as its own.
            let _ = lease.with_ownership_outcome(|| {
                write_record(
                    path,
                    lease.inner.generation.load(Ordering::SeqCst),
                    lease.inner.token.load(Ordering::SeqCst),
                )
            });
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "could not start the cache-lease heartbeat");
    }
}

fn read_record(path: &Path) -> Option<LeaseRecord> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Replace the record atomically: a reader takes no lock, so it must never observe a
/// half-written file. The temp name carries the pid so two writers cannot share one.
fn write_record(path: &Path, generation: u64, token: u64) -> io::Result<()> {
    let record =
        LeaseRecord { generation, token, pid: std::process::id(), heartbeat_secs: now_secs() };
    let body = serde_json::to_string(&record)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, body)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn is_stale(record: &LeaseRecord) -> bool {
    now_secs().saturating_sub(record.heartbeat_secs) > STALE_AFTER.as_secs()
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// An advisory cross-process lock held for the duration of a claim, released when dropped
/// (including on a crash, since both platforms release on handle close).
struct LockGuard {
    _file: File,
}

impl LockGuard {
    /// Take the lock, retrying until `budget` runs out. Both platforms poll rather than block
    /// so a wedged peer degrades a claim into an unmanaged lease instead of hanging startup.
    fn acquire(path: &Path, budget: Duration) -> io::Result<Self> {
        let deadline = Instant::now() + budget;
        loop {
            match Self::try_acquire(path) {
                Ok(guard) => return Ok(guard),
                Err(e) if Instant::now() >= deadline => return Err(e),
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }

    #[cfg(unix)]
    fn try_acquire(path: &Path) -> io::Result<Self> {
        use std::os::unix::io::AsRawFd;

        let file =
            OpenOptions::new().create(true).read(true).write(true).truncate(false).open(path)?;
        // SAFETY: `flock` takes a live descriptor and a flag word; `file` owns the descriptor
        // for the whole call and the lock is released when it closes.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { _file: file })
    }

    /// Windows has no `flock`, but an open with an empty share mode is exclusive by itself:
    /// a second opener fails with a sharing violation until the first handle closes.
    #[cfg(windows)]
    fn try_acquire(path: &Path) -> io::Result<Self> {
        use std::os::windows::fs::OpenOptionsExt;

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .share_mode(0)
            .open(path)?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_cache_layout_holds_lease_outside_workspace() {
        let workspace = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let layout = crate::cache::WorkspaceCacheLayout::from_root(cache.path().to_path_buf());

        let lease = WorkspaceLease::claim_cache(&layout);

        assert!(lease.owns_caches());
        assert!(layout.lease_path().exists());
        assert!(!workspace.path().join(".build").exists());
        lease.release();
    }

    fn record_at(path: &Path) -> LeaseRecord {
        read_record(path).expect("the lease record is readable")
    }

    fn lease_path(root: &Path) -> PathBuf {
        crate::cache::workspace_cache_dir(root).join(LEASE_FILE)
    }

    /// The newest claim owns the workspace: the daemon that started first stops writing derived
    /// caches, the one that started last keeps writing. Reverse the comparison in `recheck` and
    /// the draining generation would keep ownership while the client-facing one goes read-only.
    #[test]
    fn the_newest_claim_owns_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let first = WorkspaceLease::claim(dir.path());
        assert!(first.owns_caches(), "the only daemon owns the caches");

        let second = WorkspaceLease::claim(dir.path());
        assert_eq!(second.generation(), Some(first.generation().unwrap() + 1));
        assert!(second.owns_caches(), "the newest claim owns the caches");

        // The verdict is cached, so the demotion lands on the first re-read.
        std::thread::sleep(VERDICT_TTL);
        assert!(!first.owns_caches(), "the superseded generation stops writing");
        assert!(second.owns_caches());
    }

    /// The fence a long build's publish needs: ownership must hold FOR the write, not merely
    /// before it. A daemon whose record has been outbid runs nothing, however recently its
    /// cached verdict said otherwise — otherwise a build that started while it owned the
    /// workspace would rename itself over what the new owner had just published.
    #[test]
    fn publish_fence_latches_supersession() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WorkspaceLease::claim(dir.path());
        assert_eq!(lease.with_ownership_outcome(|| "written"), LeaseOutcome::Applied("written"));

        let newer = LeaseRecord {
            generation: lease.generation().unwrap() + 1,
            token: new_token(),
            pid: 424242,
            heartbeat_secs: now_secs(),
        };
        std::fs::write(lease_path(dir.path()), serde_json::to_string(&newer).unwrap()).unwrap();

        // No sleep: the fence reads the record itself rather than trusting the cached verdict,
        // which still says this daemon owns the workspace.
        assert!(lease.owns_caches(), "the cached verdict has not expired yet");
        assert_eq!(
            lease.with_ownership_outcome(|| "written"),
            LeaseOutcome::Terminal,
            "the write is refused anyway"
        );
        assert!(lease.is_superseded(), "the live foreign token is terminal");
    }

    #[test]
    fn typed_fence_distinguishes_transient_and_terminal_refusals() {
        let busy_dir = tempfile::tempdir().unwrap();
        let busy = WorkspaceLease::claim(busy_dir.path());
        let held = busy.hold_file_lock_for_test();
        assert_eq!(busy.with_ownership_outcome(|| "written"), LeaseOutcome::TransientRefusal);
        drop(held);

        let unclaimed_dir = tempfile::tempdir().unwrap();
        let cache = crate::cache::WorkspaceCacheLayout::for_workspace(unclaimed_dir.path());
        cache.ensure().unwrap();
        let unclaimed = WorkspaceLease {
            inner: Arc::new(Inner {
                path: Some(cache.lease_path()),
                generation: AtomicU64::new(UNCLAIMED),
                token: AtomicU64::new(0),
                owns: AtomicBool::new(false),
                superseded: AtomicBool::new(false),
                released: AtomicBool::new(false),
                checked_at: Mutex::new(None),
            }),
        };
        assert_eq!(unclaimed.with_ownership_outcome(|| "written"), LeaseOutcome::TransientRefusal);

        let foreign_dir = tempfile::tempdir().unwrap();
        let foreign = WorkspaceLease::claim(foreign_dir.path());
        let newer = LeaseRecord {
            generation: foreign.generation().unwrap() + 1,
            token: new_token(),
            pid: 424242,
            heartbeat_secs: now_secs(),
        };
        std::fs::write(lease_path(foreign_dir.path()), serde_json::to_string(&newer).unwrap())
            .unwrap();
        assert_eq!(foreign.with_ownership_outcome(|| "written"), LeaseOutcome::Terminal);
        assert!(foreign.is_superseded());

        let released_dir = tempfile::tempdir().unwrap();
        let released = WorkspaceLease::claim(released_dir.path());
        released.release();
        assert_eq!(released.with_ownership_outcome(|| "written"), LeaseOutcome::Terminal);
    }

    #[test]
    fn checkpoint_refreshes_heartbeat_and_preserves_callback_errors() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WorkspaceLease::claim(dir.path());
        let path = lease_path(dir.path());
        let record = LeaseRecord {
            generation: lease.generation().unwrap(),
            token: lease.inner.token.load(Ordering::SeqCst),
            pid: std::process::id(),
            heartbeat_secs: 0,
        };
        std::fs::write(&path, serde_json::to_string(&record).unwrap()).unwrap();

        let outcome: LeaseOutcome<Result<(), &'static str>> =
            lease.with_ownership_checkpointed(|checkpoint| {
                assert_eq!(checkpoint(), ControlFlow::Continue(()));
                assert!(record_at(&path).heartbeat_secs > 0);
                ControlFlow::Continue(Err("store failed"))
            });

        assert_eq!(outcome, LeaseOutcome::Applied(Err("store failed")));
    }

    #[test]
    fn release_during_checkpointed_callback_rolls_back_as_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WorkspaceLease::claim(dir.path());
        let worker_lease = lease.clone();
        let releaser_lease = lease.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (check_tx, check_rx) = std::sync::mpsc::channel();
        let rolled_back = Arc::new(AtomicBool::new(false));
        let worker_rolled_back = Arc::clone(&rolled_back);

        let worker = std::thread::spawn(move || {
            worker_lease.with_ownership_checkpointed(|checkpoint| {
                let mut transaction = vec!["pending"];
                entered_tx.send(()).unwrap();
                check_rx.recv().unwrap();
                if checkpoint().is_break() {
                    transaction.clear();
                    worker_rolled_back.store(true, Ordering::SeqCst);
                    return ControlFlow::Break(());
                }
                ControlFlow::Continue(transaction)
            })
        });
        entered_rx.recv().unwrap();
        let releaser = std::thread::spawn(move || releaser_lease.release());
        while !lease.is_released() {
            std::thread::yield_now();
        }
        check_tx.send(()).unwrap();

        assert_eq!(worker.join().unwrap(), LeaseOutcome::Terminal);
        assert!(rolled_back.load(Ordering::SeqCst));
        releaser.join().unwrap();
    }

    #[test]
    fn release_waits_for_only_the_admitted_batch_and_refuses_the_next() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WorkspaceLease::claim(dir.path());
        let worker_lease = lease.clone();
        let releaser_lease = lease.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let (released_tx, released_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            worker_lease.with_ownership_outcome(|| {
                entered_tx.send(()).unwrap();
                finish_rx.recv().unwrap();
                "first batch"
            })
        });
        entered_rx.recv().unwrap();
        let releaser = std::thread::spawn(move || {
            releaser_lease.release();
            released_tx.send(()).unwrap();
        });
        while !lease.is_released() {
            std::thread::yield_now();
        }
        assert!(released_rx.try_recv().is_err(), "release waits for the admitted batch");
        finish_tx.send(()).unwrap();
        assert_eq!(worker.join().unwrap(), LeaseOutcome::Applied("first batch"));
        released_rx.recv().unwrap();
        releaser.join().unwrap();
        let calls = AtomicU64::new(0);
        assert_eq!(
            lease.with_ownership_outcome(|| calls.fetch_add(1, Ordering::SeqCst)),
            LeaseOutcome::Terminal
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn takeover_between_batches_preserves_the_first_and_terminates_the_next() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WorkspaceLease::claim(dir.path());
        let applied = Arc::new(Mutex::new(Vec::new()));
        let first = Arc::clone(&applied);
        assert_eq!(
            lease.with_ownership_outcome(|| first.lock().unwrap().extend(0..64)),
            LeaseOutcome::Applied(())
        );
        let _newer = WorkspaceLease::claim(dir.path());
        let second = Arc::clone(&applied);
        assert_eq!(
            lease.with_ownership_outcome(|| second.lock().unwrap().push(64)),
            LeaseOutcome::Terminal
        );
        assert_eq!(applied.lock().unwrap().len(), 64);
    }

    /// Generations restart at 1 whenever the record is deleted, so the number cannot be the
    /// identity: with three daemons alive and the record wiped, the one that reclaims is
    /// reassigned a generation an older daemon still holds. If ownership compared numbers, both
    /// would recognise the record as their own and write the caches for good — the exact state
    /// the lease exists to prevent. The token is what tells them apart.
    #[test]
    fn a_reused_generation_does_not_make_two_daemons_owners() {
        let dir = tempfile::tempdir().unwrap();
        let first = WorkspaceLease::claim(dir.path()); // generation 1
        let second = WorkspaceLease::claim(dir.path()); // generation 2
        let third = WorkspaceLease::claim(dir.path()); // generation 3
        assert_eq!(first.generation(), Some(1), "the numbering this scenario turns on");

        std::fs::remove_file(lease_path(dir.path())).unwrap();
        std::thread::sleep(VERDICT_TTL);

        // The middle daemon reclaims first and is handed generation 1 — the number the FIRST
        // daemon is still running under.
        assert!(second.owns_caches());
        assert_eq!(second.generation(), Some(1), "the wipe restarted the numbering");

        assert!(!first.owns_caches(), "the generation it shares is not its claim");
        assert!(!third.owns_caches(), "and the newest daemon gave the workspace up too");
        assert_eq!(
            first.with_ownership_outcome(|| "published"),
            LeaseOutcome::Terminal,
            "its writes are refused"
        );
    }

    /// `.build` is a cache directory users are told they may delete, and deleting it takes the
    /// lock file with it. Every live daemon would then fail to claim forever — all read-only
    /// over a workspace nobody owns — unless the claim puts the directory back.
    #[test]
    fn deleting_the_whole_cache_directory_does_not_strand_every_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WorkspaceLease::claim(dir.path());
        std::fs::remove_dir_all(crate::cache::workspace_cache_dir(dir.path())).unwrap();

        std::thread::sleep(VERDICT_TTL);
        assert!(lease.owns_caches(), "the daemon recreates what it needs and carries on");
        assert!(lease_path(dir.path()).exists(), "and the record is back on disk");
    }

    /// A claim that loses the lock must not leave the daemon outside the coordination for good:
    /// an unclaimed lease owns nothing (so it cannot be a second writer), and it keeps trying,
    /// so a moment's contention costs a check interval rather than the daemon's whole life.
    #[test]
    fn transient_unclaimed_is_not_superseded() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = crate::cache::ensure_workspace_cache_dir(dir.path()).unwrap();
        let held = LockGuard::acquire(&cache_dir.join(LEASE_LOCK_FILE), LOCK_WAIT).unwrap();

        let lease = WorkspaceLease::claim(dir.path());
        assert_eq!(lease.generation(), Some(UNCLAIMED), "the claim could not be written");
        assert!(!lease.owns_caches(), "and an unclaimed lease writes nothing");
        assert!(!lease.is_superseded(), "temporary lock contention is not supersession");

        drop(held);
        std::thread::sleep(VERDICT_TTL);
        assert!(lease.owns_caches(), "the retry claims the workspace once the lock frees");
        assert_eq!(record_at(&lease_path(dir.path())).generation, lease.generation().unwrap());
        lease.release();
        assert!(lease.is_released());
        assert!(!lease.is_superseded(), "shutdown remains distinct from supersession");
    }

    /// Once a daemon has actually observed a live foreign owner, that observation is terminal:
    /// neither the owner's clean exit nor a third claim lets the old process write again.
    #[test]
    fn observed_foreign_owner_is_permanent() {
        let dir = tempfile::tempdir().unwrap();
        let daemon = WorkspaceLease::claim(dir.path());
        let short_lived = WorkspaceLease::claim(dir.path());

        std::thread::sleep(VERDICT_TTL);
        assert!(!daemon.owns_caches(), "the newer claim demoted the daemon");
        assert!(daemon.is_superseded());

        short_lived.release();
        let third = WorkspaceLease::claim(dir.path());
        third.release();
        std::fs::remove_dir_all(crate::cache::workspace_cache_dir(dir.path())).unwrap();

        assert!(!daemon.owns_caches(), "a superseded daemon never reclaims");
        assert!(!daemon.owns_caches_now());
        assert_eq!(daemon.with_ownership_outcome(|| "published"), LeaseOutcome::Terminal);
        assert!(!daemon.take_generation(|_| true));
        assert!(!crate::cache::workspace_cache_dir(dir.path()).exists(), "no disk I/O after latch");
    }

    #[test]
    fn observed_owner_race_cannot_reclaim() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WorkspaceLease::claim(dir.path());
        let contender = lease.clone();
        let newer = WorkspaceLease::claim(dir.path());
        let mut lifecycle = lock_recover(&lease.inner.checked_at);
        let owner = read_record(&lease_path(dir.path())).unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let reclaim = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            done_tx.send(contender.take_generation(|_| true)).unwrap();
        });
        started_rx.recv().unwrap();
        assert!(
            done_rx.recv_timeout(Duration::from_millis(20)).is_err(),
            "reclaim waits behind the lifecycle observation"
        );
        lease.latch_superseded(&owner);
        *lifecycle = Some(Instant::now());
        drop(lifecycle);
        newer.release();

        assert!(!done_rx.recv().unwrap(), "a clone cannot race the terminal latch");
        reclaim.join().unwrap();
        assert!(lease.is_superseded());
        assert!(read_record(&lease_path(dir.path())).is_none());
    }

    /// Releasing is final for the process that did it: a background pass still finishing during
    /// shutdown must not read the removed record as "nobody owns this" and claim the workspace
    /// back — the daemon it was handed to would then be silently demoted by a process on its
    /// way out.
    #[test]
    fn a_released_lease_never_takes_the_workspace_back() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WorkspaceLease::claim(dir.path());
        lease.release();

        std::thread::sleep(VERDICT_TTL);
        assert!(!lease.owns_caches(), "a released lease stays released");
        assert!(
            read_record(&lease_path(dir.path())).is_none(),
            "and it does not re-create the record it just handed back",
        );
    }

    /// A claim in flight when the process decides to leave must not complete afterwards. The
    /// record it would write is one this daemon will never heartbeat and never remove — the
    /// release has already run — so every other daemon would treat the workspace as live and
    /// owned until it went stale a minute later.
    #[test]
    fn a_claim_does_not_complete_after_the_process_has_released() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = crate::cache::ensure_workspace_cache_dir(dir.path()).unwrap();
        let held = LockGuard::acquire(&cache_dir.join(LEASE_LOCK_FILE), LOCK_WAIT).unwrap();

        // The claim cannot get the lock, so the lease starts out owning nothing.
        let lease = WorkspaceLease::claim(dir.path());
        assert_eq!(lease.generation(), Some(UNCLAIMED));

        lease.release();
        drop(held);

        std::thread::sleep(VERDICT_TTL);
        assert!(!lease.owns_caches(), "a released lease does not go on to claim");
        assert!(
            read_record(&lease_path(dir.path())).is_none(),
            "and leaves no record for other daemons to wait out",
        );
    }

    /// Releasing must only ever drop OUR record: a daemon that took the workspace over while
    /// we were shutting down keeps it, rather than being silently unclaimed by our exit.
    #[test]
    fn releasing_leaves_a_newer_owners_record_alone() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WorkspaceLease::claim(dir.path());
        let path = lease_path(dir.path());
        let newer = LeaseRecord {
            generation: lease.generation().unwrap() + 1,
            token: new_token(),
            pid: 424242,
            heartbeat_secs: now_secs(),
        };
        std::fs::write(&path, serde_json::to_string(&newer).unwrap()).unwrap();

        lease.release();
        assert_eq!(record_at(&path).generation, newer.generation, "the newer owner still holds it");
    }

    /// A lease over a directory that cannot host a record governs nothing and never blocks its
    /// daemon from maintaining caches — coordination is best-effort, never a kill switch.
    #[test]
    fn an_unclaimable_lease_still_lets_its_daemon_write() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-directory");
        std::fs::write(&file, "").unwrap();

        let lease = WorkspaceLease::claim(&file);
        assert_eq!(lease.generation(), None, "nothing was claimed");
        assert!(lease.owns_caches(), "an unmanaged lease never withholds ownership");
    }

    /// A stale record that was never observed while live is not evidence of supersession. The
    /// current daemon may reclaim it, preserving recovery after crashes and missed brief claims.
    #[test]
    fn non_witness_states_remain_reclaimable() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WorkspaceLease::claim(dir.path());
        let mine = lease.generation().unwrap();

        // A newer daemon claims, then dies: its record survives with a heartbeat that stops.
        let path = lease_path(dir.path());
        let ghost = LeaseRecord {
            generation: mine + 5,
            token: new_token(),
            pid: 424242,
            heartbeat_secs: now_secs() - STALE_AFTER.as_secs() - 1,
        };
        std::fs::write(&path, serde_json::to_string(&ghost).unwrap()).unwrap();

        assert!(lease.owns_caches_now(), "an abandoned workspace is taken back");
        assert!(!lease.is_superseded());
        assert_eq!(lease.generation(), Some(mine + 6), "the reclaim steps above the ghost");
        assert_eq!(record_at(&path).generation, mine + 6);

        let corrupt_dir = tempfile::tempdir().unwrap();
        let corrupt = WorkspaceLease::claim(corrupt_dir.path());
        std::fs::write(lease_path(corrupt_dir.path()), "not a lease record").unwrap();
        assert!(corrupt.owns_caches_now(), "a corrupt record remains recoverable");
        assert!(!corrupt.is_superseded());

        let brief_dir = tempfile::tempdir().unwrap();
        let incumbent = WorkspaceLease::claim(brief_dir.path());
        let brief = WorkspaceLease::claim(brief_dir.path());
        brief.release();
        assert!(incumbent.owns_caches_now(), "an unobserved brief claim leaves no terminal proof");
        assert!(!incumbent.is_superseded());
    }

    /// A live newer owner is NOT reclaimed: its record keeps a current heartbeat, so the
    /// superseded daemon stays read-only however often it checks.
    #[test]
    fn a_heartbeating_owner_is_never_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WorkspaceLease::claim(dir.path());
        let path = lease_path(dir.path());
        let owner = LeaseRecord {
            generation: lease.generation().unwrap() + 1,
            token: new_token(),
            pid: 424242,
            heartbeat_secs: now_secs(),
        };
        std::fs::write(&path, serde_json::to_string(&owner).unwrap()).unwrap();

        std::thread::sleep(VERDICT_TTL);
        assert!(!lease.owns_caches());
        std::thread::sleep(VERDICT_TTL);
        assert!(!lease.owns_caches(), "a live owner keeps the workspace");
        assert_eq!(record_at(&path).generation, owner.generation, "its record is untouched");
    }

    /// Clearing `.build` (a cache wipe) removes the record. A lone daemon must take the
    /// workspace back rather than read the absence as a demotion.
    #[test]
    fn a_wiped_record_is_reclaimed_by_the_live_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let lease = WorkspaceLease::claim(dir.path());
        let path = lease_path(dir.path());
        std::fs::remove_file(&path).unwrap();

        std::thread::sleep(VERDICT_TTL);
        assert!(lease.owns_caches());
        assert_eq!(record_at(&path).generation, lease.generation().unwrap());
    }

    /// The wipe must not hand the workspace to BOTH daemons. Ownership is the record's identity,
    /// not a comparison of numbers: if the superseded daemon could restore its own lower
    /// generation, the newer one would read that as "below mine, so still mine" and the two
    /// would publish over each other — precisely the state the lease exists to prevent.
    #[test]
    fn a_wiped_record_never_leaves_two_daemons_owning_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let older = WorkspaceLease::claim(dir.path());
        let newer = WorkspaceLease::claim(dir.path());
        std::thread::sleep(VERDICT_TTL);
        assert!(!older.owns_caches() && newer.owns_caches(), "the newest claim owns it");

        std::fs::remove_file(lease_path(dir.path())).unwrap();
        std::thread::sleep(VERDICT_TTL);

        // Whoever gets there first takes it; the point is that the other one gives it up.
        let older_owns = older.owns_caches();
        let newer_owns = newer.owns_caches();
        assert!(
            older_owns != newer_owns,
            "exactly one daemon owns the workspace after the wipe (older={older_owns}, \
             newer={newer_owns})",
        );
        // And the loser's writes are refused at the fence, not merely by its cached verdict.
        let loser = if older_owns { &newer } else { &older };
        assert_eq!(loser.with_ownership_outcome(|| "published"), LeaseOutcome::Terminal);
    }
}
