//! Background-loaded semantic call graph for the workspace MCP profile.
//!
//! The whole-config call graph lives in a Salsa database that must be populated
//! with every `.bsl` file plus the configuration metadata paths (the resolver
//! consults config visibility). Loading is done off-thread, mirroring the search
//! engine: tools observe [`GraphStatus`] and degrade gracefully while indexing.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide::{Analysis, RootDatabaseImpl};
use vfs::{file_set::FileSet, FileId, VfsPath};
use walkdir::WalkDir;

/// The whole workspace is loaded into a single source root.
pub(crate) const GRAPH_SOURCE_ROOT: SourceRootId = SourceRootId(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GraphStatus {
    /// Not a workspace profile — the graph is unavailable.
    Disabled,
    /// A workspace is configured but the graph has not been loaded yet; the
    /// first `graph` tool call triggers the load.
    Idle,
    /// Background load in progress.
    Loading,
    /// Ready to serve, with the indexed `.bsl` file count.
    Ready { files: usize },
    /// Load failed.
    Failed(String),
}

/// Handle to the workspace call graph database. Cheap to clone (shared `Arc`s).
///
/// Loading is lazy: building the database walks every `.bsl` file and the first
/// query forces whole-config lowering, so a server whose user never touches the
/// graph pays nothing. The load is triggered on the first `graph` tool call.
#[derive(Clone)]
pub(crate) struct GraphState {
    db: Arc<Mutex<Option<RootDatabaseImpl>>>,
    status: Arc<Mutex<GraphStatus>>,
    workspace_root: Option<PathBuf>,
}

impl GraphState {
    /// A disabled graph (reference / shared profiles).
    pub(crate) fn disabled() -> Self {
        Self {
            db: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(GraphStatus::Disabled)),
            workspace_root: None,
        }
    }

    /// A workspace graph that loads lazily on first use.
    pub(crate) fn for_workspace(workspace_root: PathBuf) -> Self {
        Self {
            db: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(GraphStatus::Idle)),
            workspace_root: Some(workspace_root),
        }
    }

    pub(crate) fn status(&self) -> GraphStatus {
        lock_recover(&self.status).clone()
    }

    pub(crate) fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    /// Trigger the background load if this is the first call. Transitions
    /// `Idle → Loading` and spawns exactly one loader thread; later calls return
    /// immediately. No-op for disabled / already-loading / ready / failed graphs.
    pub(crate) fn ensure_loading(&self) {
        let Some(workspace_root) = self.workspace_root.clone() else {
            return;
        };
        {
            let mut status = lock_recover(&self.status);
            if *status != GraphStatus::Idle {
                return;
            }
            *status = GraphStatus::Loading;
        }

        let db_slot = Arc::clone(&self.db);
        let status_slot = Arc::clone(&self.status);
        let spawned = std::thread::Builder::new()
            .name("bsl-graph-init".to_owned())
            .spawn(move || run_load(&db_slot, &status_slot, &workspace_root));
        if let Err(e) = spawned {
            set_status(&self.status, GraphStatus::Failed(format!("could not spawn loader: {e}")));
        }
    }

    /// Snapshot the database for a blocking query, if loaded. The returned
    /// [`Analysis`] owns a cheap Salsa snapshot and can be moved onto a blocking
    /// task without holding the lock during the query.
    pub(crate) fn snapshot(&self) -> Option<Analysis> {
        let guard = lock_recover(&self.db);
        let db = guard.as_ref()?.clone();
        Some(Analysis::from_database(db))
    }
}

/// Run the load on the background thread, containing panics so a failure always
/// resolves the status to `Ready` or `Failed` (never a permanent `Loading`).
fn run_load(
    db_slot: &Arc<Mutex<Option<RootDatabaseImpl>>>,
    status_slot: &Arc<Mutex<GraphStatus>>,
    workspace_root: &Path,
) {
    tracing::info!(?workspace_root, "graph database load started");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        load_workspace_db(workspace_root)
    }));
    match outcome {
        Ok(Ok((db, files))) => {
            // Recover a poisoned lock so a stored db always implies `Ready`.
            *lock_recover(db_slot) = Some(db);
            set_status(status_slot, GraphStatus::Ready { files });
            tracing::info!(files, "graph database load complete");
        }
        Ok(Err(e)) => {
            let msg = e.to_string();
            tracing::warn!("graph database load failed: {msg}");
            set_status(status_slot, GraphStatus::Failed(msg));
        }
        Err(_) => {
            tracing::error!("graph database load panicked");
            set_status(status_slot, GraphStatus::Failed("loader panicked".to_owned()));
        }
    }
}

fn set_status(slot: &Arc<Mutex<GraphStatus>>, status: GraphStatus) {
    *lock_recover(slot) = status;
}

/// Lock a mutex, recovering the inner value if a prior holder panicked. The
/// graph mutexes are only held for brief stores/reads, so a poisoned guard still
/// carries valid data.
fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Walk the configuration source and extension directories, load every `.bsl`
/// file into a fresh database, and register the config metadata paths.
fn load_workspace_db(workspace_root: &Path) -> anyhow::Result<(RootDatabaseImpl, usize)> {
    let project = project_model::Project::new(workspace_root);
    let source_path = project.source_path().to_path_buf();
    let extensions = project.extension_paths().to_vec();

    let mut scan_roots = vec![source_path.clone()];
    scan_roots.extend(extensions.iter().map(|(_, p)| p.clone()));

    let mut db = RootDatabaseImpl::default();
    let mut file_set = FileSet::new();
    let mut entries: Vec<(FileId, PathBuf)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut next_id = 0u32;

    for root in &scan_roots {
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
            let file_id = FileId(next_id);
            next_id += 1;
            file_set.insert(file_id, VfsPath::new(path.clone()));
            entries.push((file_id, path));
        }
    }

    let count = entries.len();
    db.set_source_root(GRAPH_SOURCE_ROOT, SourceRoot::new_local(file_set));
    for (file_id, path) in &entries {
        db.set_file_source_root(*file_id, GRAPH_SOURCE_ROOT);
        match std::fs::read_to_string(path) {
            Ok(text) => db.set_file_text(*file_id, &text),
            Err(e) => {
                tracing::warn!(path = %path.display(), "graph scan: read failed: {e}");
                db.set_file_text(*file_id, "");
            }
        }
    }

    // The resolver checks configuration visibility, so the config + extension
    // metadata paths must be registered just like the LSP workspace loader does.
    let mut config_paths: Vec<(Option<String>, PathBuf)> = vec![(None, source_path)];
    for (name, ext_path) in extensions {
        config_paths.push((Some(name), ext_path));
    }
    db.set_all_config_paths(config_paths);

    Ok((db, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Minimal common-module metadata descriptor so the module is declared in the
    /// configuration (the resolver refuses qualified calls to undeclared modules)
    /// and its client/server execution context is known.
    fn write_common_module(root: &Path, name: &str, server: bool, body: &str) {
        let client = !server;
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core">
	<CommonModule uuid="00000000-0000-0000-0000-0000000000{id:02}">
		<Properties>
			<Name>{name}</Name>
			<Global>false</Global>
			<ClientManagedApplication>{client}</ClientManagedApplication>
			<Server>{server}</Server>
			<ExternalConnection>false</ExternalConnection>
			<ClientOrdinaryApplication>{client}</ClientOrdinaryApplication>
			<ServerCall>false</ServerCall>
			<Privileged>false</Privileged>
			<ReturnValuesReuse>DontUse</ReturnValuesReuse>
		</Properties>
	</CommonModule>
</MetaDataObject>"#,
            id = name.len(),
        );
        write(root, &format!("CommonModules/{name}.xml"), &xml);
        write(root, &format!("CommonModules/{name}/Ext/Module.bsl"), body);
    }

    #[test]
    fn loads_workspace_and_serves_graph() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_common_module(
            root,
            "Клиент",
            false,
            "&НаКлиенте\nПроцедура Главная() Экспорт\nСервер.Считать();\nКонецПроцедуры",
        );
        write_common_module(
            root,
            "Сервер",
            true,
            "&НаСервере\nФункция Считать() Экспорт КонецФункции",
        );

        let (db, files) = load_workspace_db(root).expect("workspace loads");
        assert_eq!(files, 2);

        let analysis = Analysis::from_database(db);
        let overview = analysis.graph_overview(GRAPH_SOURCE_ROOT, Some(root), 10);
        assert_eq!(overview.edges, 1, "Клиент.Главная → Сервер.Считать is one resolved edge");
        assert_eq!(overview.client_to_server_edges, 1);

        let node = analysis
            .graph_node(
                GRAPH_SOURCE_ROOT,
                Some(root),
                "method/common/Сервер/Считать",
                ide::GraphDetail::Names,
            )
            .expect("durable id resolves after disk load");
        assert_eq!(node.node.name, "Считать");
        assert_eq!(node.node.dispatch, vec!["server"]);

        // Callers traversal reaches the client method via the resolved edge.
        let callers = analysis
            .graph_neighbors(
                GRAPH_SOURCE_ROOT,
                Some(root),
                &ide::NeighborsParams {
                    id: "method/common/Сервер/Считать",
                    dir: ide::Direction::In,
                    depth: 1,
                    max_nodes: 50,
                    detail: ide::GraphDetail::Names,
                    provenance_filter: Vec::new(),
                },
            )
            .expect("neighbors resolve");
        assert!(callers.nodes.iter().any(|n| n.id == "method/common/Клиент/Главная"));
    }
}
