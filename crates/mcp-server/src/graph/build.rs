//! Background graph build, cache adoption, and SQLite publication work.

use std::path::{Path, PathBuf};

use bsl_search::SearchEngine;

use crate::cache::graph_db_path;
use crate::graph_query::GraphDb;

#[cfg(test)]
use super::input::GRAPH_SOURCE_ROOT;
use super::scan::{classify_changes, workspace_fingerprint};
use super::state::{lock_recover, GraphState, Published, ReloadState};
use super::types::GraphStatus;

/// Modules whose edges are projected per batch when building the on-disk graph.
/// 500 keeps peak RSS comfortably bounded on a 25k-module config (measured ~2.9 GB)
/// while the resident method index resolves cross-batch calls.
pub(super) const GRAPH_BUILD_BATCH: usize = 500;

impl GraphState {
    pub(super) fn run_fused_cold_build(
        &self,
        engine: &mut SearchEngine,
        source_path: &Path,
        build_start_seq: i64,
    ) -> anyhow::Result<()> {
        let Some(workspace_root) = self.workspace_root.clone() else {
            anyhow::bail!("fused build on a non-workspace graph");
        };
        let generation =
            lock_recover(&self.inner).published.as_ref().map(|p| p.generation).unwrap_or(0) + 1;

        let source_path = source_path.to_path_buf();
        let mut sink = FusedChunkWriter::new(engine, source_path);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            build_and_publish_graph_file(&workspace_root, generation, self, Some(&mut sink))
        }));
        let built = match outcome {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return Err(e),
            Err(_) => anyhow::bail!("fused graph build panicked"),
        };
        if built.force_stale {
            tracing::warn!("fused graph build straddled a disk write; snapshot marked stale");
        }
        self.adopt_prebuilt(generation, built.fp_pre, built.files);
        self.ensure_hub_roots(&built.scan_roots, built.fp_pre.topology);
        // The fused sink just wrote every indexed document's context from THIS
        // build — nothing persisted predates it, so no whole-collection re-render.
        self.notify_published(build_start_seq, false);
        Ok(())
    }

    /// After a successful (re)build, re-point the daemon's change hub at the build
    /// snapshot's scan roots. A topology reload that added or dropped an extension
    /// root would otherwise leave the hub watching the old universe — events in a
    /// new extension would never be delivered, and every consumer would coast on
    /// its reconcile interval. A no-op when the roots did not change.
    pub(super) fn ensure_hub_roots(&self, scan_roots: &[std::path::PathBuf], built_topology: u64) {
        let (Some(hub), Some(root)) = (&self.change_hub, self.workspace_root.as_deref()) else {
            return;
        };
        // A slow build finishing after a newer topology reload must not roll the
        // shared hub back onto its older root set: re-derive the live topology
        // (config parse + discovery, no tree walk) and skip when this build's
        // snapshot is already superseded — the fresher build re-arms instead.
        let live = crate::graph::ProjectSnapshot::load(root);
        if super::scan::topology_u64(&live.configs) != built_topology {
            tracing::info!("skipping hub re-arm: the built snapshot's topology is superseded");
            return;
        }
        if !hub.ensure_roots(&crate::change_hub::watch_targets_for(root, scan_roots)) {
            tracing::warn!("graph rebuild could not re-arm the change hub onto new roots");
        }
    }

    /// Build (or rebuild) the database off-thread and publish it coherently.
    /// `is_reload` distinguishes the initial load (sets `Ready`, generation 1)
    /// from a drift-triggered reload (bumps the generation, keeps the old snapshot
    /// served on failure).
    pub(super) fn run_load(&self, is_reload: bool) {
        let Some(workspace_root) = self.workspace_root.clone() else {
            return;
        };
        // The generation this build will carry. Only one load runs at a time (the
        // initial load, then at most one reload via the claim guard), so peeking the
        // current generation without reserving it is race-free; a failed build leaves
        // it unpublished and the next attempt reuses the same number.
        let generation =
            lock_recover(&self.inner).published.as_ref().map(|p| p.generation).unwrap_or(0) + 1;

        // Capture the mark-seq at build start (before any disk read below): the post-publish
        // refresh clears only marks at or below it — drifts this build already reflects. A
        // drift stamped after this point carries a higher seq, is left for a later build, and
        // is guaranteed one by the pending-nudge machinery (every xml mark also nudges).
        let build_start_seq = self.current_mark_seq();

        // On the initial load, reuse a cached build from a previous process run if it
        // still matches the workspace — turning a multi-minute rebuild into a stat
        // walk plus an open. A reload is skipped here: it only fires once drift has
        // been detected, so the on-disk file is known stale and must be rebuilt.
        if !is_reload && self.try_publish_cached(&workspace_root, build_start_seq) {
            return;
        }

        // Cached but drifted: serve the stale snapshot immediately and catch up through
        // the reload lifecycle (its failure path keeps the snapshot and flags
        // `reload="failed"`, unlike this initial load's `Failed`). The catch-up build
        // recomputes its own generation from the just-published revision.
        if !is_reload && self.try_publish_stale_and_catch_up(&workspace_root) {
            return;
        }

        // Everything below WRITES the shared graph database. A daemon superseded by a newer
        // generation (see [`crate::workspace_lease`]) may still PUBLISH what is already on
        // disk — both reads above — but never builds: the owner is maintaining that same file,
        // and a second builder would only race its rename. Placed after the stale publish on
        // purpose, so a superseded daemon still serves the stale snapshot (with the reason on
        // its `reload` slot) instead of answering nothing at all.
        if !self.may_build() {
            self.record_load_failure(
                is_reload,
                "another daemon generation owns this workspace's derived caches; \
                 this one serves without rebuilding the graph"
                    .to_owned(),
            );
            return;
        }

        // On reload, try the body-only fast path first: if only `.bsl` bodies changed
        // (signatures intact, nothing added/removed, no `.xml` drift) reproject just
        // those modules instead of the whole config. On any ineligibility or failure
        // it returns false and we fall through to a full rebuild.
        if is_reload && self.try_incremental_reload(&workspace_root, generation, build_start_seq) {
            return;
        }

        tracing::info!(?workspace_root, is_reload, generation, "graph database build started");
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            build_and_publish_graph_file(&workspace_root, generation, self, None)
        }));

        match outcome {
            Ok(Ok(built)) => {
                if built.force_stale {
                    tracing::warn!(
                        is_reload,
                        "graph build straddled a disk write; marking snapshot stale to force reload"
                    );
                }
                // Drop the stale scan cache *before* publishing so a concurrent
                // freshness check re-scans against the new snapshot rather than a
                // pre-reload cached fingerprint.
                *lock_recover(&self.scan) = None;
                let topology_changed;
                {
                    let mut inner = lock_recover(&self.inner);
                    // Only a WITNESSED transition (a previously published topology
                    // differing from this build's) requests the whole-collection
                    // re-render. `None` deliberately reads as unchanged: a cold
                    // build must keep the boot invariant that an early publish
                    // clears no pre-existing context marks — the offline-edit
                    // warm start is covered by the stale-adopt -> catch-up chain,
                    // which publishes the old topology first and transitions here.
                    topology_changed = inner
                        .published
                        .as_ref()
                        .is_some_and(|p| p.fingerprint.topology != built.fp_pre.topology);
                    inner.published = Some(Published {
                        generation,
                        fingerprint: built.fp_pre,
                        stale: false,
                        reload: ReloadState::Idle,
                    });
                    inner.status = GraphStatus::Ready { files: built.files };
                }
                self.ensure_hub_roots(&built.scan_roots, built.fp_pre.topology);
                self.notify_published(build_start_seq, topology_changed);
                tracing::info!(
                    files = built.files,
                    generation,
                    is_reload,
                    "graph database build complete"
                );
            }
            Ok(Err(e)) => {
                let msg = e.to_string();
                tracing::warn!("graph database build failed: {msg}");
                self.record_load_failure(is_reload, msg);
            }
            Err(_) => {
                tracing::error!("graph database build panicked");
                self.record_load_failure(is_reload, "builder panicked".to_owned());
            }
        }
    }

    /// The body-only fast path for a reload. Eligible only when every drifted file is
    /// a `.bsl` whose signature hash still matches its persisted value, with nothing
    /// added/removed and no `.xml` drift — then no caller's resolution can have moved,
    /// so reprojecting just those modules yields a database byte-identical to a full
    /// rebuild. Patches a copy of the published file and atomically renames it in,
    /// then publishes `generation`. Returns `true` on success; `false` (the common
    /// case for a structural change) leaves nothing published and falls back to a full
    /// rebuild.
    fn try_incremental_reload(
        &self,
        workspace_root: &Path,
        generation: u64,
        build_start_seq: i64,
    ) -> bool {
        let db_path = graph_db_path(workspace_root);
        let stored_fp = read_stored_fingerprints(&db_path);
        if stored_fp.is_empty() {
            return false; // no per-file record (older build) → full rebuild
        }
        // ONE project snapshot serves the eligibility diff, the topology gate, the
        // patch and the straddle bracket, so a config edit mid-operation cannot mix
        // two topologies.
        let project = crate::graph::ProjectSnapshot::load(workspace_root);
        // A topology change re-shapes visibility for ANY module even when only
        // `.bsl` bodies drifted on disk — never body-patch across it.
        match GraphDb::open(&db_path).and_then(|g| g.freshness_token()) {
            Ok((_, stored_token, _))
                if stored_token.topology == super::scan::topology_u64(&project.configs) => {}
            _ => return false,
        }
        let diff =
            classify_changes(&stored_fp, &super::scan::scan_stats_over_roots(&project.scan_roots));

        // Body-only shape: at least one `.bsl` modified, nothing added/removed, no
        // metadata drift (an `.xml` change can flip visibility for any module).
        if diff.is_empty()
            || !diff.added.is_empty()
            || !diff.removed.is_empty()
            || diff.touches_metadata()
        {
            return false;
        }
        let modified_paths: Vec<PathBuf> = diff.modified.iter().map(PathBuf::from).collect();

        // Recompute each modified module's profile and partition into body-only
        // (signature unchanged) and signature-changed.
        let profiles =
            match crate::graph_db::recompute_module_profiles(workspace_root, &modified_paths) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("incremental reload: profile recompute failed: {e}");
                    return false;
                }
            };
        let stored_sig = read_stored_sig_hashes(&db_path);
        let mut sig_changed: Vec<(String, &crate::graph_db::ModuleProfile)> = Vec::new();
        for p in &modified_paths {
            let key = p.to_string_lossy().into_owned();
            let Some(profile) = profiles.get(&key) else {
                return false; // could not profile the module → full rebuild
            };
            match stored_sig.get(&key) {
                Some(Some(stored)) if *stored == profile.sig_hash => {} // body-only
                Some(Some(_)) => sig_changed.push((key, profile)),      // signature changed
                _ => return false, // no stored signature (pre-signature build) → full rebuild
            }
        }

        // A signature change is handled by the caller-delta path: reproject the changed
        // module PLUS its resolved callers, when caller-delta-safe (no new resolvable
        // name). Otherwise fall back to a full rebuild.
        let mut changed_paths = modified_paths.clone();
        if !sig_changed.is_empty() {
            let refs: Vec<(&str, &crate::graph_db::ModuleProfile)> =
                sig_changed.iter().map(|(f, p)| (f.as_str(), *p)).collect();
            match crate::graph_db::caller_delta_plan(&db_path, &refs) {
                Ok(Some(callers)) => {
                    for c in callers {
                        if !changed_paths.contains(&c) {
                            changed_paths.push(c);
                        }
                    }
                }
                Ok(None) => {
                    tracing::info!(
                        "incremental reload: signature change not caller-delta-safe; full rebuild"
                    );
                    return false;
                }
                Err(e) => {
                    tracing::warn!("incremental reload: caller-delta plan failed: {e}");
                    return false;
                }
            }
            // If the caller fan-out approaches the whole config, a full rebuild (no
            // 2.6 GB copy) is cheaper than reprojecting most modules. Compare against
            // the `.bsl` module count only — `changed_paths` are modules, while
            // `stored_fp` also counts `.xml`, which would skew the threshold.
            let module_total = stored_fp.keys().filter(|p| p.ends_with(".bsl")).count();
            if changed_paths.len() * 2 > module_total {
                tracing::info!(
                    changed = changed_paths.len(),
                    modules = module_total,
                    "incremental reload: caller-delta too broad; full rebuild"
                );
                return false;
            }
        }

        // Bracket the patch with fingerprint scans, mirroring the full build's
        // straddle detection: a write landing mid-patch marks the snapshot stale.
        let fp_pre = super::scan::workspace_fingerprint_over(&project);
        let tmp_path = db_path.with_extension(format!("db.building.{}", std::process::id()));
        let built_at = chrono::Utc::now().to_rfc3339();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let summary = crate::graph_db::update_graph_database_bodies(
                &project,
                &db_path,
                &tmp_path,
                &changed_paths,
                GRAPH_BUILD_BATCH,
                &crate::graph_db::GraphMeta {
                    revision: generation,
                    fingerprint: fp_pre,
                    files: 0,
                    built_at,
                },
            )?;
            let fp_post = super::scan::workspace_fingerprint_over(
                &crate::graph::ProjectSnapshot::load(workspace_root),
            );
            let force_stale = fp_pre != fp_post;
            {
                let conn = rusqlite::Connection::open(&tmp_path)?;
                conn.execute(
                    "INSERT OR REPLACE INTO meta (key, value) VALUES ('force_stale', ?1)",
                    rusqlite::params![if force_stale { "1" } else { "0" }],
                )?;
            }
            self.publish_or_discard(&tmp_path, &db_path)?;
            anyhow::Ok((summary.modules, fp_pre, force_stale))
        }));

        match outcome {
            Ok(Ok((files, fp, force_stale))) => {
                if force_stale {
                    tracing::warn!(
                        "incremental reload straddled a disk write; marking snapshot stale"
                    );
                }
                *lock_recover(&self.scan) = None;
                {
                    let mut inner = lock_recover(&self.inner);
                    inner.published = Some(Published {
                        generation,
                        fingerprint: fp,
                        stale: false,
                        reload: ReloadState::Idle,
                    });
                    inner.status = GraphStatus::Ready { files };
                }
                // The body-only gate proved the stored topology unchanged.
                self.notify_published(build_start_seq, false);
                tracing::info!(
                    files,
                    generation,
                    modified = changed_paths.len(),
                    "graph incremental reload complete"
                );
                true
            }
            Ok(Err(e)) => {
                tracing::warn!("incremental reload failed, falling back to full rebuild: {e}");
                let _ = std::fs::remove_file(&tmp_path);
                false
            }
            Err(_) => {
                tracing::error!("incremental reload panicked, falling back to full rebuild");
                let _ = std::fs::remove_file(&tmp_path);
                false
            }
        }
    }

    /// Publish an existing on-disk build instead of rebuilding, when it is still a
    /// valid, current, non-straddled match for the workspace. Returns `true` (and
    /// transitions to `Ready`) when the cache was reused; `false` to fall through to
    /// a full build. The fingerprint scan it runs is the same one the build would do.
    pub(super) fn try_publish_cached(&self, workspace_root: &Path, build_start_seq: i64) -> bool {
        let path = graph_db_path(workspace_root);
        let Ok(graph) = GraphDb::open(&path) else {
            return false; // missing, truncated, or stale-schema → rebuild
        };
        let Ok((revision, fingerprint, force_stale)) = graph.freshness_token() else {
            return false;
        };
        let fp_now = workspace_fingerprint(workspace_root);
        // Reuse only an exact, clean match: a fingerprint mismatch means the
        // workspace moved since the build, and `force_stale` means the build
        // straddled a write and was never a coherent snapshot.
        if force_stale || fingerprint != fp_now {
            return false;
        }
        let files = graph.files().unwrap_or(0);

        *lock_recover(&self.scan) = None;
        let mut inner = lock_recover(&self.inner);
        inner.published = Some(Published {
            generation: revision,
            fingerprint,
            stale: false,
            reload: ReloadState::Idle,
        });
        inner.status = GraphStatus::Ready { files };
        drop(inner);
        // Exact fingerprint match (files AND topology): the persisted search
        // contexts were rendered against this same workspace state.
        self.notify_published(build_start_seq, false);
        tracing::info!(files, revision, "reused cached graph database (workspace unchanged)");
        true
    }

    /// Boot variant for a cached graph that no longer matches disk: publish it anyway —
    /// stale answers now beat "still indexing" for the minutes a full rebuild takes —
    /// and pre-claim the reload slot in the SAME lock hold, then let the normal reload
    /// lifecycle catch up (incrementally when eligible, else a full rebuild). The
    /// atomic Ready+Running publish keeps every existing guard honest:
    /// `freshness()`/`claim_reload_slot` stay single-flight against the pre-claimed
    /// slot, and `consume_leftover_marks` sees `drift_pending` and defers the leftover
    /// consume to the catch-up publish — unlike a fingerprint-clean cached publish,
    /// THIS snapshot does not reflect the leftover marks' causes. A snapshot from a
    /// straddled build (`force_stale`) was never coherent and is not served. No
    /// `notify_published`: the publish hook must only run against a build that
    /// reflects current disk.
    pub(super) fn try_publish_stale_and_catch_up(&self, workspace_root: &Path) -> bool {
        let path = graph_db_path(workspace_root);
        let Ok(graph) = GraphDb::open(&path) else {
            return false; // missing, truncated, or stale-schema → full rebuild
        };
        let Ok((revision, fingerprint, force_stale)) = graph.freshness_token() else {
            return false;
        };
        if force_stale {
            return false;
        }
        // Stale on FILES is what this path exists to serve — stale on TOPOLOGY is not. A build
        // made under a different extension topology resolves names differently, so publishing it
        // would answer questions about a project shape this workspace no longer has, and every
        // later reader would compare against the foreign topology adopted here and find it
        // consistent. The clean-match path above rejects it implicitly (its fingerprint covers
        // the topology); here it has to be said.
        //
        // Not publishing it costs the transition WITNESS, though: the whole-collection context
        // re-render is normally requested by a publish that observes its predecessor's topology
        // differing from its own, and refusing to publish leaves nothing to differ from. The
        // difference is visible right here — cached file versus live configuration — so the
        // request is raised directly and the rebuild's publish carries it.
        if !super::scan::graph_file_matches_live_topology(workspace_root, &graph) {
            tracing::info!(
                "cached graph database was built for another extension topology; \
                 rebuilding instead of serving it stale, and re-rendering search contexts"
            );
            self.pending_topology_refresh.store(true, std::sync::atomic::Ordering::SeqCst);
            return false;
        }
        let files = graph.files().unwrap_or(0);
        drop(graph);

        {
            let mut inner = lock_recover(&self.inner);
            inner.published = Some(Published {
                generation: revision,
                fingerprint,
                stale: true,
                // Pre-claimed: the catch-up spawned below owns the one reload slot.
                reload: ReloadState::Running,
            });
            inner.status = GraphStatus::Ready { files };
        }
        tracing::info!(
            files,
            revision,
            "published stale cached graph database; catch-up reload starting"
        );
        self.spawn_reload();
        true
    }

    /// Move a finished build into the shared path — unless this daemon lost the workspace
    /// while it was building. See [`publish_or_discard`].
    fn publish_or_discard(&self, tmp_path: &Path, out_path: &Path) -> anyhow::Result<()> {
        publish_or_discard(self, tmp_path, out_path)
    }

    /// A failed initial load surfaces as `Failed`; a failed reload keeps the
    /// previous snapshot but flags `reload="failed"` so the agent sees it. A
    /// later drift check retries the reload (the throttle bounds the retry rate).
    ///
    /// A load that stopped because the workspace is no longer ours is recorded as a failure
    /// like any other, but flagged so it can be retried when the workspace comes back — see
    /// [`GraphState::withheld_build`]. Both shapes count: the load that never started, and the
    /// one that built for minutes and then lost the workspace at its publish. A genuinely
    /// failed build (we still own the workspace) stays terminal as before.
    fn record_load_failure(&self, is_reload: bool, msg: String) {
        if !self.may_build() {
            self.withheld_build.store(true, std::sync::atomic::Ordering::SeqCst);
        }
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

/// Build the graph into the canonical path with the full publication bracket:
/// fingerprint the workspace before and after (so a build that straddled a disk write
/// is marked `force_stale`), stamp that marker plus the file count into the file's own
/// meta, then atomically rename the temp file into place — a reader sees the previous
/// database until the swap, never a half-written one. Shared by the lazy loader
/// ([`GraphState::run_load`]) and the fused cold build; when `chunk_sink` is present,
/// the search index's chunks are streamed from the same parse pass. Returns
/// a [`PublishedBuild`].
fn build_and_publish_graph_file(
    workspace_root: &Path,
    generation: u64,
    graph: &GraphState,
    chunk_sink: Option<&mut dyn ide::FusedChunkSink>,
) -> anyhow::Result<PublishedBuild> {
    // ONE project snapshot serves the pre-scan, the build and the post-scan.
    let project = crate::graph::ProjectSnapshot::load(workspace_root);
    let fp_pre = super::scan::workspace_fingerprint_over(&project);
    let out_path = graph_db_path(workspace_root);
    // Pid-suffixed temp: two daemons over the same workspace (an old topology
    // generation draining while a new one starts) must not interleave writes into
    // one temp file — each builds its own and the atomic rename decides.
    let tmp_path = out_path.with_extension(format!("db.building.{}", std::process::id()));
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let built_at = chrono::Utc::now().to_rfc3339();
    let meta = crate::graph_db::GraphMeta {
        revision: generation,
        fingerprint: fp_pre,
        files: 0,
        built_at,
    };
    let summary = match chunk_sink {
        Some(sink) => crate::graph_db::build_graph_database_fused(
            &project,
            &tmp_path,
            GRAPH_BUILD_BATCH,
            &meta,
            sink,
        )?,
        None => {
            crate::graph_db::build_graph_database(&project, &tmp_path, GRAPH_BUILD_BATCH, &meta)?
        }
    };
    // The post-scan derives a FRESH project snapshot: the straddle check must see
    // the world as it is now, or a topology/root change landing mid-build would
    // compare the frozen snapshot against itself and publish clean.
    let fp_post = super::scan::workspace_fingerprint_over(&crate::graph::ProjectSnapshot::load(
        workspace_root,
    ));
    let force_stale = fp_pre != fp_post;
    {
        let conn = rusqlite::Connection::open(&tmp_path)?;
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('force_stale', ?1)",
            rusqlite::params![if force_stale { "1" } else { "0" }],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('files', ?1)",
            rusqlite::params![summary.modules.to_string()],
        )?;
    }
    publish_or_discard(graph, &tmp_path, &out_path)?;
    Ok(PublishedBuild {
        files: summary.modules,
        fp_pre,
        force_stale,
        scan_roots: project.scan_roots,
    })
}

/// Rename a finished build into the shared path, or throw it away.
///
/// A build takes minutes, and a newer daemon generation can claim the workspace's derived
/// caches at any point during one (see [`crate::workspace_lease`]). The rename runs with
/// ownership HELD rather than merely checked: a claim landing between a check and the rename
/// would let this build clobber what the new owner just published, and "we owned it a moment
/// ago" is exactly the guarantee a minutes-long build cannot rely on. A rename that cannot go
/// ahead discards the build, temp file and all, so nothing is left behind.
///
/// Both reasons it can fail — the workspace was taken over, or ownership could not be
/// established because a peer held the lock — re-arm the build (see
/// [`GraphState::withheld_build`]). Neither says the build itself was bad, so neither should
/// leave the graph terminally failed: the second one in particular happens while this daemon is
/// still the rightful owner. The gates that keep a superseded daemon from STARTING a build are
/// upstream; this is the one that closes the window a long build opens.
fn publish_or_discard(graph: &GraphState, tmp_path: &Path, out_path: &Path) -> anyhow::Result<()> {
    match graph.lease.with_ownership(|| std::fs::rename(tmp_path, out_path)) {
        Some(renamed) => Ok(renamed?),
        None => {
            let _ = std::fs::remove_file(tmp_path);
            graph.withheld_build.store(true, std::sync::atomic::Ordering::SeqCst);
            anyhow::bail!(
                "this daemon could not establish ownership of the workspace's derived caches \
                 when the graph build finished; the build was discarded instead of published"
            )
        }
    }
}

/// The outcome of one full build+publish pass: what was published, the identity it
/// was published under, and the scan roots of the snapshot that built it (for the
/// post-publish hub re-arm).
struct PublishedBuild {
    files: usize,
    fp_pre: crate::graph_db::GraphFp,
    force_stale: bool,
    scan_roots: Vec<PathBuf>,
}

/// Translates the graph pass's [`ide::ChunkRow`] stream into the search store for the
/// fused cold build. Filters to files under the search source root, writes each file's
/// chunks + FTS + graph context with NO embedding (filled later by
/// [`SearchEngine::embed_pending_chunks_standalone`]), and records the blake3 of the file's bytes
/// as the skip hash — matching the standalone indexer so a later run reuses unchanged
/// files.
struct FusedChunkWriter<'e> {
    engine: &'e mut SearchEngine,
    /// Canonical, `/`-normalised search source root: derives the stored relative path
    /// and excludes files outside it (e.g. extension modules the local index omits).
    source_prefix: String,
}

impl<'e> FusedChunkWriter<'e> {
    fn new(engine: &'e mut SearchEngine, source_path: PathBuf) -> Self {
        let source_prefix =
            source_path.canonicalize().unwrap_or(source_path).to_string_lossy().replace('\\', "/");
        Self { engine, source_prefix }
    }
}

impl ide::FusedChunkSink for FusedChunkWriter<'_> {
    fn emit_chunks(
        &mut self,
        rows: &[ide::ChunkRow],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // The producer emits a module's chunks consecutively, so group consecutive
        // same-path rows into one per-file write (each module appears once per batch).
        let mut groups: Vec<(String, Vec<bsl_search::Chunk>, Vec<Option<String>>)> = Vec::new();
        for row in rows {
            if groups.last().map(|(p, _, _)| p.as_str()) != Some(row.path.as_str()) {
                groups.push((row.path.clone(), Vec::new(), Vec::new()));
            }
            let (_, chunks, ctxs) = groups.last_mut().expect("just pushed");
            chunks.push(bsl_search::Chunk {
                kind: row.kind,
                name: row.symbol.clone(),
                is_export: row.is_export,
                annotations: row.annotations.clone(),
                line_start: row.line_start,
                line_end: row.line_end,
                text: row.text.clone(),
            });
            ctxs.push(row.graph_context.clone());
        }

        let prefix = self.source_prefix.trim_end_matches('/');
        for (abs, chunks, ctxs) in &groups {
            // Require a path-separator boundary after the prefix so a sibling whose name
            // merely starts with the source dir's string (e.g. `…/cf` vs `…/cf_ext`) is
            // not mistaken for a file inside the source root.
            let Some(rel) = abs
                .strip_prefix(prefix)
                .filter(|rest| rest.starts_with('/'))
                .map(|s| s.trim_start_matches('/'))
            else {
                continue; // outside the search source root (e.g. an extension module)
            };
            if rel.is_empty() {
                continue;
            }
            let bytes = match std::fs::read(abs) {
                Ok(b) => b,
                Err(_) => continue, // unreadable now → leave for the standalone indexer
            };
            let hash = bsl_search::content_blake3(&bytes);
            // Skip a file whose content is byte-identical to what is already stored: its
            // chunks and (paid-for) embeddings are kept. Re-ingesting would DELETE+reinsert
            // them with a NULL embedding and force a needless re-embed of the whole corpus on
            // every graph rebuild — the exact cost this avoids. The graph itself still rebuilds
            // fully (its own concern); only the embeddings stay incremental.
            //
            // Trade-off: the stored graph context records a method's *outbound* edges (whom it
            // calls / which metadata it reads). If a CALLEE is renamed or removed, an unchanged
            // caller's stored context can name the old target until that caller is itself
            // touched (or a `force_stale` rebuild re-ingests it). We accept this small
            // cross-file staleness in the embedding's context rather than re-embed every caller
            // of any changed symbol — embeddings are an approximation and this self-heals on the
            // next edit of the affected file.
            if self.engine.store().file_hash(rel).ok().flatten().as_deref() == Some(hash.as_slice())
            {
                continue;
            }
            self.engine.ingest_fused_file(rel, &hash, chunks, ctxs)?;
        }
        Ok(())
    }
}

/// Read the stored per-file fingerprints from a built graph's `files` table. Any
/// open/query failure (missing file, older schema without the table) yields an empty
/// map, which classifies every current file as `added` → conservative full rebuild.
pub(crate) fn read_stored_fingerprints(db_path: &Path) -> std::collections::HashMap<String, u64> {
    let mut map = std::collections::HashMap::new();
    // Read-only open: never create the file as a side effect. A missing/older DB
    // errors here and yields an empty map → every current file classified `added`.
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return map;
    };
    let Ok(mut stmt) = conn.prepare("SELECT path, fingerprint FROM files") else {
        return map;
    };
    let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))
    else {
        return map;
    };
    for row in rows.flatten() {
        map.insert(row.0, row.1);
    }
    map
}

/// Read the stored per-file signature hashes (`None` for `.xml`, and for `.bsl` built
/// before signature persistence). Read-only open; an open/query failure yields an
/// empty map → the body-only fast path treats every module as ineligible (full
/// rebuild). Separate from [`read_stored_fingerprints`] so the eligibility check can
/// distinguish "no stored signature" (NULL) from "signature present but differs".
pub(crate) fn read_stored_sig_hashes(
    db_path: &Path,
) -> std::collections::HashMap<String, Option<u64>> {
    let mut map = std::collections::HashMap::new();
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return map;
    };
    let Ok(mut stmt) = conn.prepare("SELECT path, sig_hash FROM files") else {
        return map;
    };
    let Ok(rows) = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?.map(|v| v as u64)))
    }) else {
        return map;
    };
    for row in rows.flatten() {
        map.insert(row.0, row.1);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::super::input::{enumerate_bsl_files, load_workspace_db, scan_roots};
    use super::super::scan::{scan_file_stats, scan_stats_over_roots, FileStat, WorkspaceDiff};
    use super::super::snapshot::fold_fingerprint_entries;
    use super::super::test_support::{
        meta_string, sample_workspace, seed_cache, wait_ready, write, write_common_module,
        write_extension_config, write_extension_workspace,
    };
    use super::*;
    use crate::graph_db::{build_graph_database, update_graph_database_bodies};
    use ide::Analysis;
    use rusqlite::Connection;
    use std::collections::HashSet;
    use std::fs;
    use std::time::{Duration, UNIX_EPOCH};
    use walkdir::WalkDir;

    /// End-to-end through `GraphState`: a first use builds the SQLite graph off
    /// the workspace and serves overview/node/neighbors from the opened handle.
    #[test]
    fn loads_workspace_and_serves_graph() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let graph = GraphState::for_workspace(root.to_path_buf());
        graph.ensure_loading();
        wait_ready(&graph);
        let snap = graph.snapshot().expect("ready graph snapshots an opened handle");
        let gdb = &snap.graph;

        let overview = gdb.overview(10).expect("overview");
        assert_eq!(overview.edges, 1, "Клиент.Главная → Сервер.Считать is one resolved edge");
        assert_eq!(overview.client_to_server_edges, 1);

        let node = gdb
            .node("method/common/Сервер/Считать", ide::GraphDetail::Names)
            .expect("query")
            .expect("durable id resolves from the on-disk graph");
        assert_eq!(node.node.name, "Считать");
        assert_eq!(node.node.dispatch, vec!["server"]);
        assert_eq!(node.node.qualified, None, "code nodes do not serve qualified");

        // Callers traversal reaches the client method via the resolved edge.
        let callers = gdb
            .neighbors(&ide::NeighborsParams {
                id: "method/common/Сервер/Считать",
                dir: ide::Direction::In,
                depth: 1,
                max_nodes: 50,
                detail: ide::GraphDetail::Names,
                provenance_filter: Vec::new(),
                edge_kind_filter: Vec::new(),
            })
            .expect("query")
            .expect("neighbors resolve");
        assert!(callers.nodes.iter().any(|n| n.id == "method/common/Клиент/Главная"));
        // The root endpoint is elided from served edges (absent = root), matching
        // the in-memory serve path.
        let edge = callers.edges.iter().find(|e| e.to.is_none()).expect("edge into the root");
        assert_eq!(edge.from.as_deref(), Some("method/common/Клиент/Главная"));
    }

    /// A cached build that still matches the workspace is republished as-is — no
    /// rebuild — so its `revision` and `built_at` survive the load.
    #[test]
    fn reuses_a_matching_cached_build() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        seed_cache(root, workspace_fingerprint(root));

        let graph = GraphState::for_workspace(root.to_path_buf());
        graph.ensure_loading();
        wait_ready(&graph);

        // Reused: the served revision is the cache's (7); a rebuild would reset it to 1.
        let snap = graph.snapshot().expect("ready graph snapshots");
        assert_eq!(snap.generation, 7, "served the cached revision, not a fresh build");
        // The file was not rewritten — its build timestamp is untouched.
        assert_eq!(meta_string(&graph_db_path(root), "built_at"), "cached-build-sentinel");
    }

    /// A cached build whose fingerprint no longer matches the workspace (it moved
    /// since the build) is served immediately as a stale snapshot — answers now beat
    /// "still indexing" — while the pre-claimed catch-up reload replaces it.
    #[test]
    fn serves_stale_cache_and_catches_up() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        seed_cache(root, {
            let mut fp = workspace_fingerprint(root);
            fp.files = fp.files.wrapping_add(1);
            fp
        });

        let graph = GraphState::for_workspace(root.to_path_buf());
        graph.ensure_loading();
        wait_ready(&graph);

        // Ready right away: either the stale cache (revision 7) is being served with
        // the catch-up still running, or — on a fast machine over this tiny fixture —
        // the catch-up already published revision 8. Never a from-scratch generation 1.
        let first = graph.snapshot().expect("ready graph snapshots").generation;
        assert!(
            first == 7 || first == 8,
            "the stale cache is served (or already caught up), never rebuilt at 1: {first}"
        );

        // The catch-up publishes past the cached revision and rewrites the file.
        for _ in 0..500 {
            if graph.snapshot().map(|s| s.generation) == Some(8) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let snap = graph.snapshot().expect("ready graph snapshots");
        assert_eq!(snap.generation, 8, "the catch-up reload published past the cache");
        assert_ne!(meta_string(&graph_db_path(root), "built_at"), "cached-build-sentinel");
    }

    /// The event-maintained map's fold must be bit-identical to the walk's fold, or
    /// freshness would report phantom drift after every hub-patched entry.
    #[test]
    fn fp_map_fold_matches_walk_fold() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let walk = workspace_fingerprint(root);
        let project = crate::graph::ProjectSnapshot::load(root);
        let mut entries: Vec<(String, u128, u64)> = scan_stats_over_roots(&project.scan_roots)
            .into_iter()
            .map(|s| (s.path, s.mtime, s.len))
            .collect();
        entries.sort();
        let map: std::collections::BTreeMap<String, (u128, u64)> =
            entries.into_iter().map(|(p, m, l)| (p, (m, l))).collect();
        let via_map: Vec<(String, u128, u64)> =
            map.iter().map(|(p, (m, l))| (p.clone(), *m, *l)).collect();
        assert_eq!(fold_fingerprint_entries(&via_map), walk.files, "map fold == walk fold");
    }

    /// A cached build flagged `force_stale` (it straddled a disk write and was never
    /// a coherent snapshot) is never reused even if its fingerprint matches.
    #[test]
    fn rebuilds_when_cached_build_is_force_stale() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        let fp = workspace_fingerprint(root);
        seed_cache(root, fp);
        Connection::open(graph_db_path(root))
            .unwrap()
            .execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('force_stale', '1')", [])
            .unwrap();

        let graph = GraphState::for_workspace(root.to_path_buf());
        graph.ensure_loading();
        wait_ready(&graph);

        let snap = graph.snapshot().expect("ready graph snapshots");
        assert_eq!(snap.generation, 1, "force_stale cache rebuilt at generation 1");
        assert_ne!(meta_string(&graph_db_path(root), "built_at"), "cached-build-sentinel");
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
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
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
        // `overview.modules` is the true distinct-module population (every module that owns a
        // method, plus any persisted module-body node), so it is >= the module rows actually
        // stored — module nodes are synthesized on demand, not generally persisted.
        let stored_module_rows = count("SELECT COUNT(*) FROM nodes WHERE kind='module'");
        assert!(
            overview.modules >= stored_module_rows,
            "reported modules {} >= stored module rows {stored_module_rows}",
            overview.modules,
        );
        assert!(overview.modules > 0, "the sample workspace has code modules");
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

    /// `edge_kinds` narrows a neighbours query to the requested edge kinds: a method with
    /// both a `call` and a `query_ref` out-edge returns both unfiltered, only the query_ref
    /// edge under `edge_kinds=["query_ref"]`.
    #[test]
    fn neighbors_edge_kinds_filter_isolates_one_kind() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog(root, "Номенклатура", 1);
        write_common_module(
            root,
            "Бета",
            true,
            "&НаСервере\nПроцедура ШагБ() Экспорт КонецПроцедуры",
        );
        write_common_module(
            root,
            "Альфа",
            true,
            "&НаСервере\nПроцедура ШагА() Экспорт\nБета.ШагБ();\n\
             Запрос = \"ВЫБРАТЬ Код ИЗ Справочник.Номенклатура\";\nКонецПроцедуры",
        );

        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files: 0,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let gdb = GraphDb::open(&out).expect("graph database opens");

        let mk = |kinds: Vec<String>| ide::NeighborsParams {
            id: "method/common/Альфа/ШагА",
            dir: ide::Direction::Out,
            depth: 1,
            max_nodes: 50,
            detail: ide::GraphDetail::Names,
            provenance_filter: Vec::new(),
            edge_kind_filter: kinds,
        };

        // Unfiltered: both the call to Бета.ШагБ and the query_ref to Номенклатура.
        let all = gdb.neighbors(&mk(Vec::new())).unwrap().unwrap();
        let all_kinds: Vec<&str> = all.edges.iter().map(|e| e.kind).collect();
        assert!(all_kinds.contains(&"call"), "kinds: {all_kinds:?}");
        assert!(all_kinds.contains(&"query_ref"), "kinds: {all_kinds:?}");
        // Grouped distribution mirrors the edges; nothing was capped here.
        assert_eq!(all.by_kind.get("call"), Some(&1), "by_kind: {:?}", all.by_kind);
        assert_eq!(all.by_kind.get("query_ref"), Some(&1), "by_kind: {:?}", all.by_kind);
        assert_eq!(all.by_provenance.values().sum::<usize>(), all.edges.len());
        assert!(!all.connectors_dropped, "no nodes capped, so no connectors dropped");

        // Out-direction traversal reports its callees and no callers.
        assert_eq!(all.out_total, Some(2), "two callees (Бета.ШагБ + Номенклатура query)");
        assert_eq!(all.in_total, None, "dir=out reports no caller count");

        // dir=both surfaces directional fan-out: 2 callees, 0 callers of ШагА.
        let both = gdb
            .neighbors(&ide::NeighborsParams {
                id: "method/common/Альфа/ШагА",
                dir: ide::Direction::Both,
                depth: 1,
                max_nodes: 50,
                detail: ide::GraphDetail::Names,
                provenance_filter: Vec::new(),
                edge_kind_filter: Vec::new(),
            })
            .unwrap()
            .unwrap();
        assert_eq!(both.out_total, Some(2), "both: callees counted");
        assert_eq!(both.in_total, Some(0), "both: no callers of ШагА");

        // edge_kinds=["query_ref"] keeps only the query_ref edge.
        let qr = gdb.neighbors(&mk(vec!["query_ref".to_owned()])).unwrap().unwrap();
        assert!(!qr.edges.is_empty(), "query_ref edge present");
        assert!(qr.edges.iter().all(|e| e.kind == "query_ref"), "edges: {:?}", qr.edges);
    }

    /// `node(detail=bodies)` caps its source output at `max_output_tokens`: a tiny budget
    /// truncates the body and flags `budget_exhausted`, a generous budget leaves it whole.
    #[test]
    fn node_bodies_respect_output_budget() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (_db, files) = load_workspace_db(root).expect("workspace loads");
        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let gdb = GraphDb::open(&out).expect("graph database opens");

        let id = "method/common/Сервер/Считать";
        // Tiny budget (1 token ≈ 4 chars) truncates the body and flags exhaustion.
        let tight = crate::tools::graph::node(&gdb, id, ide::GraphDetail::Bodies, 1);
        assert_eq!(tight["budget_exhausted"], serde_json::json!(true));
        assert!(tight["node"]["source"].as_str().unwrap().len() <= 4, "{tight:?}");
        // A generous budget keeps the whole body and sets no exhaustion flag.
        let loose = crate::tools::graph::node(&gdb, id, ide::GraphDetail::Bodies, 10_000);
        assert!(loose.get("budget_exhausted").is_none(), "{loose:?}");
        assert!(loose["node"]["source"].as_str().unwrap().contains("Считать"), "{loose:?}");
    }

    /// A common module with no module-level edge has no stored `module` row, yet
    /// `node(module/common/X)` resolves on demand and lists the module's members; a module
    /// with no methods reports `not_found`.
    #[test]
    fn module_node_resolves_on_demand_and_lists_members() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (_db, files) = load_workspace_db(root).expect("workspace loads");
        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let gdb = GraphDb::open(&out).expect("graph database opens");

        // The module is NOT a stored node (no module-level edge in the fixture)...
        let stored_module_rows: i64 = Connection::open(&out)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE id = 'module/common/Сервер'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored_module_rows, 0, "module has no stored row");

        // ...yet node(module/common/Сервер) resolves on demand and lists its members.
        let resolved =
            gdb.node("module/common/Сервер", ide::GraphDetail::Names).unwrap().expect("resolves");
        assert_eq!(resolved.node.kind, "module");
        let methods = resolved.node.methods.expect("module node carries its methods");
        assert!(
            methods.iter().any(|m| m.id == "method/common/Сервер/Считать" && m.name == "Считать"),
            "members listed: {methods:?}"
        );

        // A module with no methods cannot be synthesized → not_found.
        let missing = gdb.node("module/common/НетТакого", ide::GraphDetail::Names).unwrap();
        assert!(missing.is_err(), "module with no members is not_found");
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
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
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

    /// Serving overview/node/neighbors/source from the SQLite store must produce
    /// JSON byte-identical to the in-memory `ide::Analysis::graph_*` path it
    /// replaces — same fields, signatures, bodies, edges and budget behaviour.
    #[test]
    fn sqlite_serving_matches_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (db, files) = load_workspace_db(root).expect("workspace loads");
        let analysis = Analysis::from_database(db);

        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let gdb = GraphDb::open(&out).expect("graph database opens and validates");

        let id = "method/common/Сервер/Считать";

        let mem_overview =
            serde_json::to_value(analysis.graph_overview(GRAPH_SOURCE_ROOT, Some(root), 10))
                .unwrap();
        let sql_overview = serde_json::to_value(gdb.overview(10).unwrap()).unwrap();
        assert_eq!(mem_overview, sql_overview, "overview JSON");

        let mem_node = serde_json::to_value(
            analysis
                .graph_node(GRAPH_SOURCE_ROOT, Some(root), id, ide::GraphDetail::Bodies)
                .unwrap(),
        )
        .unwrap();
        let sql_node =
            serde_json::to_value(gdb.node(id, ide::GraphDetail::Bodies).unwrap().unwrap()).unwrap();
        assert_eq!(mem_node, sql_node, "node JSON (bodies detail)");

        let params = ide::NeighborsParams {
            id,
            dir: ide::Direction::In,
            depth: 1,
            max_nodes: 50,
            detail: ide::GraphDetail::Signatures,
            provenance_filter: Vec::new(),
            edge_kind_filter: Vec::new(),
        };
        let mem_nb = serde_json::to_value(
            analysis.graph_neighbors(GRAPH_SOURCE_ROOT, Some(root), &params).unwrap(),
        )
        .unwrap();
        let sql_nb = serde_json::to_value(gdb.neighbors(&params).unwrap().unwrap()).unwrap();
        assert_eq!(mem_nb, sql_nb, "neighbors JSON");

        let ids = [id.to_string()];
        let mem_src =
            serde_json::to_value(analysis.graph_source(GRAPH_SOURCE_ROOT, Some(root), &ids, 4000))
                .unwrap();
        let sql_src = serde_json::to_value(gdb.source(&ids, 4000).unwrap()).unwrap();
        assert_eq!(mem_src, sql_src, "source JSON");

        // A malformed/unknown id reports NotFound, not an infra error.
        let missing = gdb.node("method/common/Нет/Метод", ide::GraphDetail::Names).unwrap();
        assert!(missing.is_err(), "unknown id resolves to a GraphError");
    }

    /// `GraphDb::graph_context` renders a method's outbound facts (dispatch, signature,
    /// calls, metadata reads) from the stored graph — the production source for
    /// embedding enrichment. Reuses `ide::GraphContext::render`, so it is byte-identical
    /// to the in-memory renderer for the same facts.
    #[test]
    fn graph_context_renders_method_outbound_facts_from_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // A client method that calls a server method and reads a catalog via a manager.
        write_common_module(
            root,
            "Вызыватель",
            false,
            "Процедура Делать() Экспорт\n\
             Сервер.Считать();\n\
             Справочники.Контрагенты.НайтиПоКоду();\n\
             КонецПроцедуры",
        );
        write_common_module(root, "Сервер", true, "Функция Считать() Экспорт КонецФункции");

        let (_db, files) = load_workspace_db(root).expect("workspace loads");
        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let gdb = GraphDb::open(&out).expect("graph database opens");

        // The calling method carries its signature, its call, and its metadata read.
        let ctx = gdb
            .graph_context("method/common/Вызыватель/Делать")
            .unwrap()
            .expect("method has graph context");
        assert!(ctx.starts_with("Dispatch: "), "{ctx}");
        assert!(ctx.contains("\nSignature: Процедура Делать() Экспорт\n"), "{ctx}");
        assert!(ctx.contains("\nCalls: Считать\n"), "{ctx}");
        assert!(ctx.contains("\nReads: Справочник.Контрагенты\n"), "{ctx}");

        // A leaf method keeps its signature/dispatch but lists no calls or reads.
        let leaf =
            gdb.graph_context("method/common/Сервер/Считать").unwrap().expect("leaf context");
        assert!(leaf.contains("Signature: Функция Считать() Экспорт"), "{leaf}");
        assert!(!leaf.contains("Calls:"), "{leaf}");
        assert!(!leaf.contains("Reads:"), "{leaf}");

        // Non-method ids have no graph context.
        assert_eq!(gdb.graph_context("mdo/Catalog/Контрагенты").unwrap(), None);

        // The graph-DB-backed provider resolves a chunk (path, symbol) to the same text.
        let provider = crate::graph_query::GraphDbContextProvider::new(gdb);
        let via_provider = bsl_search::GraphContextProvider::graph_context(
            &provider,
            "CommonModules/Вызыватель/Ext/Module.bsl",
            "Делать",
            "procedure",
        )
        .expect("provider resolves the method");
        assert!(via_provider.contains("\nCalls: Считать\n"), "{via_provider}");
    }

    /// The fused build streams the search index's chunks from the same parse pass that
    /// produces the graph, attaching each method's graph context. That context must be
    /// byte-identical to `GraphDb::graph_context` for the stored graph (so a chunk
    /// enriched by the fused path keys the same embedding as the round-trip path), and
    /// module-header chunks must carry no context.
    #[test]
    fn fused_chunks_carry_graph_context_matching_stored_graph() {
        #[derive(Default)]
        struct CollectingSink {
            rows: Vec<ide::ChunkRow>,
        }
        impl ide::FusedChunkSink for CollectingSink {
            fn emit_chunks(
                &mut self,
                chunks: &[ide::ChunkRow],
            ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                self.rows.extend_from_slice(chunks);
                Ok(())
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_common_module(
            root,
            "Вызыватель",
            false,
            "Процедура Делать() Экспорт\n\
             Сервер.Считать();\n\
             Справочники.Контрагенты.НайтиПоКоду();\n\
             КонецПроцедуры",
        );
        write_common_module(root, "Сервер", true, "Функция Считать() Экспорт КонецФункции");

        let (_db, files) = load_workspace_db(root).expect("workspace loads");
        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        let mut sink = CollectingSink::default();
        crate::graph_db::build_graph_database_fused(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files,
                built_at: "t".to_string(),
            },
            &mut sink,
        )
        .expect("fused graph database builds");
        let gdb = GraphDb::open(&out).expect("graph database opens");

        let canon_root = root.canonicalize().unwrap().to_string_lossy().replace('\\', "/");
        let mut methods_checked = 0;
        for row in &sink.rows {
            match row.kind {
                bsl_search::ChunkKind::Procedure | bsl_search::ChunkKind::Function => {
                    let rel = row.path.strip_prefix(&canon_root).unwrap().trim_start_matches('/');
                    let id = ide::method_id_for_path(rel, &row.symbol).expect("durable id");
                    let expected = gdb.graph_context(&id).unwrap();
                    assert_eq!(
                        row.graph_context, expected,
                        "fused context for {} diverges from the stored graph",
                        row.symbol
                    );
                    methods_checked += 1;
                }
                bsl_search::ChunkKind::ModuleHeader => {
                    assert_eq!(row.graph_context, None, "header chunk must have no context");
                }
            }
        }
        assert_eq!(methods_checked, 2, "both methods should be chunked and checked");

        // The calling method's context carries its call and metadata read.
        let caller = sink.rows.iter().find(|r| r.symbol == "Делать").unwrap();
        let ctx = caller.graph_context.as_deref().expect("caller has context");
        assert!(ctx.contains("\nCalls: Считать\n"), "{ctx}");
        assert!(ctx.contains("\nReads: Справочник.Контрагенты\n"), "{ctx}");
    }

    /// Resume/incremental contract for the fused embedding pass. Re-running the fused
    /// writer over an UNCHANGED file must not wipe its already-computed embedding — a
    /// restart resumes instead of paying to re-embed the whole corpus on every graph
    /// rebuild. A CHANGED file must be re-ingested back to a pending (NULL) embedding so
    /// only the change is recomputed.
    #[test]
    fn fused_writer_preserves_embeddings_for_unchanged_files() {
        use ide::FusedChunkSink;

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path();
        let file = source.join("CommonModule.bsl");
        fs::write(&file, "Процедура Делать() Экспорт\nКонецПроцедуры").unwrap();

        let db_path = source.join("bsl-search.db");
        let mut engine = bsl_search::SearchEngine::fts_only(&db_path).unwrap();

        let abs = file.canonicalize().unwrap().to_string_lossy().replace('\\', "/");
        let row = ide::ChunkRow {
            path: abs,
            symbol: "Делать".to_owned(),
            kind: bsl_search::ChunkKind::Procedure,
            is_export: true,
            annotations: Vec::new(),
            line_start: 1,
            line_end: 2,
            text: "Процедура Делать() Экспорт\nКонецПроцедуры".to_owned(),
            graph_context: None,
        };

        {
            let mut writer = FusedChunkWriter::new(&mut engine, source.to_path_buf());
            writer.emit_chunks(std::slice::from_ref(&row)).unwrap();
        }

        // One chunk written; its embedding is still NULL, so it is pending.
        let pending = engine.store().load_pending_embedding_documents("code").unwrap();
        assert_eq!(pending.len(), 1, "the freshly ingested chunk is pending");
        let chunk_id = pending[0].0;

        // Pay for its embedding, then confirm nothing is pending.
        engine.store().set_chunk_embedding(chunk_id, &vec![0.1_f32; 1024]).unwrap();
        assert!(
            engine.store().load_pending_embedding_documents("code").unwrap().is_empty(),
            "after embedding, nothing is pending"
        );

        // Re-run the fused writer over the UNCHANGED file: the embedding must survive.
        {
            let mut writer = FusedChunkWriter::new(&mut engine, source.to_path_buf());
            writer.emit_chunks(std::slice::from_ref(&row)).unwrap();
        }
        assert!(
            engine.store().load_pending_embedding_documents("code").unwrap().is_empty(),
            "an unchanged file keeps its embedding across a fused rebuild (resume, not re-embed)"
        );
        assert_eq!(engine.chunk_count().unwrap(), 1, "no duplicate chunk");

        // Change the file on disk: the next fused pass re-ingests it to a pending
        // embedding, so only the changed file is recomputed.
        fs::write(&file, "Процедура Делать() Экспорт\nВыполнить();\nКонецПроцедуры").unwrap();
        {
            let mut writer = FusedChunkWriter::new(&mut engine, source.to_path_buf());
            writer.emit_chunks(std::slice::from_ref(&row)).unwrap();
        }
        assert_eq!(
            engine.store().load_pending_embedding_documents("code").unwrap().len(),
            1,
            "a changed file is re-ingested back to a pending embedding"
        );
    }

    /// The build parallelises per-module resolution within a batch. A batch holding
    /// several modules that call each other and touch the same metadata object must
    /// still produce the fold's graph exactly — same edges, and the shared `Mdo`
    /// node spelled by whichever module the deterministic (file-order) projection
    /// sees first. Built with a batch large enough to hold every module at once, so
    /// the concurrent `map_with` path is exercised, not the one-module-per-batch case.
    #[test]
    fn parallel_multi_module_batch_matches_in_memory() {
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
        // Both modules touch the catalog through both edge passes — a manager call
        // (Pass 2) and a query (Pass 3) — so the parallel collection of call summaries
        // AND of SDBL query refs is exercised across multiple modules in one batch.
        write_common_module(
            root,
            "Альфа",
            true,
            "&НаСервере\nПроцедура ШагА() Экспорт\nБета.ШагБ();\nСправочники.Номенклатура.СоздатьЭлемент();\nЗапрос = \"ВЫБРАТЬ Код ИЗ Справочник.Номенклатура\";\nКонецПроцедуры",
        );
        write_common_module(
            root,
            "Бета",
            true,
            "&НаСервере\nПроцедура ШагБ() Экспорт\nЗапрос = \"ВЫБРАТЬ Наименование ИЗ Справочник.Номенклатура\";\nКонецПроцедуры",
        );

        let (db, files) = load_workspace_db(root).expect("workspace loads");
        let analysis = Analysis::from_database(db);

        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        // A batch_size far above the module count puts every module in one batch.
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            100,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let gdb = GraphDb::open(&out).expect("graph database opens");

        // Overview parity covers node/edge tallies, provenance, and the
        // centrality ranking (whose nodes carry the canonical Mdo spelling).
        let mem_overview =
            serde_json::to_value(analysis.graph_overview(GRAPH_SOURCE_ROOT, Some(root), 10))
                .unwrap();
        let sql_overview = serde_json::to_value(gdb.overview(10).unwrap()).unwrap();
        assert_eq!(mem_overview, sql_overview, "overview JSON from a multi-module batch");
        // The module count is the true distinct-module population (both common modules
        // own methods), not just the module nodes that happen to be edge endpoints.
        assert_eq!(sql_overview["modules"], 2, "both common modules counted: {sql_overview}");

        // `resolve` parity: a bare method name yields the same candidates from both paths.
        let mem_resolve =
            serde_json::to_value(analysis.graph_resolve(GRAPH_SOURCE_ROOT, Some(root), "ШагБ", 10))
                .unwrap();
        let sql_resolve = serde_json::to_value(gdb.resolve("ШагБ", 10).unwrap()).unwrap();
        assert_eq!(mem_resolve, sql_resolve, "resolve candidates from a multi-module batch");
        assert!(
            sql_resolve["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "method/common/Бета/ШагБ" && c["match"] == "name"),
            "ШагБ resolves to its durable id by name: {sql_resolve}"
        );
        // Guard the coverage: the query pass really produced edges across the batch,
        // so the parallel SDBL collection path is genuinely exercised, not vacuous.
        assert!(
            sql_overview["edge_provenance"]["inferred"].as_u64().unwrap_or(0) >= 2,
            "both modules' queries yield inferred query_ref edges: {sql_overview}"
        );

        // The single catalog Mdo node is reached identically from both modules.
        let mdo_id = "mdo/Catalog/Номенклатура";
        let params = ide::NeighborsParams {
            id: mdo_id,
            dir: ide::Direction::In,
            depth: 1,
            max_nodes: 50,
            detail: ide::GraphDetail::Names,
            provenance_filter: Vec::new(),
            edge_kind_filter: Vec::new(),
        };
        let mem_nb = serde_json::to_value(
            analysis.graph_neighbors(GRAPH_SOURCE_ROOT, Some(root), &params).unwrap(),
        )
        .unwrap();
        let sql_nb = serde_json::to_value(gdb.neighbors(&params).unwrap().unwrap()).unwrap();
        assert_eq!(mem_nb, sql_nb, "Mdo neighbours from a multi-module batch");
    }

    /// When `max_nodes` cuts through a set of equal-centrality neighbours, the
    /// in-memory and SQLite paths must keep/drop the *same* nodes — both rank by
    /// `(in_degree desc, durable id asc)`. Guards the tie-break parity.
    #[test]
    fn neighbors_tie_break_matches_across_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_common_module(root, "Ядро", true, "&НаСервере\nФункция Цель() Экспорт КонецФункции");
        // Three callers, each with in-degree 0 — a three-way centrality tie.
        write_common_module(
            root,
            "Вызовы",
            true,
            "&НаСервере\n\
             Процедура А() Экспорт Ядро.Цель(); КонецПроцедуры\n\
             Процедура Б() Экспорт Ядро.Цель(); КонецПроцедуры\n\
             Процедура В() Экспорт Ядро.Цель(); КонецПроцедуры",
        );

        let (db, files) = load_workspace_db(root).expect("workspace loads");
        let analysis = Analysis::from_database(db);

        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let gdb = GraphDb::open(&out).expect("graph database opens");

        let params = ide::NeighborsParams {
            id: "method/common/Ядро/Цель",
            dir: ide::Direction::In,
            depth: 1,
            max_nodes: 1,
            detail: ide::GraphDetail::Names,
            provenance_filter: Vec::new(),
            edge_kind_filter: Vec::new(),
        };
        let mem = analysis.graph_neighbors(GRAPH_SOURCE_ROOT, Some(root), &params).unwrap();
        let sql = gdb.neighbors(&params).unwrap().unwrap();

        assert_eq!(mem.total, 3, "all three tied callers counted");
        assert_eq!(mem.nodes.len(), 1);
        assert_eq!(mem.dropped.len(), 2);
        // Explicit counts: returned matches nodes, dropped_count = total - returned.
        assert_eq!(mem.returned, 1);
        assert_eq!(mem.dropped_count, 2);
        assert_eq!(mem.dropped_count, mem.total - mem.returned);
        // The cut resolves identically on both paths, not just by count.
        assert_eq!(
            serde_json::to_value(&mem).unwrap(),
            serde_json::to_value(&sql).unwrap(),
            "tie-break keeps/drops the same nodes on both paths"
        );
    }

    /// The SQLite reader must keep the in-memory resolver's id semantics: a
    /// malformed id is `BadId` (not `NotFound`), and a metadata id resolves
    /// case-insensitively on its type and object name.
    #[test]
    fn sqlite_serving_bad_id_and_case_insensitive_mdo() {
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
        write(
            root,
            "CommonModules/Менеджер/Ext/Module.bsl",
            "Процедура Создать() Экспорт\nСправочники.Номенклатура.СоздатьЭлемент();\nКонецПроцедуры",
        );

        let files = enumerate_bsl_files(&crate::graph::ProjectSnapshot::load(root)).len();
        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let gdb = GraphDb::open(&out).expect("opens");

        let canonical = gdb
            .overview(50)
            .unwrap()
            .top_by_centrality
            .iter()
            .find(|n| n.kind == "mdo")
            .map(|n| n.id.clone())
            .expect("a catalog Mdo node");
        assert_eq!(canonical, "mdo/Catalog/Номенклатура");

        // Case-insensitive on the object name and ASCII type segment, and accepting
        // a localized type spelling (Справочник → Catalog).
        for variant in
            ["mdo/Catalog/НОМЕНКЛАТУРА", "mdo/catalog/номенклатура", "mdo/Справочник/Номенклатура"]
        {
            let r = gdb
                .node(variant, ide::GraphDetail::Names)
                .unwrap()
                .unwrap_or_else(|e| panic!("{variant} should resolve, got {e:?}"));
            assert_eq!(r.node.id, canonical, "{variant} resolves to the canonical node");
        }

        // Malformed ids are BadId, not NotFound.
        for garbage in ["garbage", "mdo/NoSuchType/X", "method/file/x"] {
            assert!(
                matches!(
                    gdb.node(garbage, ide::GraphDetail::Names).unwrap(),
                    Err(ide::GraphError::BadId { .. })
                ),
                "{garbage} must be BadId"
            );
        }
        // Well-formed but absent → NotFound.
        assert!(matches!(
            gdb.node("method/common/Нет/М", ide::GraphDetail::Names).unwrap(),
            Err(ide::GraphError::NotFound { .. })
        ));
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

    /// A `dependsOn`-only config edit touches no file the stats fold sees, so the
    /// topology component is the ONLY channel that can report it. If the fold were
    /// files-only, this drift would be invisible forever.
    #[test]
    fn fingerprint_topology_component_tracks_a_depends_on_only_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_extension_workspace(root, false);
        let base = workspace_fingerprint(root);

        write_extension_config(root, true);
        let after = workspace_fingerprint(root);
        assert_eq!(base.files, after.files, "no scanned file moved");
        assert_ne!(base.topology, after.topology, "the dependency edge changed the topology");
    }

    /// An extension appearing through zero-config auto-discovery (no analyzer config
    /// file exists at all) must flow into the topology component too — visibility
    /// re-shapes without a single config-file stat to observe.
    #[test]
    fn an_auto_discovered_extension_changes_the_topology_component() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        let base = workspace_fingerprint(root);

        write(root, "src/cfe/NewExt/Configuration.xml", "<Configuration/>");
        let after = workspace_fingerprint(root);
        assert_ne!(base.topology, after.topology, "discovery must reshape the topology");
    }

    /// The offline-edit warm start (daemon down while `dependsOn` changed): the
    /// stale cache is served, and the catch-up publish must hand its hook
    /// `topology_changed = true` — that request is what re-renders persisted
    /// search contexts built under the old topology. A files-only drift must NOT
    /// raise it.
    #[test]
    fn a_topology_only_warm_start_requests_a_whole_collection_context_refresh() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_extension_workspace(root, false);
        seed_cache(root, workspace_fingerprint(root));
        write_extension_config(root, true); // offline dependsOn edit

        let requested = Arc::new(AtomicBool::new(false));
        let hook = {
            let requested = Arc::clone(&requested);
            Arc::new(move |signal: crate::graph::GraphPublishSignal| {
                if signal.topology_changed {
                    requested.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                true
            }) as Arc<dyn Fn(crate::graph::GraphPublishSignal) -> bool + Send + Sync>
        };
        let graph = GraphState::for_workspace(root.to_path_buf()).with_publish_hook(hook);
        graph.ensure_loading();
        wait_ready(&graph);

        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline
            && !requested.load(std::sync::atomic::Ordering::SeqCst)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            requested.load(std::sync::atomic::Ordering::SeqCst),
            "the catch-up publish after a topology-only warm start must request the refresh",
        );
    }

    /// Serving a stale cache is the right trade when the workspace's FILES moved — stale
    /// answers beat "still indexing" for the minutes a rebuild takes. It is the wrong trade
    /// when the extension TOPOLOGY moved: that build resolves names against a project shape
    /// this workspace no longer has, and once adopted every later freshness check compares
    /// against the foreign topology and finds it consistent. Drop the topology check in
    /// `try_publish_stale_and_catch_up` and the foreign build is published as this
    /// workspace's answer.
    #[test]
    fn a_stale_cache_from_another_topology_is_not_published() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_extension_workspace(root, false);
        seed_cache(root, workspace_fingerprint(root));
        write_extension_config(root, true); // offline dependsOn edit

        let graph = GraphState::for_workspace(root.to_path_buf());
        assert!(
            !graph.try_publish_stale_and_catch_up(root),
            "a build made under another topology is not served, however stale-tolerant we are",
        );
        assert!(
            graph.pending_topology_refresh.load(std::sync::atomic::Ordering::SeqCst),
            "and the whole-collection context re-render is still requested",
        );
    }

    /// A cached on-disk graph built under one dependency graph is dead the moment the
    /// declared topology changes, even though not one indexed file moved.
    #[test]
    fn cached_build_is_not_reused_after_a_topology_only_change() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_extension_workspace(root, false);
        seed_cache(root, workspace_fingerprint(root));

        let graph = GraphState::for_workspace(root.to_path_buf());
        assert!(graph.try_publish_cached(root, 0), "an unchanged workspace reuses the cache");

        write_extension_config(root, true);
        let graph = GraphState::for_workspace(root.to_path_buf());
        assert!(
            !graph.try_publish_cached(root, 0),
            "a dependsOn-only edit must invalidate the cached graph"
        );
    }

    /// A build persists a per-file fingerprint for every `.bsl` AND `.xml` file, so
    /// a later reload can classify drift granularly. `sig_hash` is NULL for now.
    #[test]
    fn build_persists_per_file_fingerprints_for_bsl_and_xml() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files: 0,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");

        let conn = Connection::open(&out).unwrap();
        let bsl: i64 = conn
            .query_row("SELECT COUNT(*) FROM files WHERE path LIKE '%.bsl'", [], |r| r.get(0))
            .unwrap();
        let xml: i64 = conn
            .query_row("SELECT COUNT(*) FROM files WHERE path LIKE '%.xml'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(bsl, 2, "both common-module bodies are fingerprinted");
        assert_eq!(xml, 2, "both common-module descriptors are fingerprinted");

        // The stored fingerprints match a fresh stat-scan: an unchanged workspace
        // classifies as an empty diff.
        let stored = read_stored_fingerprints(&out);
        assert_eq!(stored.len(), 4);
        let diff = classify_changes(&stored, &scan_file_stats(root));
        assert!(
            diff.is_empty(),
            "unchanged workspace ⇒ empty diff: {:?}",
            (&diff.added, &diff.removed, &diff.modified)
        );

        // Every `.bsl` module carries a signature hash; `.xml` descriptors stay NULL.
        let bsl_sigs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE path LIKE '%.bsl' AND sig_hash IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let xml_sigs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE path LIKE '%.xml' AND sig_hash IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bsl_sigs, 2, "both module bodies get a signature hash");
        assert_eq!(xml_sigs, 0, ".xml descriptors have no signature hash");
    }

    /// The persisted signature hash is stable across a body-only edit (same method
    /// names/exports/dispatch) but changes when a signature does — the exact property
    /// the body-only fast path relies on.
    #[test]
    fn sig_hash_stable_across_body_edit_changes_on_signature_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let server_sig = |out: &Path| -> i64 {
            Connection::open(out)
                .unwrap()
                .query_row(
                    "SELECT sig_hash FROM files WHERE path LIKE '%Сервер/Ext/Module.bsl'",
                    [],
                    |r| r.get(0),
                )
                .unwrap()
        };

        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &out, 1, &meta())
            .expect("builds");
        let base = server_sig(&out);

        // Body-only edit: same signature `Функция Считать() Экспорт`, new body.
        write(
            root,
            "CommonModules/Сервер/Ext/Module.bsl",
            "&НаСервере\nФункция Считать() Экспорт\nА = 1; Возврат А;\nКонецФункции",
        );
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &out, 1, &meta())
            .expect("rebuilds");
        assert_eq!(server_sig(&out), base, "a body-only edit leaves the signature hash unchanged");

        // Signature edit: rename the function. The hash must move.
        write(
            root,
            "CommonModules/Сервер/Ext/Module.bsl",
            "&НаСервере\nФункция Считать2() Экспорт КонецФункции",
        );
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &out, 1, &meta())
            .expect("rebuilds");
        assert_ne!(server_sig(&out), base, "renaming a method changes the signature hash");
    }

    fn write_catalog(root: &Path, name: &str, id: u8) {
        write(
            root,
            &format!("Catalogs/{name}.xml"),
            &format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-0000000000{id:02}">
        <Properties><Name>{name}</Name><CodeLength>9</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#
            ),
        );
    }

    /// A catalog with one top-level attribute (`ИНН`) and a tabular section (`Товары`)
    /// carrying one column (`Цена`) — exercises the metadata-catalog pass:
    /// `mdo -> attribute`, `mdo -> tabular_section`, `tabular_section -> attribute`.
    fn write_catalog_with_attributes(root: &Path, name: &str, id: u8) {
        write(
            root,
            &format!("Catalogs/{name}.xml"),
            &format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-0000000000{id:02}">
        <Properties><Name>{name}</Name><CodeLength>9</CodeLength></Properties>
        <ChildObjects>
            <Attribute uuid="00000000-0000-0000-0000-0000000010{id:02}">
                <Properties><Name>ИНН</Name><Type><Type>xs:string</Type></Type></Properties>
            </Attribute>
            <TabularSection uuid="00000000-0000-0000-0000-0000000020{id:02}">
                <Properties><Name>Товары</Name></Properties>
                <ChildObjects>
                    <Attribute uuid="00000000-0000-0000-0000-0000000030{id:02}">
                        <Properties><Name>Цена</Name><Type><Type>xs:string</Type></Type></Properties>
                    </Attribute>
                </ChildObjects>
            </TabularSection>
        </ChildObjects>
    </Catalog>
</MetaDataObject>"#
            ),
        );
    }

    /// Write a managed form for catalog `obj`: the `Ext/Form.xml` (two named input
    /// fields) plus the form module `Ext/Form/Module.bsl`. `module_metadata.form` is
    /// loaded from the XML by path, so the form pass sees the two elements.
    fn write_catalog_form(root: &Path, obj: &str, form: &str, module_body: &str) {
        let base = format!("Catalogs/{obj}/Forms/{form}/Ext");
        write(
            root,
            &format!("{base}/Form.xml"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" version="2.10">
    <ChildItems>
        <InputField name="ПолеКод" id="1"><DataPath>Объект.Код</DataPath></InputField>
        <InputField name="ПолеНаименование" id="2"><DataPath>Объект.Наименование</DataPath></InputField>
    </ChildItems>
</Form>"#,
        );
        write(root, &format!("{base}/Form/Module.bsl"), module_body);
    }

    /// A form with a nested group (`Группа` → `ПолеВложенное`), a root field, and two
    /// form attributes — exercises the `form_item → form_item` hierarchy and the
    /// `form → form_attribute` edges.
    fn write_catalog_form_rich(root: &Path, obj: &str, form: &str, module_body: &str) {
        let base = format!("Catalogs/{obj}/Forms/{form}/Ext");
        write(
            root,
            &format!("{base}/Form.xml"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" version="2.10">
    <ChildItems>
        <InputField name="ПолеКод" id="1"><DataPath>Объект.Код</DataPath></InputField>
        <UsualGroup name="Группа" id="10">
            <ChildItems>
                <InputField name="ПолеВложенное" id="11"><DataPath>Объект.Наименование</DataPath></InputField>
            </ChildItems>
        </UsualGroup>
    </ChildItems>
    <Attributes>
        <Attribute name="Объект"/>
        <Attribute name="СписокЗначений"/>
    </Attributes>
</Form>"#,
        );
        write(root, &format!("{base}/Form/Module.bsl"), module_body);
    }

    /// A form for object `obj` whose main attribute `Объект` is typed
    /// `CatalogObject.{obj}` (a `Ref`), with UI fields bound to: a real object
    /// attribute (`Объект.ИНН`), a tabular-section column (`Объект.Товары.Цена`), a
    /// platform standard attribute (`Объект.Код` — must NOT link, excluded from the
    /// catalog), and a broken path (`~Объект.Нет` — must be skipped). Exercises the
    /// `data_binding` cross-links. Pair with `write_catalog_with_attributes(obj)`.
    fn write_catalog_form_databinding(root: &Path, obj: &str, form: &str, module_body: &str) {
        let base = format!("Catalogs/{obj}/Forms/{form}/Ext");
        write(
            root,
            &format!("{base}/Form.xml"),
            &format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" xmlns:v8="http://v8.1c.ru/8.1/data/core" version="2.10">
    <ChildItems>
        <InputField name="ПолеИНН" id="1"><DataPath>Объект.ИНН</DataPath></InputField>
        <InputField name="ПолеЦена" id="2"><DataPath>Объект.Товары.Цена</DataPath></InputField>
        <InputField name="ПолеКод" id="3"><DataPath>Объект.Код</DataPath></InputField>
        <InputField name="ПолеБитый" id="4"><DataPath>~Объект.Нет</DataPath></InputField>
        <InputField name="ПолеГлубокий" id="5"><DataPath>Объект.Товары.Цена.Лишнее</DataPath></InputField>
        <InputField name="ПолеПрочее" id="6"><DataPath>Прочее.Что</DataPath></InputField>
    </ChildItems>
    <Attributes>
        <Attribute name="Объект">
            <Type><v8:Type>cfg:CatalogObject.{obj}</v8:Type></Type>
            <MainAttribute>true</MainAttribute>
        </Attribute>
        <Attribute name="Прочее">
            <Type><v8:Type>xs:string</v8:Type></Type>
        </Attribute>
    </Attributes>
</Form>"#
            ),
        );
        write(root, &format!("{base}/Form/Module.bsl"), module_body);
    }

    /// Dump the data tables in a stable order so two databases can be compared for
    /// logical (byte-identical) equality independent of physical row order. Returns
    /// `(nodes, edges, in_degree, unresolved_calls)`.
    fn dump_data(path: &Path) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
        let conn = Connection::open(path).unwrap();
        let collect = |sql: &str, cols: usize| -> Vec<String> {
            let mut stmt = conn.prepare(sql).unwrap();
            let rows = stmt
                .query_map([], |r| {
                    let mut parts = Vec::with_capacity(cols);
                    for i in 0..cols {
                        parts
                            .push(r.get::<_, rusqlite::types::Value>(i).map(|v| format!("{v:?}"))?);
                    }
                    Ok(parts.join("|"))
                })
                .unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        let nodes = collect(
            "SELECT id, kind, name, qualified, module, file, name_offset, sig_end, src_start, \
             src_end, dispatch, is_export, addressable FROM nodes ORDER BY id",
            13,
        );
        let edges = collect(
            "SELECT from_id, to_id, kind, provenance, crosses FROM edges \
             ORDER BY from_id, to_id, kind, provenance, crosses",
            5,
        );
        let in_degree = collect("SELECT id, degree FROM in_degree ORDER BY id", 2);
        let unresolved = collect(
            "SELECT target_scope, method_lower, caller_file FROM unresolved_calls \
             ORDER BY target_scope, method_lower, caller_file",
            3,
        );
        (nodes, edges, in_degree, unresolved)
    }

    /// The body-only fast path must produce a database byte-identical to a full
    /// rebuild of the edited tree: same nodes (incl. aux GC of an orphaned object),
    /// edges, in-degree, and meta counts. The edit changes a module's edge set (drops
    /// a manager-create that orphans one catalog, adds a query to another already
    /// referenced elsewhere) without touching any signature.
    #[test]
    fn incremental_update_matches_full_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog(root, "Номенклатура", 1);
        write_catalog(root, "Контрагенты", 2);
        write_common_module(
            root,
            "Альфа",
            true,
            "&НаСервере\nПроцедура ШагА() Экспорт\nБета.ШагБ();\n\
             Запрос = \"ВЫБРАТЬ Код ИЗ Справочник.Номенклатура\";\nКонецПроцедуры",
        );
        write_common_module(
            root,
            "Бета",
            true,
            "&НаСервере\nПроцедура ШагБ() Экспорт\nСправочники.Контрагенты.СоздатьЭлемент();\nКонецПроцедуры",
        );

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let db_pre = root.join(".build/pre.db");
        fs::create_dir_all(db_pre.parent().unwrap()).unwrap();
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_pre, 1, &meta())
            .expect("pre build");

        // Body-only edit of Бета: same signature `Процедура ШагБ() Экспорт`. Drops the
        // Контрагенты manager-create (orphaning that catalog's Mdo node) and adds a
        // query to Номенклатура (already referenced by Альфа → existing spelling).
        write(
            root,
            "CommonModules/Бета/Ext/Module.bsl",
            "&НаСервере\nПроцедура ШагБ() Экспорт\n\
             Запрос = \"ВЫБРАТЬ Наименование ИЗ Справочник.Номенклатура\";\nКонецПроцедуры",
        );
        let changed = vec![root.join("CommonModules/Бета/Ext/Module.bsl").canonicalize().unwrap()];

        let db_inc = root.join(".build/inc.db");
        update_graph_database_bodies(
            &crate::graph::ProjectSnapshot::load(root),
            &db_pre,
            &db_inc,
            &changed,
            1,
            &meta(),
        )
        .expect("incremental update");

        let db_full = root.join(".build/full.db");
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_full, 1, &meta())
            .expect("full rebuild of edited tree");

        let (inc_nodes, inc_edges, inc_indeg, inc_unres) = dump_data(&db_inc);
        let (full_nodes, full_edges, full_indeg, full_unres) = dump_data(&db_full);
        assert_eq!(inc_nodes, full_nodes, "nodes (incl. orphan-GC) must match a full rebuild");
        assert_eq!(inc_edges, full_edges, "edges must match a full rebuild");
        assert_eq!(inc_indeg, full_indeg, "in-degree must match a full rebuild");
        assert_eq!(inc_unres, full_unres, "unresolved_calls must match a full rebuild");

        // The orphaned Контрагенты Mdo node is gone in both.
        assert!(
            !inc_nodes.iter().any(|n| n.contains("mdo/Catalog/Контрагенты")),
            "orphaned Контрагенты Mdo node GC'd: {inc_nodes:?}"
        );

        let meta_count = |path: &Path, key: &str| -> String {
            Connection::open(path)
                .unwrap()
                .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(meta_count(&db_inc, "nodes"), meta_count(&db_full, "nodes"), "meta node count");
        assert_eq!(meta_count(&db_inc, "edges"), meta_count(&db_full, "edges"), "meta edge count");
    }

    /// The full build's form pass emits `form`/`form_item` nodes and `contains`
    /// edges (`mdo → form`, `form → form_item`) into SQLite, and the SQL serving path
    /// counts and resolves them (case-insensitively, localized type accepted).
    #[test]
    fn sqlite_build_includes_form_nodes_and_contains_edges() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog(root, "Номенклатура", 1);
        write_catalog_form(
            root,
            "Номенклатура",
            "ФормаЭлемента",
            "&НаКлиенте\nПроцедура ПриОткрытии(Отказ)\nКонецПроцедуры",
        );

        let (_, files) = load_workspace_db(root).expect("workspace loads");
        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");

        let conn = Connection::open(&out).unwrap();
        let count = |sql: &str| -> usize {
            conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap() as usize
        };
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='form'"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='form_item'"), 2);
        // mdo → form containment.
        assert_eq!(
            count(
                "SELECT COUNT(*) FROM edges WHERE kind='contains' \
                 AND from_id='mdo/Catalog/Номенклатура' \
                 AND to_id='form/Catalog/Номенклатура/ФормаЭлемента'"
            ),
            1,
            "mdo → form contains edge"
        );
        // form → form_item containment (one per declared element).
        assert_eq!(
            count(
                "SELECT COUNT(*) FROM edges WHERE kind='contains' \
                 AND from_id='form/Catalog/Номенклатура/ФормаЭлемента'"
            ),
            2,
            "form → form_item contains edges"
        );

        let gdb = GraphDb::open(&out).expect("graph database opens");
        let overview = gdb.overview(10).unwrap();
        assert_eq!(overview.forms, 1);
        assert_eq!(overview.form_items, 2);

        // Form node resolves with a localized type segment and mixed casing.
        let node = gdb
            .node("form/Справочник/номенклатура/ФОРМАЭЛЕМЕНТА", ide::GraphDetail::Names)
            .unwrap()
            .expect("form node resolves case-insensitively");
        assert_eq!(node.node.id, "form/Catalog/Номенклатура/ФормаЭлемента");
        assert_eq!(node.node.kind, "form");
    }

    /// A body-only edit to a form module's `.bsl` must leave the form's structural
    /// nodes/edges byte-identical to a full rebuild: form structure comes from form
    /// XML, not the body, and the incremental reprojection never re-derives it.
    #[test]
    fn incremental_body_edit_preserves_form_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog(root, "Номенклатура", 1);
        write_catalog_form(
            root,
            "Номенклатура",
            "ФормаЭлемента",
            "&НаКлиенте\nПроцедура ПриОткрытии(Отказ)\nСообщить(\"a\");\nКонецПроцедуры",
        );

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let db_pre = root.join(".build/pre.db");
        fs::create_dir_all(db_pre.parent().unwrap()).unwrap();
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_pre, 1, &meta())
            .expect("pre build");

        // Body-only edit of the form module: same handler signature, different body.
        let module_rel = "Catalogs/Номенклатура/Forms/ФормаЭлемента/Ext/Form/Module.bsl";
        write(
            root,
            module_rel,
            "&НаКлиенте\nПроцедура ПриОткрытии(Отказ)\nСообщить(\"b\");\nКонецПроцедуры",
        );
        let changed = vec![root.join(module_rel).canonicalize().unwrap()];

        let db_inc = root.join(".build/inc.db");
        update_graph_database_bodies(
            &crate::graph::ProjectSnapshot::load(root),
            &db_pre,
            &db_inc,
            &changed,
            1,
            &meta(),
        )
        .expect("incremental update");

        let db_full = root.join(".build/full.db");
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_full, 1, &meta())
            .expect("full rebuild");

        let (inc_nodes, inc_edges, inc_indeg, inc_unres) = dump_data(&db_inc);
        let (full_nodes, full_edges, full_indeg, full_unres) = dump_data(&db_full);
        assert_eq!(inc_nodes, full_nodes, "nodes (incl. form/form_item) must match a full rebuild");
        assert_eq!(inc_edges, full_edges, "edges (incl. contains) must match a full rebuild");
        assert_eq!(inc_indeg, full_indeg, "in-degree must match a full rebuild");
        assert_eq!(inc_unres, full_unres, "unresolved_calls must match a full rebuild");

        // The form structure survived the body edit in the incremental path.
        assert!(
            inc_nodes.iter().any(|n| n.contains("form/Catalog/Номенклатура/ФормаЭлемента")),
            "form node preserved: {inc_nodes:?}"
        );
        assert_eq!(
            inc_edges.iter().filter(|e| e.contains("contains")).count(),
            3,
            "1 mdo→form + 2 form→form_item contains edges preserved: {inc_edges:?}"
        );
    }

    /// Form-item group hierarchy (`FormElement.parent_id`) and `Form.attributes`
    /// become graph structure: a nested element hangs off its parent group, root
    /// elements off the form, and each form attribute off the form.
    #[test]
    fn sqlite_build_models_form_hierarchy_and_attributes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog(root, "Номенклатура", 1);
        write_catalog_form_rich(
            root,
            "Номенклатура",
            "ФормаЭлемента",
            "&НаКлиенте\nПроцедура ПриОткрытии(Отказ)\nКонецПроцедуры",
        );

        let (_, files) = load_workspace_db(root).expect("workspace loads");
        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");

        let conn = Connection::open(&out).unwrap();
        let count = |sql: &str| -> usize {
            conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap() as usize
        };
        let edge = |from: &str, to: &str| -> usize {
            count(&format!(
                "SELECT COUNT(*) FROM edges WHERE kind='contains' \
                 AND from_id='{from}' AND to_id='{to}'"
            ))
        };
        let form = "form/Catalog/Номенклатура/ФормаЭлемента";
        let item = |name: &str| format!("form_item/Catalog/Номенклатура/ФормаЭлемента/{name}");

        // 3 UI elements, 2 form attributes.
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='form_item'"), 3);
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='form_attribute'"), 2);

        // Roots hang off the form; the nested field hangs off its group, NOT the form.
        assert_eq!(edge(form, &item("ПолеКод")), 1, "root field → form");
        assert_eq!(edge(form, &item("Группа")), 1, "group → form");
        assert_eq!(edge(form, &item("ПолеВложенное")), 0, "nested field is NOT a form root");
        assert_eq!(
            edge(&item("Группа"), &item("ПолеВложенное")),
            1,
            "nested field → its parent group"
        );

        // Each form attribute hangs off the form.
        assert_eq!(
            edge(form, "form_attr/Catalog/Номенклатура/ФормаЭлемента/Объект"),
            1,
            "form → form_attribute Объект"
        );
        assert_eq!(
            edge(form, "form_attr/Catalog/Номенклатура/ФормаЭлемента/СписокЗначений"),
            1,
            "form → form_attribute СписокЗначений"
        );

        let gdb = GraphDb::open(&out).expect("graph database opens");
        assert_eq!(gdb.overview(10).unwrap().form_attributes, 2);
        // A form attribute resolves with a localized type segment and mixed casing.
        let node = gdb
            .node("form_attr/Справочник/номенклатура/ФормаЭлемента/объект", ide::GraphDetail::Names)
            .unwrap()
            .expect("form attribute resolves case-insensitively");
        assert_eq!(node.node.id, "form_attr/Catalog/Номенклатура/ФормаЭлемента/Объект");
        assert_eq!(node.node.kind, "form_attribute");

        // Served edges out of the form carry the `contains` kind (not mislabelled
        // `call`), and reach both UI items and form attributes.
        let neighbors = gdb
            .neighbors(&ide::NeighborsParams {
                id: form,
                dir: ide::Direction::Out,
                depth: 1,
                max_nodes: 50,
                detail: ide::GraphDetail::Names,
                provenance_filter: Vec::new(),
                edge_kind_filter: Vec::new(),
            })
            .unwrap()
            .expect("form node resolves");
        assert!(
            !neighbors.edges.is_empty() && neighbors.edges.iter().all(|e| e.kind == "contains"),
            "all edges out of a form are `contains`: {:?}",
            neighbors.edges.iter().map(|e| e.kind).collect::<Vec<_>>()
        );
    }

    /// A body-only edit to a form module's `.bsl` must leave the form hierarchy and
    /// attribute nodes/edges byte-identical to a full rebuild (build-only structure,
    /// never re-derived by the incremental reprojection).
    #[test]
    fn incremental_body_edit_preserves_form_hierarchy_and_attributes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog(root, "Номенклатура", 1);
        write_catalog_form_rich(
            root,
            "Номенклатура",
            "ФормаЭлемента",
            "&НаКлиенте\nПроцедура ПриОткрытии(Отказ)\nСообщить(\"a\");\nКонецПроцедуры",
        );

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let db_pre = root.join(".build/pre.db");
        fs::create_dir_all(db_pre.parent().unwrap()).unwrap();
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_pre, 1, &meta())
            .expect("pre build");

        let module_rel = "Catalogs/Номенклатура/Forms/ФормаЭлемента/Ext/Form/Module.bsl";
        write(
            root,
            module_rel,
            "&НаКлиенте\nПроцедура ПриОткрытии(Отказ)\nСообщить(\"b\");\nКонецПроцедуры",
        );
        let changed = vec![root.join(module_rel).canonicalize().unwrap()];

        let db_inc = root.join(".build/inc.db");
        update_graph_database_bodies(
            &crate::graph::ProjectSnapshot::load(root),
            &db_pre,
            &db_inc,
            &changed,
            1,
            &meta(),
        )
        .expect("incremental update");

        let db_full = root.join(".build/full.db");
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_full, 1, &meta())
            .expect("full rebuild");

        let (inc_nodes, inc_edges, ..) = dump_data(&db_inc);
        let (full_nodes, full_edges, ..) = dump_data(&db_full);
        assert_eq!(inc_nodes, full_nodes, "nodes (incl. form_attribute) must match a full rebuild");
        assert_eq!(
            inc_edges, full_edges,
            "edges (incl. form_item hierarchy + form_attribute) must match a full rebuild"
        );
        // The group-hierarchy edge and the form-attribute edges survived the body edit.
        assert!(inc_edges
            .iter()
            .any(|e| e.contains("/ФормаЭлемента/Группа")
                && e.contains("/ФормаЭлемента/ПолеВложенное")));
        assert_eq!(
            inc_edges.iter().filter(|e| e.contains("form_attr/")).count(),
            2,
            "two form_attribute edges preserved: {inc_edges:?}"
        );
    }

    /// The metadata-catalog pass materialises every object's declared structure as
    /// `contains` edges, INDEPENDENT of whether code references the object. A catalog
    /// touched by no code still gets its attribute / tabular-section / column nodes.
    #[test]
    fn sqlite_build_includes_mdo_attribute_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        // Контрагенты has attributes + a tabular section but is referenced by NO code.
        write_catalog_with_attributes(root, "Контрагенты", 1);
        // A module exists only so the build has a batch to iterate (and to prove the
        // catalog object needs no code reference to appear).
        write_common_module(root, "Альфа", true, "Процедура П() Экспорт КонецПроцедуры");

        let (_, files) = load_workspace_db(root).expect("workspace loads");
        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");

        let conn = Connection::open(&out).unwrap();
        let count = |sql: &str| -> usize {
            conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap() as usize
        };
        let edge = |from: &str, to: &str| -> usize {
            count(&format!(
                "SELECT COUNT(*) FROM edges WHERE kind='contains' \
                 AND from_id='{from}' AND to_id='{to}'"
            ))
        };
        let mdo = "mdo/Catalog/Контрагенты";
        // The object node exists though no code references it.
        assert_eq!(count(&format!("SELECT COUNT(*) FROM nodes WHERE id='{mdo}'")), 1);
        // mdo -> top-level attribute.
        assert_eq!(
            edge(mdo, "attribute/Catalog/Контрагенты/ИНН"),
            1,
            "mdo -> attribute (top-level)"
        );
        // mdo -> tabular_section -> column.
        assert_eq!(
            edge(mdo, "tabular_section/Catalog/Контрагенты/Товары"),
            1,
            "mdo -> tabular_section"
        );
        assert_eq!(
            edge(
                "tabular_section/Catalog/Контрагенты/Товары",
                "ts_attr/Catalog/Контрагенты/Товары/Цена"
            ),
            1,
            "tabular_section -> column"
        );
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='tabular_section'"), 1);

        let gdb = GraphDb::open(&out).expect("graph database opens");
        let overview = gdb.overview(10).unwrap();
        assert_eq!(overview.tabular_sections, 1);
        // ИНН + Цена both stored as `attribute`-kind nodes.
        assert_eq!(overview.attributes, 2);

        // The tabular-section column resolves with a localized type + mixed casing.
        let node = gdb
            .node("ts_attr/Справочник/контрагенты/товары/цена", ide::GraphDetail::Names)
            .unwrap()
            .expect("ts column resolves case-insensitively");
        assert_eq!(node.node.id, "ts_attr/Catalog/Контрагенты/Товары/Цена");
        assert_eq!(node.node.kind, "attribute");
        // And the tabular-section node itself.
        let ts = gdb
            .node("tabular_section/Справочник/Контрагенты/Товары", ide::GraphDetail::Names)
            .unwrap()
            .expect("tabular section resolves");
        assert_eq!(ts.node.kind, "tabular_section");
    }

    /// A body-only edit leaves the whole metadata catalog (attributes, tabular
    /// sections, columns) byte-identical to a full rebuild — it is build-only, never
    /// re-derived incrementally, and the catalog is stable under body edits.
    #[test]
    fn incremental_body_edit_preserves_mdo_attribute_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog_with_attributes(root, "Контрагенты", 1);
        write_common_module(
            root,
            "Альфа",
            true,
            "&НаСервере\nПроцедура П() Экспорт\nСообщить(\"a\");\nКонецПроцедуры",
        );

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let db_pre = root.join(".build/pre.db");
        fs::create_dir_all(db_pre.parent().unwrap()).unwrap();
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_pre, 1, &meta())
            .expect("pre build");

        let module_rel = "CommonModules/Альфа/Ext/Module.bsl";
        write(
            root,
            module_rel,
            "&НаСервере\nПроцедура П() Экспорт\nСообщить(\"b\");\nКонецПроцедуры",
        );
        let changed = vec![root.join(module_rel).canonicalize().unwrap()];

        let db_inc = root.join(".build/inc.db");
        update_graph_database_bodies(
            &crate::graph::ProjectSnapshot::load(root),
            &db_pre,
            &db_inc,
            &changed,
            1,
            &meta(),
        )
        .expect("incremental update");

        let db_full = root.join(".build/full.db");
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_full, 1, &meta())
            .expect("full rebuild");

        let (inc_nodes, inc_edges, inc_indeg, ..) = dump_data(&db_inc);
        let (full_nodes, full_edges, full_indeg, ..) = dump_data(&db_full);
        assert_eq!(inc_nodes, full_nodes, "catalog nodes must match a full rebuild");
        assert_eq!(inc_edges, full_edges, "catalog contains edges must match a full rebuild");
        assert_eq!(inc_indeg, full_indeg, "in-degree must match a full rebuild");
        // The catalog structure is present and survived the body edit.
        assert!(inc_nodes.iter().any(|n| n.contains("tabular_section/Catalog/Контрагенты/Товары")));
        assert!(inc_edges.iter().any(|e| e.contains("ts_attr/Catalog/Контрагенты/Товары/Цена")));
    }

    /// The form's data model links to the object structure it mirrors: a UI field's
    /// data path → the object attribute / tabular-section column it shows, and a
    /// Ref-typed form attribute → its backing object. A standard attribute and a broken
    /// path produce no edge (no dangling).
    #[test]
    fn sqlite_build_links_form_data_to_object_fields() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog_with_attributes(root, "Контрагенты", 1);
        write_catalog_form_databinding(
            root,
            "Контрагенты",
            "ФормаЭлемента",
            "&НаКлиенте\nПроцедура ПриОткрытии(Отказ)\nКонецПроцедуры",
        );

        let (_, files) = load_workspace_db(root).expect("workspace loads");
        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");

        let conn = Connection::open(&out).unwrap();
        let count = |sql: &str| -> usize {
            conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap() as usize
        };
        let bind = |from: &str, to: &str| -> usize {
            count(&format!(
                "SELECT COUNT(*) FROM edges WHERE kind='data_binding' \
                 AND from_id='{from}' AND to_id='{to}'"
            ))
        };
        let item = |name: &str| format!("form_item/Catalog/Контрагенты/ФормаЭлемента/{name}");

        // UI field → object attribute, and → tabular-section column.
        assert_eq!(
            bind(&item("ПолеИНН"), "attribute/Catalog/Контрагенты/ИНН"),
            1,
            "field ПолеИНН shows Контрагенты.ИНН"
        );
        assert_eq!(
            bind(&item("ПолеЦена"), "ts_attr/Catalog/Контрагенты/Товары/Цена"),
            1,
            "field ПолеЦена shows the Товары.Цена column"
        );
        // Ref-typed form attribute → its backing object.
        assert_eq!(
            bind("form_attr/Catalog/Контрагенты/ФормаЭлемента/Объект", "mdo/Catalog/Контрагенты"),
            1,
            "form attribute Объект is backed by Контрагенты"
        );

        // A platform standard attribute is not in the catalog → no edge; a `~` path is
        // skipped. Neither dangles.
        assert_eq!(
            count(
                "SELECT COUNT(*) FROM edges WHERE kind='data_binding' \
                   AND to_id LIKE '%/Контрагенты/Код'"
            ),
            0,
            "standard attribute Код is not linked"
        );
        assert_eq!(
            count(&format!(
                "SELECT COUNT(*) FROM edges e WHERE e.kind='data_binding' \
             AND e.from_id='{}'",
                item("ПолеБитый")
            )),
            0,
            "broken ~ path produces no binding"
        );
        // A path through a non-Ref form attribute (`Прочее.Что`) and one deeper than a
        // tabular-section column (`Объект.Товары.Цена.Лишнее`) both resolve to nothing.
        assert_eq!(
            count(&format!(
                "SELECT COUNT(*) FROM edges WHERE kind='data_binding' \
                 AND from_id='{}'",
                item("ПолеПрочее")
            )),
            0,
            "data path through a non-Ref attribute is not linked"
        );
        assert_eq!(
            count(&format!(
                "SELECT COUNT(*) FROM edges WHERE kind='data_binding' \
                 AND from_id='{}'",
                item("ПолеГлубокий")
            )),
            0,
            "data path deeper than a tabular-section column is not linked"
        );
        // Exactly three data_binding edges total (ИНН, Цена, Объект).
        assert_eq!(count("SELECT COUNT(*) FROM edges WHERE kind='data_binding'"), 3);
        // Every data_binding endpoint resolves to a real node (no dangling).
        assert_eq!(
            count(
                "SELECT COUNT(*) FROM edges e WHERE e.kind='data_binding' \
                 AND (e.from_id NOT IN (SELECT id FROM nodes) \
                   OR e.to_id NOT IN (SELECT id FROM nodes))"
            ),
            0,
            "no dangling data_binding endpoints"
        );

        // Served via SQLite: the edge carries the `data_binding` kind, and an inbound
        // query answers "which forms show this object field".
        let gdb = GraphDb::open(&out).expect("graph database opens");
        let neighbors = gdb
            .neighbors(&ide::NeighborsParams {
                id: "attribute/Catalog/Контрагенты/ИНН",
                dir: ide::Direction::In,
                depth: 1,
                max_nodes: 50,
                detail: ide::GraphDetail::Names,
                provenance_filter: Vec::new(),
                edge_kind_filter: Vec::new(),
            })
            .unwrap()
            .expect("attribute node resolves");
        assert!(
            neighbors.edges.iter().any(|e| e.kind == "data_binding"),
            "the field's inbound edges include a data_binding from the form item: {:?}",
            neighbors.edges.iter().map(|e| e.kind).collect::<Vec<_>>()
        );
    }

    /// A body-only edit to a form module's `.bsl` leaves the `data_binding` cross-links
    /// byte-identical to a full rebuild — build-only, never re-derived incrementally.
    #[test]
    fn incremental_body_edit_preserves_data_binding_edges() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog_with_attributes(root, "Контрагенты", 1);
        write_catalog_form_databinding(
            root,
            "Контрагенты",
            "ФормаЭлемента",
            "&НаКлиенте\nПроцедура ПриОткрытии(Отказ)\nСообщить(\"a\");\nКонецПроцедуры",
        );

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let db_pre = root.join(".build/pre.db");
        fs::create_dir_all(db_pre.parent().unwrap()).unwrap();
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_pre, 1, &meta())
            .expect("pre build");

        let module_rel = "Catalogs/Контрагенты/Forms/ФормаЭлемента/Ext/Form/Module.bsl";
        write(
            root,
            module_rel,
            "&НаКлиенте\nПроцедура ПриОткрытии(Отказ)\nСообщить(\"b\");\nКонецПроцедуры",
        );
        let changed = vec![root.join(module_rel).canonicalize().unwrap()];

        let db_inc = root.join(".build/inc.db");
        update_graph_database_bodies(
            &crate::graph::ProjectSnapshot::load(root),
            &db_pre,
            &db_inc,
            &changed,
            1,
            &meta(),
        )
        .expect("incremental update");

        let db_full = root.join(".build/full.db");
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_full, 1, &meta())
            .expect("full rebuild");

        let (inc_nodes, inc_edges, ..) = dump_data(&db_inc);
        let (full_nodes, full_edges, ..) = dump_data(&db_full);
        assert_eq!(inc_nodes, full_nodes, "nodes must match a full rebuild");
        assert_eq!(inc_edges, full_edges, "data_binding edges must match a full rebuild");
        assert_eq!(
            inc_edges.iter().filter(|e| e.contains("data_binding")).count(),
            3,
            "three data_binding edges preserved: {inc_edges:?}"
        );
    }

    /// A changed module referencing an existing object with a different casing must
    /// bail to a full rebuild (it may be the object's first-seen owner, whose new
    /// spelling a full rebuild would adopt but the DB-pinned fast path cannot).
    #[test]
    fn incremental_update_bails_on_aux_casing_drift() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog(root, "Номенклатура", 1);
        write_common_module(
            root,
            "Альфа",
            true,
            "&НаСервере\nПроцедура ШагА() Экспорт\n\
             Запрос = \"ВЫБРАТЬ Код ИЗ Справочник.Номенклатура\";\nКонецПроцедуры",
        );
        write_common_module(
            root,
            "Бета",
            true,
            "&НаСервере\nПроцедура ШагБ() Экспорт\nКонецПроцедуры",
        );

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let db_pre = root.join(".build/pre.db");
        fs::create_dir_all(db_pre.parent().unwrap()).unwrap();
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_pre, 1, &meta())
            .expect("pre build");

        // Бета references the SAME catalog with a different spelling.
        write(
            root,
            "CommonModules/Бета/Ext/Module.bsl",
            "&НаСервере\nПроцедура ШагБ() Экспорт\n\
             Запрос = \"ВЫБРАТЬ Код ИЗ Справочник.НОМЕНКЛАТУРА\";\nКонецПроцедуры",
        );
        let changed = vec![root.join("CommonModules/Бета/Ext/Module.bsl").canonicalize().unwrap()];
        let db_inc = root.join(".build/inc.db");
        let result = update_graph_database_bodies(
            &crate::graph::ProjectSnapshot::load(root),
            &db_pre,
            &db_inc,
            &changed,
            1,
            &meta(),
        );
        assert!(result.is_err(), "casing drift must bail to full rebuild, got {result:?}");
    }

    /// A changed module dropping its last reference to an object that survives via an
    /// unchanged module must bail (the surviving module could re-own the object with a
    /// different canonical spelling on a full rebuild).
    #[test]
    fn incremental_update_bails_on_dropped_shared_aux() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog(root, "Номенклатура", 1);
        let body = "&НаСервере\nПроцедура {m}() Экспорт\n\
                    Запрос = \"ВЫБРАТЬ Код ИЗ Справочник.Номенклатура\";\nКонецПроцедуры";
        write_common_module(root, "Альфа", true, &body.replace("{m}", "ШагА"));
        write_common_module(root, "Бета", true, &body.replace("{m}", "ШагБ"));

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let db_pre = root.join(".build/pre.db");
        fs::create_dir_all(db_pre.parent().unwrap()).unwrap();
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_pre, 1, &meta())
            .expect("pre build");

        // Бета drops its query; Альфа still references Номенклатура (it survives).
        write(
            root,
            "CommonModules/Бета/Ext/Module.bsl",
            "&НаСервере\nПроцедура ШагБ() Экспорт\nКонецПроцедуры",
        );
        let changed = vec![root.join("CommonModules/Бета/Ext/Module.bsl").canonicalize().unwrap()];
        let db_inc = root.join(".build/inc.db");
        let result = update_graph_database_bodies(
            &crate::graph::ProjectSnapshot::load(root),
            &db_pre,
            &db_inc,
            &changed,
            1,
            &meta(),
        );
        assert!(result.is_err(), "dropping a shared aux ref must bail, got {result:?}");
    }

    /// When two modules reference one object with inconsistent casing, the full build
    /// records it as a casing variant, and a body-only edit of a module touching that
    /// object bails to a full rebuild — even though the edit itself keeps the casing
    /// consistent (the fast path cannot reconstruct cross-module first-seen ordering).
    #[test]
    fn incremental_update_bails_on_recorded_casing_variant() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog(root, "Номенклатура", 1);
        // Альфа (earlier file-id) and Гамма spell the same catalog differently.
        write_common_module(
            root,
            "Альфа",
            true,
            "&НаСервере\nПроцедура ШагА() Экспорт\n\
             Запрос = \"ВЫБРАТЬ Код ИЗ Справочник.Номенклатура\";\nКонецПроцедуры",
        );
        write_common_module(
            root,
            "Гамма",
            true,
            "&НаСервере\nПроцедура ШагГ() Экспорт\n\
             Запрос = \"ВЫБРАТЬ Код ИЗ Справочник.НОМЕНКЛАТУРА\";\nКонецПроцедуры",
        );

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let db_pre = root.join(".build/pre.db");
        fs::create_dir_all(db_pre.parent().unwrap()).unwrap();
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_pre, 1, &meta())
            .expect("pre build");

        // The build recorded the inconsistent casing.
        let variants: String = Connection::open(&db_pre)
            .unwrap()
            .query_row("SELECT value FROM meta WHERE key='casing_variants'", [], |r| r.get(0))
            .unwrap();
        assert!(
            variants.lines().any(|k| k == "catalog/номенклатура"),
            "build records the casing variant: {variants:?}"
        );

        // Body-only edit of Альфа keeping its consistent casing — still bails, because
        // Альфа touches the variant object.
        write(
            root,
            "CommonModules/Альфа/Ext/Module.bsl",
            "&НаСервере\nПроцедура ШагА() Экспорт\n\
             Запрос = \"ВЫБРАТЬ Наименование ИЗ Справочник.Номенклатура\";\nКонецПроцедуры",
        );
        let changed = vec![root.join("CommonModules/Альфа/Ext/Module.bsl").canonicalize().unwrap()];
        let db_inc = root.join(".build/inc.db");
        let result = update_graph_database_bodies(
            &crate::graph::ProjectSnapshot::load(root),
            &db_pre,
            &db_inc,
            &changed,
            1,
            &meta(),
        );
        assert!(result.is_err(), "touching a recorded casing variant must bail, got {result:?}");
    }

    /// A multi-file body-only edit that introduces a NEW inconsistently-cased object
    /// (one not referenced before) succeeds on the fast path AND records the variant,
    /// so a later single-module reload refuses the fast path for it.
    #[test]
    fn incremental_update_records_newly_introduced_casing_variant() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_catalog(root, "Товары", 1);
        // Neither module references Товары yet.
        write_common_module(
            root,
            "Альфа",
            true,
            "&НаСервере\nПроцедура ШагА() Экспорт\nКонецПроцедуры",
        );
        write_common_module(
            root,
            "Бета",
            true,
            "&НаСервере\nПроцедура ШагБ() Экспорт\nКонецПроцедуры",
        );

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let db_pre = root.join(".build/pre.db");
        fs::create_dir_all(db_pre.parent().unwrap()).unwrap();
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_pre, 1, &meta())
            .expect("pre build");

        // Both modules now reference Товары with inconsistent casing.
        write(
            root,
            "CommonModules/Альфа/Ext/Module.bsl",
            "&НаСервере\nПроцедура ШагА() Экспорт\n\
             Запрос = \"ВЫБРАТЬ Код ИЗ Справочник.Товары\";\nКонецПроцедуры",
        );
        write(
            root,
            "CommonModules/Бета/Ext/Module.bsl",
            "&НаСервере\nПроцедура ШагБ() Экспорт\n\
             Запрос = \"ВЫБРАТЬ Код ИЗ Справочник.ТОВАРЫ\";\nКонецПроцедуры",
        );
        let changed = vec![
            root.join("CommonModules/Альфа/Ext/Module.bsl").canonicalize().unwrap(),
            root.join("CommonModules/Бета/Ext/Module.bsl").canonicalize().unwrap(),
        ];
        let db_inc = root.join(".build/inc.db");
        update_graph_database_bodies(
            &crate::graph::ProjectSnapshot::load(root),
            &db_pre,
            &db_inc,
            &changed,
            1,
            &meta(),
        )
        .expect("multi-file body-only update succeeds (current result is still correct)");

        // The newly-introduced inconsistency is now persisted, so a later reload bails.
        let variants: String = Connection::open(&db_inc)
            .unwrap()
            .query_row("SELECT value FROM meta WHERE key='casing_variants'", [], |r| r.get(0))
            .unwrap();
        assert!(
            variants.lines().any(|k| k == "catalog/товары"),
            "incremental update records the introduced casing variant: {variants:?}"
        );

        // And the incremental DB is still byte-identical to a full rebuild of this tree.
        let db_full = root.join(".build/full.db");
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_full, 1, &meta())
            .expect("full rebuild");
        let (inc_nodes, inc_edges, _, inc_unres) = dump_data(&db_inc);
        let (full_nodes, full_edges, _, full_unres) = dump_data(&db_full);
        assert_eq!(inc_nodes, full_nodes, "nodes match a full rebuild");
        assert_eq!(inc_edges, full_edges, "edges match a full rebuild");
        assert_eq!(inc_unres, full_unres, "unresolved_calls match a full rebuild");

        // The persisted variant set is byte-identical too (both sides sort).
        let variants_meta = |path: &Path| -> String {
            Connection::open(path)
                .unwrap()
                .query_row("SELECT value FROM meta WHERE key='casing_variants'", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(
            variants_meta(&db_inc),
            variants_meta(&db_full),
            "casing_variants meta row matches a full rebuild byte-for-byte"
        );
    }

    /// Caller-delta path: removing an exported method from B must update B's resolved
    /// callers (their edge to the removed method vanishes) byte-identically to a full
    /// rebuild. The reprojection set is the one `caller_delta_plan` derives.
    #[test]
    fn caller_delta_update_matches_full_rebuild_on_method_removal() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_common_module(root, "Ядро", true, "&НаСервере\nПроцедура М() Экспорт КонецПроцедуры\nПроцедура Н() Экспорт КонецПроцедуры");
        write_common_module(
            root,
            "Алиса",
            true,
            "&НаСервере\nПроцедура ШагА() Экспорт\nЯдро.М();\nКонецПроцедуры",
        );
        write_common_module(
            root,
            "Вера",
            true,
            "&НаСервере\nПроцедура ШагВ() Экспорт\nЯдро.Н();\nКонецПроцедуры",
        );

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let db_pre = root.join(".build/pre.db");
        fs::create_dir_all(db_pre.parent().unwrap()).unwrap();
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_pre, 1, &meta())
            .expect("pre build");

        // Remove Ядро.М (keep Н) — a signature change that only shrinks the resolvable
        // surface, so it is caller-delta-safe.
        write(
            root,
            "CommonModules/Ядро/Ext/Module.bsl",
            "&НаСервере\nПроцедура Н() Экспорт КонецПроцедуры",
        );
        let core_path = root.join("CommonModules/Ядро/Ext/Module.bsl").canonicalize().unwrap();
        let core_key = core_path.to_string_lossy().into_owned();

        let profiles =
            crate::graph_db::recompute_module_profiles(root, std::slice::from_ref(&core_path))
                .unwrap();
        let profile = profiles.get(&core_key).expect("profiled Ядро");
        let callers = crate::graph_db::caller_delta_plan(&db_pre, &[(core_key.as_str(), profile)])
            .unwrap()
            .expect("method removal is caller-delta-safe");
        // Both Алиса (called the removed М) and Вера (called Н) are resolved callers.
        assert_eq!(callers.len(), 2, "both callers discovered: {callers:?}");

        let mut changed = vec![core_path];
        changed.extend(callers);
        let db_inc = root.join(".build/inc.db");
        crate::graph_db::update_graph_database_bodies(
            &crate::graph::ProjectSnapshot::load(root),
            &db_pre,
            &db_inc,
            &changed,
            1,
            &meta(),
        )
        .expect("caller-delta update");

        let db_full = root.join(".build/full.db");
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_full, 1, &meta())
            .expect("full rebuild");
        let (inc_nodes, inc_edges, inc_indeg, inc_unres) = dump_data(&db_inc);
        let (full_nodes, full_edges, full_indeg, full_unres) = dump_data(&db_full);
        assert_eq!(inc_nodes, full_nodes, "nodes match a full rebuild");
        assert_eq!(inc_edges, full_edges, "edges match a full rebuild");
        assert_eq!(inc_indeg, full_indeg, "in-degree matches a full rebuild");
        assert_eq!(inc_unres, full_unres, "unresolved_calls match a full rebuild");
        assert!(
            !inc_nodes.iter().any(|n| n.contains("method/common/Ядро/М")),
            "removed method node gone: {inc_nodes:?}"
        );
    }

    /// IB-3b: ADDING an exported method must reproject the callers whose previously-
    /// unresolved `Ядро.Новый()` now resolves — found via the `unresolved_calls`
    /// reverse index, not `edges_to`. Byte-identical to a full rebuild.
    #[test]
    fn caller_delta_update_matches_full_rebuild_on_method_addition() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_common_module(root, "Ядро", true, "&НаСервере\nПроцедура М() Экспорт КонецПроцедуры");
        // Алиса calls Ядро.Новый, which does not exist yet → unresolved (no stored edge).
        write_common_module(
            root,
            "Алиса",
            true,
            "&НаСервере\nПроцедура ШагА() Экспорт\nЯдро.Новый();\nКонецПроцедуры",
        );

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let db_pre = root.join(".build/pre.db");
        fs::create_dir_all(db_pre.parent().unwrap()).unwrap();
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_pre, 1, &meta())
            .expect("pre build");

        // The build recorded Алиса's unresolved call to Ядро.Новый, and stored no edge.
        let (_, pre_edges, _, pre_unres) = dump_data(&db_pre);
        assert!(
            pre_unres.iter().any(|u| u.contains("common/Ядро") && u.contains("новый")),
            "unresolved call recorded: {pre_unres:?}"
        );
        assert!(
            !pre_edges.iter().any(|e| e.contains("method/common/Ядро/Новый")),
            "no edge to the not-yet-existing method"
        );

        // Add Ядро.Новый exported.
        write(root, "CommonModules/Ядро/Ext/Module.bsl", "&НаСервере\nПроцедура М() Экспорт КонецПроцедуры\nПроцедура Новый() Экспорт КонецПроцедуры");
        let core_path = root.join("CommonModules/Ядро/Ext/Module.bsl").canonicalize().unwrap();
        let core_key = core_path.to_string_lossy().into_owned();
        let profiles =
            crate::graph_db::recompute_module_profiles(root, std::slice::from_ref(&core_path))
                .unwrap();
        let profile = profiles.get(&core_key).unwrap();
        let callers = crate::graph_db::caller_delta_plan(&db_pre, &[(core_key.as_str(), profile)])
            .unwrap()
            .expect("addition is eligible via the unresolved index");
        // Алиса is found through the reverse index (it has no stored edge into Ядро).
        assert_eq!(callers.len(), 1, "the unresolved caller is discovered: {callers:?}");

        let mut changed = vec![core_path];
        changed.extend(callers);
        let db_inc = root.join(".build/inc.db");
        crate::graph_db::update_graph_database_bodies(
            &crate::graph::ProjectSnapshot::load(root),
            &db_pre,
            &db_inc,
            &changed,
            1,
            &meta(),
        )
        .expect("caller-delta update");

        let db_full = root.join(".build/full.db");
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_full, 1, &meta())
            .expect("full rebuild");
        let (inc_nodes, inc_edges, inc_indeg, inc_unres) = dump_data(&db_inc);
        let (full_nodes, full_edges, full_indeg, full_unres) = dump_data(&db_full);
        assert_eq!(inc_nodes, full_nodes, "nodes match a full rebuild");
        assert_eq!(inc_edges, full_edges, "edges match a full rebuild");
        assert_eq!(inc_indeg, full_indeg, "in-degree matches a full rebuild");
        assert_eq!(inc_unres, full_unres, "unresolved_calls match a full rebuild");
        assert!(
            inc_edges.iter().any(|e| e.contains("method/common/Ядро/Новый")),
            "the newly-resolving caller's edge appears: {inc_edges:?}"
        );
        assert!(
            !inc_unres.iter().any(|u| u.contains("common/Ядро") && u.contains("новый")),
            "the resolved call is no longer in the unresolved index: {inc_unres:?}"
        );
    }

    /// A body-only edit that ADDS an unresolved call must refresh the reverse index
    /// (so a later addition of that method finds this caller), byte-identically to a
    /// full rebuild.
    #[test]
    fn incremental_body_edit_refreshes_unresolved_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_common_module(root, "Ядро", true, "&НаСервере\nПроцедура М() Экспорт КонецПроцедуры");
        write_common_module(
            root,
            "Алиса",
            true,
            "&НаСервере\nПроцедура ШагА() Экспорт КонецПроцедуры",
        );

        let meta = || crate::graph_db::GraphMeta {
            revision: 1,
            fingerprint: crate::graph_db::GraphFp::default(),
            files: 0,
            built_at: "t".to_string(),
        };
        let db_pre = root.join(".build/pre.db");
        fs::create_dir_all(db_pre.parent().unwrap()).unwrap();
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_pre, 1, &meta())
            .expect("pre build");

        // Body-only edit (ШагА signature unchanged): add a call to the missing Ядро.Завтра.
        write(
            root,
            "CommonModules/Алиса/Ext/Module.bsl",
            "&НаСервере\nПроцедура ШагА() Экспорт\nЯдро.Завтра();\nКонецПроцедуры",
        );
        let changed = vec![root.join("CommonModules/Алиса/Ext/Module.bsl").canonicalize().unwrap()];
        let db_inc = root.join(".build/inc.db");
        crate::graph_db::update_graph_database_bodies(
            &crate::graph::ProjectSnapshot::load(root),
            &db_pre,
            &db_inc,
            &changed,
            1,
            &meta(),
        )
        .expect("body-only update");

        let db_full = root.join(".build/full.db");
        build_graph_database(&crate::graph::ProjectSnapshot::load(root), &db_full, 1, &meta())
            .expect("full rebuild");
        let (_, _, _, inc_unres) = dump_data(&db_inc);
        let (_, _, _, full_unres) = dump_data(&db_full);
        assert!(
            inc_unres.iter().any(|u| u.contains("common/Ядро") && u.contains("завтра")),
            "the newly-added unresolved call is indexed: {inc_unres:?}"
        );
        assert_eq!(inc_unres, full_unres, "unresolved_calls match a full rebuild");
    }

    /// `classify_changes` sorts each modified/added/removed file into the right
    /// bucket, and `.xml` drift is flagged for the (forced) full-rebuild path.
    #[test]
    fn classify_changes_buckets_add_remove_modify_and_flags_xml() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            &crate::graph::ProjectSnapshot::load(root),
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                files: 0,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let stored = read_stored_fingerprints(&out);

        // Modify one body, add a new module, remove an existing one.
        write(
            root,
            "CommonModules/Сервер/Ext/Module.bsl",
            "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции",
        );
        write_common_module(
            root,
            "Новый",
            true,
            "&НаСервере\nПроцедура П() Экспорт КонецПроцедуры",
        );
        fs::remove_file(root.join("CommonModules/Клиент/Ext/Module.bsl")).unwrap();

        let diff = classify_changes(&stored, &scan_file_stats(root));
        assert!(!diff.is_empty());

        let ends = |v: &[String], suffix: &str| v.iter().filter(|p| p.ends_with(suffix)).count();
        assert_eq!(ends(&diff.modified, "Сервер/Ext/Module.bsl"), 1, "edited body is modified");
        assert_eq!(ends(&diff.added, "Новый/Ext/Module.bsl"), 1, "new body is added");
        // The new module also drops a new `.xml` descriptor → metadata drift.
        assert_eq!(ends(&diff.added, "Новый.xml"), 1, "new descriptor is added");
        assert_eq!(ends(&diff.removed, "Клиент/Ext/Module.bsl"), 1, "deleted body is removed");
        assert!(diff.touches_metadata(), "an added .xml descriptor forces the full-rebuild path");

        // A modified-only `.bsl` (no add/remove, no `.xml`) does NOT flag metadata.
        let body_only = WorkspaceDiff {
            added: vec![],
            removed: vec![],
            modified: vec!["/cfg/SomeModule/Ext/Module.bsl".to_string()],
        };
        assert!(!body_only.touches_metadata(), "a body-only change does not touch metadata");
    }

    /// End-to-end: a signature change (method removal) drifts the workspace, and the
    /// reload takes the caller-delta path — bumping the generation and serving a graph
    /// where the removed method (and its caller's edge) is gone.
    #[test]
    fn drift_with_signature_change_reloads_via_caller_delta() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_common_module(root, "Ядро", true, "&НаСервере\nФункция Цель() Экспорт КонецФункции\nФункция Прочее() Экспорт КонецФункции");
        write_common_module(
            root,
            "Вызов",
            true,
            "&НаСервере\nПроцедура Звать() Экспорт\nЯдро.Цель();\nКонецПроцедуры",
        );

        let mut graph = GraphState::for_workspace(root.to_path_buf());
        graph.drift_interval = Duration::ZERO;
        graph.ensure_loading();
        wait_ready(&graph);

        let snap1 = graph.snapshot().expect("ready");
        assert!(snap1
            .graph
            .node("method/common/Ядро/Цель", ide::GraphDetail::Names)
            .unwrap()
            .is_ok());

        // Remove Ядро.Цель — a caller-delta-safe signature change.
        write(
            root,
            "CommonModules/Ядро/Ext/Module.bsl",
            "&НаСервере\nФункция Прочее() Экспорт КонецФункции",
        );
        let drifted = graph.freshness(&snap1);
        assert!(drifted.stale, "removal drifts the workspace");

        // The caller-delta reload publishes generation 2 with the method gone.
        let mut settled = None;
        for _ in 0..200 {
            let snap = graph.snapshot().expect("snapshot");
            if snap.generation == 2 {
                settled = Some(snap);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let snap2 = settled.expect("reload published generation 2");
        assert!(
            snap2.graph.node("method/common/Ядро/Цель", ide::GraphDetail::Names).unwrap().is_err(),
            "removed method no longer resolves after caller-delta reload"
        );
        // The caller's edge into the removed method is gone (Вызов has no out-edges now).
        let overview = snap2.graph.overview(10).expect("overview");
        assert_eq!(overview.edges, 0, "the caller's edge to the removed method vanished");
    }

    /// The straightforward sequential scan the parallel per-directory version replaces:
    /// canonicalise every file individually, dedup, in walk order. Kept as the parity
    /// oracle so the optimisation cannot silently change the file universe. Takes explicit
    /// roots (each a dir or a file) so a file-root case can be exercised too.
    #[cfg(test)]
    fn scan_stats_over_roots_reference(roots: &[PathBuf]) -> Vec<FileStat> {
        let mut stats: Vec<FileStat> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        for root in roots {
            for entry in WalkDir::new(root).follow_links(true) {
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
                let path =
                    entry.path().canonicalize().unwrap_or_else(|_| entry.path().to_path_buf());
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
                stats.push(FileStat { path: path.to_string_lossy().into_owned(), mtime, len });
            }
        }
        stats
    }

    /// The parallel, per-directory-canonical scan yields the same `(canonical path,
    /// fingerprint)` set as the sequential reference — through nested directories, a
    /// symlinked subtree, and a file symlink (all canonicalise to the same targets, so
    /// dedup collapses the duplicate reachable paths identically).
    #[test]
    fn scan_file_stats_matches_reference() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_common_module(root, "Сервер", true, "&НаСервере\nФункция Ч() Экспорт КонецФункции");
        write_common_module(
            root,
            "Клиент",
            false,
            "&НаКлиенте\nПроцедура П() Экспорт КонецПроцедуры",
        );
        // A deeper nested directory.
        write(
            root,
            "Documents/Док/Forms/Форма/Ext/Form/Module.bsl",
            "Процедура Р() КонецПроцедуры",
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            // A real subtree reachable BOTH directly and through a directory symlink.
            write(root, "_real/Sub/File.bsl", "Процедура С() КонецПроцедуры");
            symlink(root.join("_real"), root.join("Linked")).unwrap();
            // A file that is itself a symlink to a real `.bsl`.
            symlink(root.join("CommonModules/Сервер/Ext/Module.bsl"), root.join("Alias.bsl"))
                .unwrap();
        }

        // A scan-root that is itself a FILE (a misconfigured extension path), which the
        // partitioning must still stat rather than silently drop. It lives OUTSIDE the
        // directory roots so it is reachable ONLY as an explicit file-root.
        let ext_dir = tempfile::tempdir().unwrap();
        let file_root = ext_dir.path().join("Standalone.xml");
        std::fs::write(&file_root, "<Configuration/>").unwrap();
        let mut roots = scan_roots(root);
        roots.push(file_root.clone());

        let key = |s: &FileStat| (s.path.clone(), s.fingerprint());
        let mut got: Vec<_> = scan_stats_over_roots(&roots).iter().map(key).collect();
        let mut want: Vec<_> = scan_stats_over_roots_reference(&roots).iter().map(key).collect();
        got.sort();
        want.sort();
        assert_eq!(got, want, "parallel scan must match the sequential reference byte-for-byte");
        assert!(!got.is_empty(), "the fixture produced files");
        let file_root_canonical =
            file_root.canonicalize().unwrap_or(file_root).to_string_lossy().into_owned();
        assert!(
            got.iter().any(|(p, _)| *p == file_root_canonical),
            "a file scan-root must be stat'd, not dropped",
        );
    }
}
