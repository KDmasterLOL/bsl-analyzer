use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Instant, UNIX_EPOCH};

use crate::change_hub::{ChangeEntry, ChangeKind};
use crate::graph_query::GraphDb;

use super::state::{lock_recover, GraphState, ReloadState};
use super::types::Freshness;

/// How often the query-path freshness fold must come from a real walk instead of the
/// event-maintained map. Bounds how long a change the hub cannot observe can keep
/// freshness wrong.
pub(super) const WALK_VERIFY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// `canonical path → (mtime nanos, len)` state maintained from hub deliveries and
/// periodically re-anchored by a complete walk. `topology` carries the topology
/// hash observed at the last walk: hub deliveries patch only file stats, and a
/// config-file delivery (which may change the topology) drops the whole map so
/// the next check walks — and re-derives the project — instead of folding a
/// stale topology under fresh file stats.
#[derive(Default)]
pub(super) struct FpMapState {
    pub(super) map: Option<std::collections::BTreeMap<String, (u128, u64)>>,
    pub(super) walked_at: Option<Instant>,
    pub(super) topology: u64,
    /// Verdict of the walk that anchored `map`: hub deliveries patch stats but
    /// cannot re-judge completeness, so the last walk's verdict rides along.
    pub(super) clean: bool,
}

/// Throttled cache of the last on-disk fingerprint scan. Guarded by its own mutex
/// held across the walk, so concurrent callers serialize onto one scan per window.
pub(super) struct ScanCache {
    pub(super) at: Instant,
    pub(super) disk_fp: crate::graph_db::GraphFp,
    /// Whether the scan behind `disk_fp` covered the whole tree — the reload
    /// decision needs it to retire a `force_stale` build once the tree heals.
    pub(super) clean: bool,
}

/// Cap on idle pooled snapshot handles. Concurrent graph queries beyond it just open
/// their own handle, exactly as before pooling.
const SNAPSHOT_POOL_CAP: usize = 4;

/// A pooled idle read handle plus the freshness token it was opened under.
pub(super) struct PooledSnapshotEntry {
    pub(super) generation: u64,
    fingerprint: crate::graph_db::GraphFp,
    force_stale: bool,
    db: GraphDb,
}

/// A served graph handle plus the freshness token it was built at. Capturing the
/// generation/fingerprint at snapshot time (not at response time) keeps the
/// envelope's `revision`/`stale` consistent with the data actually returned, even
/// if a reload publishes a newer generation while the query runs. The handle is an
/// own read-only connection opened against the on-disk SQLite graph.
pub(crate) struct GraphSnapshot {
    pub graph: PooledGraphDb,
    pub(super) generation: u64,
    fingerprint: crate::graph_db::GraphFp,
    force_stale: bool,
    /// Modules this artefact was built without being able to read. Makes `stale`
    /// true — the graph is missing their nodes and edges — WITHOUT making
    /// `wants_reload` true, since rebuilding cannot read them either.
    unread_files: usize,
    /// The root table this workspace publishes, for turning a node's stored file path into
    /// a `(root_id, path)` pair. `None` on a boot that published a cached graph before the
    /// project was loaded — a real serving state, not a test-only one.
    workspace_roots: Option<bsl_search::WorkspaceRoots>,
}

impl GraphSnapshot {
    /// Modules this artefact could not read when it was built or last patched.
    pub(crate) fn unread_files(&self) -> usize {
        self.unread_files
    }

    /// The root table, when this snapshot has one.
    pub(crate) fn workspace_roots(&self) -> Option<&bsl_search::WorkspaceRoots> {
        self.workspace_roots.as_ref()
    }
}

/// A read handle checked out of (and returned to) [`GraphState::snapshot_pool`].
/// Dereferences to the underlying [`GraphDb`]; on drop the handle goes back to the
/// pool (up to [`SNAPSHOT_POOL_CAP`]) so the next query skips the multi-GB open.
pub(crate) struct PooledGraphDb {
    entry: Option<PooledSnapshotEntry>,
    pool: Arc<Mutex<Vec<PooledSnapshotEntry>>>,
}

impl std::ops::Deref for PooledGraphDb {
    type Target = GraphDb;

    fn deref(&self) -> &GraphDb {
        &self.entry.as_ref().expect("pooled handle is present until drop").db
    }
}

impl Drop for PooledGraphDb {
    fn drop(&mut self) {
        if let Some(entry) = self.entry.take() {
            let mut pool = self.pool.lock().unwrap_or_else(|e| e.into_inner());
            if pool.len() < SNAPSHOT_POOL_CAP {
                pool.push(entry);
            }
        }
    }
}

impl GraphState {
    /// Snapshot the graph for a blocking query, if built. The returned
    /// [`GraphSnapshot`] owns a read-only SQLite handle and its freshness token,
    /// and can be moved onto a blocking task without holding the lock during the
    /// query.
    pub(crate) fn snapshot(&self) -> Option<GraphSnapshot> {
        let (published_generation, published_topology, workspace_roots) = {
            let inner = lock_recover(&self.inner);
            let published = inner.published.as_ref()?;
            // Cloned under the same lock as the generation: an answer must describe the
            // publication it was served from, roots included.
            (published.generation, published.fingerprint.topology, published.search_roots.clone())
        };
        {
            let mut pool = lock_recover(&self.snapshot_pool);
            while let Some(entry) = pool.pop() {
                if entry.generation == published_generation {
                    let (generation, fingerprint, force_stale) =
                        (entry.generation, entry.fingerprint, entry.force_stale);
                    let unread_files = entry.db.unread_files();
                    return Some(GraphSnapshot {
                        graph: PooledGraphDb {
                            entry: Some(entry),
                            pool: Arc::clone(&self.snapshot_pool),
                        },
                        generation,
                        fingerprint,
                        force_stale,
                        unread_files,
                        workspace_roots,
                    });
                }
            }
        }
        let path = self.graph_db_path()?;
        let graph = GraphDb::open(&path).ok()?;
        let (generation, fingerprint, force_stale) = graph.freshness_token().ok()?;
        // Read from the artefact itself, like the freshness token beside it: an
        // adoption path that reconstructed this from elsewhere would report "no holes"
        // for a graph built blind.
        let unread_files = graph.unread_files();
        // The file at this path is not necessarily the one WE published: a daemon of another
        // generation over the same workspace (see [`crate::workspace_lease`]) renames its own
        // build into place, and a build made under a different extension topology answers
        // different questions about the same code. Serving it would be a silent wrong answer,
        // so a foreign topology reads as "no snapshot" instead. The comparison is against our
        // last publish, so between our own rename and the matching `published` update a
        // topology-changing build of ours also reads as none — the honest answer while a
        // publish is in flight.
        if fingerprint.topology != published_topology {
            tracing::warn!(
                file_topology = fingerprint.topology,
                published_topology,
                "graph database on disk was built for another extension topology; not serving it"
            );
            return None;
        }
        Some(GraphSnapshot {
            graph: PooledGraphDb {
                entry: Some(PooledSnapshotEntry {
                    generation,
                    fingerprint,
                    force_stale,
                    db: graph,
                }),
                pool: Arc::clone(&self.snapshot_pool),
            },
            generation,
            fingerprint,
            force_stale,
            unread_files,
            workspace_roots,
        })
    }

    /// Report the freshness of `snapshot` relative to disk, and on drift kick an
    /// async reload (at most one in flight). `stale`/`revision` are relative to the
    /// snapshot that served the response; the reload decision is relative to the
    /// latest published snapshot. Walks the filesystem, so call from a blocking
    /// context.
    pub(crate) fn freshness(&self, snapshot: &GraphSnapshot) -> Freshness {
        let disk = self.current_disk_fp();
        let stale = snapshot.force_stale
            || snapshot.unread_files > 0
            || disk.map(|(fp, _)| fp != snapshot.fingerprint).unwrap_or(false);
        // Read before the lock: the lease may go to disk, and the inner mutex serializes every
        // graph query. Drift is still reported (the response says `stale`), but only the daemon
        // that owns the workspace's derived caches acts on it — see [`crate::workspace_lease`].
        let may_build = self.may_build();

        let mut inner = lock_recover(&self.inner);
        let Some(published) = inner.published.as_mut() else {
            return Freshness {
                revision: snapshot.generation,
                stale,
                reload: "none",
                topology: snapshot.fingerprint.topology,
            };
        };
        let mut reload = published.reload.label();
        let claim_reload =
            published.wants_reload(disk) && published.reload != ReloadState::Running && may_build;
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

        Freshness {
            revision: snapshot.generation,
            stale,
            reload,
            topology: snapshot.fingerprint.topology,
        }
    }

    pub(super) fn current_disk_fp(&self) -> Option<(crate::graph_db::GraphFp, bool)> {
        let root = self.workspace_root.as_deref()?;
        self.invalidate_scan_on_hub_drift();
        let mut cache = lock_recover(&self.scan);
        if let Some(c) = cache.as_ref() {
            if c.at.elapsed() < self.drift_interval {
                return Some((c.disk_fp, c.clean));
            }
        }
        // Asked about OUR cursor, not about the hub at large: `invalidate_scan_on_hub_drift`
        // above has just drained it, so any debt left here is the hub's own incompleteness
        // — while a shared verdict would also carry the debt of a consumer that simply
        // stopped draining, and put this one on a full walk for as long as that lasted.
        let hub_healthy = matches!(
            &self.change_hub,
            Some(hub) if matches!(
                hub.health_for(*lock_recover(&self.hub_cursor)),
                crate::change_hub::Health::Healthy
            )
        );
        if !hub_healthy {
            let mut fp_state = lock_recover(&self.fp_map);
            fp_state.map = None;
            fp_state.walked_at = None;
        }
        if hub_healthy {
            let fp_state = lock_recover(&self.fp_map);
            if let (Some(map), Some(walked_at)) = (fp_state.map.as_ref(), fp_state.walked_at) {
                if walked_at.elapsed() < WALK_VERIFY_INTERVAL {
                    let entries: Vec<(String, u128, u64)> =
                        map.iter().map(|(p, (m, l))| (p.clone(), *m, *l)).collect();
                    let fp = crate::graph_db::GraphFp {
                        files: fold_fingerprint_entries(&entries),
                        topology: fp_state.topology,
                    };
                    let clean = fp_state.clean;
                    *cache = Some(ScanCache { at: Instant::now(), disk_fp: fp, clean });
                    return Some((fp, clean));
                }
            }
        }
        self.scan_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // ONE project load serves both components: the roots the stats walk and
        // the topology hash come from the same snapshot, so the fold can never
        // pair one project state's files with another's topology.
        let project = super::input::ProjectSnapshot::load(root);
        let universe = super::universe::ScannedUniverse::scan(&project.scan_roots);
        let clean = universe.clean();
        let mut entries: Vec<(String, u128, u64)> =
            universe.stats.into_iter().map(|s| (s.path, s.mtime, s.len)).collect();
        entries.sort();
        let topology = super::scan::topology_u64(&project.configs);
        let fp = crate::graph_db::GraphFp { files: fold_fingerprint_entries(&entries), topology };
        {
            let mut fp_state = lock_recover(&self.fp_map);
            fp_state.map = Some(entries.into_iter().map(|(p, m, l)| (p, (m, l))).collect());
            fp_state.walked_at = Some(Instant::now());
            fp_state.topology = topology;
            fp_state.clean = clean;
        }
        *cache = Some(ScanCache { at: Instant::now(), disk_fp: fp, clean });
        Some((fp, clean))
    }

    fn invalidate_scan_on_hub_drift(&self) {
        let Some(hub) = &self.change_hub else {
            return;
        };
        let cursor = {
            let mut slot = lock_recover(&self.hub_cursor);
            match *slot {
                Some(cursor) => cursor,
                None => {
                    let cursor = hub.subscribe();
                    *slot = Some(cursor);
                    cursor
                }
            }
        };
        let batch = hub.drain(cursor);
        *lock_recover(&self.hub_cursor) = Some(batch.cursor);
        if batch.rescan_required {
            *lock_recover(&self.scan) = None;
            let mut fp_state = lock_recover(&self.fp_map);
            fp_state.map = None;
            fp_state.walked_at = None;
            return;
        }
        let relevant: Vec<&ChangeEntry> =
            batch.entries.iter().filter(|e| entry_touches_scan_universe(e)).collect();
        if relevant.is_empty() {
            return;
        }
        *lock_recover(&self.scan) = None;
        let mut fp_state = lock_recover(&self.fp_map);
        // A subtree removal invalidates paths the entry list cannot enumerate; a
        // config-file change may alter the topology AND the scan-root universe.
        // Either way the patched map would lie — drop it so the next check walks
        // (and re-derives the project).
        if relevant.iter().any(|e| e.kind == ChangeKind::SubtreeRemoved || entry_is_config_file(e))
        {
            fp_state.map = None;
            fp_state.walked_at = None;
            return;
        }
        let Some(map) = fp_state.map.as_mut() else {
            return;
        };
        for entry in relevant {
            let key = entry.canonical.to_string_lossy().into_owned();
            match stat_pair(&entry.canonical) {
                Some(pair) => {
                    map.insert(key, pair);
                }
                None => {
                    map.remove(&key);
                }
            }
        }
    }
}

fn entry_touches_scan_universe(entry: &ChangeEntry) -> bool {
    if entry.kind == ChangeKind::SubtreeRemoved {
        return true;
    }
    let is_scan_ext = |path: &Path| {
        bsl_conventions::has_extension(path, bsl_conventions::BSL_EXTENSION)
            || bsl_conventions::has_extension(path, bsl_conventions::XML_EXTENSION)
    };
    is_scan_ext(&entry.canonical) || is_scan_ext(&entry.raw) || entry_is_config_file(entry)
}

/// Whether a delivered change is one of the analyzer config files — an edit there
/// can change the extension topology (and with it the scan-root universe) without
/// touching a single `.bsl`/`.xml`.
fn entry_is_config_file(entry: &ChangeEntry) -> bool {
    let is_config = |path: &Path| {
        path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| project_model::CONFIG_FILE_NAMES.contains(&n))
    };
    is_config(&entry.canonical) || is_config(&entry.raw)
}

pub(super) fn fold_fingerprint_entries(entries: &[(String, u128, u64)]) -> u64 {
    let mut hasher = DefaultHasher::new();
    entries.hash(&mut hasher);
    hasher.finish()
}

fn stat_pair(path: &Path) -> Option<(u128, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Some((mtime, meta.len()))
}

#[cfg(test)]
mod tests {
    use super::super::scan::workspace_fingerprint;
    use super::super::state::{lock_recover, GraphState};
    use super::super::test_support::{sample_workspace, seed_cache, wait_ready, write};
    use super::*;
    use crate::change_hub::WorkspaceChangeHub;
    use std::time::Duration;

    #[test]
    fn a_case_variant_module_still_touches_the_scan_universe() {
        let path = std::path::PathBuf::from("/w/CommonModules/X/Ext/Module.BSL");
        let entry = ChangeEntry {
            canonical: path.clone(),
            raw: path,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        assert!(
            entry_touches_scan_universe(&entry),
            "Module.BSL входит во вселенную скана — хаб обязан сбросить кэш отпечатка"
        );
    }

    /// A second daemon generation over the same workspace renames ITS build into the shared
    /// path. On a pool miss the reopen must not serve that file when it was built under a
    /// different extension topology — it answers different questions about the same code —
    /// while a replacement under the SAME topology (a build that differs only on an axis the
    /// graph does not depend on, e.g. binary version) stays serveable. Drop the topology
    /// comparison in `snapshot` and the foreign build is served as this workspace's answer.
    #[test]
    fn a_graph_file_built_for_another_topology_is_not_served() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        seed_cache(root, workspace_fingerprint(root));

        let graph = GraphState::for_workspace(root.to_path_buf());
        graph.ensure_loading();
        wait_ready(&graph);

        let published = graph.snapshot().expect("the freshly built graph serves").fingerprint;
        lock_recover(&graph.snapshot_pool).clear();

        seed_cache(root, crate::graph_db::GraphFp { topology: published.topology, ..published });
        assert!(
            graph.snapshot().is_some(),
            "a replacement built under our own topology is still serveable",
        );
        lock_recover(&graph.snapshot_pool).clear();

        seed_cache(
            root,
            crate::graph_db::GraphFp { topology: published.topology.wrapping_add(1), ..published },
        );
        assert!(
            graph.snapshot().is_none(),
            "a build made under another extension topology is not served as ours",
        );
    }

    #[test]
    fn snapshot_pool_reuses_and_discards_superseded_handles() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        seed_cache(root, workspace_fingerprint(root));

        let graph = GraphState::for_workspace(root.to_path_buf());
        graph.ensure_loading();
        wait_ready(&graph);

        let pool_len = || lock_recover(&graph.snapshot_pool).len();
        assert_eq!(pool_len(), 0, "no idle handles before the first query");
        let s1 = graph.snapshot().expect("snapshots");
        drop(s1);
        assert_eq!(pool_len(), 1, "the dropped handle parks in the pool");
        let s2 = graph.snapshot().expect("snapshots");
        assert_eq!(pool_len(), 0, "the parked handle is checked out, not re-opened");
        drop(s2);
        assert_eq!(pool_len(), 1);

        {
            let mut pool = lock_recover(&graph.snapshot_pool);
            let entry = pool.pop().expect("one parked entry");
            pool.push(PooledSnapshotEntry { generation: entry.generation + 100, ..entry });
        }
        let s3 = graph.snapshot().expect("snapshots");
        assert_eq!(s3.generation, 7, "a superseded handle never serves a new request");
        assert_eq!(pool_len(), 0, "the superseded entry was discarded at checkout");
    }

    #[test]
    fn drift_marks_stale_and_async_reload_bumps_generation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut graph = GraphState::for_workspace(root.to_path_buf());
        graph.drift_interval = Duration::ZERO;
        graph.ensure_loading();
        wait_ready(&graph);

        let snap1 = graph.snapshot().expect("ready graph snapshots");
        let fresh = graph.freshness(&snap1);
        assert_eq!(fresh.revision, 1);
        assert!(!fresh.stale);
        assert_eq!(fresh.reload, "none");

        write(
            root,
            "CommonModules/Сервер/Ext/Module.bsl",
            "&НаСервере\nФункция Считать() Экспорт Возврат 42; КонецФункции",
        );
        let drifted = graph.freshness(&snap1);
        assert!(drifted.stale, "an on-disk edit must read as stale");
        assert_eq!(drifted.revision, 1, "the stale response still serves the old generation");
        assert!(matches!(drifted.reload, "running" | "failed"));

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

    #[test]
    fn graph_freshness_invalidates_on_hub_delivery() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        sample_workspace(root);

        let hub = WorkspaceChangeHub::start(vec![root.to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let mut graph = GraphState::for_workspace(root.to_path_buf()).with_change_hub(hub.clone());
        graph.drift_interval = Duration::from_secs(120);
        graph.ensure_loading();
        wait_ready(&graph);

        let snap = graph.snapshot().expect("ready");
        assert!(!graph.freshness(&snap).stale, "a freshly built graph is not stale");

        let mut observer = hub.subscribe();
        std::thread::sleep(Duration::from_millis(10));
        write(
            root,
            "CommonModules/Сервер/Ext/Module.bsl",
            "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции",
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut delivered = false;
        while Instant::now() < deadline {
            let batch = hub.drain(observer);
            observer = batch.cursor;
            if batch.entries.iter().any(|e| e.raw.to_string_lossy().contains("Module.bsl")) {
                delivered = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(delivered, "the hub delivered the edit");
        assert!(
            graph.freshness(&snap).stale,
            "a hub-delivered edit is seen without waiting out the drift throttle",
        );
    }

    /// The full live-daemon chain for a topology-only change: a served graph must
    /// read stale after a `dependsOn`-only config edit (no `.bsl`/`.xml` touched),
    /// and the kicked reload must publish a fresh generation that reads clean.
    #[test]
    fn a_depends_on_only_edit_marks_a_served_graph_stale_and_reloads() {
        use super::super::test_support::{write_extension_config, write_extension_workspace};

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_extension_workspace(root, false);

        let mut graph = GraphState::for_workspace(root.to_path_buf());
        graph.drift_interval = Duration::ZERO;
        graph.ensure_loading();
        wait_ready(&graph);

        let snap = graph.snapshot().expect("ready graph snapshots");
        assert!(!graph.freshness(&snap).stale, "a freshly built graph is not stale");

        write_extension_config(root, true);
        let drifted = graph.freshness(&snap);
        assert!(drifted.stale, "a dependsOn-only edit must read as stale");
        assert!(matches!(drifted.reload, "running" | "failed"));

        let mut settled = None;
        for _ in 0..500 {
            let snap = graph.snapshot().expect("snapshot");
            if snap.generation == 2 {
                settled = Some(graph.freshness(&snap));
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let settled = settled.expect("the topology-triggered reload did not publish");
        assert!(!settled.stale, "the reloaded graph reflects the new topology");
    }

    /// End-to-end root re-arm: an extension root added by a topology reload lies
    /// OUTSIDE the hub's original coverage, and after the reload publishes, events
    /// under that root must be hub-delivered — proof the rebuild re-pointed the
    /// live watcher instead of leaving the new subtree to the reconciler.
    #[test]
    fn a_topology_reload_rearms_the_hub_onto_the_new_extension_root() {
        use super::super::test_support::write;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let ext_dir = tempfile::tempdir().unwrap();
        let ext = ext_dir.path();
        super::super::test_support::sample_workspace(root);
        write(root, "Configuration.xml", "<Configuration/>");
        write(ext, "Configuration.xml", "<Configuration/>");

        let hub = WorkspaceChangeHub::start(vec![root.to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let mut graph = GraphState::for_workspace(root.to_path_buf()).with_change_hub(hub.clone());
        graph.drift_interval = Duration::ZERO;
        graph.ensure_loading();
        super::super::test_support::wait_ready(&graph);
        let snap = graph.snapshot().expect("ready");
        assert!(!graph.freshness(&snap).stale);

        // Declare the out-of-tree extension: a topology-only reload trigger.
        std::fs::write(
            root.join("bsl-analyzer.toml"),
            format!(
                "[source]\nroot = \".\"\nextensions = [{{ name = \"a\", path = {:?} }}]\n",
                ext.to_string_lossy()
            ),
        )
        .unwrap();
        // Staleness lands once the hub delivers the config event (the throttled
        // fast path deliberately serves the cached topology until then).
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !graph.freshness(&snap).stale {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(graph.freshness(&snap).stale, "the new extension root must read as drift");
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if graph.snapshot().map(|s| s.generation) == Some(2) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(graph.snapshot().map(|s| s.generation), Some(2), "reload published");

        // The re-armed hub must deliver events under the NEW root. The write is
        // repeated per poll so a delivery is observed even if the ack landed a
        // moment after the generation became visible.
        let cursor = hub.subscribe();
        let file = ext.join("Новый.bsl");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut cursor = cursor;
        let mut seen = false;
        while Instant::now() < deadline {
            std::fs::write(&file, "Процедура П()\nКонецПроцедуры").unwrap();
            std::thread::sleep(Duration::from_millis(50));
            let batch = hub.drain(cursor);
            cursor = batch.cursor;
            if batch.entries.iter().any(|e| e.raw == file) {
                seen = true;
                break;
            }
        }
        assert!(seen, "the hub must deliver events under the newly-added extension root");
    }

    /// Another consumer that stopped draining owes its own reconcile. Answering that debt
    /// here used to drop the graph's fingerprint map and buy a full tree walk on every
    /// freshness check — for as long as the other consumer stayed silent, which is
    /// forever if its thread is gone.
    #[test]
    fn a_foreign_cursors_debt_does_not_cost_the_graph_a_walk() {
        use crate::change_hub::WorkspaceChangeHub;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let hub = WorkspaceChangeHub::start(vec![root.to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let mut graph = GraphState::for_workspace(root.to_path_buf()).with_change_hub(hub.clone());
        // Without this the throttled scan cache answers before the health question is ever
        // asked, and this test would pass no matter what the answer would have been.
        graph.drift_interval = Duration::ZERO;
        graph.ensure_loading();
        wait_ready(&graph);
        // The graph's own cursor exists and is clean from here on: `current_disk_fp`
        // drains it before it asks anything. Two calls settle the map and its own debt.
        let _ = graph.current_disk_fp();
        let _ = graph.current_disk_fp();

        // A stranger subscribes and never drains; then everyone is asked to reconcile.
        // The graph answers for ITSELF with one walk — that debt is genuinely its own —
        // and the stranger's stays outstanding for ever after.
        let _stranger = hub.subscribe();
        hub.degrade_external();
        let _ = graph.current_disk_fp();

        let walks = graph.scan_count.load(std::sync::atomic::Ordering::SeqCst);
        let _ = graph.current_disk_fp();
        assert_eq!(
            graph.scan_count.load(std::sync::atomic::Ordering::SeqCst),
            walks,
            "somebody else's outstanding reconcile is not the graph's to pay for"
        );
    }

    /// The other half, and the one that keeps the first honest: when the HUB cannot
    /// deliver, the graph must keep walking however clean its own cursor is. Without this
    /// leg, replacing the health question with an unconditional fast path passes every
    /// other gate here while going quietly blind.
    ///
    /// The carrier is a hub whose thread never started, not a blind root: the graph
    /// re-declares the hub's targets as it builds, which would take an unwatched root out
    /// of the declaration and leave the hub honestly healthy — a stand that proves nothing.
    #[test]
    fn a_hub_that_cannot_deliver_still_sends_the_graph_back_to_a_walk() {
        use crate::change_hub::{WatchTarget, WorkspaceChangeHub};

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        let hub = WorkspaceChangeHub::start_with_unstartable_thread(vec![WatchTarget::recursive(
            root.to_path_buf(),
        )]);

        let mut graph = GraphState::for_workspace(root.to_path_buf()).with_change_hub(hub);
        graph.drift_interval = Duration::ZERO;
        graph.ensure_loading();
        wait_ready(&graph);
        let _ = graph.current_disk_fp();
        let walks = graph.scan_count.load(std::sync::atomic::Ordering::SeqCst);

        let _ = graph.current_disk_fp();
        assert!(
            graph.scan_count.load(std::sync::atomic::Ordering::SeqCst) > walks,
            "a hub that will never deliver leaves the graph nothing to trust"
        );
    }

    /// A config-file change delivered by the hub must invalidate the throttled
    /// fingerprint cache AND the event-maintained stat map immediately — the map
    /// can only patch file stats, not the topology, so serving its fold after a
    /// config edit would keep a stale topology fresh for up to the walk interval.
    #[test]
    fn graph_freshness_sees_a_config_edit_through_the_hub() {
        use super::super::test_support::{write_extension_config, write_extension_workspace};

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_extension_workspace(root, false);

        let hub = WorkspaceChangeHub::start(vec![root.to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let mut graph = GraphState::for_workspace(root.to_path_buf()).with_change_hub(hub.clone());
        graph.drift_interval = Duration::from_secs(120);
        graph.ensure_loading();
        wait_ready(&graph);

        let snap = graph.snapshot().expect("ready");
        assert!(!graph.freshness(&snap).stale, "a freshly built graph is not stale");

        let mut observer = hub.subscribe();
        std::thread::sleep(Duration::from_millis(10));
        // Re-written per poll: under a fully parallel test run the inotify queue
        // can lag well past a single write's event window.
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut delivered = false;
        while Instant::now() < deadline {
            write_extension_config(root, true);
            std::thread::sleep(Duration::from_millis(20));
            let batch = hub.drain(observer);
            observer = batch.cursor;
            if batch.entries.iter().any(|e| e.raw.to_string_lossy().contains("bsl-analyzer.toml")) {
                delivered = true;
                break;
            }
        }
        assert!(
            delivered,
            "the hub delivered the config edit (events_seen={}, health={:?})",
            hub.events_seen(),
            hub.health(),
        );
        assert!(
            graph.freshness(&snap).stale,
            "a hub-delivered config edit is seen without waiting out the drift throttle",
        );
    }

    #[test]
    fn graph_freshness_ignores_non_scan_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        sample_workspace(root);

        let hub = WorkspaceChangeHub::start(vec![root.to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)));
        let mut graph = GraphState::for_workspace(root.to_path_buf()).with_change_hub(hub.clone());
        graph.drift_interval = Duration::from_secs(120);
        graph.ensure_loading();
        wait_ready(&graph);

        let snap = graph.snapshot().expect("ready");
        assert!(!graph.freshness(&snap).stale, "a freshly built graph is not stale");
        let scans_after_prime = graph.scan_count();

        let mut observer = hub.subscribe();
        std::thread::sleep(Duration::from_millis(10));
        write(root, "CommonModules/Сервер/Ext/Module.bsl.tmp", "editor swap file");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut delivered = false;
        while Instant::now() < deadline {
            let batch = hub.drain(observer);
            observer = batch.cursor;
            if batch.entries.iter().any(|e| e.raw.to_string_lossy().contains(".tmp")) {
                delivered = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(delivered, "the hub delivered the .tmp file");
        assert!(!graph.freshness(&snap).stale, "a temp file does not make the graph stale");
        assert_eq!(
            graph.scan_count(),
            scans_after_prime,
            "an irrelevant temp file must not invalidate the cache and re-trigger a scan",
        );
    }
}
