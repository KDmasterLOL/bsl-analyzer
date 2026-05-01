//! An implementation of `loader::Handle`, based on `walkdir` and `notify`.
//!
//! The file watching bits here are untested and quite probably buggy. For this
//! reason, by default we don't watch files and rely on editor's file watching
//! capabilities.
//!
//! Hopefully, one day a reliable file watching/walking crate appears on
//! crates.io, and we can reduce this to trivial glue code.

use std::{
    fs,
    path::{Component, Path},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};

use crossbeam_channel::{select, unbounded, Receiver, Sender};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use paths::{AbsPath, AbsPathBuf, Utf8PathBuf};
use rayon::iter::{IndexedParallelIterator as _, IntoParallelIterator as _, ParallelIterator};
use rustc_hash::FxHashSet;
use vfs::loader::{self, LoadingProgress};
use walkdir::WalkDir;

/// Approximate byte budget per `Message::Loaded` chunk emitted by
/// [`NotifyActor::load_entry`]. Sized so the transient peak (raw `Vec<u8>` on
/// the loader thread plus converted `Arc<str>` on the receiver during
/// conversion) stays bounded for ERP-scale workspaces.
const LOADED_CHUNK_BYTES: usize = 64 * 1024 * 1024;

/// Path-count budget per `Message::WatchOnly` chunk emitted by
/// [`NotifyActor::load_entry`]. Watch-only entries carry no bytes, so the
/// transient peak is dominated by `AbsPathBuf` + `Vec` overhead on the
/// receiver side. ~4096 paths × ~200 B ≈ ~800 KiB per batch keeps shutdown
/// and `set_config` reload latency low without wasting per-path send
/// overhead on tiny chunks.
const WATCH_ONLY_CHUNK_PATHS: usize = 4096;

#[derive(Debug)]
pub struct NotifyHandle {
    // Relative order of fields below is significant.
    sender: Sender<Message>,
    /// Shared with the worker actor. The custom [`Drop`] impl flips this
    /// before the inbox sender drops so an in-flight scan or `parallel_count`
    /// pass — which never call `send`, so they can't detect receiver
    /// disconnect on their own — bails out cooperatively before the
    /// `JoinHandle` drop blocks on `join`.
    shutdown: Arc<AtomicBool>,
    _thread: stdx::thread::JoinHandle,
}

impl Drop for NotifyHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        // Field drop order continues: `sender` next (closes the inbox so the
        // actor wakes from `next_event` once the current scan unwinds),
        // then `_thread` joins.
    }
}

#[derive(Debug)]
enum Message {
    Config(loader::Config),
    Invalidate(AbsPathBuf),
}

impl loader::Handle for NotifyHandle {
    fn spawn(sender: loader::Sender) -> NotifyHandle {
        let shutdown = Arc::new(AtomicBool::new(false));
        let actor = NotifyActor::new(sender, Arc::clone(&shutdown));
        let (sender, receiver) = unbounded::<Message>();
        let thread = stdx::thread::Builder::new(stdx::thread::ThreadIntent::Worker, "VfsLoader")
            .spawn(move || actor.run(receiver))
            .expect("failed to spawn thread");
        NotifyHandle { sender, shutdown, _thread: thread }
    }

    fn set_config(&mut self, config: loader::Config) {
        self.sender.send(Message::Config(config)).unwrap();
    }

    fn invalidate(&mut self, path: AbsPathBuf) {
        self.sender.send(Message::Invalidate(path)).unwrap();
    }

    fn load_sync(&mut self, path: &AbsPath) -> Option<Vec<u8>> {
        read(path)
    }
}

type NotifyEvent = notify::Result<notify::Event>;

struct NotifyActor {
    sender: loader::Sender,
    /// Cooperative cancellation latch. Set in two ways:
    /// (a) [`NotifyHandle::drop`] flips it on shutdown so a count pass or
    ///     parallel scan that issues no [`Self::send`] calls still notices
    ///     the receiver is gone; (b) [`Self::send`] flips it on
    ///     [`crossbeam_channel::SendError`] so peer rayon workers and the
    ///     per-file walk see disconnect on their next probe. Once set, all
    ///     further sends and iterations become no-ops, which lets shutdown
    ///     — or a future `set_config` reload — abort a stale scan in tens
    ///     of milliseconds.
    shutdown: Arc<AtomicBool>,
    watched_file_entries: FxHashSet<AbsPathBuf>,
    watched_dir_entries: Vec<loader::Directories>,
    // Drop order is significant.
    watcher: Option<(RecommendedWatcher, Receiver<NotifyEvent>)>,
}

#[derive(Debug)]
enum Event {
    Message(Message),
    NotifyEvent(NotifyEvent),
}

impl NotifyActor {
    fn new(sender: loader::Sender, shutdown: Arc<AtomicBool>) -> NotifyActor {
        NotifyActor {
            sender,
            shutdown,
            watched_dir_entries: Vec::new(),
            watched_file_entries: FxHashSet::default(),
            watcher: None,
        }
    }

    fn next_event(&self, receiver: &Receiver<Message>) -> Option<Event> {
        let Some((_, watcher_receiver)) = &self.watcher else {
            return receiver.recv().ok().map(Event::Message);
        };

        select! {
            recv(receiver) -> it => it.ok().map(Event::Message),
            recv(watcher_receiver) -> it => Some(Event::NotifyEvent(it.unwrap())),
        }
    }

    fn run(mut self, inbox: Receiver<Message>) {
        while let Some(event) = self.next_event(&inbox) {
            tracing::debug!(?event, "vfs-notify event");
            match event {
                Event::Message(msg) => match msg {
                    Message::Config(config) => {
                        self.watcher = None;
                        if !config.watch.is_empty() {
                            let (watcher_sender, watcher_receiver) = unbounded();
                            let watcher = log_notify_error(RecommendedWatcher::new(
                                move |event| {
                                    // we don't care about the error. If sending fails that usually
                                    // means we were dropped, so unwrapping will just add to the
                                    // panic noise.
                                    _ = watcher_sender.send(event);
                                },
                                Config::default(),
                            ));
                            self.watcher = watcher.map(|it| (it, watcher_receiver));
                        }

                        let config_version = config.version;

                        self.watched_dir_entries.clear();
                        self.watched_file_entries.clear();

                        // Send Scanning immediately so user sees feedback right away
                        self.send(loader::Message::Progress {
                            n_total: 0,
                            n_done: LoadingProgress::Scanning,
                            config_version,
                            dir: None,
                        });

                        // If the receiver was already gone before we kicked
                        // off the scan (e.g., LSP was killed during start-up
                        // or a stale `set_config` raced with shutdown),
                        // `send` above latched `shutdown`. Bail before
                        // walking the workspace for a count nobody will
                        // observe.
                        if self.shutdown.load(Ordering::Relaxed) {
                            continue;
                        }

                        // First pass: count total files (uses WalkDir, may
                        // take time on large projects). Pass `&self.shutdown`
                        // through so a disconnect mid-count short-circuits
                        // the parallel walker via `WalkState::Quit`.
                        let count_start = Instant::now();
                        let n_total: usize = config
                            .load
                            .iter()
                            .map(|e| Self::count_files_in_entry(e, self.shutdown.as_ref()))
                            .sum();
                        if self.shutdown.load(Ordering::Relaxed) {
                            tracing::debug!("count pass aborted by shutdown latch");
                            continue;
                        }
                        tracing::info!(
                            n_total,
                            elapsed_ms = count_start.elapsed().as_millis() as u64,
                            "vfs: count pass complete",
                        );

                        self.send(loader::Message::Progress {
                            n_total,
                            n_done: LoadingProgress::Started,
                            config_version,
                            dir: None,
                        });

                        let (entry_tx, entry_rx) = unbounded();
                        let (watch_tx, watch_rx) = unbounded();
                        let processed = AtomicUsize::new(0);
                        let last_reported = AtomicUsize::new(0);
                        const PROGRESS_BATCH_SIZE: usize = 50; // Report every 50 files

                        let load_start = Instant::now();
                        let shutdown: &AtomicBool = self.shutdown.as_ref();
                        config.load.into_par_iter().enumerate().for_each(|(i, entry)| {
                            // Bail out before touching disk if a peer worker
                            // has already observed receiver disconnect.
                            if shutdown.load(Ordering::Relaxed) {
                                return;
                            }
                            let do_watch = config.watch.contains(&i);
                            if do_watch {
                                _ = entry_tx.send(entry.clone());
                            }
                            Self::load_entry(
                                |f| _ = watch_tx.send(f.to_owned()),
                                entry,
                                do_watch,
                                |file| {
                                    // Directory progress (keep as is)
                                    self.send(loader::Message::Progress {
                                        n_total,
                                        n_done: LoadingProgress::Progress(
                                            processed.load(std::sync::atomic::Ordering::Relaxed),
                                        ),
                                        dir: Some(file),
                                        config_version,
                                    });
                                },
                                || {
                                    // Per-file callback for batched progress updates
                                    let current = processed
                                        .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
                                        + 1;
                                    let last =
                                        last_reported.load(std::sync::atomic::Ordering::Relaxed);

                                    // Send progress every PROGRESS_BATCH_SIZE files
                                    if (current - last >= PROGRESS_BATCH_SIZE || current == n_total)
                                        && last_reported
                                            .compare_exchange(
                                                last,
                                                current,
                                                std::sync::atomic::Ordering::AcqRel,
                                                std::sync::atomic::Ordering::Relaxed,
                                            )
                                            .is_ok()
                                    {
                                        self.send(loader::Message::Progress {
                                            n_total,
                                            n_done: LoadingProgress::Progress(current),
                                            config_version,
                                            dir: None,
                                        });
                                    }
                                },
                                |files| self.send(loader::Message::Loaded { files }),
                                LOADED_CHUNK_BYTES,
                                |files| self.send(loader::Message::WatchOnly { files }),
                                WATCH_ONLY_CHUNK_PATHS,
                                shutdown,
                            );
                        });

                        // Files loaded - send Finished immediately so UI updates
                        tracing::info!(
                            n_total,
                            elapsed_ms = load_start.elapsed().as_millis() as u64,
                            "vfs: parallel read pass complete, sending LoadingProgress::Finished",
                        );
                        self.send(loader::Message::Progress {
                            n_total,
                            n_done: LoadingProgress::Finished,
                            config_version,
                            dir: None,
                        });

                        // Setup watchers after reporting completion (non-blocking for UI)
                        drop(watch_tx);
                        tracing::debug!("Setting up file watchers...");
                        let watch_count = watch_rx.len();
                        for path in watch_rx {
                            self.watch(&path);
                        }
                        tracing::debug!("Finished setting up {} file watchers", watch_count);

                        drop(entry_tx);
                        for entry in entry_rx {
                            match entry {
                                loader::Entry::Files(files) => {
                                    self.watched_file_entries.extend(files)
                                }
                                loader::Entry::Directories(dir) => {
                                    self.watched_dir_entries.push(dir)
                                }
                            }
                        }
                        tracing::debug!("File watching setup complete");
                    }
                    Message::Invalidate(path) => {
                        let contents = read(path.as_path());
                        let files = vec![(path, contents)];
                        self.send(loader::Message::Changed { files });
                    }
                },
                Event::NotifyEvent(event) => {
                    if let Some(event) = log_notify_error(event) {
                        if matches!(
                            event.kind,
                            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                        ) {
                            // Classify each path through `Directories::classify_file`
                            // so a watch-only XML does not silently get
                            // re-loaded as content via `Message::Changed`.
                            // `watched_file_entries` (raw `Entry::Files`
                            // lists) is always `LoadContent` — those
                            // entries carry no rules.
                            //
                            // Pre-existing limitation: `Remove` events lose
                            // the path's metadata before this handler
                            // runs, so they fall through `fs::metadata`
                            // and are dropped here for *both* load modes.
                            // PR-3 will fix Remove handling alongside
                            // wiring the watch-only consumer side
                            // (`Vfs::register_watch_only` dispatch from
                            // `Message::WatchOnly`); doing it earlier
                            // would emit deletions that no consumer can
                            // observe.
                            let mut changed_files: Vec<(AbsPathBuf, Option<Vec<u8>>)> = Vec::new();
                            let mut watch_only_files: Vec<AbsPathBuf> = Vec::new();
                            for path in event.paths {
                                let Some(path) = Utf8PathBuf::from_path_buf(path)
                                    .ok()
                                    .and_then(|p| AbsPathBuf::try_from(p).ok())
                                else {
                                    continue;
                                };
                                let Ok(meta) = fs::metadata(&path) else {
                                    continue;
                                };
                                if meta.file_type().is_dir()
                                    && self
                                        .watched_dir_entries
                                        .iter()
                                        .any(|dir| dir.contains_dir(&path))
                                {
                                    self.watch(path.as_ref());
                                    continue;
                                }
                                if !meta.file_type().is_file() {
                                    continue;
                                }

                                // Determine the file's load mode across all
                                // matching entries. `LoadContent` wins
                                // globally — if any entry classifies this
                                // path as content-loaded, that is the
                                // semantics, even if a different (later)
                                // entry tags the same path as watch-only.
                                // This guarantees deterministic behaviour
                                // when overlapping `Directories` entries
                                // are configured (e.g. an extension root
                                // that overlaps a metadata root). Without
                                // this rule, the order in which entries
                                // were enqueued during the parallel scan
                                // would silently decide whether a file
                                // is loaded as content or watch-only.
                                let mode = if self.watched_file_entries.contains(&path) {
                                    Some(loader::LoadMode::LoadContent)
                                } else {
                                    let mut found: Option<loader::LoadMode> = None;
                                    for dir in &self.watched_dir_entries {
                                        if let Some(m) = dir.classify_file(&path) {
                                            match m {
                                                loader::LoadMode::LoadContent => {
                                                    found = Some(loader::LoadMode::LoadContent);
                                                    break;
                                                }
                                                loader::LoadMode::WatchOnly => {
                                                    found
                                                        .get_or_insert(loader::LoadMode::WatchOnly);
                                                }
                                            }
                                        }
                                    }
                                    found
                                };
                                match mode {
                                    Some(loader::LoadMode::LoadContent) => {
                                        let contents = read(&path);
                                        changed_files.push((path, contents));
                                    }
                                    Some(loader::LoadMode::WatchOnly) => {
                                        watch_only_files.push(path);
                                    }
                                    None => continue,
                                }
                            }
                            if !changed_files.is_empty() {
                                self.send(loader::Message::Changed { files: changed_files });
                            }
                            if !watch_only_files.is_empty() {
                                self.send(loader::Message::WatchOnly { files: watch_only_files });
                            }
                        }
                    }
                }
            }
        }
    }

    /// Load files for `entry`, streaming results to `send_loaded` /
    /// `send_watch_only` in chunks.
    ///
    /// Files matching [`Directories::extensions`](loader::Directories::extensions)
    /// or a [`FileRule`](loader::FileRule) with [`LoadMode::LoadContent`](loader::LoadMode::LoadContent)
    /// are read from disk and shipped via `send_loaded` whenever the
    /// in-flight buffer reaches `chunk_threshold` bytes; a file larger than
    /// the threshold becomes its own single-file chunk, so the algorithm
    /// degrades gracefully on huge inputs.
    ///
    /// Files matching a rule with [`LoadMode::WatchOnly`](loader::LoadMode::WatchOnly)
    /// are routed to `send_watch_only` (path only, no read) every
    /// `watch_chunk_paths` paths. Buffers for the two modes are independent
    /// so a slow read of a large content file does not delay the watch-only
    /// stream and vice versa.
    ///
    /// `shutdown` is polled before every file read so a parallel worker that
    /// observes receiver disconnect can short-circuit the in-flight scan
    /// without waiting for `WalkDir` to finish enumerating the workspace.
    // Each callback wires this private associated function into a different
    // observable on `NotifyActor` (watcher set, root-progress messages, the
    // per-file progress counter, and one of the two chunked senders).
    // Folding them into a struct trades the same parameter count for
    // indirection without simplifying the caller — the only invocation
    // site is `NotifyActor::run`.
    #[allow(clippy::too_many_arguments)]
    fn load_entry(
        mut watch: impl FnMut(&Path),
        entry: loader::Entry,
        do_watch: bool,
        send_message: impl Fn(AbsPathBuf),
        on_file_loaded: impl Fn(),
        send_loaded: impl Fn(Vec<(AbsPathBuf, Option<Vec<u8>>)>),
        chunk_threshold: usize,
        send_watch_only: impl Fn(Vec<AbsPathBuf>),
        watch_chunk_paths: usize,
        shutdown: &AtomicBool,
    ) {
        let mut loaded_buf: Vec<(AbsPathBuf, Option<Vec<u8>>)> = Vec::new();
        let mut loaded_bytes: usize = 0;
        let mut watch_only_buf: Vec<AbsPathBuf> = Vec::new();

        let mut push_loaded = |file: AbsPathBuf, contents: Option<Vec<u8>>| {
            loaded_bytes += contents.as_ref().map(|c| c.len()).unwrap_or(0);
            loaded_buf.push((file, contents));
            if loaded_bytes >= chunk_threshold {
                send_loaded(std::mem::take(&mut loaded_buf));
                loaded_bytes = 0;
            }
        };

        let mut push_watch_only = |file: AbsPathBuf| {
            watch_only_buf.push(file);
            if watch_only_buf.len() >= watch_chunk_paths {
                send_watch_only(std::mem::take(&mut watch_only_buf));
            }
        };

        match entry {
            loader::Entry::Files(files) => {
                // `Entry::Files` is an explicit list with no extension/rule
                // metadata; its members are always loaded as content (this
                // matches legacy behaviour and the assumption made by the
                // only existing caller — `bsl-analyzer.toml` config files).
                for file in files {
                    if shutdown.load(Ordering::Relaxed) {
                        return;
                    }
                    if do_watch {
                        watch(file.as_ref());
                    }
                    let contents = read(file.as_path());
                    on_file_loaded();
                    push_loaded(file, contents);
                }
            }
            loader::Entry::Directories(dirs) => {
                for root in &dirs.include {
                    if shutdown.load(Ordering::Relaxed) {
                        return;
                    }
                    // Watch root directory with Recursive mode (handles all subdirs)
                    if do_watch {
                        watch(root.as_ref());
                    }

                    send_message(root.clone());
                    let walkdir =
                        WalkDir::new(root).follow_links(true).into_iter().filter_entry(|entry| {
                            // `WalkDir` does not expose a cancellation API, but
                            // `filter_entry` is consulted for every node it
                            // is about to visit. Returning `false` here
                            // skips the entry (and, if it's a directory,
                            // prunes the entire subtree from descent), so
                            // an in-flight walk drains its pending stack
                            // without recursing further once shutdown is
                            // latched. Pairs with the per-file `shutdown`
                            // probe in the load loop below for a two-tier
                            // bail-out.
                            if shutdown.load(Ordering::Relaxed) {
                                return false;
                            }
                            if !entry.file_type().is_dir() {
                                return true;
                            }
                            let path = entry.path();

                            // Only check for cycles if this is actually a symlink
                            if entry.path_is_symlink() && symlink_might_be_cyclic(path) {
                                return false;
                            }

                            // We want to filter out subdirectories that are roots themselves, because they will be visited separately.
                            dirs.exclude.iter().all(|it| it != path)
                                && (root == path || dirs.include.iter().all(|it| it != path))
                        });

                    let files = walkdir.filter_map(|it| it.ok()).filter_map(|entry| {
                        let depth = entry.depth();
                        let is_dir = entry.file_type().is_dir();
                        let is_file = entry.file_type().is_file();
                        let abs_path = AbsPathBuf::try_from(
                            Utf8PathBuf::from_path_buf(entry.into_path()).ok()?,
                        )
                        .ok()?;
                        if depth < 2 && is_dir {
                            send_message(abs_path.clone());
                        }
                        // Note: No per-subdirectory watch - root is watched with Recursive mode
                        if !is_file {
                            return None;
                        }
                        // Classify the file by extension. `extensions`
                        // wins over rules so a half-migrated caller (still
                        // listing an extension in both fields) never
                        // silently downgrades to watch-only. Path-level
                        // include/exclude is enforced by `filter_entry`
                        // above, so we only check the extension here —
                        // matching legacy behaviour.
                        let ext = abs_path.extension().unwrap_or_default();
                        if dirs.extensions.iter().any(|it| it.as_str() == ext) {
                            return Some((abs_path, loader::LoadMode::LoadContent));
                        }
                        for rule in &dirs.rules {
                            if rule.extensions.iter().any(|it| it.as_str() == ext) {
                                return Some((abs_path, rule.load_mode));
                            }
                        }
                        None
                    });

                    for (file, mode) in files {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        on_file_loaded();
                        match mode {
                            loader::LoadMode::LoadContent => {
                                let contents = read(file.as_path());
                                push_loaded(file, contents);
                            }
                            loader::LoadMode::WatchOnly => {
                                push_watch_only(file);
                            }
                        }
                    }
                }
            }
        }

        let aborted = shutdown.load(Ordering::Relaxed);
        if !loaded_buf.is_empty() && !aborted {
            send_loaded(loaded_buf);
        }
        if !watch_only_buf.is_empty() && !aborted {
            send_watch_only(watch_only_buf);
        }
    }

    /// Count files in an entry without reading their contents.
    /// Uses parallel directory traversal for better performance on large projects.
    /// `cancel` is polled inside the walker so a shutdown latched between
    /// `Scanning` and `Started` short-circuits the count pass without first
    /// enumerating every workspace path.
    fn count_files_in_entry(entry: &loader::Entry, cancel: &AtomicBool) -> usize {
        match entry {
            loader::Entry::Files(files) => files.len(),
            loader::Entry::Directories(dirs) => {
                let roots: Vec<&std::path::Path> =
                    dirs.include.iter().map(|p| p.as_path().as_ref()).collect();
                let excludes: Vec<&std::path::Path> =
                    dirs.exclude.iter().map(|p| p.as_path().as_ref()).collect();
                // Union of `extensions` and rule extensions, deduplicated.
                // `n_total` shipped to the progress bar must include
                // watch-only files so the user sees a truthful
                // "n loaded out of m" indicator across both load modes.
                let mut extensions: Vec<&str> =
                    dirs.extensions.iter().map(|s| s.as_str()).collect();
                for rule in &dirs.rules {
                    for ext in &rule.extensions {
                        let s = ext.as_str();
                        if !extensions.contains(&s) {
                            extensions.push(s);
                        }
                    }
                }

                stdx::fs::parallel_count_cancellable(
                    &roots,
                    &stdx::fs::WalkConfig {
                        extensions: &extensions,
                        excludes: &excludes,
                        follow_links: true,
                    },
                    Some(cancel),
                )
            }
        }
    }

    fn watch(&mut self, path: &Path) {
        if let Some((watcher, _)) = &mut self.watcher {
            // Use Recursive mode - FSEvents handles this efficiently on macOS
            // This avoids creating thousands of watchers for each subdirectory
            log_notify_error(watcher.watch(path, RecursiveMode::Recursive));
        }
    }

    #[track_caller]
    fn send(&self, msg: loader::Message) {
        // Latch-checked early-out: once shutdown is set we stop emitting,
        // and (combined with the per-file `shutdown` checks in
        // [`Self::load_entry`]) the in-flight scan unwinds without finishing
        // disk reads for results nobody will consume.
        if self.shutdown.load(Ordering::Relaxed) {
            return;
        }
        // The receiver lives in `GlobalState`; once it drops (graceful or
        // panicked shutdown), the bounded sender would otherwise hang the
        // loader thread on full-channel waits. Latch shutdown on disconnect
        // so peer sends and walks see it and abort.
        if let Err(crossbeam_channel::SendError(_)) = self.sender.send(msg) {
            tracing::debug!("loader receiver disconnected, aborting vfs scan");
            self.shutdown.store(true, Ordering::Release);
        }
    }
}

fn read(path: &AbsPath) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

fn log_notify_error<T>(res: notify::Result<T>) -> Option<T> {
    res.map_err(|err| tracing::warn!("notify error: {}", err)).ok()
}

/// Is `path` a symlink to a parent directory?
///
/// Including this path is guaranteed to cause an infinite loop. This
/// heuristic is not sufficient to catch all symlink cycles (it's
/// possible to construct cycle using two or more symlinks), but it
/// catches common cases.
/// Check if a symlink points to a parent directory (potential cycle).
/// `is_symlink` should be obtained from DirEntry::path_is_symlink() to avoid extra syscall.
fn symlink_might_be_cyclic(path: &Path) -> bool {
    let Ok(destination) = std::fs::read_link(path) else {
        return false;
    };

    // If the symlink is of the form "../..", it's a parent symlink.
    let is_relative_parent =
        destination.components().all(|c| matches!(c, Component::CurDir | Component::ParentDir));

    is_relative_parent || path.starts_with(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Build a test fixture: `count` files of `bytes_per_file` bytes each,
    /// extension `.txt`, inside a fresh temp directory. Returns the temp
    /// guard and the `Directories` entry pointing at it.
    fn fixture(count: usize, bytes_per_file: usize) -> (tempfile::TempDir, loader::Directories) {
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..count {
            let path = dir.path().join(format!("file_{i}.txt"));
            std::fs::write(&path, vec![b'x'; bytes_per_file]).expect("write fixture");
        }
        let abs_root = AbsPathBuf::assert_utf8(dir.path().to_path_buf());
        let dirs = loader::Directories {
            extensions: vec!["txt".to_string()],
            include: vec![abs_root],
            exclude: vec![],
            rules: Vec::new(),
        };
        (dir, dirs)
    }

    /// Captured output from `load_entry`: per-chunk path/size summaries
    /// for content-loaded files and per-chunk path lists for watch-only
    /// files. Tests that only care about content can read `loaded`.
    #[derive(Default)]
    struct LoadCapture {
        loaded: Vec<Vec<(AbsPathBuf, usize)>>,
        watch_only: Vec<Vec<AbsPathBuf>>,
    }

    /// Drive `load_entry` with the given threshold and capture the chunks it
    /// emits. Watch / progress callbacks are stubbed; shutdown stays false.
    fn run(dirs: loader::Directories, threshold: usize) -> Vec<Vec<(AbsPathBuf, usize)>> {
        let shutdown = AtomicBool::new(false);
        run_with_shutdown(dirs, threshold, &shutdown)
    }

    fn run_with_shutdown(
        dirs: loader::Directories,
        threshold: usize,
        shutdown: &AtomicBool,
    ) -> Vec<Vec<(AbsPathBuf, usize)>> {
        run_full_with_shutdown(dirs, threshold, WATCH_ONLY_CHUNK_PATHS, shutdown).loaded
    }

    /// Like [`run`] but also captures `Message::WatchOnly` chunks. Used by
    /// tests that exercise rule-based dispatch.
    fn run_full(dirs: loader::Directories, threshold: usize, watch_chunk: usize) -> LoadCapture {
        let shutdown = AtomicBool::new(false);
        run_full_with_shutdown(dirs, threshold, watch_chunk, &shutdown)
    }

    fn run_full_with_shutdown(
        dirs: loader::Directories,
        threshold: usize,
        watch_chunk: usize,
        shutdown: &AtomicBool,
    ) -> LoadCapture {
        let loaded: Mutex<Vec<Vec<(AbsPathBuf, usize)>>> = Mutex::new(Vec::new());
        let watch_only: Mutex<Vec<Vec<AbsPathBuf>>> = Mutex::new(Vec::new());
        NotifyActor::load_entry(
            |_| {},
            loader::Entry::Directories(dirs),
            false,
            |_| {},
            || {},
            |files| {
                let summary =
                    files.into_iter().map(|(p, c)| (p, c.map(|c| c.len()).unwrap_or(0))).collect();
                loaded.lock().unwrap().push(summary);
            },
            threshold,
            |files| {
                watch_only.lock().unwrap().push(files);
            },
            watch_chunk,
            shutdown,
        );
        LoadCapture {
            loaded: loaded.into_inner().unwrap(),
            watch_only: watch_only.into_inner().unwrap(),
        }
    }

    #[test]
    fn under_threshold_emits_single_chunk() {
        let (_guard, dirs) = fixture(5, 4 * 1024);
        let chunks = run(dirs, 1024 * 1024);
        assert_eq!(chunks.len(), 1, "expected one chunk under threshold, got {chunks:#?}");
        assert_eq!(chunks[0].len(), 5);
    }

    #[test]
    fn over_threshold_emits_multiple_chunks() {
        // 8 files × 4 KiB = 32 KiB total, threshold 10 KiB → at least 3 chunks.
        let (_guard, dirs) = fixture(8, 4 * 1024);
        let chunks = run(dirs, 10 * 1024);
        assert!(
            chunks.len() >= 3,
            "expected multiple chunks over threshold, got {} chunks: {:#?}",
            chunks.len(),
            chunks,
        );
        let total_files: usize = chunks.iter().map(|c| c.len()).sum();
        assert_eq!(total_files, 8, "all files must be emitted exactly once across chunks");
    }

    #[test]
    fn empty_entry_emits_no_chunks() {
        let (_guard, dirs) = fixture(0, 0);
        let chunks = run(dirs, 1024);
        assert!(chunks.is_empty(), "empty entry must not emit any Loaded message");
    }

    #[test]
    fn oversized_single_file_becomes_own_chunk() {
        // One 8 KiB file with a 1 KiB threshold: pushed first, then flushed
        // once the buffer overshoots — the algorithm degrades gracefully on
        // files larger than the threshold instead of refusing to flush.
        let (_guard, dirs) = fixture(1, 8 * 1024);
        let chunks = run(dirs, 1024);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 1);
        assert_eq!(chunks[0][0].1, 8 * 1024);
    }

    /// Build a fixture with `bsl_count` BSL files and `xml_count` XML
    /// files, all of `bytes_per_file` bytes, in a fresh temp directory.
    /// Used by tests that exercise the new rule-based dispatch.
    fn fixture_mixed(
        bsl_count: usize,
        xml_count: usize,
        bytes_per_file: usize,
    ) -> (tempfile::TempDir, AbsPathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..bsl_count {
            let path = dir.path().join(format!("module_{i}.bsl"));
            std::fs::write(&path, vec![b'x'; bytes_per_file]).expect("write bsl fixture");
        }
        for i in 0..xml_count {
            let path = dir.path().join(format!("form_{i}.xml"));
            std::fs::write(&path, vec![b'x'; bytes_per_file]).expect("write xml fixture");
        }
        let abs_root = AbsPathBuf::assert_utf8(dir.path().to_path_buf());
        (dir, abs_root)
    }

    #[test]
    fn watch_only_rule_dispatches_to_separate_buffer() {
        // 3 .bsl + 5 .xml; .bsl is loaded as content, .xml watch-only.
        // Verify the two streams are independent and that no .xml file
        // ever appears in the loaded buffer (i.e. its bytes were never
        // read from disk into a `Vec<u8>`).
        let (_guard, root) = fixture_mixed(3, 5, 1024);
        let dirs = loader::Directories {
            extensions: vec!["bsl".to_string()],
            include: vec![root],
            exclude: vec![],
            rules: vec![loader::FileRule {
                extensions: vec!["xml".to_string()],
                load_mode: loader::LoadMode::WatchOnly,
            }],
        };

        let cap = run_full(dirs, 1024 * 1024, 1024);
        let total_loaded: usize = cap.loaded.iter().map(|c| c.len()).sum();
        let total_watch_only: usize = cap.watch_only.iter().map(|c| c.len()).sum();
        assert_eq!(total_loaded, 3, "exactly the 3 .bsl files should be loaded");
        assert_eq!(total_watch_only, 5, "exactly the 5 .xml files should be watch-only");

        // No .xml may leak into the loaded chunks.
        for chunk in &cap.loaded {
            for (path, _) in chunk {
                assert_ne!(path.extension(), Some("xml"), "xml must not be in loaded buffer");
            }
        }
        // No .bsl may leak into the watch-only chunks.
        for chunk in &cap.watch_only {
            for path in chunk {
                assert_ne!(path.extension(), Some("bsl"), "bsl must not be in watch-only buffer");
            }
        }
    }

    #[test]
    fn pure_watch_only_emits_only_watch_only_chunks() {
        // No content extensions at all — only an XML watch-only rule. The
        // loaded buffer should never be touched.
        let (_guard, root) = fixture_mixed(0, 4, 1024);
        let dirs = loader::Directories {
            extensions: vec![],
            include: vec![root],
            exclude: vec![],
            rules: vec![loader::FileRule {
                extensions: vec!["xml".to_string()],
                load_mode: loader::LoadMode::WatchOnly,
            }],
        };

        let cap = run_full(dirs, 1024 * 1024, 1024);
        assert!(cap.loaded.is_empty(), "no content extensions configured, got {:#?}", cap.loaded);
        let total_watch_only: usize = cap.watch_only.iter().map(|c| c.len()).sum();
        assert_eq!(total_watch_only, 4);
    }

    #[test]
    fn watch_only_chunking_respects_path_threshold() {
        // 7 watch-only paths, threshold 3 → at least 3 chunks (3+3+1)
        // and every path appears exactly once.
        let (_guard, root) = fixture_mixed(0, 7, 64);
        let dirs = loader::Directories {
            extensions: vec![],
            include: vec![root],
            exclude: vec![],
            rules: vec![loader::FileRule {
                extensions: vec!["xml".to_string()],
                load_mode: loader::LoadMode::WatchOnly,
            }],
        };

        let cap = run_full(dirs, 1024 * 1024, 3);
        assert!(
            cap.watch_only.len() >= 3,
            "expected ≥3 watch-only chunks at threshold 3, got {} chunks",
            cap.watch_only.len(),
        );
        let total: usize = cap.watch_only.iter().map(|c| c.len()).sum();
        assert_eq!(total, 7, "all watch-only paths emitted exactly once");
    }

    #[test]
    fn extensions_win_over_rules_for_same_extension() {
        // Defensive: if a half-migrated caller lists an extension in both
        // `extensions` and a `WatchOnly` rule, content semantics must
        // win — never silently downgrade.
        let (_guard, root) = fixture_mixed(0, 2, 64);
        let dirs = loader::Directories {
            extensions: vec!["xml".to_string()],
            include: vec![root],
            exclude: vec![],
            rules: vec![loader::FileRule {
                extensions: vec!["xml".to_string()],
                load_mode: loader::LoadMode::WatchOnly,
            }],
        };

        let cap = run_full(dirs, 1024 * 1024, 1024);
        let total_loaded: usize = cap.loaded.iter().map(|c| c.len()).sum();
        assert_eq!(total_loaded, 2, "extensions must win — both .xml load as content");
        assert!(cap.watch_only.is_empty(), "watch-only buffer must stay empty");
    }

    #[test]
    fn count_files_unions_extensions_and_rules() {
        // count_files_in_entry must include rule-matched files in the
        // total, otherwise the progress bar would understate `n_total`
        // when the workspace contains watch-only metadata.
        let (_guard, root) = fixture_mixed(3, 5, 64);
        let dirs = loader::Directories {
            extensions: vec!["bsl".to_string()],
            include: vec![root],
            exclude: vec![],
            rules: vec![loader::FileRule {
                extensions: vec!["xml".to_string()],
                load_mode: loader::LoadMode::WatchOnly,
            }],
        };
        let cancel = AtomicBool::new(false);
        let count = NotifyActor::count_files_in_entry(&loader::Entry::Directories(dirs), &cancel);
        assert_eq!(count, 8, "count must union extensions + rules");
    }

    #[test]
    fn pre_set_shutdown_skips_scan_and_flush() {
        // 50 files × 1 KiB; if shutdown was already latched before
        // load_entry is called, the per-file walk must short-circuit and
        // the trailing flush must not emit anything either.
        let (_guard, dirs) = fixture(50, 1024);
        let shutdown = AtomicBool::new(true);
        let chunks = run_with_shutdown(dirs, 100 * 1024, &shutdown);
        assert!(chunks.is_empty(), "shutdown latch must suppress all sends, got {chunks:#?}");
    }

    #[test]
    fn handle_drop_latches_shutdown() {
        // Constructs the inner pieces of `NotifyHandle::spawn` directly so
        // the test exercises only the `Drop` impl, without racing a real
        // loader. The worker thread returns immediately so `JoinHandle`
        // drop joins instantly; the assertion is that the shared shutdown
        // flag is observable as `true` before the handle's fields complete
        // their drops — the contract that a count pass or scan needs to
        // notice receiver disconnect when no `send` has been issued yet.
        let shutdown = Arc::new(AtomicBool::new(false));
        let (sender, _receiver) = unbounded::<Message>();
        let thread = stdx::thread::Builder::new(stdx::thread::ThreadIntent::Worker, "test-drop")
            .spawn(|| {})
            .expect("spawn test worker");

        let handle = NotifyHandle { sender, shutdown: Arc::clone(&shutdown), _thread: thread };

        assert!(!shutdown.load(Ordering::Relaxed), "flag is false before drop");
        drop(handle);
        assert!(shutdown.load(Ordering::Relaxed), "Drop must latch shutdown");
    }
}
