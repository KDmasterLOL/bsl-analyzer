use std::{
    fs,
    path::{Component, Path},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, OnceLock,
    },
    time::Instant,
};

use crossbeam_channel::{bounded, select, unbounded, Receiver, Sender, TrySendError};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use paths::{AbsPath, AbsPathBuf, Utf8PathBuf};
use rustc_hash::FxHashSet;
use vfs::loader::{self, LoadingProgress};
use walkdir::WalkDir;

const LOADED_CHUNK_BYTES: usize = 64 * 1024 * 1024;

const WATCH_ONLY_CHUNK_PATHS: usize = 4096;

/// Env override for the directory-scan reader pool size (see [`reader_count`]).
const READERS_ENV: &str = "BSL_VFS_READERS";

/// Bound on paths queued to the reader pool. A path is a cheap `AbsPathBuf`; a
/// generous queue keeps readers fed past short walk stalls without unbounded
/// memory. The loader drains a result whenever this fills (backpressure).
const READER_PATHS_BOUND: usize = 512;

/// Bound on read results buffered back from the pool. Each is `(path, bytes)`;
/// at ~57 KB average that caps in-flight read memory at ~15 MB plus outliers,
/// on top of the single 64 MB chunk buffer the loader accumulates.
const READER_RESULTS_BOUND: usize = 256;

/// Number of reader threads for a directory scan: `available_parallelism`
/// clamped to `[2, 8]`, overridable via `BSL_VFS_READERS`. These are dedicated
/// `std`/`stdx` threads, never the global rayon pool — a pool thread parked on
/// the bounded loader channel is a work-stealing deadlock window (see the
/// comment in `NotifyActor::run`).
fn reader_count() -> usize {
    static CACHED: OnceLock<usize> = OnceLock::new();
    *CACHED.get_or_init(|| {
        if let Ok(raw) = std::env::var(READERS_ENV) {
            match raw.parse::<usize>() {
                Ok(parsed) => {
                    let clamped = parsed.clamp(1, 32);
                    tracing::info!(readers = clamped, raw = %raw, "Resolved BSL_VFS_READERS");
                    return clamped;
                }
                Err(_) => tracing::warn!(
                    raw = %raw,
                    "BSL_VFS_READERS is not a valid usize; falling back to default",
                ),
            }
        }
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(2, 8)
    })
}

#[derive(Debug)]
pub struct NotifyHandle {
    sender: Sender<Message>,
    shutdown: Arc<AtomicBool>,
    _thread: stdx::thread::JoinHandle,
}

impl Drop for NotifyHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
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
    shutdown: Arc<AtomicBool>,
    watched_file_entries: FxHashSet<AbsPathBuf>,
    watched_only_file_entries: FxHashSet<AbsPathBuf>,
    watched_dir_entries: Vec<loader::Directories>,
    watcher: Option<(RecommendedWatcher, Receiver<NotifyEvent>)>,
}

#[derive(Debug)]
enum Event {
    Message(Message),
    NotifyEvent(NotifyEvent),
}

/// What a single path from a filesystem event should produce, derived from the
/// path's current on-disk state plus the watch configuration. A REMOVED path
/// (no longer stat-able) is still classified by path so its deletion is
/// delivered instead of being silently dropped.
#[derive(Debug, PartialEq, Eq)]
enum EventPathAction {
    /// Not watched, or an irrelevant directory event — ignore.
    Ignore,
    /// A directory under a watched root appeared — start watching it.
    WatchDir,
    /// A watched content file that exists — (re)load its bytes.
    LoadContent,
    /// A watched watch-only file changed (exists or removed) — signal only.
    WatchOnly,
    /// A watched content file was removed — deliver as a deletion.
    Delete,
    /// A path under a watched root was removed but is not a watched file — most
    /// likely a removed directory whose subtree must be expanded by the consumer
    /// (the loader cannot enumerate children that no longer exist on disk).
    DeleteSubtree,
}

impl NotifyActor {
    fn new(sender: loader::Sender, shutdown: Arc<AtomicBool>) -> NotifyActor {
        NotifyActor {
            sender,
            shutdown,
            watched_dir_entries: Vec::new(),
            watched_file_entries: FxHashSet::default(),
            watched_only_file_entries: FxHashSet::default(),
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
                                    _ = watcher_sender.send(event);
                                },
                                Config::default(),
                            ));
                            self.watcher = watcher.map(|it| (it, watcher_receiver));
                        }

                        let config_version = config.version;

                        self.watched_dir_entries.clear();
                        self.watched_file_entries.clear();
                        self.watched_only_file_entries.clear();

                        self.send(loader::Message::Progress {
                            n_total: 0,
                            n_done: LoadingProgress::Scanning,
                            config_version,
                            dir: None,
                        });

                        if self.shutdown.load(Ordering::Relaxed) {
                            continue;
                        }

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
                        const PROGRESS_BATCH_SIZE: usize = 50;

                        let load_start = Instant::now();
                        let shutdown: &AtomicBool = self.shutdown.as_ref();
                        // Entries load sequentially on this dedicated thread, NOT
                        // on the global rayon pool. These loads block on the
                        // bounded loader channel (backpressure against the
                        // consumer's event loop), and a pool thread parked in such
                        // a send is a deadlock window: a rayon wait elsewhere in
                        // the process (e.g. the metadata loader's scope inside a
                        // Salsa query) can steal the load task and park a thread
                        // whose db clone that same event loop is waiting on. There
                        // are only one or two entries (the workspace walk plus
                        // config files), so entry-level parallelism bought nothing.
                        for (i, entry) in config.load.into_iter().enumerate() {
                            if shutdown.load(Ordering::Relaxed) {
                                break;
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
                                    let current = processed
                                        .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
                                        + 1;
                                    let last =
                                        last_reported.load(std::sync::atomic::Ordering::Relaxed);

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
                        }

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

                        drop(watch_tx);
                        drop(entry_tx);
                        for entry in entry_rx {
                            match entry {
                                loader::Entry::Files(files) => {
                                    self.watched_file_entries.extend(files)
                                }
                                loader::Entry::WatchOnlyFiles(files) => {
                                    self.watched_only_file_entries.extend(files)
                                }
                                loader::Entry::Directories(dir) => {
                                    self.watched_dir_entries.push(dir)
                                }
                            }
                        }
                        tracing::debug!("Setting up file watchers...");
                        let watch_count = watch_rx.len();
                        for path in watch_rx {
                            self.watch(&path);
                        }
                        tracing::debug!("Finished setting up {} file watchers", watch_count);
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
                            let mut changed_files: Vec<(AbsPathBuf, Option<Vec<u8>>)> = Vec::new();
                            let mut watch_only_files: Vec<AbsPathBuf> = Vec::new();
                            let mut removed_recursive: Vec<AbsPathBuf> = Vec::new();
                            for path in event.paths {
                                let Some(path) = Utf8PathBuf::from_path_buf(path)
                                    .ok()
                                    .and_then(|p| AbsPathBuf::try_from(p).ok())
                                else {
                                    continue;
                                };
                                match self.classify_event_path(&path) {
                                    EventPathAction::WatchDir => self.watch(path.as_ref()),
                                    EventPathAction::LoadContent => {
                                        let contents = read(&path);
                                        changed_files.push((path, contents));
                                    }
                                    EventPathAction::WatchOnly => watch_only_files.push(path),
                                    // Deliver the removal as a deletion (`None`
                                    // contents) so the consumer can tombstone it.
                                    EventPathAction::Delete => changed_files.push((path, None)),
                                    EventPathAction::DeleteSubtree => removed_recursive.push(path),
                                    EventPathAction::Ignore => {}
                                }
                            }
                            if !changed_files.is_empty() {
                                self.send(loader::Message::Changed { files: changed_files });
                            }
                            if !watch_only_files.is_empty() {
                                self.send(loader::Message::WatchOnly { files: watch_only_files });
                            }
                            if !removed_recursive.is_empty() {
                                self.send(loader::Message::RemovedRecursive {
                                    paths: removed_recursive,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

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
            loader::Entry::WatchOnlyFiles(files) => {
                for file in files {
                    if shutdown.load(Ordering::Relaxed) {
                        return;
                    }
                    if do_watch {
                        watch(file.as_ref());
                    }
                    on_file_loaded();
                    push_watch_only(file);
                }
            }
            loader::Entry::Directories(dirs) => {
                // A scan-owned pool of dedicated reader threads turns the
                // sequential per-file `read` into parallel I/O. The walk stays on
                // this loader thread (it warms dentries and the mixed
                // walk+read pipeline is walk-bound); only the `read` syscalls fan
                // out. The pool is created here and joined before this arm
                // returns, so readers never outlive their scan — a repeat
                // `Message::Config` (processed only after this returns) and a
                // shutdown both leave no stragglers.
                let readers = reader_count();
                let (paths_tx, paths_rx) = bounded::<AbsPathBuf>(READER_PATHS_BOUND);
                let (results_tx, results_rx) =
                    bounded::<(AbsPathBuf, Option<Vec<u8>>)>(READER_RESULTS_BOUND);
                let mut handles = Vec::with_capacity(readers);
                for i in 0..readers {
                    let paths_rx = paths_rx.clone();
                    let results_tx = results_tx.clone();
                    let handle = stdx::thread::Builder::new(
                        stdx::thread::ThreadIntent::Worker,
                        format!("VfsReader{i}"),
                    )
                    .spawn(move || {
                        // Read until the loader drops `paths_tx` (scan done) or the
                        // results channel disconnects (loader gone). No other
                        // side effects: the reader never touches the loader
                        // channel, only this private results channel.
                        while let Ok(path) = paths_rx.recv() {
                            let contents = read(path.as_path());
                            if results_tx.send((path, contents)).is_err() {
                                break;
                            }
                        }
                    })
                    .expect("failed to spawn vfs reader thread");
                    handles.push(handle);
                }
                // The loader keeps only the producing end of `paths` and the
                // consuming end of `results`; dropping the others lets the
                // channels disconnect once the readers exit, terminating the
                // drain loop below.
                drop(paths_rx);
                drop(results_tx);

                let mut submitted: usize = 0;
                let mut delivered: usize = 0;

                'walk: for root in &dirs.include {
                    if shutdown.load(Ordering::Relaxed) {
                        break 'walk;
                    }
                    if do_watch {
                        watch(root.as_ref());
                    }

                    send_message(root.clone());
                    let walkdir =
                        WalkDir::new(root).follow_links(true).into_iter().filter_entry(|entry| {
                            if shutdown.load(Ordering::Relaxed) {
                                return false;
                            }
                            if !entry.file_type().is_dir() {
                                return true;
                            }
                            let path = entry.path();

                            if entry.path_is_symlink() && symlink_might_be_cyclic(path) {
                                return false;
                            }

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
                        if !is_file {
                            return None;
                        }
                        let ext = abs_path.extension().unwrap_or_default();
                        if dirs.extensions.iter().any(|it| it.eq_ignore_ascii_case(ext)) {
                            return Some((abs_path, loader::LoadMode::LoadContent));
                        }
                        for rule in &dirs.rules {
                            if rule.extensions.iter().any(|it| it.eq_ignore_ascii_case(ext)) {
                                return Some((abs_path, rule.load_mode));
                            }
                        }
                        None
                    });

                    for (file, mode) in files {
                        if shutdown.load(Ordering::Relaxed) {
                            break 'walk;
                        }
                        match mode {
                            loader::LoadMode::LoadContent => {
                                // Hand the path to the pool. On a full queue, the
                                // loader (not a reader) drains one finished read —
                                // this is the only place `push_loaded` runs, so
                                // the blocking loader-channel send always stays on
                                // this thread, and the in-flight memory is bounded.
                                let mut file = file;
                                loop {
                                    match paths_tx.try_send(file) {
                                        Ok(()) => {
                                            submitted += 1;
                                            break;
                                        }
                                        Err(TrySendError::Full(f)) => {
                                            match results_rx.recv() {
                                                Ok((path, contents)) => {
                                                    on_file_loaded();
                                                    push_loaded(path, contents);
                                                    delivered += 1;
                                                }
                                                // Readers vanished — abandon the
                                                // scan; the drain below joins them.
                                                Err(_) => break 'walk,
                                            }
                                            file = f;
                                        }
                                        Err(TrySendError::Disconnected(_)) => break 'walk,
                                    }
                                }
                            }
                            loader::LoadMode::WatchOnly => {
                                on_file_loaded();
                                push_watch_only(file);
                            }
                        }
                    }
                }

                // Stop submitting and drain every in-flight read. Draining to
                // disconnect (not to a submitted==delivered count) keeps a reader
                // blocked on a full results channel from hanging the join on an
                // aborted scan; once `paths_tx` is dropped the readers finish the
                // queue, emit their last results, and exit.
                drop(paths_tx);
                while let Ok((path, contents)) = results_rx.recv() {
                    on_file_loaded();
                    push_loaded(path, contents);
                    delivered += 1;
                }
                debug_assert_eq!(submitted, delivered, "every queued read must be delivered once");
                for handle in handles {
                    let () = handle.join();
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

    /// Decide what a single changed path should produce, tolerating removals.
    /// Existing paths are routed by their on-disk type; a path that no longer
    /// exists (a removal) is classified by path alone — a watched content file
    /// becomes a [`EventPathAction::Delete`] (so the consumer tombstones it)
    /// rather than being dropped because `fs::metadata` failed.
    ///
    /// Known limitation: a *coalesced* directory removal (where a watch backend
    /// reports only the directory path, not each child) classifies as `Ignore` —
    /// the directory has no extension, so its descendants are not individually
    /// tombstoned. On inotify (Linux) each child file under a recursive watch
    /// emits its own removal event, which IS handled, so this only affects
    /// backends that collapse a subtree delete into one directory event.
    fn classify_event_path(&self, path: &AbsPathBuf) -> EventPathAction {
        if self.watched_only_file_entries.contains(path) {
            return EventPathAction::WatchOnly;
        }
        match fs::symlink_metadata(path) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return match self.classify_watched_path(path) {
                        Some(loader::LoadMode::WatchOnly) => EventPathAction::WatchOnly,
                        _ => EventPathAction::Ignore,
                    };
                }
                if meta.file_type().is_dir() {
                    if self.watched_dir_entries.iter().any(|dir| dir.contains_dir(path)) {
                        return EventPathAction::WatchDir;
                    }
                    return EventPathAction::Ignore;
                }
                if !meta.file_type().is_file() {
                    return EventPathAction::Ignore;
                }
                match self.classify_watched_path(path) {
                    Some(loader::LoadMode::LoadContent) => EventPathAction::LoadContent,
                    Some(loader::LoadMode::WatchOnly) => EventPathAction::WatchOnly,
                    None => EventPathAction::Ignore,
                }
            }
            // Only an actual absence (`NotFound`) is a removal. A transient stat
            // error (permissions, interrupted, a momentary race) must NOT tombstone
            // an existing file, so anything else is ignored and left as-is.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                match self.classify_watched_path(path) {
                    Some(loader::LoadMode::LoadContent) => EventPathAction::Delete,
                    Some(loader::LoadMode::WatchOnly) => EventPathAction::WatchOnly,
                    // An extension-less path under a watched root that is gone is
                    // most likely a removed directory; hand it to the consumer to
                    // expand into its loaded descendants. (Removed unwatched FILES
                    // have an extension and are ignored — nothing tracks them.)
                    None if path.extension().is_none() && self.within_watched_root(path) => {
                        EventPathAction::DeleteSubtree
                    }
                    None => EventPathAction::Ignore,
                }
            }
            Err(_) => EventPathAction::Ignore,
        }
    }

    /// Whether `path` lies under one of the watched directory roots (used to
    /// recognize a removed directory, which has no extension to classify by).
    fn within_watched_root(&self, path: &AbsPathBuf) -> bool {
        self.watched_dir_entries.iter().any(|dir| dir.contains_dir(path))
    }

    fn classify_watched_path(&self, path: &AbsPathBuf) -> Option<loader::LoadMode> {
        if self.watched_file_entries.contains(path) {
            return Some(loader::LoadMode::LoadContent);
        }
        if self.watched_only_file_entries.contains(path) {
            return Some(loader::LoadMode::WatchOnly);
        }
        let mut found: Option<loader::LoadMode> = None;
        for dir in &self.watched_dir_entries {
            if let Some(m) = dir.classify_file(path) {
                match m {
                    loader::LoadMode::LoadContent => {
                        return Some(loader::LoadMode::LoadContent);
                    }
                    loader::LoadMode::WatchOnly => {
                        found.get_or_insert(loader::LoadMode::WatchOnly);
                    }
                }
            }
        }
        found
    }

    fn count_files_in_entry(entry: &loader::Entry, cancel: &AtomicBool) -> usize {
        match entry {
            loader::Entry::Files(files) | loader::Entry::WatchOnlyFiles(files) => files.len(),
            loader::Entry::Directories(dirs) => {
                let roots: Vec<&std::path::Path> =
                    dirs.include.iter().map(|p| p.as_path().as_ref()).collect();
                let excludes: Vec<&std::path::Path> =
                    dirs.exclude.iter().map(|p| p.as_path().as_ref()).collect();
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
            let exact = self
                .watched_file_entries
                .iter()
                .chain(&self.watched_only_file_entries)
                .any(|entry| entry.as_path() == path);
            let target = if exact { path.parent().unwrap_or(path) } else { path };
            let mode = if exact { RecursiveMode::NonRecursive } else { RecursiveMode::Recursive };
            log_notify_error(watcher.watch(target, mode));
        }
    }

    #[track_caller]
    fn send(&self, msg: loader::Message) {
        if self.shutdown.load(Ordering::Relaxed) {
            return;
        }
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

fn symlink_might_be_cyclic(path: &Path) -> bool {
    let Ok(destination) = std::fs::read_link(path) else {
        return false;
    };

    let is_relative_parent =
        destination.components().all(|c| matches!(c, Component::CurDir | Component::ParentDir));

    is_relative_parent || path.starts_with(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

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

    #[derive(Default)]
    struct LoadCapture {
        loaded: Vec<Vec<(AbsPathBuf, usize)>>,
        watch_only: Vec<Vec<AbsPathBuf>>,
    }

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
        let (_guard, dirs) = fixture(1, 8 * 1024);
        let chunks = run(dirs, 1024);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 1);
        assert_eq!(chunks[0][0].1, 8 * 1024);
    }

    #[test]
    fn parallel_pool_delivers_every_file_exactly_once() {
        // With N>1 readers, every walked file must be delivered once: no loss,
        // no duplication, regardless of read-completion order.
        let (_guard, dirs) = fixture(500, 64);
        let chunks = run(dirs, 1024 * 1024);
        let all: Vec<AbsPathBuf> = chunks.into_iter().flatten().map(|(p, _)| p).collect();
        let unique: std::collections::HashSet<AbsPathBuf> = all.iter().cloned().collect();
        assert_eq!(all.len(), 500, "each file must be delivered exactly once");
        assert_eq!(unique.len(), 500, "all 500 distinct files must be delivered");
    }

    #[test]
    fn slow_consumer_backpressure_does_not_deadlock() {
        // A consumer that sleeps on every `send_loaded` parks the loader while
        // readers fill the bounded results channel; the loader must keep draining
        // and still deliver every file (backpressure smoke test).
        let (_guard, dirs) = fixture(300, 4 * 1024);
        let shutdown = AtomicBool::new(false);
        let delivered = Mutex::new(0usize);
        NotifyActor::load_entry(
            |_| {},
            loader::Entry::Directories(dirs),
            false,
            |_| {},
            || {},
            |files| {
                std::thread::sleep(std::time::Duration::from_millis(1));
                *delivered.lock().unwrap() += files.len();
            },
            8 * 1024,
            |_| {},
            WATCH_ONLY_CHUNK_PATHS,
            &shutdown,
        );
        assert_eq!(*delivered.lock().unwrap(), 300, "every file delivered despite slow consumer");
    }

    #[test]
    fn shutdown_midscan_joins_readers_without_hang() {
        // Flipping the shutdown latch partway through the scan must terminate the
        // walk, drain in-flight reads, and join the reader pool (reaching the
        // assertion at all proves the join did not hang). The aborted scan
        // suppresses the tail flush, so not all files are delivered.
        let (_guard, dirs) = fixture(500, 64);
        let shutdown = AtomicBool::new(false);
        let seen = AtomicUsize::new(0);
        let delivered = Mutex::new(0usize);
        NotifyActor::load_entry(
            |_| {},
            loader::Entry::Directories(dirs),
            false,
            |_| {},
            || {
                if seen.fetch_add(1, Ordering::AcqRel) + 1 >= 10 {
                    shutdown.store(true, Ordering::Release);
                }
            },
            |files| {
                *delivered.lock().unwrap() += files.len();
            },
            1024 * 1024,
            |_| {},
            WATCH_ONLY_CHUNK_PATHS,
            &shutdown,
        );
        assert!(*delivered.lock().unwrap() <= 500);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_file_is_delivered_as_none() {
        // A file that exists at walk time but whose bytes cannot be read (here a
        // 0o000 file) is delivered with `None` contents, so the consumer can
        // degrade it rather than the read being silently dropped.
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let readable = dir.path().join("ok.txt");
        std::fs::write(&readable, b"hello").expect("write readable");
        let blocked = dir.path().join("blocked.txt");
        std::fs::write(&blocked, b"secret").expect("write blocked");
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000))
            .expect("chmod 000");

        // Root bypasses permission bits — if we can still read it, skip.
        if std::fs::read(&blocked).is_ok() {
            let _ = std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o644));
            return;
        }

        let abs_root = AbsPathBuf::assert_utf8(dir.path().to_path_buf());
        let dirs = loader::Directories {
            extensions: vec!["txt".to_string()],
            include: vec![abs_root],
            exclude: vec![],
            rules: Vec::new(),
        };

        let shutdown = AtomicBool::new(false);
        let loaded: Mutex<Vec<(AbsPathBuf, Option<usize>)>> = Mutex::new(Vec::new());
        NotifyActor::load_entry(
            |_| {},
            loader::Entry::Directories(dirs),
            false,
            |_| {},
            || {},
            |files| {
                let mut g = loaded.lock().unwrap();
                for (p, c) in files {
                    g.push((p, c.map(|v| v.len())));
                }
            },
            1024 * 1024,
            |_| {},
            WATCH_ONLY_CHUNK_PATHS,
            &shutdown,
        );

        let _ = std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o644));

        let loaded = loaded.into_inner().unwrap();
        let find =
            |name: &str| loaded.iter().find(|(p, _)| p.file_name() == Some(name)).map(|(_, c)| *c);
        assert_eq!(find("ok.txt"), Some(Some(5)), "readable file delivers its bytes");
        assert_eq!(find("blocked.txt"), Some(None), "unreadable file delivers None");
    }

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

        for chunk in &cap.loaded {
            for (path, _) in chunk {
                assert_ne!(path.extension(), Some("xml"), "xml must not be in loaded buffer");
            }
        }
        for chunk in &cap.watch_only {
            for path in chunk {
                assert_ne!(path.extension(), Some("bsl"), "bsl must not be in watch-only buffer");
            }
        }
    }

    #[test]
    fn pure_watch_only_emits_only_watch_only_chunks() {
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
    fn explicit_watch_only_files_are_never_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = AbsPathBuf::assert_utf8(dir.path().join("baseline.json"));
        std::fs::write(path.as_path(), b"secret").unwrap();
        let loaded = Mutex::new(Vec::new());
        let watched = Mutex::new(Vec::new());
        NotifyActor::load_entry(
            |_| {},
            loader::Entry::WatchOnlyFiles(vec![path.clone()]),
            false,
            |_| {},
            || {},
            |files| loaded.lock().unwrap().extend(files),
            LOADED_CHUNK_BYTES,
            |files| watched.lock().unwrap().extend(files),
            WATCH_ONLY_CHUNK_PATHS,
            &AtomicBool::new(false),
        );
        assert!(loaded.into_inner().unwrap().is_empty());
        assert_eq!(watched.into_inner().unwrap(), vec![path]);
    }

    #[test]
    fn watch_only_chunking_respects_path_threshold() {
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

    fn actor_with_watched_dirs(dirs: Vec<loader::Directories>) -> NotifyActor {
        let (tx, _rx) = crossbeam_channel::unbounded::<loader::Message>();
        let mut actor = NotifyActor::new(tx, Arc::new(AtomicBool::new(false)));
        actor.watched_dir_entries = dirs;
        actor
    }

    fn dirs_for(
        root: &AbsPathBuf,
        content_ext: &[&str],
        watch_ext: &[&str],
    ) -> loader::Directories {
        let rules = if watch_ext.is_empty() {
            Vec::new()
        } else {
            vec![loader::FileRule {
                extensions: watch_ext.iter().map(|s| (*s).to_string()).collect(),
                load_mode: loader::LoadMode::WatchOnly,
            }]
        };
        loader::Directories {
            extensions: content_ext.iter().map(|s| (*s).to_string()).collect(),
            include: vec![root.clone()],
            exclude: vec![],
            rules,
        }
    }

    #[test]
    fn classify_watched_path_dispatches_load_modes() {
        let (_g, root) = fixture_mixed(0, 0, 0);
        let dirs = dirs_for(&root, &["bsl"], &["xml"]);
        let actor = actor_with_watched_dirs(vec![dirs]);

        let bsl =
            AbsPathBuf::assert_utf8(AsRef::<std::path::Path>::as_ref(&root).join("Module.bsl"));
        let xml = AbsPathBuf::assert_utf8(AsRef::<std::path::Path>::as_ref(&root).join("Form.xml"));
        let other =
            AbsPathBuf::assert_utf8(AsRef::<std::path::Path>::as_ref(&root).join("README.md"));

        assert_eq!(actor.classify_watched_path(&bsl), Some(loader::LoadMode::LoadContent));
        assert_eq!(actor.classify_watched_path(&xml), Some(loader::LoadMode::WatchOnly));
        assert_eq!(actor.classify_watched_path(&other), None);
    }

    #[test]
    fn classify_event_path_delivers_removals_and_loads_existing() {
        let (_g, root) = fixture_mixed(0, 0, 0);
        let dirs = dirs_for(&root, &["bsl"], &["xml"]);
        let actor = actor_with_watched_dirs(vec![dirs]);

        let join = |name: &str| {
            AbsPathBuf::assert_utf8(AsRef::<std::path::Path>::as_ref(&root).join(name))
        };

        // A removed (no longer stat-able) watched content file must be delivered
        // as a deletion, not dropped — this is the regression guard.
        assert_eq!(actor.classify_event_path(&join("Gone.bsl")), EventPathAction::Delete);
        // A removed watch-only file routes to metadata invalidation.
        assert_eq!(actor.classify_event_path(&join("Gone.xml")), EventPathAction::WatchOnly);
        // A removed unwatched FILE (has an extension) is ignored.
        assert_eq!(actor.classify_event_path(&join("Gone.md")), EventPathAction::Ignore);
        // A removed extension-less path under a watched root looks like a removed
        // directory → handed to the consumer to expand into descendants.
        assert_eq!(actor.classify_event_path(&join("RemovedDir")), EventPathAction::DeleteSubtree);

        // An existing watched content file still loads its bytes.
        let existing = join("Module.bsl");
        std::fs::write(AsRef::<std::path::Path>::as_ref(&existing), b"x").unwrap();
        assert_eq!(actor.classify_event_path(&existing), EventPathAction::LoadContent);
    }

    #[test]
    fn classify_watched_path_is_case_insensitive() {
        let (_g, root) = fixture_mixed(0, 0, 0);
        let dirs = dirs_for(&root, &["bsl"], &["xml"]);
        let actor = actor_with_watched_dirs(vec![dirs]);

        let bsl_upper =
            AbsPathBuf::assert_utf8(AsRef::<std::path::Path>::as_ref(&root).join("Module.BSL"));
        let xml_upper =
            AbsPathBuf::assert_utf8(AsRef::<std::path::Path>::as_ref(&root).join("Form.XML"));
        assert_eq!(actor.classify_watched_path(&bsl_upper), Some(loader::LoadMode::LoadContent));
        assert_eq!(actor.classify_watched_path(&xml_upper), Some(loader::LoadMode::WatchOnly));
    }

    #[test]
    fn classify_watched_path_load_content_wins_across_entries() {
        let (_g, root) = fixture_mixed(0, 0, 0);
        let load_first = vec![dirs_for(&root, &["xml"], &[]), dirs_for(&root, &[], &["xml"])];
        let load_last = vec![dirs_for(&root, &[], &["xml"]), dirs_for(&root, &["xml"], &[])];
        let xml = AbsPathBuf::assert_utf8(AsRef::<std::path::Path>::as_ref(&root).join("a.xml"));

        let actor1 = actor_with_watched_dirs(load_first);
        let actor2 = actor_with_watched_dirs(load_last);
        assert_eq!(actor1.classify_watched_path(&xml), Some(loader::LoadMode::LoadContent));
        assert_eq!(actor2.classify_watched_path(&xml), Some(loader::LoadMode::LoadContent));
    }

    #[test]
    fn pre_set_shutdown_skips_scan_and_flush() {
        let (_guard, dirs) = fixture(50, 1024);
        let shutdown = AtomicBool::new(true);
        let chunks = run_with_shutdown(dirs, 100 * 1024, &shutdown);
        assert!(chunks.is_empty(), "shutdown latch must suppress all sends, got {chunks:#?}");
    }

    #[test]
    fn handle_drop_latches_shutdown() {
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
