//! The single owner of the per-workspace derived-cache directory.
//!
//! Every SQLite index the server builds from a workspace (the call graph and the
//! code-search index) lives under `<workspace>/.build`. Centralising the path here
//! keeps the directory layout in one place instead of being reconstructed ad-hoc at
//! each call site. The directory is a rebuildable cache: it is safe to delete and is
//! re-created on demand.
//!
//! Workspace-independent caches (the platform reference-search database) live in the
//! user's OS cache directory, not here — see `state::reference_search_db_path`.

use std::path::{Path, PathBuf};

/// Resolved locations of every cache derived from one workspace.
///
/// The source tree and the cache root are deliberately independent: callers may
/// keep the default `<workspace>/.build` layout or supply an external root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCacheLayout {
    root: PathBuf,
}

impl WorkspaceCacheLayout {
    /// The backwards-compatible lazy layout under `<workspace>/.build`.
    pub fn for_workspace(workspace_root: &Path) -> Self {
        let root = workspace_root.join(".build");
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        Self { root }
    }

    /// A layout whose root has already been resolved by the caller.
    pub fn from_root(root: PathBuf) -> Self {
        Self { root }
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
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Bind this cache root to `workspace_root`, refusing a root another workspace already
    /// owns.
    ///
    /// Until the root became configurable, a shared cache path meant a shared workspace, and
    /// both the writer lease and fingerprint-based cache adoption still assume it: two
    /// daemons pointed at one root would fight over one lease and answer queries from a graph
    /// built out of a different configuration. The stamp is written exclusively, so of two
    /// processes racing on a fresh root exactly one creates it and the other reads it back
    /// and compares.
    pub fn claim_workspace(&self, workspace_root: &Path) -> std::io::Result<()> {
        self.ensure()?;
        let stamp = self.root.join("workspace-owner");
        let owner = workspace_root.to_string_lossy();
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&stamp) {
            Ok(mut file) => {
                use std::io::Write;
                return file.write_all(owner.as_bytes());
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let recorded = std::fs::read_to_string(&stamp)?;
        if recorded.trim() == owner.trim() {
            return Ok(());
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "cache directory {} already holds the derived caches of {}; \
                 give {} a cache directory of its own",
                self.root.display(),
                recorded.trim(),
                workspace_root.display()
            ),
        ))
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
        self.root.join("writer.lease.lock")
    }

    pub fn stall_report_path(&self) -> PathBuf {
        self.root.join("bsl-graph-stall-report.txt")
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

    #[test]
    fn a_cache_root_serves_one_workspace_and_refuses_the_next() {
        let shared = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let layout = WorkspaceCacheLayout::from_root(shared.path().to_path_buf());

        layout.claim_workspace(first.path()).unwrap();
        // Re-claiming is how the second daemon generation over the same workspace comes up.
        layout.claim_workspace(first.path()).unwrap();
        let refused = layout.claim_workspace(second.path()).unwrap_err();

        assert_eq!(refused.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(refused.to_string().contains(&first.path().display().to_string()));
    }
}
