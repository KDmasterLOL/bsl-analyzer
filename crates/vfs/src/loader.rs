use std::fmt;

use paths::{AbsPath, AbsPathBuf};

#[derive(Debug, Clone)]
pub enum Entry {
    Files(Vec<AbsPathBuf>),
    Directories(Directories),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LoadMode {
    LoadContent,
    WatchOnly,
}

#[derive(Debug, Clone)]
pub struct FileRule {
    pub extensions: Vec<String>,
    pub load_mode: LoadMode,
}

#[derive(Debug, Clone, Default)]
pub struct Directories {
    pub extensions: Vec<String>,
    pub include: Vec<AbsPathBuf>,
    pub exclude: Vec<AbsPathBuf>,
    pub rules: Vec<FileRule>,
}

#[derive(Debug)]
pub struct Config {
    pub version: u32,
    pub load: Vec<Entry>,
    pub watch: Vec<usize>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LoadingProgress {
    Scanning,
    Started,
    Progress(usize),
    Finished,
}

pub enum Message {
    Progress {
        n_total: usize,
        n_done: LoadingProgress,
        dir: Option<AbsPathBuf>,
        config_version: u32,
    },
    Loaded {
        files: Vec<(AbsPathBuf, Option<Vec<u8>>)>,
    },
    Changed {
        files: Vec<(AbsPathBuf, Option<Vec<u8>>)>,
    },
    WatchOnly {
        files: Vec<AbsPathBuf>,
    },
}

pub type Sender = crossbeam_channel::Sender<Message>;

pub trait Handle: fmt::Debug {
    fn spawn(sender: Sender) -> Self
    where
        Self: Sized;

    fn set_config(&mut self, config: Config);

    fn invalidate(&mut self, path: AbsPathBuf);

    fn load_sync(&mut self, path: &AbsPath) -> Option<Vec<u8>>;
}

impl Entry {
    pub fn rs_files_recursively(base: AbsPathBuf) -> Entry {
        Entry::Directories(dirs(base, &[".git"]))
    }

    pub fn local_cargo_package(base: AbsPathBuf) -> Entry {
        Entry::Directories(dirs(base, &[".git", "target"]))
    }

    pub fn cargo_package_dependency(base: AbsPathBuf) -> Entry {
        Entry::Directories(dirs(base, &[".git", "/tests", "/examples", "/benches"]))
    }

    pub fn contains_file(&self, path: &AbsPath) -> bool {
        match self {
            Entry::Files(files) => files.iter().any(|it| it == path),
            Entry::Directories(dirs) => dirs.contains_file(path),
        }
    }

    pub fn contains_dir(&self, path: &AbsPath) -> bool {
        match self {
            Entry::Files(_) => false,
            Entry::Directories(dirs) => dirs.contains_dir(path),
        }
    }
}

impl Directories {
    pub fn classify_file(&self, path: &AbsPath) -> Option<LoadMode> {
        if !self.includes_path(path) {
            return None;
        }
        let ext = path.extension().unwrap_or_default();
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

    pub fn contains_file(&self, path: &AbsPath) -> bool {
        self.classify_file(path).is_some()
    }

    pub fn contains_dir(&self, path: &AbsPath) -> bool {
        self.includes_path(path)
    }

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
