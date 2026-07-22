use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide::RootDatabaseImpl;
use vfs::FileId;
use walkdir::WalkDir;

/// The whole workspace is loaded into a single source root.
pub(crate) const GRAPH_SOURCE_ROOT: SourceRootId = SourceRootId(0);

/// One immutable projection of the validated project, loaded ONCE per
/// operation (graph build, incremental update, resident build): the scan
/// universe and the workspace-configs snapshot (roots + dependency closures +
/// topology fingerprint) travel together, so no operation can mix the file
/// enumeration of one project state with the config registration of another.
pub(crate) struct ProjectSnapshot {
    pub workspace_root: PathBuf,
    pub scan_roots: Vec<PathBuf>,
    pub configs: ide::WorkspaceConfigsSnapshot,
}

impl ProjectSnapshot {
    /// Graph passes run only after the daemon bootstrap validated the project;
    /// a config broken by a mid-session edit restricts the scan to the
    /// workspace root (loud in logs) instead of walking a wrong universe.
    pub(crate) fn load(workspace_root: &Path) -> Self {
        match project_model::Project::new(workspace_root) {
            Ok(project) => Self::from_project(&project),
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "invalid project; graph scan restricted to workspace root, no config roots"
                );
                Self {
                    workspace_root: workspace_root.to_path_buf(),
                    scan_roots: vec![workspace_root.to_path_buf()],
                    configs: ide::WorkspaceConfigsSnapshot::default(),
                }
            }
        }
    }

    pub(crate) fn from_project(project: &project_model::Project) -> Self {
        let mut scan_roots = vec![project.source_path().to_path_buf()];
        scan_roots.extend(project.extension_paths().iter().map(|(_, p)| p.clone()));
        // The MCP file universe is enumerated canonically (`enumerate_bsl_files`
        // canonicalizes every `.bsl`), so the registered roots must be canonical
        // too — a raw symlinked root would miss both prefix matching and the
        // unbootstrapped root-join fallbacks.
        Self {
            workspace_root: project.root.clone(),
            scan_roots,
            configs: ide::WorkspaceConfigsSnapshot::from_project(project).canonicalized(),
        }
    }
}

/// The configuration source directory plus every extension directory — the file
/// universe both the loader and the drift scan must agree on.
pub(super) fn scan_roots(workspace_root: &Path) -> Vec<PathBuf> {
    ProjectSnapshot::load(workspace_root).scan_roots
}

/// Enumerate every `.bsl` file under the config + extension roots, assigning a
/// stable [`FileId`] in walk order. No file text is read — this is the cheap
/// file-id↔path map that lets the graph build load one batch of texts at a time
/// while keeping ids consistent across batches.
pub(crate) fn enumerate_bsl_files(project: &ProjectSnapshot) -> Vec<(FileId, PathBuf)> {
    let mut entries: Vec<(FileId, PathBuf)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut next_id = 0u32;
    for root in &project.scan_roots {
        for entry in WalkDir::new(root).follow_links(true) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("graph scan: walk error: {e}");
                    continue;
                }
            };
            if !entry.file_type().is_file()
                || entry.path().extension().and_then(|e| e.to_str()) != Some("bsl")
            {
                continue;
            }
            let path = entry.path().canonicalize().unwrap_or_else(|_| entry.path().to_path_buf());
            if !seen.insert(path.clone()) {
                continue;
            }
            entries.push((FileId(next_id), path));
            next_id += 1;
        }
    }
    entries
}

pub(crate) use ide_host_core::build_source_root;

/// Build a batch database that shares the whole-workspace `source_root` (so any
/// target is addressable by path through the module index) but loads text only for
/// `batch_files` — the only modules this database lowers.
///
/// `file_source_root` is set ONLY for `batch_files`: the per-file source-root input
/// is read solely for the file being lowered (resolver / infer / `get_file_path`),
/// and the build never lowers a non-batch file. Cross-batch call targets resolve
/// through the path-keyed module index built from the shared source root, which
/// never consults `file_source_root`. Setting it for all files would re-pay a
/// whole-config-sized loop on every batch database for no resolution benefit.
pub(crate) fn db_for_files(
    source_root: &SourceRoot,
    batch_files: &[(FileId, PathBuf)],
    configs: &ide::WorkspaceConfigsSnapshot,
    config_cache: Option<&Arc<ide::GraphConfigCache>>,
) -> RootDatabaseImpl {
    let mut db = RootDatabaseImpl::default();
    if let Some(cache) = config_cache {
        db.set_graph_config_cache(Arc::clone(cache));
    }
    db.set_source_root(GRAPH_SOURCE_ROOT, source_root.clone());
    for (file_id, path) in batch_files {
        db.set_file_source_root(*file_id, GRAPH_SOURCE_ROOT);
        match std::fs::read_to_string(path) {
            Ok(text) => db.set_file_text(*file_id, &text),
            Err(e) => {
                tracing::warn!(path = %path.display(), "graph scan: read failed: {e}");
                db.set_file_text(*file_id, "");
            }
        }
    }
    db.set_workspace_configs_snapshot(configs.clone());
    ide::warm_batch_config_roots(&db, batch_files, &configs.paths);
    db
}

/// Like [`db_for_files`] but disk-backed: registers each file's content revision
/// instead of pinning its text as a salsa input, then drops the text. The resident
/// diagnostics database holds the WHOLE workspace, so the eager `set_file_text` path
/// would pin every file's `Arc<str>` in the overlay map (outside the salsa LRU) and
/// OOM on a large config. Here `file_text_query` re-reads each file from disk on
/// demand under its `lru` cap (`base_db::queries::file_text_query`), verifying the
/// bytes against the recorded revision — the same disk-backed contract the LSP server
/// and the CLI `analyze` path use, so only the working set's text stays resident.
///
/// `file_source_root` is set for every file (not just a batch): `file_text_query`
/// derives the on-disk path through it, so a lazily-read file must have it. An
/// unreadable file falls back to an empty overlay so a later query yields `""`
/// instead of panicking on the disk re-read.
pub(crate) fn db_for_files_lazy(
    source_root: &SourceRoot,
    all_files: &[(FileId, PathBuf)],
    configs: &ide::WorkspaceConfigsSnapshot,
    config_cache: Option<&Arc<ide::GraphConfigCache>>,
) -> RootDatabaseImpl {
    let mut db = RootDatabaseImpl::default();
    if let Some(cache) = config_cache {
        db.set_graph_config_cache(Arc::clone(cache));
    }
    db.set_source_root(GRAPH_SOURCE_ROOT, source_root.clone());
    ide_host_core::register_files_disk_backed(&mut db, GRAPH_SOURCE_ROOT, all_files);
    db.set_workspace_configs_snapshot(configs.clone());
    db
}

/// Walk the configuration source and extension directories, load every `.bsl`
/// file into a fresh database, and register the config metadata paths. Test-only:
/// the production graph is built straight into SQLite per batch, never as one
/// whole-config in-memory database.
#[cfg(test)]
pub(super) fn load_workspace_db(
    workspace_root: &Path,
) -> anyhow::Result<(RootDatabaseImpl, usize)> {
    let project = ProjectSnapshot::load(workspace_root);
    let files = enumerate_bsl_files(&project);
    let source_root = build_source_root(&files);
    let db = db_for_files(&source_root, &files, &project.configs, None);
    Ok((db, files.len()))
}
