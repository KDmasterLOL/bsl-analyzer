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

/// The per-workspace derived-cache directory (`<workspace>/.build`).
pub fn workspace_cache_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".build")
}

/// Ensure the workspace cache directory exists, returning its path.
pub fn ensure_workspace_cache_dir(workspace_root: &Path) -> std::io::Result<PathBuf> {
    let dir = workspace_cache_dir(workspace_root);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The call-graph SQLite index path under the workspace cache directory.
pub fn graph_db_path(workspace_root: &Path) -> PathBuf {
    workspace_cache_dir(workspace_root).join("bsl-graph.db")
}

/// The code-search SQLite index path under the workspace cache directory.
pub fn search_db_path(workspace_root: &Path) -> PathBuf {
    workspace_cache_dir(workspace_root).join("bsl-search.db")
}
