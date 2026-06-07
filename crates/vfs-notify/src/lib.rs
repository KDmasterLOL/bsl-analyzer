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

const LOADED_CHUNK_BYTES: usize = 64 * 1024 * 1024;

const WATCH_ONLY_CHUNK_PATHS: usize = 4096;

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
                                    _ = watcher_sender.send(event);
                                },
                                Config::default(),
                            ));
                            self.watcher = watcher.map(|it| (it, watcher_receiver));
                        }

                        let config_version = config.version;

                        self.watched_dir_entries.clear();
                        self.watched_file_entries.clear();

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
                        config.load.into_par_iter().enumerate().for_each(|(i, entry)| {
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
                        });

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
                            let mut changed_files: Vec<(AbsPathBuf, Option<Vec<u8>>)> = Vec::new();
                            let mut watch_only_files: Vec<AbsPathBuf> = Vec::new();
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
                                    EventPathAction::Ignore => {}
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
            loader::Entry::Directories(dirs) => {
                for root in &dirs.include {
                    if shutdown.load(Ordering::Relaxed) {
                        return;
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
        match fs::metadata(path) {
            Ok(meta) => {
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
                    None => EventPathAction::Ignore,
                }
            }
            Err(_) => EventPathAction::Ignore,
        }
    }

    fn classify_watched_path(&self, path: &AbsPathBuf) -> Option<loader::LoadMode> {
        if self.watched_file_entries.contains(path) {
            return Some(loader::LoadMode::LoadContent);
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
            loader::Entry::Files(files) => files.len(),
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
            log_notify_error(watcher.watch(path, RecursiveMode::Recursive));
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
        // A removed unwatched file is ignored.
        assert_eq!(actor.classify_event_path(&join("Gone.md")), EventPathAction::Ignore);

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
