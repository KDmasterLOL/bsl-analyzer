use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Instant, UNIX_EPOCH};

use crate::cache::graph_db_path;
use crate::change_hub::{ChangeEntry, ChangeKind};
use crate::graph_query::GraphDb;

use super::scan::scan_file_stats;
use super::types::Freshness;
use super::{lock_recover, GraphState, ReloadState};

/// How often the query-path freshness fold must come from a real walk instead of the
/// event-maintained map. Bounds how long a change the hub cannot observe can keep
/// freshness wrong.
pub(super) const WALK_VERIFY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// `canonical path → (mtime nanos, len)` state maintained from hub deliveries and
/// periodically re-anchored by a complete walk.
#[derive(Default)]
pub(super) struct FpMapState {
    pub(super) map: Option<std::collections::BTreeMap<String, (u128, u64)>>,
    pub(super) walked_at: Option<Instant>,
}

/// Throttled cache of the last on-disk fingerprint scan. Guarded by its own mutex
/// held across the walk, so concurrent callers serialize onto one scan per window.
pub(super) struct ScanCache {
    pub(super) at: Instant,
    pub(super) disk_fp: u64,
}

/// Cap on idle pooled snapshot handles. Concurrent graph queries beyond it just open
/// their own handle, exactly as before pooling.
const SNAPSHOT_POOL_CAP: usize = 4;

/// A pooled idle read handle plus the freshness token it was opened under.
pub(super) struct PooledSnapshotEntry {
    pub(super) generation: u64,
    fingerprint: u64,
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
    fingerprint: u64,
    force_stale: bool,
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
        let published_generation = lock_recover(&self.inner).published.as_ref()?.generation;
        {
            let mut pool = lock_recover(&self.snapshot_pool);
            while let Some(entry) = pool.pop() {
                if entry.generation == published_generation {
                    let (generation, fingerprint, force_stale) =
                        (entry.generation, entry.fingerprint, entry.force_stale);
                    return Some(GraphSnapshot {
                        graph: PooledGraphDb {
                            entry: Some(entry),
                            pool: Arc::clone(&self.snapshot_pool),
                        },
                        generation,
                        fingerprint,
                        force_stale,
                    });
                }
            }
        }
        let path = graph_db_path(self.workspace_root.as_deref()?);
        let graph = GraphDb::open(&path).ok()?;
        let (generation, fingerprint, force_stale) = graph.freshness_token().ok()?;
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
        })
    }

    /// Report the freshness of `snapshot` relative to disk, and on drift kick an
    /// async reload (at most one in flight). `stale`/`revision` are relative to the
    /// snapshot that served the response; the reload decision is relative to the
    /// latest published snapshot. Walks the filesystem, so call from a blocking
    /// context.
    pub(crate) fn freshness(&self, snapshot: &GraphSnapshot) -> Freshness {
        let disk_fp = self.current_disk_fp();
        let stale =
            snapshot.force_stale || disk_fp.map(|fp| fp != snapshot.fingerprint).unwrap_or(false);

        let mut inner = lock_recover(&self.inner);
        let Some(published) = inner.published.as_mut() else {
            return Freshness { revision: snapshot.generation, stale, reload: "none" };
        };
        let mut reload = published.reload.label();
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

    pub(super) fn current_disk_fp(&self) -> Option<u64> {
        let root = self.workspace_root.as_deref()?;
        self.invalidate_scan_on_hub_drift();
        let mut cache = lock_recover(&self.scan);
        if let Some(c) = cache.as_ref() {
            if c.at.elapsed() < self.drift_interval {
                return Some(c.disk_fp);
            }
        }
        let hub_healthy = matches!(
            &self.change_hub,
            Some(hub) if matches!(hub.health(), crate::change_hub::Health::Healthy)
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
                    let fp = fold_fingerprint_entries(&entries);
                    *cache = Some(ScanCache { at: Instant::now(), disk_fp: fp });
                    return Some(fp);
                }
            }
        }
        self.scan_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut entries: Vec<(String, u128, u64)> =
            scan_file_stats(root).into_iter().map(|s| (s.path, s.mtime, s.len)).collect();
        entries.sort();
        let fp = fold_fingerprint_entries(&entries);
        {
            let mut fp_state = lock_recover(&self.fp_map);
            fp_state.map = Some(entries.into_iter().map(|(p, m, l)| (p, (m, l))).collect());
            fp_state.walked_at = Some(Instant::now());
        }
        *cache = Some(ScanCache { at: Instant::now(), disk_fp: fp });
        Some(fp)
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
        if relevant.iter().any(|e| e.kind == ChangeKind::SubtreeRemoved) {
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
        matches!(path.extension().and_then(|e| e.to_str()), Some("bsl") | Some("xml"))
    };
    is_scan_ext(&entry.canonical) || is_scan_ext(&entry.raw)
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
    use super::super::test_support::{sample_workspace, seed_cache, wait_ready, write};
    use super::super::{lock_recover, GraphState};
    use super::*;
    use crate::change_hub::WorkspaceChangeHub;
    use std::time::Duration;

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
