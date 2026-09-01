//! The single owner of the per-workspace derived-cache directory.
//!
//! Every SQLite index the server builds from a workspace (the call graph and the
//! code-search index) lives under one root: `<workspace>/.build` by default, or the
//! directory `mcp serve --cache-dir` names. Centralising the path here keeps the
//! directory layout in one place instead of being reconstructed ad-hoc at each call
//! site. The directory is a rebuildable cache: it is safe to delete and is re-created
//! on demand.
//!
//! Workspace-independent caches (the platform reference-search database) live in the
//! user's OS cache directory, not here — see `state::reference_search_db_path`.

use std::path::{Path, PathBuf};

/// The lock serializing lease reads and writes. Defined here, not next to the lease code, so
/// the directory's file names have one definition and a rename cannot leave the layout behind.
pub(crate) const LEASE_LOCK_FILE: &str = "writer.lease.lock";

/// The one-shot artifact a wedged build leaves behind when daemon file logging is off.
pub(crate) const STALL_REPORT_FILE: &str = "bsl-graph-stall-report.txt";

/// Resolved locations of every cache derived from one workspace.
///
/// The source tree and the cache root are deliberately independent: callers may
/// keep the default `<workspace>/.build` layout or supply an external root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCacheLayout {
    root: PathBuf,
    /// The root as the caller spelled it, before canonicalisation.
    ///
    /// Kept because a file watcher reports events under the spelling its watch was
    /// armed with, and that is the pre-canonical one: on Windows `canonicalize`
    /// answers `\\?\C:\...` while the watcher says `C:\...`, and on any platform a
    /// symlinked root and its target are two names for the same directory. A caller
    /// that has to tell "is this event inside my own cache" needs both, and keeping
    /// only the canonical one is a filter that silently matches nothing.
    declared: PathBuf,
}

impl WorkspaceCacheLayout {
    /// The backwards-compatible lazy layout under `<workspace>/.build`.
    pub fn for_workspace(workspace_root: &Path) -> Self {
        let declared = workspace_root.join(".build");
        let root = std::fs::canonicalize(&declared).unwrap_or_else(|_| declared.clone());
        Self { root, declared }
    }

    /// A layout whose root has already been resolved by the caller.
    pub fn from_root(root: PathBuf) -> Self {
        Self { declared: root.clone(), root }
    }

    /// Resolve, create, and canonicalize an explicit `--cache-dir` value.
    pub fn prepare_explicit(path: &Path, current_dir: &Path) -> std::io::Result<Self> {
        let requested =
            if path.is_absolute() { path.to_path_buf() } else { current_dir.join(path) };
        std::fs::create_dir_all(&requested).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("failed to create --cache-dir {}: {error}", requested.display()),
            )
        })?;
        let root = requested.canonicalize().map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("failed to canonicalize --cache-dir {}: {error}", requested.display()),
            )
        })?;
        Ok(Self { root, declared: requested })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every spelling this cache root can appear under in a file-watcher event.
    ///
    /// Both, and deduplicated only by the caller: the two coincide whenever the path
    /// was already canonical, and differ exactly in the cases a single-spelling check
    /// gets wrong.
    pub fn spellings(&self) -> [&Path; 2] {
        [self.declared.as_path(), self.root.as_path()]
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)
    }

    pub fn graph_db_path(&self) -> PathBuf {
        self.root.join("bsl-graph.db")
    }

    pub fn search_db_path(&self) -> PathBuf {
        self.root.join("bsl-search.db")
    }

    pub fn lease_path(&self) -> PathBuf {
        self.root.join("writer.lease")
    }

    pub fn lease_lock_path(&self) -> PathBuf {
        self.root.join(LEASE_LOCK_FILE)
    }

    pub fn stall_report_path(&self) -> PathBuf {
        self.root.join(STALL_REPORT_FILE)
    }

    pub fn daemon_log_path(&self) -> PathBuf {
        self.root.join("bsl-analyzer-daemon.log")
    }
}

/// The per-workspace derived-cache directory (`<workspace>/.build`).
#[cfg(test)]
pub fn workspace_cache_dir(workspace_root: &Path) -> PathBuf {
    WorkspaceCacheLayout::for_workspace(workspace_root).root
}

/// Ensure the workspace cache directory exists, returning its path.
#[cfg(test)]
pub fn ensure_workspace_cache_dir(workspace_root: &Path) -> std::io::Result<PathBuf> {
    let layout = WorkspaceCacheLayout::for_workspace(workspace_root);
    layout.ensure()?;
    Ok(layout.root)
}

/// The call-graph SQLite index path under the workspace cache directory.
#[cfg(test)]
pub fn graph_db_path(workspace_root: &Path) -> PathBuf {
    WorkspaceCacheLayout::for_workspace(workspace_root).graph_db_path()
}

/// The code-search SQLite index path under the workspace cache directory.
#[cfg(test)]
pub fn search_db_path(workspace_root: &Path) -> PathBuf {
    WorkspaceCacheLayout::for_workspace(workspace_root).search_db_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_stays_under_workspace_build_without_creating_it() {
        let workspace = tempfile::tempdir().unwrap();

        let layout = WorkspaceCacheLayout::for_workspace(workspace.path());

        assert_eq!(layout.root(), workspace.path().join(".build"));
        assert!(!layout.root().exists(), "default construction stays lazy");
    }

    #[test]
    fn explicit_relative_cache_is_created_and_canonicalized() {
        let cwd = tempfile::tempdir().unwrap();

        let layout =
            WorkspaceCacheLayout::prepare_explicit(Path::new("кеш с пробелом"), cwd.path())
                .unwrap();

        assert_eq!(layout.root(), cwd.path().join("кеш с пробелом").canonicalize().unwrap());
    }

    #[test]
    fn layout_owns_every_workspace_cache_file_name() {
        let root = PathBuf::from("external-cache");
        let layout = WorkspaceCacheLayout::from_root(root.clone());

        assert_eq!(layout.graph_db_path(), root.join("bsl-graph.db"));
        assert_eq!(layout.search_db_path(), root.join("bsl-search.db"));
        assert_eq!(layout.lease_path(), root.join("writer.lease"));
        assert_eq!(layout.lease_lock_path(), root.join("writer.lease.lock"));
        assert_eq!(layout.stall_report_path(), root.join("bsl-graph-stall-report.txt"));
        assert_eq!(layout.daemon_log_path(), root.join("bsl-analyzer-daemon.log"));
    }
}
