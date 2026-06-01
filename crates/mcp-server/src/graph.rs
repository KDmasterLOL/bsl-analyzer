//! Background-loaded semantic call graph for the workspace MCP profile.
//!
//! The whole-config call graph lives in a Salsa database that must be populated
//! with every `.bsl` file plus the configuration metadata paths (the resolver
//! consults config visibility). Loading is done off-thread, mirroring the search
//! engine: tools observe [`GraphStatus`] and degrade gracefully while indexing.
//!
//! Freshness is **pull-on-request**: each `graph` call cheaply checks whether the
//! workspace drifted on disk since the snapshot it served and, on drift, kicks an
//! async reload while still serving the current (stale) snapshot. The agent-facing
//! freshness token is a monotonic [`GraphState`] *generation* — a wholesale reload
//! builds a brand-new database whose internal Salsa revision restarts, so a
//! database-level counter would not be monotonic across reloads.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide::{Analysis, RootDatabaseImpl};
use rustc_hash::FxHashSet;
use vfs::{file_set::FileSet, FileId, VfsPath};
use walkdir::WalkDir;

/// The whole workspace is loaded into a single source root.
pub(crate) const GRAPH_SOURCE_ROOT: SourceRootId = SourceRootId(0);

/// Minimum time between on-disk drift scans. A scan stats every `.bsl`/`.xml`
/// file under the config roots, so throttling bounds its cost regardless of how
/// fast an agent fires `graph` calls.
const DRIFT_CHECK_INTERVAL: Duration = Duration::from_secs(2);

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

/// State of an in-flight or last-attempted background reload, surfaced to agents
/// so a failed reload is visible rather than leaving them at `stale=true` forever.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReloadState {
    /// No reload in flight; the published snapshot is the latest.
    Idle,
    /// A reload triggered by detected drift is running in the background.
    Running,
    /// The last reload failed; the previous snapshot is still served.
    Failed(String),
}

impl ReloadState {
    fn label(&self) -> &'static str {
        match self {
            ReloadState::Idle => "none",
            ReloadState::Running => "running",
            ReloadState::Failed(_) => "failed",
        }
    }
}

/// A coherently published snapshot: the database, the generation it was built at,
/// the on-disk fingerprint it reflects, and any reload in flight. All four move
/// together under one lock so a reader never observes a torn mix (e.g. a bumped
/// generation paired with the old database).
struct Published {
    db: RootDatabaseImpl,
    generation: u64,
    fingerprint: u64,
    /// Set when the load straddled a disk write and the db is an indeterminate
    /// mix. Forces `stale=true` regardless of fingerprint equality, so an ABA
    /// rollback to a byte/mtime-identical pre-load state cannot mask it.
    force_stale: bool,
    reload: ReloadState,
}

/// Everything mutable about the published graph, guarded by a single mutex. Locks
/// are only held for brief reads/swaps — the load and the drift scan run without
/// this lock held.
struct Inner {
    status: GraphStatus,
    published: Option<Published>,
}

/// Throttled cache of the last on-disk fingerprint scan. Guarded by its own mutex
/// held *across* the walk, so concurrent callers serialize onto one scan per
/// window rather than all walking the tree (no thundering herd).
struct ScanCache {
    at: Instant,
    disk_fp: u64,
}

/// A served database snapshot plus the freshness token it was built at. Capturing
/// the generation/fingerprint at snapshot time (not at response time) keeps the
/// envelope's `revision`/`stale` consistent with the data actually returned, even
/// if a reload publishes a newer generation while the query runs.
pub(crate) struct GraphSnapshot {
    pub analysis: Analysis,
    generation: u64,
    fingerprint: u64,
    force_stale: bool,
}

/// Freshness verdict for one `graph` response.
pub(crate) struct Freshness {
    /// The generation of the snapshot that served this response.
    pub revision: u64,
    /// The workspace drifted on disk since this snapshot was built.
    pub stale: bool,
    /// Reload state: `"none"`, `"running"`, or `"failed"`.
    pub reload: &'static str,
}

/// Handle to the workspace call graph database. Cheap to clone (shared `Arc`s).
///
/// Loading is lazy: building the database walks every `.bsl` file and the first
/// query forces whole-config lowering, so a server whose user never touches the
/// graph pays nothing. The load is triggered on the first `graph` tool call.
#[derive(Clone)]
pub(crate) struct GraphState {
    inner: Arc<Mutex<Inner>>,
    scan: Arc<Mutex<Option<ScanCache>>>,
    workspace_root: Option<PathBuf>,
    drift_interval: Duration,
}

impl GraphState {
    /// A disabled graph (reference / shared profiles).
    pub(crate) fn disabled() -> Self {
        Self::with_status(GraphStatus::Disabled, None)
    }

    /// A workspace graph that loads lazily on first use.
    pub(crate) fn for_workspace(workspace_root: PathBuf) -> Self {
        Self::with_status(GraphStatus::Idle, Some(workspace_root))
    }

    fn with_status(status: GraphStatus, workspace_root: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner { status, published: None })),
            scan: Arc::new(Mutex::new(None)),
            workspace_root,
            drift_interval: DRIFT_CHECK_INTERVAL,
        }
    }

    pub(crate) fn status(&self) -> GraphStatus {
        lock_recover(&self.inner).status.clone()
    }

    pub(crate) fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    /// Trigger the background load if this is the first call. Transitions
    /// `Idle → Loading` and spawns exactly one loader thread; later calls return
    /// immediately. No-op for disabled / already-loading / ready / failed graphs.
    pub(crate) fn ensure_loading(&self) {
        if self.workspace_root.is_none() {
            return;
        }
        {
            let mut inner = lock_recover(&self.inner);
            if inner.status != GraphStatus::Idle {
                return;
            }
            inner.status = GraphStatus::Loading;
        }

        let state = self.clone();
        let spawned = std::thread::Builder::new()
            .name("bsl-graph-init".to_owned())
            .spawn(move || state.run_load(false));
        if let Err(e) = spawned {
            let mut inner = lock_recover(&self.inner);
            inner.status = GraphStatus::Failed(format!("could not spawn loader: {e}"));
        }
    }

    /// Snapshot the database for a blocking query, if loaded, capturing the
    /// generation and fingerprint it reflects. The returned [`GraphSnapshot`] owns
    /// a cheap Salsa snapshot and can be moved onto a blocking task without holding
    /// the lock during the query.
    pub(crate) fn snapshot(&self) -> Option<GraphSnapshot> {
        let inner = lock_recover(&self.inner);
        let published = inner.published.as_ref()?;
        Some(GraphSnapshot {
            analysis: Analysis::from_database(published.db.clone()),
            generation: published.generation,
            fingerprint: published.fingerprint,
            force_stale: published.force_stale,
        })
    }

    /// Report the freshness of `snapshot` relative to disk, and on drift kick an
    /// async reload (at most one in flight). `stale`/`revision` are relative to the
    /// snapshot that served the response; the reload decision is relative to the
    /// latest published snapshot. Walks the filesystem, so call from a blocking
    /// context.
    pub(crate) fn freshness(&self, snapshot: &GraphSnapshot) -> Freshness {
        let disk_fp = self.current_disk_fp();
        // A snapshot from a straddled load is unconditionally stale until a clean
        // reload replaces it — equality alone could be fooled by an ABA rollback to
        // the pre-load fingerprint.
        let stale =
            snapshot.force_stale || disk_fp.map(|fp| fp != snapshot.fingerprint).unwrap_or(false);

        let mut inner = lock_recover(&self.inner);
        let Some(published) = inner.published.as_mut() else {
            return Freshness { revision: snapshot.generation, stale, reload: "none" };
        };
        let mut reload = published.reload.label();
        // Claim the single reload slot under the lock — comparing the fresh disk
        // fingerprint against the *current* published one means a reload that
        // landed during our walk is not re-triggered.
        let claim_reload = disk_fp.map(|fp| fp != published.fingerprint).unwrap_or(false)
            && published.reload != ReloadState::Running;
        if claim_reload {
            published.reload = ReloadState::Running;
            reload = "running";
        }
        drop(inner);

        if claim_reload {
            let state = self.clone();
            let spawned = std::thread::Builder::new()
                .name("bsl-graph-reload".to_owned())
                .spawn(move || state.run_load(true));
            if let Err(e) = spawned {
                let mut inner = lock_recover(&self.inner);
                if let Some(p) = inner.published.as_mut() {
                    p.reload = ReloadState::Failed(format!("could not spawn reload: {e}"));
                }
                reload = "failed";
            }
        }

        Freshness { revision: snapshot.generation, stale, reload }
    }

    /// The current on-disk fingerprint, throttled to one scan per drift interval.
    /// The scan mutex is held across the walk, so concurrent callers within the
    /// window block briefly then reuse the cached value instead of re-walking.
    /// `None` when no workspace is configured.
    fn current_disk_fp(&self) -> Option<u64> {
        let root = self.workspace_root.as_deref()?;
        let mut cache = lock_recover(&self.scan);
        if let Some(c) = cache.as_ref() {
            if c.at.elapsed() < self.drift_interval {
                return Some(c.disk_fp);
            }
        }
        let fp = workspace_fingerprint(root);
        *cache = Some(ScanCache { at: Instant::now(), disk_fp: fp });
        Some(fp)
    }

    /// Build (or rebuild) the database off-thread and publish it coherently.
    /// `is_reload` distinguishes the initial load (sets `Ready`, generation 1)
    /// from a drift-triggered reload (bumps the generation, keeps the old snapshot
    /// served on failure).
    fn run_load(&self, is_reload: bool) {
        let Some(workspace_root) = self.workspace_root.clone() else {
            return;
        };
        tracing::info!(?workspace_root, is_reload, "graph database load started");
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Bracket the load with two fingerprint scans. The database reads files
            // between them, so when `fp_pre == fp_post` the disk did not move during
            // the load and the db provably reflects exactly that state — publish it
            // as fresh. When they differ the load straddled a write and the db is an
            // indeterminate mix; we still publish the (better) new db but mark it
            // `force_stale` so freshness reports it stale until a clean reload
            // replaces it — never claiming a coherent state the load could not
            // capture, even under an ABA rollback to the pre-load fingerprint.
            let fp_pre = workspace_fingerprint(&workspace_root);
            let (db, files) = load_workspace_db(&workspace_root)?;
            let fp_post = workspace_fingerprint(&workspace_root);
            anyhow::Ok((db, files, fp_pre, fp_post))
        }));

        match outcome {
            Ok(Ok((db, files, fp_pre, fp_post))) => {
                let force_stale = fp_pre != fp_post;
                if force_stale {
                    tracing::warn!(
                        is_reload,
                        "graph load straddled a disk write; marking snapshot stale to force reload"
                    );
                }
                // Drop the stale scan cache *before* publishing so a concurrent
                // freshness check re-scans against the new snapshot rather than a
                // pre-reload cached fingerprint.
                *lock_recover(&self.scan) = None;
                let generation = {
                    let mut inner = lock_recover(&self.inner);
                    let generation =
                        inner.published.as_ref().map(|p| p.generation).unwrap_or(0) + 1;
                    inner.published = Some(Published {
                        db,
                        generation,
                        fingerprint: fp_pre,
                        force_stale,
                        reload: ReloadState::Idle,
                    });
                    inner.status = GraphStatus::Ready { files };
                    generation
                };
                tracing::info!(files, generation, is_reload, "graph database load complete");
            }
            Ok(Err(e)) => {
                let msg = e.to_string();
                tracing::warn!("graph database load failed: {msg}");
                self.record_load_failure(is_reload, msg);
            }
            Err(_) => {
                tracing::error!("graph database load panicked");
                self.record_load_failure(is_reload, "loader panicked".to_owned());
            }
        }
    }

    /// A failed initial load surfaces as `Failed`; a failed reload keeps the
    /// previous snapshot but flags `reload="failed"` so the agent sees it. A
    /// later drift check retries the reload (the throttle bounds the retry rate).
    fn record_load_failure(&self, is_reload: bool, msg: String) {
        let mut inner = lock_recover(&self.inner);
        if is_reload {
            if let Some(p) = inner.published.as_mut() {
                p.reload = ReloadState::Failed(msg);
            }
        } else {
            inner.status = GraphStatus::Failed(msg);
        }
    }
}

/// Lock a mutex, recovering the inner value if a prior holder panicked. The graph
/// mutexes guard brief stores/reads (and one throttled scan), so a poisoned guard
/// still carries valid data.
fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The configuration source directory plus every extension directory — the file
/// universe both the loader and the drift scan must agree on.
fn scan_roots(workspace_root: &Path) -> Vec<PathBuf> {
    let project = project_model::Project::new(workspace_root);
    let mut roots = vec![project.source_path().to_path_buf()];
    roots.extend(project.extension_paths().iter().map(|(_, p)| p.clone()));
    roots
}

/// A cheap, order-independent fingerprint of the graph-relevant files on disk.
///
/// Covers both `.bsl` sources and `.xml` metadata descriptors: graph resolution
/// depends on configuration visibility registered from the metadata, not only on
/// module text, so a `.bsl`-only fingerprint would miss metadata-only drift. Uses
/// `(canonical path, mtime, len)` — stat only, no file reads — and mirrors the
/// loader's scan roots and symlink/canonicalization policy so it compares the same
/// file universe (otherwise it would report phantom drift).
fn workspace_fingerprint(workspace_root: &Path) -> u64 {
    let mut entries: Vec<(String, u128, u64)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for root in scan_roots(workspace_root) {
        for entry in WalkDir::new(&root).follow_links(true) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            match entry.path().extension().and_then(|e| e.to_str()) {
                Some("bsl") | Some("xml") => {}
                _ => continue,
            }
            let path = entry.path().canonicalize().unwrap_or_else(|_| entry.path().to_path_buf());
            if !seen.insert(path.clone()) {
                continue;
            }
            let (mtime, len) = entry
                .metadata()
                .ok()
                .map(|m| {
                    let mtime = m
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_nanos())
                        .unwrap_or(0);
                    (mtime, m.len())
                })
                .unwrap_or((0, 0));
            entries.push((path.to_string_lossy().into_owned(), mtime, len));
        }
    }

    entries.sort();
    let mut hasher = DefaultHasher::new();
    entries.hash(&mut hasher);
    hasher.finish()
}

/// The configuration source + extension metadata paths the resolver needs for
/// visibility checks, registered on every database (full or per-batch) just like
/// the LSP workspace loader does.
pub(crate) fn config_metadata_paths(workspace_root: &Path) -> Vec<(Option<String>, PathBuf)> {
    let project = project_model::Project::new(workspace_root);
    let mut config_paths: Vec<(Option<String>, PathBuf)> =
        vec![(None, project.source_path().to_path_buf())];
    for (name, ext_path) in project.extension_paths() {
        config_paths.push((Some(name.clone()), ext_path.clone()));
    }
    config_paths
}

/// Enumerate every `.bsl` file under the config + extension roots, assigning a
/// stable [`FileId`] in walk order. No file text is read — this is the cheap
/// file-id↔path map that lets the graph build load one batch of texts at a time
/// while keeping ids consistent across batches.
pub(crate) fn enumerate_bsl_files(workspace_root: &Path) -> Vec<(FileId, PathBuf)> {
    let mut entries: Vec<(FileId, PathBuf)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut next_id = 0u32;
    for root in scan_roots(workspace_root) {
        for entry in WalkDir::new(&root).follow_links(true) {
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

/// Build a database whose source root maps EVERY file's path — so cross-module
/// resolution through the module index can find any target's [`FileId`] — but
/// loads text only for the files in `load_text`. Files outside `load_text` are
/// addressable by path yet never lowered, so a per-batch build pays HIR cost only
/// for its own batch while still resolving calls into the rest of the config.
pub(crate) fn db_for_files(
    all_files: &[(FileId, PathBuf)],
    load_text: &FxHashSet<FileId>,
    config_paths: &[(Option<String>, PathBuf)],
) -> RootDatabaseImpl {
    let mut db = RootDatabaseImpl::default();
    let mut file_set = FileSet::new();
    for (file_id, path) in all_files {
        file_set.insert(*file_id, VfsPath::new(path.clone()));
    }
    db.set_source_root(GRAPH_SOURCE_ROOT, SourceRoot::new_local(file_set));
    for (file_id, path) in all_files {
        db.set_file_source_root(*file_id, GRAPH_SOURCE_ROOT);
        if !load_text.contains(file_id) {
            continue;
        }
        match std::fs::read_to_string(path) {
            Ok(text) => db.set_file_text(*file_id, &text),
            Err(e) => {
                tracing::warn!(path = %path.display(), "graph scan: read failed: {e}");
                db.set_file_text(*file_id, "");
            }
        }
    }
    db.set_all_config_paths(config_paths.to_vec());
    db
}

/// Walk the configuration source and extension directories, load every `.bsl`
/// file into a fresh database, and register the config metadata paths.
fn load_workspace_db(workspace_root: &Path) -> anyhow::Result<(RootDatabaseImpl, usize)> {
    let files = enumerate_bsl_files(workspace_root);
    let config_paths = config_metadata_paths(workspace_root);
    let load_text: FxHashSet<FileId> = files.iter().map(|(f, _)| *f).collect();
    let db = db_for_files(&files, &load_text, &config_paths);
    Ok((db, files.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_db::build_graph_database;
    use rusqlite::Connection;
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

    fn sample_workspace(root: &Path) {
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
    }

    fn wait_ready(graph: &GraphState) {
        for _ in 0..200 {
            match graph.status() {
                GraphStatus::Ready { .. } => return,
                GraphStatus::Failed(msg) => panic!("graph load failed: {msg}"),
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        panic!("graph did not become ready");
    }

    #[test]
    fn loads_workspace_and_serves_graph() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

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

    /// The streaming SQLite build must reproduce the in-memory graph: identical
    /// node-kind tallies, edge counts, durable ids, dispatch and in-degree.
    #[test]
    fn sqlite_build_matches_in_memory_graph() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (db, files) = load_workspace_db(root).expect("workspace loads");
        let analysis = Analysis::from_database(db.clone());
        let overview = analysis.graph_overview(GRAPH_SOURCE_ROOT, Some(root), 10);

        let out = root.join(".build/bsl-graph.db");
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        let summary = build_graph_database(
            root,
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: 0,
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        assert_eq!(summary.edges, overview.edges);

        let conn = Connection::open(&out).unwrap();
        let count = |sql: &str| -> usize {
            conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap() as usize
        };

        assert_eq!(count("SELECT COUNT(*) FROM nodes"), overview.nodes);
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='method'"), overview.methods);
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='module'"), overview.modules);
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='mdo'"), overview.mdos);
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='attribute'"), overview.attributes);
        assert_eq!(count("SELECT COUNT(*) FROM edges"), overview.edges);
        assert_eq!(
            count("SELECT COUNT(*) FROM edges WHERE crosses=1"),
            overview.client_to_server_edges
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM edges WHERE provenance='resolved'"),
            *overview.edge_provenance.get("resolved").unwrap_or(&0)
        );

        let (name, dispatch): (String, String) = conn
            .query_row(
                "SELECT name, dispatch FROM nodes WHERE id = ?1",
                rusqlite::params!["method/common/Сервер/Считать"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((name.as_str(), dispatch.as_str()), ("Считать", "server"));

        let in_degree: i64 = conn
            .query_row(
                "SELECT degree FROM in_degree WHERE id = 'method/common/Сервер/Считать'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(in_degree, 1, "Сервер.Считать is called once");
    }

    /// A metadata object reached by a manager call in one module and by an SDBL
    /// query in another, across separate batches (`batch_size = 1`), must get the
    /// SAME durable `Mdo` node id from the streaming build as the in-memory fold.
    /// The build runs call edges across all batches before query edges, mirroring
    /// the fold's Pass-2-then-Pass-3 order, so the first-seen (canonical) spelling —
    /// and thus the id — cannot diverge even when the call and query sites differ in
    /// case.
    #[test]
    fn cross_batch_mdo_node_id_matches_fold() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write(
            root,
            "Catalogs/Номенклатура.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000001">
        <Properties><Name>Номенклатура</Name><CodeLength>9</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#,
        );
        // One module creates via the manager (canonical case), another reads it in a
        // query (upper case). Their batch order is fixed by walk order; the build's
        // global call-before-query order decides the canonical spelling regardless.
        write(
            root,
            "CommonModules/Менеджер/Ext/Module.bsl",
            "Процедура Создать() Экспорт\nСправочники.Номенклатура.СоздатьЭлемент();\nКонецПроцедуры",
        );
        write(
            root,
            "CommonModules/Отчет/Ext/Module.bsl",
            "Процедура Читать() Экспорт\n\
             Запрос = \"ВЫБРАТЬ Код ИЗ Справочник.НОМЕНКЛАТУРА\";\nКонецПроцедуры",
        );

        let (db, files) = load_workspace_db(root).expect("workspace loads");
        let analysis = Analysis::from_database(db);
        let fold = analysis.graph_overview(GRAPH_SOURCE_ROOT, Some(root), 50);
        let fold_mdo: Vec<&str> = fold
            .top_by_centrality
            .iter()
            .filter(|n| n.kind == "mdo")
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(fold_mdo.len(), 1, "exactly one catalog Mdo node in the fold: {fold_mdo:?}");
        let fold_id = fold_mdo[0];

        let out = root.join(".build/bsl-graph.db");
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            root,
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: 0,
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");

        let conn = Connection::open(&out).unwrap();
        let sqlite_mdo: Vec<String> = {
            let mut stmt = conn.prepare("SELECT id FROM nodes WHERE kind='mdo'").unwrap();
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(sqlite_mdo.len(), 1, "exactly one catalog Mdo node in SQLite: {sqlite_mdo:?}");
        assert_eq!(
            sqlite_mdo[0], fold_id,
            "cross-batch Mdo node id must be byte-identical to the in-memory fold's"
        );
    }

    #[test]
    fn fingerprint_changes_on_bsl_edit_and_xml_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let base = workspace_fingerprint(root);

        // A `.bsl` body edit (different length) shifts the fingerprint.
        write(
            root,
            "CommonModules/Сервер/Ext/Module.bsl",
            "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции",
        );
        let after_bsl = workspace_fingerprint(root);
        assert_ne!(base, after_bsl, "a .bsl edit must change the fingerprint");

        // A `.xml` metadata edit must also shift it — graph resolution depends on
        // configuration metadata, not only module text.
        write(root, "CommonModules/Сервер.xml", "<MetaDataObject/>");
        let after_xml = workspace_fingerprint(root);
        assert_ne!(after_bsl, after_xml, "a .xml metadata edit must change the fingerprint");
    }

    #[test]
    fn drift_marks_stale_and_async_reload_bumps_generation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut graph = GraphState::for_workspace(root.to_path_buf());
        // Disable throttling so each check actually scans.
        graph.drift_interval = Duration::ZERO;
        graph.ensure_loading();
        wait_ready(&graph);

        let snap1 = graph.snapshot().expect("ready graph snapshots");
        let fresh = graph.freshness(&snap1);
        assert_eq!(fresh.revision, 1);
        assert!(!fresh.stale);
        assert_eq!(fresh.reload, "none");

        // Edit a module on disk: a freshness check against the old snapshot must
        // read as stale and kick a reload that publishes a bumped generation.
        write(
            root,
            "CommonModules/Сервер/Ext/Module.bsl",
            "&НаСервере\nФункция Считать() Экспорт Возврат 42; КонецФункции",
        );
        let drifted = graph.freshness(&snap1);
        assert!(drifted.stale, "an on-disk edit must read as stale");
        assert_eq!(drifted.revision, 1, "the stale response still serves the old generation");
        assert!(matches!(drifted.reload, "running" | "failed"));

        // The async reload publishes generation 2; a fresh snapshot is then clean.
        let mut settled = None;
        for _ in 0..200 {
            let snap = graph.snapshot().expect("snapshot");
            if snap.generation == 2 {
                settled = Some(graph.freshness(&snap));
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let settled = settled.expect("reload did not publish a new generation");
        assert!(!settled.stale);
        assert_eq!(settled.revision, 2);
        assert_eq!(settled.reload, "none");
    }
}
