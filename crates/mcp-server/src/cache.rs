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

/// Names the workspace whose derived caches a configurable root holds.
const OWNER_STAMP_FILE: &str = "workspace-owner";

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

    /// Bind this cache root to `workspace_root`, refusing a root that already serves another
    /// workspace.
    ///
    /// Until the root became configurable, a shared cache path meant a shared workspace, and
    /// both the writer lease and fingerprint-based cache adoption still assume it: two daemons
    /// pointed at one root would fight over one lease and answer queries from a graph built
    /// out of a different configuration.
    ///
    /// Two shapes of prior occupancy are refused. A stamp naming a different workspace is the
    /// direct one. The other is a root already holding derived databases without any stamp:
    /// the default `<workspace>/.build` is deliberately never stamped, so pointing
    /// `--cache-dir` at a neighbouring project's default would otherwise be adopted in
    /// silence. Re-using one's OWN default root explicitly stays allowed — same directory,
    /// same workspace.
    pub fn claim_workspace(&self, workspace_root: &Path) -> std::io::Result<()> {
        self.ensure()?;
        let owner = workspace_root.to_string_lossy();
        let owner = owner.trim();

        match self.recorded_owner()? {
            Some(recorded) if recorded == owner => return Ok(()),
            Some(recorded) => return Err(self.occupied_by(&recorded, workspace_root)),
            None => {}
        }
        if self.holds_derived_databases()
            && self.root != Self::for_workspace(workspace_root).root
            && !self.stamp_path().exists()
        {
            return Err(self.occupied_by("another workspace", workspace_root));
        }
        self.write_stamp(owner)
    }

    fn stamp_path(&self) -> PathBuf {
        self.root.join(OWNER_STAMP_FILE)
    }

    /// The recorded owner, or `None` when the root is unclaimed.
    ///
    /// An empty stamp is `None` rather than an owner named "": the writer creates the file and
    /// fills it with a second syscall, so a reader can catch that window, and a writer killed
    /// inside it leaves the empty file behind for good. Reading it as an owner would refuse
    /// the very workspace that created the directory, permanently and with a message naming
    /// nobody. The short retry covers the live race; a stamp still empty afterwards is an
    /// orphan and gets rewritten by the caller.
    fn recorded_owner(&self) -> std::io::Result<Option<String>> {
        const RETRIES: u32 = 5;
        for attempt in 0..RETRIES {
            match std::fs::read_to_string(self.stamp_path()) {
                Ok(recorded) if !recorded.trim().is_empty() => {
                    return Ok(Some(recorded.trim().to_owned()))
                }
                Ok(_) if attempt + 1 < RETRIES => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Ok(_) => return Ok(None),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    /// Write the stamp so no reader can observe it half-written: fill a temporary file, then
    /// rename it over the name. The pid suffix keeps two racing writers off one temp file.
    fn write_stamp(&self, owner: &str) -> std::io::Result<()> {
        let temp = self.root.join(format!("{OWNER_STAMP_FILE}.{}", std::process::id()));
        std::fs::write(&temp, owner.as_bytes())?;
        std::fs::rename(&temp, self.stamp_path())
    }

    fn holds_derived_databases(&self) -> bool {
        self.graph_db_path().exists() || self.search_db_path().exists()
    }

    fn occupied_by(&self, recorded: &str, workspace_root: &Path) -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "cache directory {} already holds the derived caches of {}; \
                 give {} a cache directory of its own",
                self.root.display(),
                recorded,
                workspace_root.display()
            ),
        )
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

    /// The stamp is created and filled by two separate syscalls, so a reader can see it empty
    /// — and a writer killed in between leaves it empty for good. An empty stamp names nobody
    /// and must never lock its own workspace out of the directory it created.
    #[test]
    fn an_empty_stamp_is_repaired_rather_than_read_as_a_foreign_owner() {
        let cache = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(cache.path().join(OWNER_STAMP_FILE), "").unwrap();
        let layout = WorkspaceCacheLayout::from_root(cache.path().to_path_buf());

        layout.claim_workspace(workspace.path()).unwrap();

        let repaired = std::fs::read_to_string(cache.path().join(OWNER_STAMP_FILE)).unwrap();
        assert_eq!(repaired.trim(), workspace.path().to_string_lossy().trim());
    }

    /// The default root is never stamped, so a neighbouring project's `.build` carries no
    /// owner — yet it is exactly the directory a mistyped `--cache-dir` lands in.
    #[test]
    fn an_unstamped_directory_holding_databases_is_refused() {
        let neighbour = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let occupied = neighbour.path().join(".build");
        std::fs::create_dir_all(&occupied).unwrap();
        std::fs::write(occupied.join("bsl-graph.db"), b"the neighbour's graph").unwrap();
        let layout = WorkspaceCacheLayout::from_root(occupied);

        let refused = layout.claim_workspace(workspace.path()).unwrap_err();

        assert_eq!(refused.kind(), std::io::ErrorKind::AlreadyExists);
    }

    /// Pointing `--cache-dir` at one's own default root is the same directory the daemon
    /// would have used anyway, databases and all.
    #[test]
    fn a_workspace_may_claim_its_own_populated_default_root() {
        let workspace = tempfile::tempdir().unwrap();
        let own = workspace.path().join(".build");
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(own.join("bsl-graph.db"), b"its own graph").unwrap();
        let layout = WorkspaceCacheLayout::for_workspace(workspace.path());

        layout.claim_workspace(workspace.path()).unwrap();
    }
}
