//! Dynamically compatible interface for file watching and reading.
use std::fmt;

use paths::{AbsPath, AbsPathBuf};

/// A set of files on the file system.
#[derive(Debug, Clone)]
pub enum Entry {
    /// The `Entry` is represented by a raw set of files.
    Files(Vec<AbsPathBuf>),
    /// The `Entry` is represented by `Directories`.
    Directories(Directories),
}

/// What the loader should do with files matching a [`FileRule`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LoadMode {
    /// Read the file contents and ship them via [`Message::Loaded`] /
    /// [`Message::Changed`]. This is the default behaviour for the legacy
    /// [`Directories::extensions`] field — every extension listed there is
    /// implicitly `LoadContent`.
    LoadContent,
    /// Allocate a `FileId` and register the path for change-tracking, but
    /// do not read its bytes from disk. The loader emits
    /// [`Message::WatchOnly`] (instead of `Loaded`) so the consumer can
    /// react without paying the resident-memory cost of an `Arc<str>` copy.
    WatchOnly,
}

/// A rule that classifies files inside a [`Directories`] entry by extension.
///
/// Rules are layered on top of [`Directories::extensions`]: a path matching
/// `extensions` is always [`LoadMode::LoadContent`]; a path matching a
/// rule's extension is dispatched per the rule's [`LoadMode`]. If both
/// match, `extensions` wins (load-content semantics) so a half-migrated
/// caller never silently downgrades an existing extension to watch-only.
#[derive(Debug, Clone)]
pub struct FileRule {
    /// File extensions matched by this rule (no leading dot).
    pub extensions: Vec<String>,
    /// What to do with files matching this rule.
    pub load_mode: LoadMode,
}

/// Specifies a set of files on the file system.
///
/// A file is included if:
///   * it has an extension listed in [`extensions`](Self::extensions) or
///     matches one of the [`rules`](Self::rules)
///   * it is under an `include` path
///   * it is not under `exclude` path
///
/// If many include/exclude paths match, the longest one wins.
///
/// If a path is in both `include` and `exclude`, the `exclude` one wins.
#[derive(Debug, Clone, Default)]
pub struct Directories {
    pub extensions: Vec<String>,
    pub include: Vec<AbsPathBuf>,
    pub exclude: Vec<AbsPathBuf>,
    /// Per-extension rules layered on top of `extensions`. Empty in legacy
    /// callers; populated by the project-model classifier (PR-3) for
    /// metadata files (XML) that should be watched but not resident in
    /// the VFS as `Arc<str>`.
    pub rules: Vec<FileRule>,
}

/// [`Handle`]'s configuration.
#[derive(Debug)]
pub struct Config {
    /// Version number to associate progress updates to the right config
    /// version.
    pub version: u32,
    /// Set of initially loaded files.
    pub load: Vec<Entry>,
    /// Index of watched entries in `load`.
    ///
    /// If a path in a watched entry is modified,the [`Handle`] should notify it.
    pub watch: Vec<usize>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LoadingProgress {
    /// Scanning workspace directories (before file count is known).
    Scanning,
    /// File count determined, loading started.
    Started,
    /// Loading in progress (n files loaded).
    Progress(usize),
    /// Loading finished.
    Finished,
}

/// Message about an action taken by a [`Handle`].
pub enum Message {
    /// Indicate a gradual progress.
    ///
    /// This is supposed to be the number of loaded files.
    Progress {
        /// The total files to be loaded.
        n_total: usize,
        /// The files that have been loaded successfully.
        n_done: LoadingProgress,
        /// The dir being loaded, `None` if its for a file.
        dir: Option<AbsPathBuf>,
        /// The [`Config`] version.
        config_version: u32,
    },
    /// The handle loaded the following files' content for the first time.
    Loaded { files: Vec<(AbsPathBuf, Option<Vec<u8>>)> },
    /// The handle loaded the following files' content.
    Changed { files: Vec<(AbsPathBuf, Option<Vec<u8>>)> },
    /// The handle observed paths matching a [`LoadMode::WatchOnly`] rule.
    /// Only the paths are shipped; contents are not read. Consumers that
    /// need a `FileId` for change-tracking should pair this with
    /// [`Vfs::register_watch_only`](crate::Vfs::register_watch_only).
    ///
    /// Currently emitted for the initial scan and for `Create` / `Modify`
    /// notify events. **Not yet emitted for `Remove`**: the watcher path
    /// requires `fs::metadata` and silently drops deletions for both load
    /// modes — a pre-existing limitation, deferred to PR-3 alongside the
    /// `register_watch_only` consumer wiring (emitting deletions with no
    /// consumer to observe them is worse than the latent gap). When PR-3
    /// lands, consumers will handle add / modify / delete uniformly by
    /// re-running their downstream loader (e.g. metadata XML reader).
    WatchOnly { files: Vec<AbsPathBuf> },
}

/// Type that will receive [`Messages`](Message) from a [`Handle`].
pub type Sender = crossbeam_channel::Sender<Message>;

/// Interface for reading and watching files.
pub trait Handle: fmt::Debug {
    /// Spawn a new handle with the given `sender`.
    fn spawn(sender: Sender) -> Self
    where
        Self: Sized;

    /// Set this handle's configuration.
    fn set_config(&mut self, config: Config);

    /// The file's content at `path` has been modified, and should be reloaded.
    fn invalidate(&mut self, path: AbsPathBuf);

    /// Load the content of the given file, returning [`None`] if it does not
    /// exists.
    fn load_sync(&mut self, path: &AbsPath) -> Option<Vec<u8>>;
}

impl Entry {
    /// Returns:
    /// ```text
    /// Entry::Directories(Directories {
    ///     extensions: ["rs"],
    ///     include: [base],
    ///     exclude: [base/.git],
    /// })
    /// ```
    pub fn rs_files_recursively(base: AbsPathBuf) -> Entry {
        Entry::Directories(dirs(base, &[".git"]))
    }

    /// Returns:
    /// ```text
    /// Entry::Directories(Directories {
    ///     extensions: ["rs"],
    ///     include: [base],
    ///     exclude: [base/.git, base/target],
    /// })
    /// ```
    pub fn local_cargo_package(base: AbsPathBuf) -> Entry {
        Entry::Directories(dirs(base, &[".git", "target"]))
    }

    /// Returns:
    /// ```text
    /// Entry::Directories(Directories {
    ///     extensions: ["rs"],
    ///     include: [base],
    ///     exclude: [base/.git, /tests, /examples, /benches],
    /// })
    /// ```
    pub fn cargo_package_dependency(base: AbsPathBuf) -> Entry {
        Entry::Directories(dirs(base, &[".git", "/tests", "/examples", "/benches"]))
    }

    /// Returns `true` if `path` is included in `self`.
    ///
    /// See [`Directories::contains_file`].
    pub fn contains_file(&self, path: &AbsPath) -> bool {
        match self {
            Entry::Files(files) => files.iter().any(|it| it == path),
            Entry::Directories(dirs) => dirs.contains_file(path),
        }
    }

    /// Returns `true` if `path` is included in `self`.
    ///
    /// - If `self` is `Entry::Files`, returns `false`
    /// - Else, see [`Directories::contains_dir`].
    pub fn contains_dir(&self, path: &AbsPath) -> bool {
        match self {
            Entry::Files(_) => false,
            Entry::Directories(dirs) => dirs.contains_dir(path),
        }
    }
}

impl Directories {
    /// Classify `path` by [`LoadMode`], or return `None` if the path is not
    /// included in this directory entry.
    ///
    /// Resolution order:
    /// 1. The path must be under [`include`](Self::include) and not under
    ///    [`exclude`](Self::exclude); otherwise returns `None`.
    /// 2. If the extension is in [`extensions`](Self::extensions), returns
    ///    [`LoadMode::LoadContent`].
    /// 3. Otherwise, the first matching rule in [`rules`](Self::rules) wins.
    /// 4. If no rule matches, returns `None`.
    pub fn classify_file(&self, path: &AbsPath) -> Option<LoadMode> {
        if !self.includes_path(path) {
            return None;
        }
        let ext = path.extension().unwrap_or_default();
        // Case-insensitive — matches `project_model::file_role` (the
        // single source of truth at the LSP boundary) and 1C:Enterprise
        // semantics on Windows / macOS filesystems where `Module.BSL`
        // and `Module.bsl` are the same file. Without this, a deleted
        // `Form.XML` from a Windows-authored configuration would be
        // dropped from the watcher dispatch.
        if self.extensions.iter().any(|it| it.eq_ignore_ascii_case(ext)) {
            return Some(LoadMode::LoadContent);
        }
        for rule in &self.rules {
            if rule.extensions.iter().any(|it| it.eq_ignore_ascii_case(ext)) {
                return Some(rule.load_mode);
            }
        }
        None
    }

    /// Returns `true` if `path` is included in `self` under any load mode.
    pub fn contains_file(&self, path: &AbsPath) -> bool {
        self.classify_file(path).is_some()
    }

    /// Returns `true` if `path` is included in `self`.
    ///
    /// Since `path` is supposed to be a directory, this will not take extension
    /// into account.
    pub fn contains_dir(&self, path: &AbsPath) -> bool {
        self.includes_path(path)
    }

    /// Returns `true` if `path` is included in `self`.
    ///
    /// It is included if
    ///   - An element in `self.include` is a prefix of `path`.
    ///   - This path is longer than any element in `self.exclude` that is a prefix
    ///     of `path`. In case of equality, exclusion wins.
    fn includes_path(&self, path: &AbsPath) -> bool {
        let mut include: Option<&AbsPathBuf> = None;
        for incl in &self.include {
            if path.starts_with(incl) {
                include = Some(match include {
                    Some(prev) if prev.starts_with(incl) => prev,
                    _ => incl,
                });
            }
        }

        let include = match include {
            Some(it) => it,
            None => return false,
        };

        !self.exclude.iter().any(|excl| path.starts_with(excl) && excl.starts_with(include))
    }
}

/// Returns :
/// ```text
/// Directories {
///     extensions: ["rs"],
///     include: [base],
///     exclude: [base/<exclude>],
/// }
/// ```
fn dirs(base: AbsPathBuf, exclude: &[&str]) -> Directories {
    let exclude = exclude.iter().map(|it| base.join(it)).collect::<Vec<_>>();
    Directories {
        extensions: vec!["rs".to_owned()],
        include: vec![base],
        exclude,
        rules: Vec::new(),
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::Loaded { files } => {
                f.debug_struct("Loaded").field("n_files", &files.len()).finish()
            }
            Message::Changed { files } => {
                f.debug_struct("Changed").field("n_files", &files.len()).finish()
            }
            Message::WatchOnly { files } => {
                f.debug_struct("WatchOnly").field("n_files", &files.len()).finish()
            }
            Message::Progress { n_total, n_done, dir, config_version } => f
                .debug_struct("Progress")
                .field("n_total", n_total)
                .field("n_done", n_done)
                .field("dir", dir)
                .field("config_version", config_version)
                .finish(),
        }
    }
}

#[test]
fn handle_is_dyn_compatible() {
    fn _assert(_: &dyn Handle) {}
}
