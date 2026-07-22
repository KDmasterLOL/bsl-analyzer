use super::{SharedSearchEngine, SharedState, MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY};
use crate::change_hub::WorkspaceChangeHub;
use crate::graph::GraphState;
use bsl_search::SearchEngine;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Test seam: force a reconcile walk (the overflow rescan and the boot store reconcile) to count as
/// errored, so a test can assert the reconcile is skipped (a partial walk must never be treated as
/// authoritative and delete healthy files) — and, at boot, that a Clean init downgrades to a prime.
#[cfg(test)]
pub(super) static FORCE_REWALK_WALK_ERROR: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

impl SharedState {
    /// Drive the search overlay from the change hub. Search is one sink among
    /// several: it drains its own cursor and applies the shared drift classification
    /// (stateless policy) — `.bsl` bodies marked dirty, deleted `.bsl` removed from the
    /// store, `.xml` metadata resolved to the affected documents' context. The raw
    /// (non-canonical) path is used so the strip against the configured source root
    /// still matches when that root has symlinks.
    pub(super) fn spawn_search_sink(
        hub: WorkspaceChangeHub,
        engine: SharedSearchEngine,
        watcher_ready: Arc<AtomicBool>,
        watch_root: PathBuf,
        graph: GraphState,
    ) {
        std::thread::Builder::new()
            .name("bsl-search-overlay-watch".to_owned())
            .spawn(move || {
                // Setup is asynchronous, so wait for it to settle rather than racing
                // a bare `is_watching` check that would bail before the watch arms.
                if !hub.wait_until_watching(Duration::from_secs(60)) {
                    tracing::warn!(
                        "workspace change hub is not watching; search overlay stays in scan mode"
                    );
                    return;
                }

                // Publish readiness before the engine may exist: the engine's own
                // configuration step checks this flag and enables watcher mode when
                // it finishes initializing. Enabling here too covers a warm engine
                // that is already published.
                watcher_ready.store(true, Ordering::SeqCst);
                if let Ok(mut guard) = engine.lock() {
                    if let Some(engine) = guard.as_mut() {
                        engine.enable_workspace_watcher_mode();
                    }
                }
                tracing::info!("search overlay sink subscribed to workspace change hub");

                let mut cursor = hub.subscribe();
                let mut generation = 0u64;
                loop {
                    // Wake on new drift; the timeout only bounds how long a shutdown
                    // takes to be noticed (the daemon detaches this thread).
                    generation = hub.wait_for_change(generation, Duration::from_secs(30));
                    let batch = hub.drain(cursor);
                    cursor = batch.cursor;
                    Self::apply_search_drift(
                        &engine,
                        &watch_root,
                        &batch.entries,
                        batch.rescan_required,
                        &graph,
                    );
                }
            })
            .ok();
    }

    /// Apply one drained batch to the search overlay. Extracted from the sink loop so it
    /// is unit-testable without driving the thread. On overflow (exact paths lost) it
    /// re-walks the whole tree; otherwise it classifies (stateless policy) and applies
    /// each bucket: `.bsl` bodies dirty, deleted `.bsl` removed, `.xml` → affected context.
    pub(super) fn apply_search_drift(
        engine: &SharedSearchEngine,
        watch_root: &Path,
        entries: &[crate::change_hub::ChangeEntry],
        rescan_required: bool,
        graph: &GraphState,
    ) {
        // Overflow means the hub dropped detail: the exact changed paths are lost.
        // Restore parity with the old unbounded watcher (which never lost a `.bsl`) by
        // re-marking every workspace `.bsl` dirty, so the overlay's incremental refresh
        // reconsiders them all.
        if rescan_required {
            tracing::warn!(
                "workspace change hub overflowed; re-marking all workspace .bsl paths dirty for the search overlay"
            );
            Self::rewalk_workspace_bsl_dirty(engine, watch_root);
            return;
        }

        // Search keeps no per-path baseline, so the stateless policy (no baseline, empty
        // config set) buckets straight from on-disk truth.
        let class =
            crate::drift_classify::classify_drift(entries, &std::collections::HashSet::new(), None);

        // Modified `.bsl` bodies: mark dirty for the overlay's incremental refresh.
        for dp in &class.bsl_modified {
            Self::mark_search_path_dirty(engine, &dp.raw);
        }

        // Deleted `.bsl`: drop from the store so it stops appearing in results.
        if !class.bsl_removed.is_empty() {
            Self::remove_search_paths(engine, class.bsl_removed.iter().map(|d| d.raw.as_path()));
        }

        // Changed `.xml` metadata: mark the affected documents' stored context stale, then
        // nudge the graph to catch up. The context re-render only runs on a graph publish;
        // without this nudge a user who only calls `search_code` never triggers a rebuild,
        // so the marks would sit unresolved forever. The nudge is single-flight and never
        // blocks — it schedules a background rebuild whose publish fires the refresh hook.
        if !class.xml_paths.is_empty()
            && Self::mark_xml_affected_context_dirty(engine, &class.xml_paths, graph)
        {
            graph.nudge_rebuild();
        }

        // A subtree removal lost the descendant list → reconsider the whole tree.
        if class.structural_rescan {
            Self::rewalk_workspace_bsl_dirty(engine, watch_root);
        }
    }

    /// Remove a batch of deleted `.bsl` files from the workspace store. Each removal
    /// evicts exactly that file's vectors from the live index incrementally (no full
    /// rebuild, no sidecar rewrite — the row deletion already invalidates the persisted
    /// sidecar), so a large deletion no longer stalls under the engine lock. A path that
    /// is not a workspace `.bsl` is skipped.
    fn remove_search_paths<'a>(engine: &SharedSearchEngine, paths: impl Iterator<Item = &'a Path>) {
        if let Ok(mut guard) = engine.lock() {
            if let Some(engine) = guard.as_mut() {
                for path in paths {
                    match engine.remove_workspace_path(path) {
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!(path = ?path, "failed to remove workspace file: {e}")
                        }
                    }
                }
            }
        }
    }

    /// Resolve each changed `.xml` descriptor to the workspace documents it affects and
    /// mark their stored graph context stale, so a later reindex/embed pass re-renders it
    /// (marking only — the render is deferred, so it never races the graph's own drift
    /// catch-up). OWNED modules resolve by path convention: an MDO / common-module /
    /// service / form descriptor at `<Dir>/<Name>.xml` owns every `.bsl` under the sibling
    /// `<Dir>/<Name>/` subtree. Any `.xml` directly at the workspace root (a
    /// configuration-root descriptor, whose change can shift any module's context)
    /// conservatively marks the whole collection. REFERENCING modules (a module that
    /// merely READS the changed MDO — its rendered `graph_context` embeds the object's
    /// metadata reads) are additionally resolved through the persisted graph's inbound read
    /// edges (see [`Self::resolve_referencing_module_rels`]).
    ///
    /// The filesystem walk (owned-subtree resolution) and the graph db read (referencing
    /// resolution) both run OUTSIDE the engine lock; the lock is taken only briefly for the
    /// store writes. Returns whether it marked at least one path context-dirty (owned,
    /// referencing, or a whole-collection mark), so the caller can gate the graph catch-up
    /// nudge on real work having been queued.
    fn mark_xml_affected_context_dirty(
        engine: &SharedSearchEngine,
        xml_paths: &[crate::drift_classify::DriftPath],
        graph: &GraphState,
    ) -> bool {
        // Read the workspace root once (brief lock), then resolve owned subtrees off-lock.
        let workspace_root = {
            let Ok(guard) = engine.lock() else { return false };
            let Some(engine) = guard.as_ref() else { return false };
            engine.workspace_root().map(Path::to_path_buf)
        };

        let mut owned_modules: Vec<PathBuf> = Vec::new();
        let mut mark_whole_collection = false;
        for dp in xml_paths {
            match owned_module_subtree(&dp.raw) {
                Some(subtree) => owned_modules.extend(walk_bsl_files(&subtree)),
                None if is_workspace_root_xml(&dp.raw, workspace_root.as_deref()) => {
                    mark_whole_collection = true;
                }
                None => {}
            }
        }

        // Referencing modules: resolved off any lock via the persisted graph, BEFORE the
        // store-write lock below (the graph db read must never nest under the engine lock).
        let referencing_rels =
            Self::resolve_referencing_module_rels(graph, xml_paths, workspace_root.as_deref());

        if owned_modules.is_empty() && referencing_rels.is_empty() && !mark_whole_collection {
            return false;
        }

        // Brief lock for the store writes only.
        let Ok(guard) = engine.lock() else { return false };
        let Some(engine) = guard.as_ref() else { return false };
        let mut marked = false;
        if mark_whole_collection {
            match engine.mark_workspace_context_dirty() {
                Ok(count) => marked |= count > 0,
                Err(e) => tracing::warn!("failed to mark collection context dirty: {e}"),
            }
        }
        for bsl in owned_modules {
            match engine.mark_workspace_path_context_dirty(&bsl) {
                Ok(did) => marked |= did,
                Err(e) => tracing::warn!(path = ?bsl, "failed to mark context dirty: {e}"),
            }
        }
        for rel in referencing_rels {
            match engine.mark_workspace_path_context_dirty(&rel) {
                Ok(did) => marked |= did,
                Err(e) => {
                    tracing::warn!(path = %rel, "failed to mark referencing context dirty: {e}")
                }
            }
        }
        marked
    }

    /// Reverse-look-up the workspace modules that READ any changed MDO, returning their
    /// workspace-relative `.bsl` keys (the spelling the `code` collection stores). A metadata
    /// change alters the `graph_context` of every module that reads the object — not just its
    /// owned modules — and the persisted graph is the only record of who reads what.
    ///
    /// Queries the CURRENTLY PUBLISHED graph via [`GraphState::snapshot`], which gates on a
    /// published build and opens the read-only db off the graph's inner lock. Pre-drift edges
    /// are exactly right here: the set of referencing modules is defined by OTHER modules'
    /// bodies, which this `.xml` edit did not touch — the follow-up rebuild only re-renders the
    /// contexts marked here, it never changes who references the object. No published graph yet
    /// (or an `.xml` that maps to no MDO node — a form/command/config-root descriptor) → an
    /// empty set, so referencing marks are simply skipped and the owned marks + nudge still fire;
    /// a later publish consumes whatever marks then exist. Degrades, never blocks or errors.
    ///
    /// Off-lock throughout: opens the graph db once and runs one index-backed inbound-edge
    /// query per resolved MDO node id, so a batch of N `.xml` edits does at most N indexed
    /// queries, never a table scan.
    fn resolve_referencing_module_rels(
        graph: &GraphState,
        xml_paths: &[crate::drift_classify::DriftPath],
        workspace_root: Option<&Path>,
    ) -> std::collections::HashSet<String> {
        let mut rels = std::collections::HashSet::new();
        let mdo_ids: Vec<String> =
            xml_paths.iter().filter_map(|dp| xml_to_mdo_id(&dp.raw)).collect();
        if mdo_ids.is_empty() {
            return rels;
        }
        let Some(workspace_root) = workspace_root else { return rels };
        let Some(snapshot) = graph.snapshot() else { return rels };
        let source_prefix = canonical_source_prefix(workspace_root);
        for mdo_id in mdo_ids {
            match snapshot.graph.referencing_files(&mdo_id) {
                Ok(files) => {
                    for file in files {
                        if let Some(rel) = graph_file_to_rel(&file, &source_prefix) {
                            rels.insert(rel);
                        }
                    }
                }
                Err(e) => tracing::warn!(mdo = %mdo_id, "referencing-files lookup failed: {e}"),
            }
        }
        rels
    }

    /// Re-mark every workspace `.bsl` dirty for the search overlay, then reconcile the
    /// store against what is actually on disk. Used when the change hub overflowed or a
    /// subtree was removed and the exact changed paths are no longer known, so the overlay
    /// must reconsider the whole tree. Marking alone only covers files that STILL exist; a
    /// file deleted during the lost window would keep its FTS rows and vectors forever, so
    /// the reconcile diffs the walked (present) set against the stored set and removes the
    /// gone paths. The walk runs OUTSIDE the engine lock; the reconcile takes the lock only
    /// for its bounded O(stored) store writes.
    fn rewalk_workspace_bsl_dirty(engine: &SharedSearchEngine, watch_root: &Path) {
        let mut present: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        let mut walk_errors = 0usize;
        for file in walkdir::WalkDir::new(watch_root).follow_links(true) {
            let file = match file {
                Ok(file) => file,
                Err(e) => {
                    walk_errors += 1;
                    tracing::warn!("search rescan walk error: {e}");
                    continue;
                }
            };
            if file.file_type().is_file() {
                let path = file.path();
                if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")) {
                    present.insert(path.to_path_buf());
                }
                Self::mark_search_path_dirty(engine, path);
            }
        }
        #[cfg(test)]
        if FORCE_REWALK_WALK_ERROR.load(Ordering::SeqCst) {
            walk_errors += 1;
        }
        // A partial walk (permission error, symlink loop, transient IO) is NOT authoritative:
        // `present` is missing healthy files, so reconciling against it would delete them from
        // the store. Only reconcile when the walk completed with zero errors; marking the found
        // files dirty already happened above regardless.
        if walk_errors > 0 {
            tracing::warn!(
                walk_errors,
                "search rescan walk incomplete; skipping reconcile to avoid deleting healthy files"
            );
            return;
        }
        if let Ok(mut guard) = engine.lock() {
            if let Some(engine) = guard.as_mut() {
                match engine.reconcile_workspace_files(&present) {
                    Ok(removed) if removed > 0 => {
                        tracing::info!(
                            removed,
                            "search rescan reconciled deleted files out of the index"
                        )
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("search rescan reconcile failed: {e}"),
                }
            }
        }
    }

    /// Reconcile the just-indexed workspace store against on-disk truth at BOOT, on the still-owned
    /// engine (no shared lock held), BEFORE the overlay-init decision is applied. A boot index step
    /// (`index_directory_deferred` / `index_directory_fts`, or a fused parse ingest) only re-ingests
    /// files that EXIST now — it never removes rows for a `.bsl` DELETED while the daemon was down.
    /// So a store row for a vanished file survives, and an [`OverlayInit::Clean`] — which asserts the
    /// store already equals the working tree — would serve that ghost forever. This walks the source
    /// tree (error-aware) and, on a CLEAN walk, calls [`SearchEngine::reconcile_workspace_files`] to
    /// remove every stored-but-gone path (tombstone + overlay dirty + incremental vector eviction —
    /// the same removal path the overflow rescan ships).
    ///
    /// Returns whether the store was PROVEN reconciled: `false` on any walk error OR a reconcile
    /// failure. A partial walk's `present` set is short, so trusting it would delete healthy rows —
    /// hence the S1 gate (skip reconcile on any walk error) is kept verbatim. And because a failed
    /// walk could not prove reconciliation, the caller must NOT stay Clean: it downgrades to a prime,
    /// whose own scan lazily hides files it finds missing. A prime's scan may itself be incomplete
    /// after a walk error, but a prime never ASSERTS a clean store the way `Clean` does — it only
    /// serves what it can see and hides the rest — so it is the strictly safer degraded default,
    /// matching the pre-existing behavior for a store that could not be reconciled.
    pub(super) fn reconcile_boot_store_with_disk(
        engine: &mut SearchEngine,
        source_root: &Path,
    ) -> bool {
        let mut present: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        let mut walk_errors = 0usize;
        for entry in walkdir::WalkDir::new(source_root).follow_links(true) {
            match entry {
                Ok(entry) => {
                    if entry.file_type().is_file()
                        && entry
                            .path()
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("bsl"))
                    {
                        present.insert(entry.path().to_path_buf());
                    }
                }
                Err(e) => {
                    walk_errors += 1;
                    tracing::warn!("search boot reconcile walk error: {e}");
                }
            }
        }
        #[cfg(test)]
        if FORCE_REWALK_WALK_ERROR.load(Ordering::SeqCst) {
            walk_errors += 1;
        }
        if walk_errors > 0 {
            tracing::warn!(
                walk_errors,
                "search boot reconcile walk incomplete; priming the overlay instead of clean-init"
            );
            return false;
        }
        match engine.reconcile_workspace_files(&present) {
            Ok(removed) => {
                if removed > 0 {
                    tracing::info!(
                        removed,
                        "search boot reconciled deleted files out of the store"
                    );
                }
                true
            }
            Err(e) => {
                tracing::warn!("search boot reconcile failed; priming the overlay instead: {e}");
                false
            }
        }
    }
    /// Mark one path dirty in the search overlay if it is a `.bsl` file. Filtering
    /// on the consumer side keeps the hub itself extension-agnostic.
    fn mark_search_path_dirty(engine: &SharedSearchEngine, path: &Path) {
        if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")) {
            return;
        }
        if let Ok(guard) = engine.lock() {
            if let Some(engine) = guard.as_ref() {
                if let Err(e) = engine.mark_workspace_path_dirty(path) {
                    tracing::warn!(path = ?path, "failed to mark workspace file dirty: {e}");
                }
            }
        }
    }
}

/// Prefetch resident snapshots for the overlay's dirty paths and feed them into the
/// incremental reindex, so a following query serves chunks cut from the SHARED resident
/// parse instead of a second disk read+parse. Called at the top of a code-search request,
/// before the query acquires the engine lock.
///
/// Bounded to [`MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY`] paths per call.
///
/// Lock discipline: the resident read must never overlap the engine lock. So this
/// reads the dirty-path list and the source handle under a brief engine lock, RELEASES it,
/// fetches the snapshots with NO lock held, then applies them under a second brief engine
/// lock that only touches the overlay cache (never the resident). A resident that is
/// absent/loading, or a path it cannot serve, is simply missing from the map and the
/// reindex disk-reads it — so search never regresses when the resident is unavailable.
pub(super) fn prefetch_resident_overlay(engine: &SharedSearchEngine) {
    let (source, workspace_root, dirty) = {
        let Ok(guard) = engine.lock() else { return };
        let Some(engine) = guard.as_ref() else { return };
        let Some(source) = engine.module_snapshot_source() else { return };
        // The overlay keys dirty paths relative to THIS engine root (the project's — possibly
        // nested — source root); resolving them for the resident needs the same root.
        let Some(workspace_root) = engine.workspace_root().map(Path::to_path_buf) else {
            return;
        };
        match engine.workspace_overlay_dirty_paths() {
            Ok(dirty) => (source, workspace_root, dirty),
            Err(e) => {
                tracing::debug!("overlay dirty-path read failed: {e}");
                return;
            }
        }
    };
    if dirty.is_empty() {
        return;
    }

    // Search and diagnostics drain independent hub cursors and a query never polls drift on
    // its own, so the resident is usually BEHIND disk on the just-edited files. Reconcile
    // pending drift FIRST — off the engine lock, resident lock only (I3 holds) — so the
    // snapshot text below matches disk and the byte-compare hits instead of falling back to a
    // disk read. A resident rebuild in flight is skipped inside the drain, never blocking here.
    source.catch_up();

    // Resident reads run OFF the engine lock. The `!Send` parses stay in this local map on
    // the calling thread and never cross a thread or an await boundary.
    let mut snapshots: std::collections::HashMap<String, bsl_search::ModuleSnapshot> =
        std::collections::HashMap::new();
    // Cap the per-query resident prefetch: a branch switch can dirty thousands of paths, and
    // fetching+reindexing them all on the query thread would be unbounded work. Serve at most
    // this many from the shared parse per query; the remainder STAY dirty and are picked up by
    // the query's own lazy disk refresh and by later queries' prefetches. The cap is the whole
    // budget — no separate time budget needed.
    for rel_path in dirty.iter().take(MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY) {
        // Resolve the engine-relative dirty path to an ABSOLUTE path against the engine root
        // before handing it to the resident: the resident is indexed under the OUTER workspace
        // root, so a bare relative path would be re-joined against that root and silently miss
        // on every nested config. The snapshot map stays keyed by the engine rel, which is
        // what `reindex_dirty_from_snapshots` looks up.
        let abs_path = workspace_root.join(rel_path);
        if let bsl_search::SnapshotFetch::Fetched(snapshot) =
            source.text_and_parse(&abs_path.to_string_lossy())
        {
            snapshots.insert(rel_path.clone(), snapshot);
        }
    }
    if snapshots.is_empty() {
        return;
    }

    let Ok(guard) = engine.lock() else { return };
    let Some(engine) = guard.as_ref() else { return };
    if let Err(e) = engine.reindex_dirty_from_snapshots(&snapshots) {
        tracing::debug!("resident-fed overlay reindex failed: {e}");
    }
}

/// The owned-module subtree of a metadata descriptor `.xml`: `<Dir>/<Name>/` beside a
/// `<Dir>/<Name>.xml`, when that directory exists. Every `.bsl` under it (object /
/// manager / recordset / form / command modules, or a common-module / service body) is
/// owned by the object the descriptor defines — so the path convention covers ordinary
/// MDOs (which carry no substrate back-link) and common-modules/services alike, with no
/// resident lookup and no resident/engine lock coupling.
fn owned_module_subtree(xml: &Path) -> Option<PathBuf> {
    let stem = xml.file_stem()?;
    let subtree = xml.parent()?.join(stem);
    subtree.is_dir().then_some(subtree)
}

/// Every `.bsl` file under `dir`.
fn walk_bsl_files(dir: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .filter(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")))
        .collect()
}

/// Map a metadata descriptor `.xml` at `<KindPlural>/<Name>.xml` to its graph MDO node id
/// `mdo/<EnglishType>/<Name>` (the id the fused build encodes, verified against
/// `ide::GraphRowEncoder`). `None` when the parent directory is not a known metadata-kind
/// plural — a form/command descriptor, an `Ext/…` file, or a configuration-root descriptor —
/// since those carry no `mdo/` node and thus no inbound read edges to reverse-look-up. The
/// `<KindPlural>` → [`bsl_metadata::MdoType`] mapping reuses the canonical
/// [`bsl_metadata::MdoType::from_plural`] table rather than duplicating a directory map.
fn xml_to_mdo_id(xml: &Path) -> Option<String> {
    let name = xml.file_stem()?.to_str()?;
    let kind_dir = xml.parent()?.file_name()?.to_str()?;
    let mdo_type = bsl_metadata::MdoType::from_plural(kind_dir)?;
    Some(format!("mdo/{}/{name}", mdo_type.english_name()))
}

/// The canonical, `/`-normalised source prefix used to relativise a graph `nodes.file`
/// (stored absolute + canonical by `enumerate_bsl_files`) into the `code` collection key,
/// derived exactly as `FusedChunkWriter` derives its stored rel paths so the two agree.
fn canonical_source_prefix(workspace_root: &Path) -> String {
    workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

/// Relativise an absolute, `/`-normalised graph `nodes.file` to the `code` collection key,
/// mirroring `FusedChunkWriter`: strip the source prefix, require a path-separator boundary
/// so a sibling root whose name merely starts with the prefix string is not mistaken for a
/// child, then drop the leading `/`. `None` for a file outside the source root (an extension
/// module the local index omits) or an empty remainder.
fn graph_file_to_rel(file: &str, source_prefix: &str) -> Option<String> {
    let prefix = source_prefix.trim_end_matches('/');
    let rel =
        file.strip_prefix(prefix).filter(|rest| rest.starts_with('/'))?.trim_start_matches('/');
    (!rel.is_empty()).then(|| rel.to_owned())
}

/// Whether `xml` sits directly at the workspace root — any such descriptor
/// (`Configuration.xml`, `ConfigDumpInfo.xml`, a plugin's root descriptor, …) can shift
/// any module's context, so it is handled conservatively by marking the whole collection
/// rather than a resolvable owned subtree. When the workspace root is unknown, fall back
/// to the `Configuration.xml` name so the conservative branch still fires for the one
/// descriptor guaranteed to live at the root.
fn is_workspace_root_xml(xml: &Path, workspace_root: Option<&Path>) -> bool {
    match workspace_root {
        Some(root) => xml.parent() == Some(root),
        None => xml.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Configuration.xml")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{
        write_common_module, write_common_module_tree, EnvVarGuard, ENV_LOCK,
    };
    use super::{SharedState, FORCE_REWALK_WALK_ERROR, MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY};
    use crate::state::types::OverlayInit;
    use bsl_search::{IndexedDocument, SearchEngine};
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    #[test]
    fn search_sink_marks_only_bsl_paths_dirty() {
        use crate::change_hub::WorkspaceChangeHub;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::{Duration, Instant};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let hub = WorkspaceChangeHub::start(vec![workspace.clone()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)), "the watch must arm");
        // A second cursor observes the raw accumulator independently of the sink.
        let observer = hub.subscribe();

        let watcher_ready = Arc::new(AtomicBool::new(false));
        SharedState::spawn_search_sink(
            hub.clone(),
            Arc::clone(&engine_arc),
            Arc::clone(&watcher_ready),
            workspace.clone(),
            crate::graph::GraphState::disabled(),
        );

        // Wait deterministically for the sink to subscribe (observer + sink = 2
        // cursors) before mutating the tree, so its cursor covers the changes below.
        let deadline = Instant::now() + Duration::from_secs(5);
        while hub.active_cursor_count() < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(hub.active_cursor_count(), 2, "the sink subscribed its cursor");

        let bsl = workspace.join("Module.bsl");
        std::fs::write(&bsl, "Процедура П()\nКонецПроцедуры").unwrap();
        let xml = workspace.join("Configuration.xml");
        std::fs::write(&xml, "<Configuration/>").unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut dirty_has_bsl = false;
        while Instant::now() < deadline {
            let snapshot = {
                let guard = engine_arc.lock().unwrap();
                guard.as_ref().unwrap().workspace_overlay_dirty_paths_snapshot().unwrap()
            };
            if snapshot.keys().any(|p| p.ends_with("Module.bsl")) {
                dirty_has_bsl = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(dirty_has_bsl, "the .bsl change is marked dirty for the search overlay");

        let snapshot = {
            let guard = engine_arc.lock().unwrap();
            guard.as_ref().unwrap().workspace_overlay_dirty_paths_snapshot().unwrap()
        };
        assert!(
            !snapshot.keys().any(|p| p.ends_with("Configuration.xml")),
            "search ignores non-.bsl paths",
        );
        assert!(watcher_ready.load(Ordering::SeqCst), "the sink publishes watcher readiness");

        // The hub itself accepted the .xml change; only the consumer filtered it.
        // The event is asynchronous, so poll the observer cursor until it lands.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observer = observer;
        let mut saw_xml = false;
        while Instant::now() < deadline {
            let batch = hub.drain(observer);
            observer = batch.cursor;
            if batch.entries.iter().any(|e| e.raw.ends_with("Configuration.xml")) {
                saw_xml = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(saw_xml, "the accumulator carries the .xml change for other consumers");
    }

    /// On a hub overflow the exact changed paths are lost, so the sink re-walks the
    /// workspace and marks every `.bsl` dirty (and nothing else), restoring the
    /// old unbounded watcher's guarantee that no `.bsl` change is dropped.
    #[test]
    fn search_sink_rewalks_all_bsl_on_overflow() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        // Watcher mode makes `mark_workspace_path_dirty` record into the dirty set.
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        // A nested tree of `.bsl` plus a non-`.bsl` file that must NOT be marked.
        let nested = workspace.join("CommonModules").join("Модуль");
        fs::create_dir_all(&nested).unwrap();
        let a = workspace.join("A.bsl");
        let b = nested.join("B.bsl");
        fs::write(&a, "Процедура П()\nКонецПроцедуры").unwrap();
        fs::write(&b, "Процедура П()\nКонецПроцедуры").unwrap();
        fs::write(workspace.join("Configuration.xml"), "<Configuration/>").unwrap();

        SharedState::rewalk_workspace_bsl_dirty(&engine_arc, &workspace);

        let snapshot = {
            let guard = engine_arc.lock().unwrap();
            guard.as_ref().unwrap().workspace_overlay_dirty_paths_snapshot().unwrap()
        };
        assert!(snapshot.keys().any(|p| p.ends_with("A.bsl")), "top-level .bsl re-marked");
        assert!(snapshot.keys().any(|p| p.ends_with("B.bsl")), "nested .bsl re-marked");
        assert!(
            !snapshot.keys().any(|p| p.ends_with("Configuration.xml")),
            "non-.bsl paths are left alone",
        );
    }

    /// A deleted `.bsl` is removed from the workspace store so it stops appearing in
    /// results — closing the pre-existing gap where a deleted file lingered in FTS.
    #[test]
    fn search_sink_removes_deleted_bsl_from_results() {
        use crate::change_hub::{ChangeEntry, ChangeKind};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine
            .sync_indexed_documents_in_collection(
                "code",
                &[IndexedDocument {
                    collection: "code".to_owned(),
                    path: "Removed.bsl".to_owned(),
                    symbol_name: "УдаляемаяПроцедура".to_owned(),
                    kind: "procedure".to_owned(),
                    line_start: 0,
                    line_end: 1,
                    text: "Процедура УдаляемаяПроцедура()\nКонецПроцедуры".to_owned(),
                    content_hash: "h".to_owned(),
                    graph_context: None,
                }],
                None,
            )
            .unwrap();
        assert_eq!(engine.file_count().unwrap(), 1);
        assert!(
            !engine.text_search("УдаляемаяПроцедура", 10, Some("code")).unwrap().is_empty(),
            "the indexed file is initially found",
        );
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        // The file is gone from disk: classification re-stats it (stats are truth) → removed.
        let removed = workspace.join("Removed.bsl");
        let entry = ChangeEntry {
            canonical: removed.clone(),
            raw: removed,
            kind: ChangeKind::MaybeRemoved,
            seq: 1,
        };
        SharedState::apply_search_drift(
            &engine_arc,
            &workspace,
            &[entry],
            false,
            &crate::graph::GraphState::disabled(),
        );

        let guard = engine_arc.lock().unwrap();
        let engine = guard.as_ref().unwrap();
        assert_eq!(engine.file_count().unwrap(), 0, "the deleted file is dropped from the store");
        assert!(
            engine.text_search("УдаляемаяПроцедура", 10, Some("code")).unwrap().is_empty(),
            "the deleted file no longer appears in FTS results",
        );
    }

    /// An `.xml` metadata edit marks only the owned modules (the sibling `<Dir>/<Name>/`
    /// subtree) context-dirty via the store side table; unrelated modules are untouched
    /// and nothing is marked dirty — proving the resolver walks the owned subtree only,
    /// never the whole workspace.
    #[test]
    fn search_sink_xml_marks_only_owned_modules_context_dirty() {
        use crate::change_hub::{ChangeEntry, ChangeKind};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        // An MDO descriptor with an owned module, plus an unrelated object elsewhere.
        let owned = workspace.join("Catalogs/Товары/Ext/ObjectModule.bsl");
        fs::create_dir_all(owned.parent().unwrap()).unwrap();
        fs::write(&owned, "Процедура П()\nКонецПроцедуры").unwrap();
        let unrelated = workspace.join("Catalogs/Другой/Ext/ObjectModule.bsl");
        fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
        fs::write(&unrelated, "Процедура П()\nКонецПроцедуры").unwrap();
        let xml = workspace.join("Catalogs/Товары.xml");
        fs::write(&xml, "<MetaDataObject/>").unwrap();

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let entry = ChangeEntry {
            canonical: xml.clone(),
            raw: xml,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        SharedState::apply_search_drift(
            &engine_arc,
            &workspace,
            &[entry],
            false,
            &crate::graph::GraphState::disabled(),
        );

        let guard = engine_arc.lock().unwrap();
        let engine = guard.as_ref().unwrap();
        let dirty = engine.context_dirty_paths("code").unwrap();
        assert!(
            dirty.contains("Catalogs/Товары/Ext/ObjectModule.bsl"),
            "the owned module is marked context-dirty: {dirty:?}",
        );
        assert!(
            !dirty.contains("Catalogs/Другой/Ext/ObjectModule.bsl"),
            "an unrelated object's module is left untouched: {dirty:?}",
        );
        assert_eq!(dirty.len(), 1, "only the owned subtree is marked, not the whole tree");
        // The xml path is metadata context, not a body edit: nothing is marked dirty and
        // no whole-workspace walk ran.
        let snapshot = engine.workspace_overlay_dirty_paths_snapshot().unwrap();
        assert!(snapshot.is_empty(), "an xml edit marks no body dirty and triggers no walk");
    }

    /// A metadata `.xml` edit marks BOTH the object's owned modules (path convention) AND the
    /// REFERENCING modules — those whose `graph_context` embeds a read of the object — resolved
    /// through the persisted graph's inbound read edges. A module that references nothing about
    /// the object is left untouched.
    ///
    /// Revert-proof: drop the `resolve_referencing_module_rels` call in
    /// `mark_xml_affected_context_dirty` and the referencing module `Б` is no longer marked —
    /// the referencing assertion fails.
    #[test]
    fn search_sink_xml_marks_owned_and_referencing_modules_context_dirty() {
        use crate::change_hub::{ChangeEntry, ChangeKind};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        fs::write(workspace.join("Configuration.xml"), "<Configuration/>").unwrap();

        // Catalog Х with an OWNED object module (A), resolved by path convention.
        let xml = workspace.join("Catalogs/Х.xml");
        fs::create_dir_all(xml.parent().unwrap()).unwrap();
        fs::write(
            &xml,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000001">
        <Properties><Name>Х</Name><CodeLength>9</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#,
        )
        .unwrap();
        let owned_a = workspace.join("Catalogs/Х/Ext/ObjectModule.bsl");
        fs::create_dir_all(owned_a.parent().unwrap()).unwrap();
        fs::write(&owned_a, "Процедура П() Экспорт\nКонецПроцедуры").unwrap();

        // Referencing common module Б reads the catalog (manager access + query) → inbound
        // read edges into `mdo/Catalog/Х`. Non-referencing module В reads nothing about it.
        write_common_module(
            &workspace,
            "Б",
            "&НаСервере\nПроцедура ЧитаетХ() Экспорт\nСправочники.Х.СоздатьЭлемент();\nЗапрос = \"ВЫБРАТЬ Код ИЗ Справочник.Х\";\nКонецПроцедуры",
        );
        write_common_module(
            &workspace,
            "В",
            "&НаСервере\nПроцедура НичегоНеЧитает() Экспорт\nВозврат;\nКонецПроцедуры",
        );

        // Build + publish the graph so the reverse lookup has real inbound edges to read.
        let out = crate::cache::graph_db_path(&workspace);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        let summary = crate::graph_db::build_graph_database(
            &crate::graph::ProjectSnapshot::load(&workspace),
            &out,
            100,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: 0,
                files: 0,
                built_at: "t".to_owned(),
            },
        )
        .expect("graph builds");
        let graph = crate::graph::GraphState::for_workspace(workspace.clone());
        graph.adopt_prebuilt(1, 0, summary.modules);

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let entry = ChangeEntry {
            canonical: xml.clone(),
            raw: xml,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        SharedState::apply_search_drift(&engine_arc, &workspace, &[entry], false, &graph);

        let guard = engine_arc.lock().unwrap();
        let engine = guard.as_ref().unwrap();
        let dirty = engine.context_dirty_paths("code").unwrap();
        assert!(
            dirty.contains("Catalogs/Х/Ext/ObjectModule.bsl"),
            "the owned module is marked context-dirty: {dirty:?}",
        );
        assert!(
            dirty.contains("CommonModules/Б/Ext/Module.bsl"),
            "the referencing module (reads the catalog) is marked context-dirty: {dirty:?}",
        );
        assert!(
            !dirty.contains("CommonModules/В/Ext/Module.bsl"),
            "a module that references nothing about the catalog is left untouched: {dirty:?}",
        );
    }

    /// An `.xml` edit BEFORE any graph is published degrades: owned modules are still marked
    /// (path convention needs no graph) and referencing resolution is silently skipped — no
    /// error, no panic. The reverse lookup only rides a published graph.
    #[test]
    fn search_sink_xml_referencing_degrades_without_published_graph() {
        use crate::change_hub::{ChangeEntry, ChangeKind};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        fs::write(workspace.join("Configuration.xml"), "<Configuration/>").unwrap();
        let xml = workspace.join("Catalogs/Х.xml");
        fs::create_dir_all(xml.parent().unwrap()).unwrap();
        fs::write(&xml, "<MetaDataObject/>").unwrap();
        let owned_a = workspace.join("Catalogs/Х/Ext/ObjectModule.bsl");
        fs::create_dir_all(owned_a.parent().unwrap()).unwrap();
        fs::write(&owned_a, "Процедура П() Экспорт\nКонецПроцедуры").unwrap();
        // A would-be referencing module exists on disk but there is NO published graph, so it
        // is not discoverable and must not be marked.
        write_common_module(
            &workspace,
            "Б",
            "&НаСервере\nПроцедура ЧитаетХ() Экспорт\nСправочники.Х.СоздатьЭлемент();\nКонецПроцедуры",
        );

        // A workspace graph that has never been built → `snapshot()` returns None.
        let graph = crate::graph::GraphState::for_workspace(workspace.clone());

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let entry = ChangeEntry {
            canonical: xml.clone(),
            raw: xml,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        SharedState::apply_search_drift(&engine_arc, &workspace, &[entry], false, &graph);

        let guard = engine_arc.lock().unwrap();
        let engine = guard.as_ref().unwrap();
        let dirty = engine.context_dirty_paths("code").unwrap();
        assert!(
            dirty.contains("Catalogs/Х/Ext/ObjectModule.bsl"),
            "the owned module is still marked without a published graph: {dirty:?}",
        );
        assert!(
            !dirty.contains("CommonModules/Б/Ext/Module.bsl"),
            "referencing resolution is skipped with no published graph: {dirty:?}",
        );
    }

    /// ANY `.xml` directly at the workspace root (not only `Configuration.xml`), with no
    /// owned-module subtree, conservatively marks the whole collection context-dirty — a
    /// root descriptor change can shift any module's context.
    #[test]
    fn search_sink_root_xml_marks_whole_collection_context_dirty() {
        use crate::change_hub::{ChangeEntry, ChangeKind};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        let doc = |path: &str, sym: &str| IndexedDocument {
            collection: "code".to_owned(),
            path: path.to_owned(),
            symbol_name: sym.to_owned(),
            kind: "procedure".to_owned(),
            line_start: 0,
            line_end: 1,
            text: format!("Процедура {sym}()\nКонецПроцедуры"),
            content_hash: "h".to_owned(),
            graph_context: None,
        };
        engine
            .sync_indexed_documents_in_collection(
                "code",
                &[doc("A.bsl", "Ааа"), doc("B.bsl", "Ббб")],
                None,
            )
            .unwrap();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        // A root `.xml` NOT named Configuration.xml, with no sibling `<stem>/` subtree.
        let xml = workspace.join("SomePlugin.xml");
        fs::write(&xml, "<Root/>").unwrap();
        let entry = ChangeEntry {
            canonical: xml.clone(),
            raw: xml,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        SharedState::apply_search_drift(
            &engine_arc,
            &workspace,
            &[entry],
            false,
            &crate::graph::GraphState::disabled(),
        );

        let guard = engine_arc.lock().unwrap();
        let engine = guard.as_ref().unwrap();
        let dirty = engine.context_dirty_paths("code").unwrap();
        assert_eq!(dirty.len(), 2, "a root .xml marks every indexed file: {dirty:?}");
        assert!(dirty.contains("A.bsl") && dirty.contains("B.bsl"));
    }
    /// An `.xml` drift whose owned module is marked context-dirty must NUDGE the graph to
    /// catch up — otherwise a search-only user (who never triggers a `graph` tool freshness
    /// check) leaves the marks unresolved forever. Asserting the graph left `Idle` with NO
    /// graph tool call. Disable the `graph.nudge_rebuild()` call → the graph stays `Idle` and
    /// this fails.
    #[test]
    fn search_sink_xml_drift_nudges_graph_to_catch_up() {
        use crate::change_hub::{ChangeEntry, ChangeKind};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        // An MDO descriptor with an owned module so the xml resolves to a real dirty mark.
        let owned = workspace.join("Catalogs/Товары/Ext/ObjectModule.bsl");
        fs::create_dir_all(owned.parent().unwrap()).unwrap();
        fs::write(&owned, "Процедура П()\nКонецПроцедуры").unwrap();
        let xml = workspace.join("Catalogs/Товары.xml");
        fs::write(&xml, "<MetaDataObject/>").unwrap();

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let graph = crate::graph::GraphState::for_workspace(workspace.clone());
        assert_eq!(graph.status(), crate::graph::GraphStatus::Idle, "graph starts idle");

        let entry = ChangeEntry {
            canonical: xml.clone(),
            raw: xml,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        SharedState::apply_search_drift(&engine_arc, &workspace, &[entry], false, &graph);

        assert_ne!(
            graph.status(),
            crate::graph::GraphStatus::Idle,
            "the xml drift nudged the graph to catch up without any graph tool call",
        );
    }
    /// A partial rescan walk (an error mid-walk) must NOT reconcile: `present` is missing healthy
    /// files, so deleting stored files against it would evict live data. Only a clean walk
    /// reconciles. Reverting the walk-error guard deletes the stored file on the errored walk.
    #[test]
    fn rescan_walk_error_skips_reconcile_and_keeps_stored_files() {
        use bsl_search::{Chunk, ChunkKind, Store};

        // This test toggles the process-global `FORCE_REWALK_WALK_ERROR` seam; serialize against the
        // boot-reconcile tests (which read it) so its forced error can't leak into their walk.
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file(
                    "Gone.bsl",
                    b"ha",
                    &[Chunk {
                        kind: ChunkKind::Procedure,
                        name: "П".to_owned(),
                        is_export: true,
                        annotations: vec![],
                        line_start: 0,
                        line_end: 1,
                        text: "Процедура П()\nКонецПроцедуры".to_owned(),
                    }],
                    None,
                )
                .unwrap();
        }
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.enable_workspace_watcher_mode();
        assert_eq!(engine.file_count().unwrap(), 1, "the stored file is present");
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        struct ResetWalkErr;
        impl Drop for ResetWalkErr {
            fn drop(&mut self) {
                FORCE_REWALK_WALK_ERROR.store(false, super::Ordering::SeqCst);
            }
        }

        // Errored walk: reconcile is skipped, so the stored (disk-absent) file SURVIVES.
        {
            FORCE_REWALK_WALK_ERROR.store(true, super::Ordering::SeqCst);
            let _reset = ResetWalkErr;
            SharedState::rewalk_workspace_bsl_dirty(&engine_arc, &workspace);
            assert_eq!(
                engine_arc.lock().unwrap().as_ref().unwrap().file_count().unwrap(),
                1,
                "a partial walk must not reconcile healthy files out of the store",
            );
        }

        // Clean walk: the stored-but-absent file is reconciled out.
        SharedState::rewalk_workspace_bsl_dirty(&engine_arc, &workspace);
        assert_eq!(
            engine_arc.lock().unwrap().as_ref().unwrap().file_count().unwrap(),
            0,
            "a clean walk reconciles the deleted file out",
        );
    }
    /// The overlay keys dirty paths relative to the ENGINE root (the nested config source root),
    /// while the resident is indexed under the OUTER workspace root. `prefetch_resident_overlay`
    /// must resolve each dirty rel to an absolute path against the engine root before asking the
    /// resident, so a nested config (every real workspace) actually gets a resident-fed reindex.
    /// Reverting the absolute-join (passing the rel verbatim) leaves the resident-fed count at 0.
    #[test]
    fn prefetch_resident_overlay_feeds_nested_config_from_resident() {
        use crate::diagnostics_state::{
            DiagnosticsState, DiagnosticsStatus, ResidentModuleSnapshotSource,
        };
        use std::time::{Duration, Instant};

        let dir = tempdir().unwrap();
        let outer = dir.path().to_path_buf();
        let cf = outer.join("src").join("cf");
        fs::create_dir_all(&cf).unwrap();
        fs::write(
            cf.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &cf,
            "Сервер",
            "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n",
        );
        let module = cf.join("CommonModules").join("Сервер").join("Ext").join("Module.bsl");

        // Overlay engine rooted at the NESTED config root, so `source_path != outer`.
        let mut engine = SearchEngine::fts_only(&outer.join("search.db")).unwrap();
        engine.set_workspace_root(cf.clone());
        engine.enable_workspace_watcher_mode();
        engine.prime_workspace_overlay().unwrap();

        // The file grows on disk so the reindex genuinely rebuilds it (fingerprint differs).
        fs::write(
            &module,
            "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n\
             Процедура Ещё() Экспорт КонецПроцедуры\n",
        )
        .unwrap();

        // The resident is built against the OUTER root AFTER the edit, so it holds the new bytes.
        let diagnostics = DiagnosticsState::for_workspace(outer.clone());
        diagnostics.ensure_loading();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !matches!(diagnostics.status(), DiagnosticsStatus::Ready { .. }) {
            assert!(Instant::now() < deadline, "the resident did not become ready");
            std::thread::sleep(Duration::from_millis(20));
        }

        let source: Arc<dyn bsl_search::ModuleSnapshotSource> =
            Arc::new(ResidentModuleSnapshotSource::new(diagnostics.clone()));
        engine.set_module_snapshot_source(source);
        assert!(
            engine.mark_workspace_path_dirty(&module).unwrap(),
            "the nested module marks dirty"
        );

        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));
        SharedState::prefetch_resident_overlay(&engine_arc);

        let fed = engine_arc
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .workspace_overlay_resident_fed_count()
            .unwrap();
        assert_eq!(
            fed, 1,
            "a nested-config dirty path must be served from the resident's shared parse",
        );
    }

    /// Search and diagnostics drain independent hub cursors, so a just-edited file leaves the
    /// resident BEHIND disk. `prefetch_resident_overlay` must catch the resident up on pending
    /// drift FIRST, so the snapshot text matches disk and the reindex is resident-fed rather than
    /// falling back to a disk read. Reverting the `catch_up` call leaves the resident stale, the
    /// byte-compare misses, and the resident-fed count stays 0.
    #[test]
    fn prefetch_resident_overlay_catches_up_stale_resident_before_reading() {
        use crate::change_hub::WorkspaceChangeHub;
        use crate::diagnostics_state::{
            DiagnosticsState, DiagnosticsStatus, ResidentModuleSnapshotSource,
        };
        use std::time::{Duration, Instant};

        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(
            root.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &root,
            "Сервер",
            "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n",
        );
        let module = root.join("CommonModules").join("Сервер").join("Ext").join("Module.bsl");

        let hub = WorkspaceChangeHub::start(vec![root.clone()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)), "the hub must arm");
        let mut observer = hub.subscribe();

        let mut engine = SearchEngine::fts_only(&root.join("search.db")).unwrap();
        engine.set_workspace_root(root.clone());
        engine.enable_workspace_watcher_mode();
        engine.prime_workspace_overlay().unwrap();

        // Resident built at v1, wired to the SAME hub, but it never polls drift on its own.
        let diagnostics =
            DiagnosticsState::for_workspace(root.clone()).with_change_hub(hub.clone());
        diagnostics.ensure_loading();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !matches!(diagnostics.status(), DiagnosticsStatus::Ready { .. }) {
            assert!(Instant::now() < deadline, "the resident did not become ready");
            std::thread::sleep(Duration::from_millis(20));
        }

        let source: Arc<dyn bsl_search::ModuleSnapshotSource> =
            Arc::new(ResidentModuleSnapshotSource::new(diagnostics.clone()));
        engine.set_module_snapshot_source(source);

        // Edit on disk (v2, longer): the resident's recorded revision is now stale.
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            &module,
            "&НаСервере\nФункция Ч() Экспорт Возврат 2; КонецФункции\n\
             Процедура Ещё() Экспорт КонецПроцедуры\n",
        )
        .unwrap();
        assert!(engine.mark_workspace_path_dirty(&module).unwrap());

        // Wait until the hub delivered the edit, so the diagnostics cursor drains it in `catch_up`.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut delivered = false;
        while Instant::now() < deadline {
            let batch = hub.drain(observer);
            observer = batch.cursor;
            if batch.entries.iter().any(|e| e.raw.to_string_lossy().ends_with("Module.bsl")) {
                delivered = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(delivered, "the hub delivered the edit");

        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));
        SharedState::prefetch_resident_overlay(&engine_arc);

        let fed = engine_arc
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .workspace_overlay_resident_fed_count()
            .unwrap();
        assert_eq!(
            fed, 1,
            "catch_up must reconcile the stale resident so the snapshot matches disk (fed reindex)",
        );
    }

    /// The per-query prefetch is capped: marking N + k paths dirty serves exactly N from the
    /// shared parse in one prefetch, and the remaining k stay dirty for the lazy disk path / a
    /// later prefetch. This bounds the query-path work S2 adds.
    #[test]
    fn prefetch_resident_overlay_caps_paths_per_query() {
        use bsl_search::{ModuleSnapshot, ModuleSnapshotSource, SnapshotFetch};

        struct DiskFakeSource;
        impl ModuleSnapshotSource for DiskFakeSource {
            fn text_and_parse(&self, path: &str) -> SnapshotFetch {
                match std::fs::read_to_string(path) {
                    Ok(text) => {
                        let root = parser::parse(&text).syntax_node();
                        SnapshotFetch::Fetched(ModuleSnapshot { text: text.into(), root })
                    }
                    Err(_) => SnapshotFetch::Unavailable,
                }
            }
        }

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let mut engine = SearchEngine::fts_only(&workspace.join("search.db")).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.enable_workspace_watcher_mode();
        engine.prime_workspace_overlay().unwrap();
        engine.set_module_snapshot_source(Arc::new(DiskFakeSource));

        let extra = 3usize;
        let total = MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY + extra;
        for i in 0..total {
            let rel = format!("Module{i}.bsl");
            fs::write(workspace.join(&rel), format!("Процедура П{i}()\nКонецПроцедуры\n")).unwrap();
            assert!(engine.mark_workspace_path_dirty(workspace.join(&rel)).unwrap());
        }

        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));
        SharedState::prefetch_resident_overlay(&engine_arc);

        let guard = engine_arc.lock().unwrap();
        let engine = guard.as_ref().unwrap();
        assert_eq!(
            engine.workspace_overlay_resident_fed_count().unwrap(),
            MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY,
            "exactly the per-query cap is served from the shared parse",
        );
        assert_eq!(
            engine.workspace_overlay_dirty_paths().unwrap().len(),
            extra,
            "paths beyond the cap stay dirty for the lazy disk path / a later prefetch",
        );
    }
    /// Unit proof of the shared boot reconcile that every Clean branch funnels through
    /// ([`SharedState::reconcile_boot_store_with_disk`]): a store row for a file DELETED while the
    /// daemon was down is reconciled out, while a present file is kept, and the helper reports the
    /// store PROVEN reconciled. The fused / standalone-deferred / FTS-cold Clean branches all call
    /// this exact helper after their index step, so proving it here proves the deletion is removed on
    /// each — without standing up a full graph build for the fused path. Store-level `file_count` is
    /// asserted so the removal is real, not overlay-hidden.
    #[test]
    fn boot_reconcile_removes_deleted_file_keeps_present() {
        // The boot reconcile reads the process-global `FORCE_REWALK_WALK_ERROR` seam; serialize
        // against the walk-error tests that toggle it so a concurrent set can't force a false error.
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        write_common_module_tree(
            &workspace,
            "Улетевший",
            "&НаСервере\nФункция ИсчезнувшийСимвол() Экспорт Возврат 1; КонецФункции\n",
        );
        write_common_module_tree(
            &workspace,
            "Постоянный",
            "&НаСервере\nФункция ЖивойСимвол() Экспорт Возврат 1; КонецФункции\n",
        );

        let db_path = dir.path().join("search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.index_directory_fts(&workspace).unwrap();
        assert_eq!(engine.file_count().unwrap(), 2, "both modules are indexed");

        // The Улетевший module vanishes while the daemon is down.
        fs::remove_dir_all(workspace.join("CommonModules").join("Улетевший")).unwrap();
        fs::remove_file(workspace.join("CommonModules").join("Улетевший.xml")).unwrap();

        let reconciled = SharedState::reconcile_boot_store_with_disk(&mut engine, &workspace);
        assert!(reconciled, "a clean walk proves the store reconciled");
        assert_eq!(
            engine.file_count().unwrap(),
            1,
            "the deleted file's rows are reconciled out of the store",
        );
        let files: Vec<String> = engine
            .store()
            .all_files_in_collection("code")
            .unwrap()
            .into_iter()
            .map(|(path, _hash)| path)
            .collect();
        assert!(
            files.iter().any(|p| p.contains("Постоянный")) && files.len() == 1,
            "only the present module survives: {files:?}",
        );
    }
    /// A walk error at boot cannot prove the store was reconciled, so a Clean branch must DOWNGRADE
    /// to a prime rather than assert a false clean. Force the reconcile walk to error and drive a
    /// cold FTS-only boot (otherwise Clean) through the real init path: it must select Prime.
    /// Reverting the downgrade (staying Clean on a failed walk) fails this.
    #[test]
    fn boot_walk_error_downgrades_clean_to_prime() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _embedding_url = EnvVarGuard::unset("EMBEDDING_URL");
        let _embedding_model = EnvVarGuard::unset("EMBEDDING_MODEL");

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        fs::write(
            workspace.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &workspace,
            "Сервер",
            "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n",
        );
        let watcher_ready = Arc::new(AtomicBool::new(false));

        struct ResetWalkErr;
        impl Drop for ResetWalkErr {
            fn drop(&mut self) {
                FORCE_REWALK_WALK_ERROR.store(false, super::Ordering::SeqCst);
            }
        }
        FORCE_REWALK_WALK_ERROR.store(true, super::Ordering::SeqCst);
        let _reset = ResetWalkErr;

        let init = SharedState::init_workspace_search_engine(
            &workspace,
            &watcher_ready,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("cold FTS-only init produces an engine");
        assert!(
            matches!(init.overlay_init, OverlayInit::Prime),
            "a boot whose reconcile walk errored must prime, not assert a false clean",
        );
    }
}
