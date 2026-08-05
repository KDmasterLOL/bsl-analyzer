use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::change_hub::{ChangeEntry, Health, WorkspaceChangeHub};
use crate::graph::scan::{classify_changes, FileStat, WorkspaceDiff};

use super::lifecycle::{lock_recover, DiagnosticsState, Inner};
use super::resident::apply_resident_changes;
use super::types::{DiagnosticsStatus, Freshness, ReloadState};

pub(super) const DRIFT_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// How often the drift poll re-resolves the scope's git refs (base, HEAD) to
/// notice ref-only movement. A couple of `rev-parse`s per minute is free.
pub(super) const SCOPE_REF_CHECK_INTERVAL: Duration = Duration::from_secs(60);

pub(super) const FORCE_RESCAN_FLOOR: Duration = Duration::from_millis(250);

pub(super) const RECONCILE_INTERVAL: Duration = Duration::from_secs(90);

pub(super) const MIN_RECONCILE_INTERVAL: Duration = Duration::from_secs(5);

pub(super) const CONFIG_FILES: [&str; 3] =
    ["bsl-analyzer.toml", ".bsl-analyzer.json", ".bsl-language-server.json"];

pub(super) fn reconcile_interval() -> Duration {
    clamp_reconcile_interval(
        std::env::var("BSL_MCP_RECONCILE_SECS").ok().and_then(|s| s.parse::<u64>().ok()),
    )
}

pub(super) fn clamp_reconcile_interval(secs: Option<u64>) -> Duration {
    match secs {
        Some(0) | None => RECONCILE_INTERVAL,
        Some(secs) => Duration::from_secs(secs).max(MIN_RECONCILE_INTERVAL),
    }
}

pub(super) struct ScanCache {
    pub(super) at: Instant,
    pub(super) stats: Vec<FileStat>,
    pub(super) config_fp: u64,
    /// The baseline this snapshot is comparable against — see `Inner::baseline_epoch`.
    pub(super) baseline_epoch: u64,
}

impl DiagnosticsState {
    #[cfg(test)]
    pub(super) fn scan_count(&self) -> usize {
        self.scan_count.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(super) fn has_hub_cursor(&self) -> bool {
        lock_recover(&self.hub_cursor).is_some()
    }

    #[cfg(test)]
    pub(super) fn drain_and_discard_cursor(&self) {
        let cursor = *lock_recover(&self.hub_cursor);
        if let (Some(hub), Some(cursor)) = (&self.change_hub, cursor) {
            let batch = hub.drain(cursor);
            *lock_recover(&self.hub_cursor) = Some(batch.cursor);
        }
    }

    #[cfg(test)]
    pub(super) fn set_reconcile_probe(&self, f: impl FnOnce() + Send + 'static) {
        *lock_recover(&self.reconcile_probe) = Some(Box::new(f));
    }

    #[cfg(test)]
    pub(super) fn set_post_scan_probe(&self, f: impl FnOnce() + Send + 'static) {
        *lock_recover(&self.post_scan_probe) = Some(Box::new(f));
    }

    #[cfg(test)]
    pub(super) fn set_pre_drain_probe(&self, f: impl FnOnce() + Send + 'static) {
        *lock_recover(&self.pre_drain_probe) = Some(Box::new(f));
    }

    pub(super) fn poll_drift(&self) {
        let Some(root) = self.workspace_root.clone() else {
            return;
        };
        if !matches!(self.status(), DiagnosticsStatus::Ready { .. }) {
            return;
        }

        // Ref-only git movement (fetch/rebase/reset) changes what the scope
        // should be without any watched-file event; this is the only place
        // that can notice it.
        self.check_scope_ref_drift(&root);

        // The `metadata object` miss escape hatch forces a scan regardless of hub health.
        if self.force_scan.swap(false, Ordering::SeqCst) {
            self.poll_drift_via_scan(&root);
            return;
        }

        // Healthy hub → event-driven drain (O(change), no scan on the hot path).
        // No hub, or a degraded one, → today's throttled scan-on-read (parity).
        //
        // Health is asked about OUR cursor: another consumer that stopped draining owes
        // its own reconcile, and answering that debt here would put these diagnostics on
        // a full scan for as long as the other one stays silent.
        let cursor = *lock_recover(&self.hub_cursor);
        match &self.change_hub {
            Some(hub) if matches!(hub.health_for(cursor), Health::Healthy) => {
                self.poll_drift_via_drain(hub, &root);
            }
            _ => {
                self.poll_drift_via_scan(&root);
            }
        }
    }

    fn poll_drift_via_scan(&self, root: &Path) -> bool {
        let Some(scan) = self.throttled_scan(root) else {
            return false;
        };

        // Diff under a short read lock against the last-applied stats.
        let (changes, config_changed) = {
            let inner = lock_recover(&self.inner);
            let stored: HashMap<String, u64> = inner.stats.clone();
            (classify_changes(&stored, &scan.stats), inner.config_fp != scan.config_fp)
        };
        self.apply_scan_drift(&changes, config_changed, &scan)
    }

    fn apply_scan_drift(
        &self,
        changes: &WorkspaceDiff,
        config_changed: bool,
        scan: &OwnedScan,
    ) -> bool {
        if changes.is_empty() && !config_changed {
            return false;
        }

        // Only an analyzer-config edit forces a full rebuild (it can change the
        // extension set and the effective diagnostics config). Everything else — any
        // `.xml` add/remove/edit, `.bsl` body edits, AND `.bsl` files appearing or
        // vanishing — is reconciled into the live resident in place. A removed subtree
        // needs no special case here: the scan diff enumerates every descendant.
        if config_changed {
            self.kick_full_reload();
            return true;
        }

        // XML drift spans all three buckets: an added/removed object is a structural
        // listing change, an edited one a content change — the substrate refresh handles
        // all of them by re-discovery + re-read of changed/new composing files.
        let xml_paths: Vec<PathBuf> = changes
            .added
            .iter()
            .chain(&changes.removed)
            .chain(&changes.modified)
            .filter(|p| bsl_conventions::str_has_extension(p, bsl_conventions::XML_EXTENSION))
            .map(PathBuf::from)
            .collect();
        let added_bsl: Vec<String> = changes
            .added
            .iter()
            .filter(|p| !bsl_conventions::str_has_extension(p, bsl_conventions::XML_EXTENSION))
            .cloned()
            .collect();
        let modified_bsl: Vec<String> = changes
            .modified
            .iter()
            .filter(|p| !bsl_conventions::str_has_extension(p, bsl_conventions::XML_EXTENSION))
            .cloned()
            .collect();
        let removed_bsl: Vec<String> = changes
            .removed
            .iter()
            .filter(|p| !bsl_conventions::str_has_extension(p, bsl_conventions::XML_EXTENSION))
            .cloned()
            .collect();
        self.apply_metadata_and_body_drift(
            &xml_paths,
            &added_bsl,
            &modified_bsl,
            &removed_bsl,
            scan,
        );
        true
    }

    fn poll_drift_via_drain(&self, hub: &WorkspaceChangeHub, root: &Path) {
        // A full rebuild in flight will publish a fresh resident whose baseline scan already
        // reflects disk, and `apply_drained_resident` defers to it (bails on `Running`).
        // Draining now would advance the cursor past events the apply then drops — the
        // resident would miss the whole rebuild window. Leave them pending: the reload
        // re-subscribes a fresh cursor at its start, and the next poll after it finishes
        // drains that window onto the new resident. Mirrors the scan path, which bails
        // without rebasing its baseline so the drift is re-detected.
        if lock_recover(&self.inner).reload == ReloadState::Running {
            return;
        }
        let Some(cursor) = *lock_recover(&self.hub_cursor) else {
            // Ready but no cursor yet (a read racing the build's subscribe): reconcile via
            // scan this once; the next poll uses the cursor.
            self.poll_drift_via_scan(root);
            return;
        };
        #[cfg(test)]
        self.fire_pre_drain_probe();

        let batch = hub.drain(cursor);
        *lock_recover(&self.hub_cursor) = Some(batch.cursor);
        // A debt raised between the health decision above and this drain: the scan covers
        // what the hub dropped, and the entries cover what the scan's cache is too old to
        // see — this path does not force a fresh walk, so a cache younger than the drift
        // interval answers with a snapshot taken before these very entries.
        if batch.rescan_required {
            self.poll_drift_via_scan(root);
            if lock_recover(&self.inner).reload == ReloadState::Running {
                return;
            }
        }
        if batch.entries.is_empty() {
            return;
        }
        self.apply_drained_entries(&batch.entries);
    }

    pub(super) fn apply_drained_entries(&self, entries: &[ChangeEntry]) {
        let baseline: HashMap<String, u64> = lock_recover(&self.inner).stats.clone();
        // The analyzer config files fingerprinted by `config_files_fingerprint` — canonicalised
        // to match the drained key spelling. Only a file at THIS exact location is config
        // drift; an identically-named file elsewhere in the tree is not (parity with the
        // scan path, which fingerprints only `root.join(name)`).
        let config_paths = self.config_file_paths();

        let class = crate::drift_classify::classify_drift(entries, &config_paths, Some(&baseline));

        // A config edit changes the effective diagnostics/extension setup and a removed
        // subtree hides descendants the drain could not enumerate — neither is
        // expressible in place. Everything else (xml, and `.bsl` added / modified /
        // removed) reconciles into the live resident.
        if class.config_changed || class.structural_rescan {
            self.kick_full_reload();
            return;
        }
        if class.xml_paths.is_empty()
            && class.bsl_modified.is_empty()
            && class.bsl_added.is_empty()
            && class.bsl_removed.is_empty()
        {
            return;
        }
        let xml_paths: Vec<PathBuf> =
            class.xml_paths.iter().map(|d| PathBuf::from(&d.key)).collect();
        let added_bsl: Vec<String> = class.bsl_added.iter().map(|d| d.key.clone()).collect();
        let modified_bsl: Vec<String> = class.bsl_modified.iter().map(|d| d.key.clone()).collect();
        let removed_bsl: Vec<String> = class.bsl_removed.iter().map(|d| d.key.clone()).collect();
        self.apply_drained_resident(
            &xml_paths,
            &added_bsl,
            &modified_bsl,
            &removed_bsl,
            &class.removed_keys,
            &class.new_fp,
        );
        // The baseline just moved, which leaves every cached snapshot describing the world
        // before the move. Dropping the cache is what stops the next scan within the drift
        // interval from being compared against a baseline it predates.
        *lock_recover(&self.scan) = None;
    }

    fn apply_drained_resident(
        &self,
        xml_paths: &[PathBuf],
        added_bsl: &[String],
        modified_bsl: &[String],
        removed_bsl: &[String],
        removed_keys: &[String],
        new_fp: &HashMap<String, u64>,
    ) {
        let mut needs_rebuild = false;
        let mut rescope: Option<(PathBuf, String)> = None;
        {
            let mut inner = lock_recover(&self.inner);
            if inner.reload == ReloadState::Running {
                return;
            }
            let Inner {
                resident: Some(resident), stats, generation, status, baseline_epoch, ..
            } = &mut *inner
            else {
                return;
            };
            let (rebuild, moved) = apply_resident_changes(
                resident,
                xml_paths,
                added_bsl,
                modified_bsl,
                removed_bsl,
                |p| new_fp.get(p).copied(),
                stats,
            );
            if rebuild {
                needs_rebuild = true;
            } else {
                // Body drift moved the working copy the vendor-diff scope was
                // computed against (save, checkout, pull). The git diff can take
                // seconds, so only capture the inputs here — the recompute runs
                // after this lock is released. No-op without a configured base.
                if !added_bsl.is_empty() || !modified_bsl.is_empty() || !removed_bsl.is_empty() {
                    if let Some(base) = resident.diff_base.clone() {
                        rescope = Some((resident.workspace_root().to_path_buf(), base));
                    }
                }
                for (key, fp) in new_fp {
                    stats.insert(key.clone(), *fp);
                }
                for key in removed_keys {
                    stats.remove(key);
                }
                *baseline_epoch += 1;
                if moved {
                    *generation += 1;
                    // An add/remove changed the served file universe; keep the
                    // observable `Ready { files }` count truthful.
                    *status = DiagnosticsStatus::Ready { files: resident.by_path.len() };
                    tracing::info!(
                        xml = xml_paths.len(),
                        added = added_bsl.len(),
                        bodies = modified_bsl.len(),
                        removed = removed_bsl.len(),
                        generation = *generation,
                        "diagnostics event-driven drift refresh",
                    );
                }
            }
        }
        if needs_rebuild {
            self.kick_full_reload();
        } else if let Some((root, base)) = rescope {
            self.rescope_out_of_lock(&root, &base);
        }
    }

    fn apply_metadata_and_body_drift(
        &self,
        xml_paths: &[PathBuf],
        added_bsl: &[String],
        modified_bsl: &[String],
        removed_bsl: &[String],
        scan: &OwnedScan,
    ) {
        let new_fp: HashMap<&str, u64> =
            scan.stats.iter().map(|s| (s.path.as_str(), s.fingerprint())).collect();

        let mut needs_rebuild = false;
        let mut rescope: Option<(PathBuf, String)> = None;
        {
            let mut inner = lock_recover(&self.inner);
            // A full rebuild already in flight will publish a fresh resident; defer to it
            // rather than mutating a resident that is about to be replaced.
            if inner.reload == ReloadState::Running {
                return;
            }
            // Another caller may have reconciled this exact scan already (both passed the
            // throttle, then serialised here); bail so we neither re-walk the roots nor
            // double-bump the generation.
            if classify_changes(&inner.stats, &scan.stats).is_empty()
                && inner.config_fp == scan.config_fp
            {
                return;
            }
            // The baseline moved after this snapshot was taken, so the two describe
            // different worlds and their diff runs backwards (Ф12): a file added since
            // would be applied as a deletion. Checked HERE, under the lock that does the
            // applying — a check at the caller leaves the window between the two open, and
            // there is more than one caller.
            if scan.baseline_epoch != inner.baseline_epoch {
                tracing::debug!(
                    snapshot = scan.baseline_epoch,
                    baseline = inner.baseline_epoch,
                    "dropping a scan whose baseline moved under it; the next poll rescans"
                );
                // The reason this scan ran is one-shot — a reconcile debt cleared by the
                // drain that raised it, or a forced rescan already consumed — and refusing
                // the snapshot answers none of it. Without re-arming, a healthy cursor
                // sends every following read back down the drain path and the drift the hub
                // lost stays unapplied until the watchdog tick, with `stale` reading false.
                self.force_scan.store(true, Ordering::SeqCst);
                // And drop the snapshot itself: `throttled_scan` caches unconditionally, so
                // a snapshot taken before the move is cached after it and would be handed
                // to every read until it cools — each one refusing it again. The epoch only
                // grows, so this one can never become applicable.
                drop(inner);
                *lock_recover(&self.scan) = None;
                return;
            }
            let Inner {
                resident: Some(resident), stats, generation, status, baseline_epoch, ..
            } = &mut *inner
            else {
                return;
            };
            let (rebuild, moved) = apply_resident_changes(
                resident,
                xml_paths,
                added_bsl,
                modified_bsl,
                removed_bsl,
                |p| new_fp.get(p).copied(),
                stats,
            );
            if rebuild {
                needs_rebuild = true;
            } else {
                // Body drift moved the working copy the vendor-diff scope was
                // computed against (save, checkout, pull). The git diff can take
                // seconds, so only capture the inputs here — the recompute runs
                // after this lock is released. No-op without a configured base.
                if !added_bsl.is_empty() || !modified_bsl.is_empty() || !removed_bsl.is_empty() {
                    if let Some(base) = resident.diff_base.clone() {
                        rescope = Some((resident.workspace_root().to_path_buf(), base));
                    }
                }
                // Advance the drift baseline to the scan we reconciled against: every
                // applied body and every XML add/remove/edit is now reflected in the
                // resident, so its state equals `scan`. Rebasing even when nothing moved
                // (a pure mtime touch with unchanged content) stops us re-scanning it
                // every window.
                *stats = scan.stats.iter().map(|s| (s.path.clone(), s.fingerprint())).collect();
                *baseline_epoch += 1;
                if moved {
                    *generation += 1;
                    // An add/remove changed the served file universe; keep the
                    // observable `Ready { files }` count truthful.
                    *status = DiagnosticsStatus::Ready { files: resident.by_path.len() };
                    tracing::info!(
                        xml = xml_paths.len(),
                        added = added_bsl.len(),
                        bodies = modified_bsl.len(),
                        removed = removed_bsl.len(),
                        generation = *generation,
                        "diagnostics metadata drift refresh",
                    );
                }
            }
        }
        if needs_rebuild {
            self.kick_full_reload();
        } else if let Some((root, base)) = rescope {
            self.rescope_out_of_lock(&root, &base);
        }
    }

    /// Recompute the vendor-diff scope OUTSIDE the resident lock (the git diff
    /// can take seconds on a large working copy) and publish it under a short
    /// lock — unless a full reload started meanwhile: the rebuilt resident
    /// computes its own fresh scope.
    fn rescope_out_of_lock(&self, root: &Path, base: &str) {
        let (scope, identity) = super::resident::build_scope(root, base);
        let mut inner = lock_recover(&self.inner);
        if inner.reload == ReloadState::Running {
            return;
        }
        if let Some(resident) = inner.resident.as_mut() {
            resident.config.scope = scope;
            resident.scope_identity = identity;
        }
    }

    /// Compare the scope's resolved (base, HEAD) OIDs — and the author
    /// filter's pinned HEAD — against the live refs, throttled off the request
    /// hot path, and rebuild whichever went stale.
    fn check_scope_ref_drift(&self, root: &Path) {
        {
            let mut at = lock_recover(&self.scope_ref_check_at);
            if at.is_some_and(|t| t.elapsed() < SCOPE_REF_CHECK_INTERVAL) {
                return;
            }
            *at = Some(Instant::now());
        }
        let (base, stored, author_identity, ignored_authors, generation) = {
            let inner = lock_recover(&self.inner);
            let Some(resident) = inner.resident.as_ref() else { return };
            (
                resident.diff_base.clone(),
                resident.scope_identity.clone(),
                resident.author_filter.as_ref().map(|f| (f.head_identity(), f.mailmap_fp())),
                resident.ignored_authors.clone(),
                inner.generation,
            )
        };

        if let Some(base) = base {
            if let Ok(identity) = vcs::scope_ref_identity(root, &base) {
                if stored.as_ref().is_some_and(|s| s != &identity) {
                    tracing::info!(
                        "scope refs moved without file events; rebuilding vendor-diff scope"
                    );
                    self.rescope_out_of_lock(root, &base);
                }
            }
        }

        if author_filter_rebuild_needed(
            &ignored_authors,
            author_identity.as_ref().map(|(head, mm)| (head.as_str(), *mm)),
            vcs::head_identity(root).ok().as_deref(),
            vcs::mailmap_fingerprint(root),
        ) {
            tracing::info!("attribution inputs moved; rebuilding the ignored-authors filter");
            // Rebuild outside the lock (repo open + ref resolve), publish
            // under a short one — unless the resident changed meanwhile (a
            // reload or drift apply bumped the generation): the fresh resident
            // carries or will recompute its own filter, and a stale publish
            // here could overwrite it.
            let filter = super::resident::build_author_filter(root, &ignored_authors);
            let mut inner = lock_recover(&self.inner);
            if inner.reload == ReloadState::Running || inner.generation != generation {
                return;
            }
            if let Some(resident) = inner.resident.as_mut() {
                resident.author_filter = filter;
            }
        }
    }

    pub(crate) fn force_rescan(&self) {
        let mut cache = lock_recover(&self.scan);
        let stale = cache.as_ref().is_none_or(|c| c.at.elapsed() >= FORCE_RESCAN_FLOOR);
        if stale {
            *cache = None;
            // Route the next poll through the scan even when the hub is healthy: the
            // event path would not re-observe an object the caller thinks it just added.
            self.force_scan.store(true, Ordering::SeqCst);
        }
    }

    pub(super) fn resubscribe_cursor(&self) {
        let Some(hub) = &self.change_hub else {
            return;
        };
        let mut slot = lock_recover(&self.hub_cursor);
        *slot = Some(match slot.take() {
            // The rebuild this precedes can fail, and a failed rebuild leaves the OLD
            // resident serving. Its outstanding reconcile has to survive the swap, or it
            // would be settled against a baseline that was never taken.
            Some(old) => hub.resubscribe(old),
            None => hub.subscribe(),
        });
    }

    fn config_file_paths(&self) -> std::collections::HashSet<PathBuf> {
        let Some(root) = self.workspace_root.as_deref() else {
            return std::collections::HashSet::new();
        };
        CONFIG_FILES
            .iter()
            .map(|name| {
                let path = root.join(name);
                path.canonicalize().unwrap_or(path)
            })
            .collect()
    }

    pub(super) fn drop_cursor(&self) {
        let Some(hub) = &self.change_hub else {
            return;
        };
        let mut slot = lock_recover(&self.hub_cursor);
        if let Some(old) = slot.take() {
            hub.unsubscribe(old);
        }
    }

    fn drain_delivered_paths(&self, hub: &WorkspaceChangeHub, root: &Path) -> DeliveredPaths {
        let cursor = *lock_recover(&self.hub_cursor);
        let Some(cursor) = cursor else {
            return DeliveredPaths::default();
        };
        let batch = hub.drain(cursor);
        *lock_recover(&self.hub_cursor) = Some(batch.cursor);
        // A demand to reconcile is not a retraction of the paths in the same batch: the hub
        // keeps its entries on every input but a channel overflow, and a path it handed over
        // was handed over whatever else it also asked for. The scan still runs — the entries
        // are a SUBSET of the truth, not all of it.
        if batch.rescan_required {
            self.poll_drift_via_scan(root);
        }
        let delivered = DeliveredPaths::of(&batch.entries);
        if !batch.entries.is_empty() {
            // The scan above compares against a cache that may predate this batch, so it can
            // leave a delivered path unapplied; applying the entries covers exactly those.
            self.apply_drained_entries(&batch.entries);
        }
        delivered
    }

    pub(super) fn reconcile_tick(&self) {
        let Some(root) = self.workspace_root.clone() else {
            return;
        };
        let Some(hub) = self.change_hub.clone() else {
            return;
        };
        if !matches!(self.status(), DiagnosticsStatus::Ready { .. }) {
            return;
        }

        // 1. Apply everything the hub delivered so far. Draining also clears this cursor's
        //    reconcile flag, so a degraded hub recovers once the scan below is clean. The
        //    delivered paths are remembered so they are not later mistaken for a miss.
        let mut delivered = self.drain_delivered_paths(&hub, &root);

        // A delivered structural change kicked a full rebuild that will re-baseline the
        // whole workspace and re-subscribe a fresh cursor; nothing more to reconcile here.
        if lock_recover(&self.inner).reload == ReloadState::Running {
            return;
        }

        #[cfg(test)]
        self.fire_reconcile_probe();

        // 2. Fresh scan: classify the drift the events did not cover (do not apply yet).
        *lock_recover(&self.scan) = None;
        let Some(scan) = self.throttled_scan(&root) else {
            return;
        };
        let (changes, config_changed) = {
            let inner = lock_recover(&self.inner);
            (classify_changes(&inner.stats, &scan.stats), inner.config_fp != scan.config_fp)
        };
        if changes.is_empty() && !config_changed {
            return;
        }

        #[cfg(test)]
        self.fire_post_scan_probe();

        // 3. A legitimate edit may have landed AFTER step 1's drain but DURING the scan
        //    above. Drain once more: the paths it now delivers were merely late, not missed.
        delivered.extend(self.drain_delivered_paths(&hub, &root));

        // 4. Apply the residual drift (the just-delivered paths now match and are skipped).
        self.apply_scan_drift(&changes, config_changed, &scan);

        // 5. Degrade only if a FILE change (bsl/xml) was genuinely undelivered. Config drift
        //    is already fully rebuilt above and is expected to reach the reconciler in nested
        //    layouts (the config file sits above the watched root), so it is not a miss.
        let missed = has_undelivered_drift(&changes, &delivered);
        if missed {
            tracing::warn!(
                "diagnostics reconciler found drift the change hub did not deliver; \
                 degrading to scan-on-read until the watcher recovers"
            );
            hub.degrade_external();
        }
    }

    #[cfg(test)]
    fn fire_reconcile_probe(&self) {
        let probe = lock_recover(&self.reconcile_probe).take();
        if let Some(probe) = probe {
            probe();
        }
    }

    #[cfg(test)]
    fn fire_post_scan_probe(&self) {
        let probe = lock_recover(&self.post_scan_probe).take();
        if let Some(probe) = probe {
            probe();
        }
    }

    #[cfg(test)]
    fn fire_pre_drain_probe(&self) {
        let probe = lock_recover(&self.pre_drain_probe).take();
        if let Some(probe) = probe {
            probe();
        }
    }

    pub(super) fn throttled_scan(&self, root: &Path) -> Option<OwnedScan> {
        let mut cache = lock_recover(&self.scan);
        if let Some(c) = cache.as_ref() {
            if c.at.elapsed() < self.drift_interval {
                return Some(OwnedScan {
                    stats: c.stats.clone(),
                    config_fp: c.config_fp,
                    baseline_epoch: c.baseline_epoch,
                });
            }
        }
        // Read BEFORE the walk, never after. Too old is safe — the snapshot is refused and
        // the next poll walks again; too new would let a snapshot that predates a baseline
        // move claim it was taken after one.
        let baseline_epoch = lock_recover(&self.inner).baseline_epoch;
        self.scan_count.fetch_add(1, Ordering::SeqCst);
        // One project load per scan: the stat universe and the config identity must
        // describe the same project state, mirroring how the build derives its
        // baseline — otherwise the comparison could pair one state's files with
        // another's topology and mask (or fabricate) drift.
        let config_files_fp = config_files_fingerprint(root);
        let project = crate::graph::input::ProjectSnapshot::load(root);
        let stats = crate::graph::scan::scan_stats_over_roots(&project.scan_roots);
        let config_fp = config_identity(config_files_fp, &project.configs);
        *cache =
            Some(ScanCache { at: Instant::now(), stats: stats.clone(), config_fp, baseline_epoch });
        Some(OwnedScan { stats, config_fp, baseline_epoch })
    }
}

/// What a tick's drains handed over, in the two shapes the hub can name it: exact paths,
/// and directories whose disappearance stands for descendants no drain can enumerate.
///
/// The distinction is not cosmetic. A vanished directory arrives as ONE entry while the scan
/// lists every file that was under it, so comparing by equality alone would call each of
/// those a miss — and a miss is answered by charging every consumer of the hub a full
/// reconcile.
#[derive(Default)]
pub(super) struct DeliveredPaths {
    exact: std::collections::HashSet<String>,
    subtrees: Vec<PathBuf>,
}

impl DeliveredPaths {
    fn of(entries: &[ChangeEntry]) -> Self {
        let mut delivered = Self::default();
        for entry in entries {
            // EVERY reported removal stands for what was under it. The hub names a vanished
            // path a subtree by the absence of an extension — the only thing knowable about
            // a path that is gone — so a directory called `Dir.v1` arrives as an ordinary
            // removal. Taking only the explicit kind turns its descendants, which the scan
            // does list, into a miss, and a miss costs every consumer a full reconcile.
            // A path that really was a file loses nothing by this: no scan path lies under
            // it, so it answers for itself alone anyway.
            if matches!(
                entry.kind,
                crate::change_hub::ChangeKind::SubtreeRemoved
                    | crate::change_hub::ChangeKind::MaybeRemoved
            ) {
                delivered.subtrees.push(entry.canonical.clone());
            }
            delivered.exact.insert(entry.canonical.to_string_lossy().into_owned());
        }
        delivered
    }

    fn extend(&mut self, other: Self) {
        self.exact.extend(other.exact);
        self.subtrees.extend(other.subtrees);
    }

    /// Whether this path was delivered by name.
    fn covers(&self, path: &str) -> bool {
        self.exact.contains(path)
    }

    /// Whether this path's DISAPPEARANCE was delivered — by name, or as part of a directory
    /// that vanished. Containment is by whole components, so `Dir` never answers for `Dir2`.
    ///
    /// Separate from [`Self::covers`] because an event carries a direction. "This directory
    /// is gone" accounts for the files that were under it and for nothing else: a file that
    /// appeared after the directory was recreated is a change nobody reported, and counting
    /// the removal as its delivery would hide a watcher that stopped seeing that subtree.
    fn covers_removal(&self, path: &str) -> bool {
        self.covers(path) || self.subtrees.iter().any(|dir| Path::new(path).starts_with(dir))
    }
}

/// Whether the scan found drift the hub never handed over — the question a degrade answers,
/// and the reason the two directions are not interchangeable.
///
/// A disappearance may be accounted for by the directory that vanished with it. An
/// appearance or an edit may not: the entry that reported the directory gone says nothing
/// about what showed up there afterwards, and treating it as delivery would silently forgive
/// a watcher that stopped seeing that subtree.
fn has_undelivered_drift(changes: &WorkspaceDiff, delivered: &DeliveredPaths) -> bool {
    changes.removed.iter().any(|p| !delivered.covers_removal(p))
        || changes.added.iter().chain(&changes.modified).any(|p| !delivered.covers(p))
}

pub(super) struct OwnedScan {
    pub(super) stats: Vec<FileStat>,
    pub(super) config_fp: u64,
    pub(super) baseline_epoch: u64,
}

pub(super) fn compute_freshness(inner: &Inner, scan: Option<&OwnedScan>) -> Freshness {
    let drifted = match scan {
        Some(s) => {
            inner.config_fp != s.config_fp || !classify_changes(&inner.stats, &s.stats).is_empty()
        }
        None => false,
    };
    Freshness {
        revision: inner.generation,
        stale: drifted || inner.reload == ReloadState::Running,
        reload: inner.reload.label(),
    }
}

/// The resident's "config drift" identity: the config-file stat fold PLUS the
/// extension-topology hash of `configs`. Folding the topology in covers changes no
/// config-file stat can see — an auto-discovered extension appearing or vanishing
/// re-shapes visibility with `bsl-analyzer.toml` untouched (or absent entirely).
pub(super) fn config_identity(
    config_files_fp: u64,
    configs: &ide::WorkspaceConfigsSnapshot,
) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    (config_files_fp, crate::graph::scan::topology_u64(configs)).hash(&mut hasher);
    hasher.finish()
}

/// [`config_identity`] against a freshly derived project state — the cheap
/// (config-file stat + config parse + extension discovery, no tree walk) probe
/// the post-publication re-check and the throttled scan use. The stat is taken
/// BEFORE the project load, pairing with how the build captures its baseline.
pub(super) fn config_identity_now(root: &Path) -> u64 {
    let config_files_fp = config_files_fingerprint(root);
    config_identity(config_files_fp, &crate::graph::input::ProjectSnapshot::load(root).configs)
}

/// Whether the ignored-authors filter must be (re)built. Fail-open logic:
/// a filter attributing against inputs that no longer exist or moved is
/// rebuilt (an unreadable repository then yields `None` = no suppression),
/// and a missing filter is retried once the repository becomes usable again.
fn author_filter_rebuild_needed(
    configured_authors: &[String],
    stored: Option<(&str, u64)>,
    live_head: Option<&str>,
    live_mailmap: Option<u64>,
) -> bool {
    if configured_authors.is_empty() {
        return false;
    }
    match (stored, live_head) {
        // Attribution inputs moved under an active filter.
        (Some((stored_head, stored_mm)), Some(live)) => {
            stored_head != live || live_mailmap != Some(stored_mm)
        }
        // Active filter over a repository that can no longer resolve HEAD:
        // rebuild (and fail open) rather than keep suppressing against
        // obsolete history.
        (Some(_), None) => true,
        // No filter was built but the repository is usable now — retry.
        (None, Some(_)) => true,
        (None, None) => false,
    }
}

pub(super) fn config_files_fingerprint(root: &Path) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::UNIX_EPOCH;

    let mut entries: Vec<(String, u64, u128)> = Vec::new();
    for name in CONFIG_FILES {
        let path = root.join(name);
        if let Ok(meta) = std::fs::metadata(&path) {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            entries.push((name.to_string(), meta.len(), mtime));
        }
    }
    entries.sort();
    let mut hasher = DefaultHasher::new();
    entries.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use crate::change_hub::ChangeEntry;
    use crate::diagnostics_state::ResidentOutcome;

    use super::super::drift::{
        author_filter_rebuild_needed, clamp_reconcile_interval, ScanCache, FORCE_RESCAN_FLOOR,
        MIN_RECONCILE_INTERVAL, RECONCILE_INTERVAL,
    };
    use super::super::test_support::*;

    #[test]
    fn author_filter_rebuild_decision_covers_every_transition() {
        let authors = vec!["Фирма 1С".to_string()];
        let none: &[String] = &[];

        // Not configured: never rebuild, whatever the repo state.
        assert!(!author_filter_rebuild_needed(none, None, Some("h1"), Some(1)));
        assert!(!author_filter_rebuild_needed(none, Some(("h1", 1)), None, None));

        // Steady state: same HEAD, same mailmap.
        assert!(!author_filter_rebuild_needed(&authors, Some(("h1", 1)), Some("h1"), Some(1)));
        // HEAD moved, mailmap edited, or mailmap unreadable → rebuild.
        assert!(author_filter_rebuild_needed(&authors, Some(("h1", 1)), Some("h2"), Some(1)));
        assert!(author_filter_rebuild_needed(&authors, Some(("h1", 1)), Some("h1"), Some(2)));
        assert!(author_filter_rebuild_needed(&authors, Some(("h1", 1)), Some("h1"), None));
        // Active filter over a repo that lost HEAD → rebuild (fails open).
        assert!(author_filter_rebuild_needed(&authors, Some(("h1", 1)), None, None));
        // Filter missing, repo became usable → retry; still broken → wait.
        assert!(author_filter_rebuild_needed(&authors, None, Some("h1"), Some(1)));
        assert!(!author_filter_rebuild_needed(&authors, None, None, None));
    }
    use super::*;
    use crate::change_hub::ChangeKind;
    use ide::DiagnosticsConfig;
    use std::fs;
    use std::sync::{Arc, Mutex};

    /// The resident's config identity must move on a `dependsOn`-only edit while the
    /// per-file stat channel stays silent — that identity is the only trigger a full
    /// rebuild (with re-derived closures) has for such a change.
    #[test]
    fn config_identity_tracks_a_depends_on_only_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        fs::create_dir_all(root.join("ext/a")).unwrap();
        fs::create_dir_all(root.join("ext/b")).unwrap();
        let config = |deps: &str| {
            format!(
                "[source]\nroot = \".\"\nextensions = [\n  \
                 {{ name = \"a\", path = \"ext/a\" }},\n  \
                 {{ name = \"b\", path = \"ext/b\"{deps} }},\n]\n"
            )
        };
        fs::write(root.join("bsl-analyzer.toml"), config("")).unwrap();
        let before = config_identity_now(root);

        fs::write(root.join("bsl-analyzer.toml"), config(", dependsOn = [\"a\"]")).unwrap();
        assert_ne!(
            before,
            config_identity_now(root),
            "the dependency edge must change the resident's config identity"
        );
    }

    /// With NO analyzer config file at all, an extension appearing through
    /// auto-discovery must still change the config identity: there is no
    /// config-file stat to observe, only the re-derived topology.
    #[test]
    fn config_identity_sees_an_auto_discovered_extension_without_any_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        let before = config_identity_now(root);

        write(root, "src/cfe/NewExt/Configuration.xml", "<Configuration/>");
        assert_ne!(
            before,
            config_identity_now(root),
            "discovery must reshape the config identity with zero config files"
        );
    }

    /// Editing `bsl-analyzer.toml` is structural drift: the resident fully reloads and
    /// re-derives its effective config, so a later `file`/`workspace` sees the new
    /// settings — the same single source LSP and CLI would pick up.
    #[test]
    fn config_edit_triggers_reload_with_new_config() {
        use ide::DiagnosticCode;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = false\n");

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0); // scan every read
        state.ensure_loading();
        wait_ready(&state);

        let typo0 = state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo));
        assert!(matches!(typo0, ResidentOutcome::Ready(true, _)), "initial toml disables Typo");

        // Flip the config; mtime/len change is what config drift keys on.
        std::thread::sleep(Duration::from_millis(10));
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = true\n");

        // A read sees config drift → full reload (off-thread); poll until it lands.
        let mut reloaded = false;
        for _ in 0..200 {
            if let ResidentOutcome::Ready(false, _) =
                state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo))
            {
                reloaded = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(reloaded, "config edit reloads the resident with the updated diagnostics config");
    }

    /// A disabled handle never loads and reads degrade to `Disabled`.
    #[test]
    fn disabled_handle_does_not_load() {
        let state = DiagnosticsState::disabled();
        state.ensure_loading();
        assert_eq!(state.status(), DiagnosticsStatus::Disabled);
        let out = state.read(|_, _| 1usize);
        assert!(matches!(out, ResidentOutcome::Disabled));
    }

    /// Editing a `.bsl` body drifts the workspace; the next read applies an
    /// incremental `set_file_text` and bumps the generation, with no full rebuild.
    #[test]
    fn incremental_reload_on_body_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0); // scan every read
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();

        // Modify the body; mtime/len change is what the drift scan keys on.
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            module_path(root, "Сервер"),
            "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции\n",
        )
        .unwrap();

        // A read triggers drift handling; the edited text must be resident afterwards.
        let _ = state.read(|_, _| ());
        // Give a beat in case the apply raced; then re-read.
        for _ in 0..50 {
            if state.generation() > gen0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            let _ = state.read(|_, _| ());
        }
        assert!(state.generation() > gen0, "incremental apply should bump the generation");
        assert!(
            matches!(state.status(), DiagnosticsStatus::Ready { .. }),
            "incremental apply stays Ready, no rebuild churn"
        );

        let text = state.read(|resident, _gen| {
            let file_id = resident.file_id_for(&module_path(root, "Сервер")).unwrap();
            resident.analysis().file_text(file_id)
        });
        match text {
            ResidentOutcome::Ready(t, _) => {
                assert!(t.contains("Возврат 1"), "edited text resident")
            }
            _ => panic!("expected Ready"),
        }
    }

    /// A brand-new common module (descriptor + body) registers into the live resident:
    /// no rebuild (pre-existing FileIds stay stable — a re-enumeration would shift them,
    /// the new name sorts first), and the substrate lists the module (its module-level
    /// diagnostic fires, which requires the `module_file` back-link).
    #[test]
    fn incremental_add_of_new_common_module() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();
        let existing = module_path(root, "Сервер");
        let existing_id_before = match state.read(|r, _| r.file_id_for(&existing)) {
            ResidentOutcome::Ready(Some(id), _) => id,
            _ => panic!("existing module resolves before the add"),
        };

        // The name sorts before "Сервер", so a full re-enumeration would renumber it.
        std::thread::sleep(Duration::from_millis(10));
        write_common_module(root, "ААльфа", true, "Процедура Внутренняя()\nКонецПроцедуры");

        assert!(wait_for_apply(&state, gen0), "the add applies in place");
        assert!(
            matches!(state.status(), DiagnosticsStatus::Ready { .. }),
            "incremental add stays Ready"
        );

        let added = module_path(root, "ААльфа");
        let out = state.read(|resident, _| {
            let existing_id_after =
                resident.file_id_for(&existing).expect("existing module still resolves");
            let new_id = resident.file_id_for(&added).expect("new module resolves");
            let findings = resident.analysis().diagnostics(new_id, &DiagnosticsConfig::default());
            (existing_id_after, findings)
        });
        let ResidentOutcome::Ready((existing_id_after, findings), _) = out else {
            panic!("expected Ready")
        };
        assert_eq!(
            existing_id_after, existing_id_before,
            "pre-existing FileIds survive an incremental add (a rebuild would renumber)"
        );
        assert!(
            findings.iter().any(|d| d.code.as_str() == "CommonModuleMissingAPI"),
            "the module-level diagnostic fires — the substrate listed the new module: {:?}",
            findings.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
        );
    }

    /// A new body with no metadata descriptor still registers (readable, findings served);
    /// it just carries no substrate listing until its `.xml` lands.
    #[test]
    fn incremental_add_of_bare_body_without_descriptor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();

        std::thread::sleep(Duration::from_millis(10));
        let body = module_path(root, "БезОписания");
        fs::create_dir_all(body.parent().unwrap()).unwrap();
        fs::write(&body, "Процедура Тест()\n    Перем Неиспользуемая;\nКонецПроцедуры").unwrap();

        assert!(wait_for_apply(&state, gen0), "the bare add applies in place");
        let out = state.read(|resident, _| {
            let id = resident.file_id_for(&body).expect("bare body resolves");
            resident.analysis().diagnostics(id, &DiagnosticsConfig::default()).len()
        });
        assert!(matches!(out, ResidentOutcome::Ready(n, _) if n > 0), "findings served");
    }

    /// A deleted body unregisters in place: the path stops resolving, the survivor keeps
    /// its FileId, and the state stays Ready without a rebuild.
    #[test]
    fn incremental_remove_of_module_body() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write_common_module(root, "Удаляемый", true, "Процедура У()\nКонецПроцедуры");

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();
        let survivor = module_path(root, "Сервер");
        let survivor_id_before = match state.read(|r, _| r.file_id_for(&survivor)) {
            ResidentOutcome::Ready(Some(id), _) => id,
            _ => panic!("survivor resolves before the removal"),
        };

        std::thread::sleep(Duration::from_millis(10));
        let doomed = module_path(root, "Удаляемый");
        fs::remove_file(&doomed).unwrap();

        assert!(wait_for_apply(&state, gen0), "the removal applies in place");
        assert!(
            matches!(state.status(), DiagnosticsStatus::Ready { .. }),
            "incremental removal stays Ready"
        );
        let out = state
            .read(|resident, _| (resident.file_id_for(&doomed), resident.file_id_for(&survivor)));
        let ResidentOutcome::Ready((gone, kept), _) = out else { panic!("expected Ready") };
        assert!(gone.is_none(), "the removed body no longer resolves");
        assert_eq!(kept, Some(survivor_id_before), "the survivor keeps its FileId");
    }

    /// Idle eviction drops the resident db back to `Idle` after the quiet period, and
    /// a later read rebuilds it.
    #[test]
    fn idle_eviction_drops_and_rebuilds() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.eviction_after = Duration::from_millis(50);
        state.ensure_loading();
        wait_ready(&state);

        // No reads for longer than the eviction window → sweeper drops it.
        for _ in 0..300 {
            if state.status() == DiagnosticsStatus::Idle {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(state.status(), DiagnosticsStatus::Idle, "resident evicted after idle");

        // A later use rebuilds.
        state.ensure_loading();
        wait_ready(&state);
        assert!(matches!(state.status(), DiagnosticsStatus::Ready { .. }));
    }

    /// `status_report` reflects the lifecycle: `idle` before load, `ready` with the file
    /// count and a bumped generation after, and `reload = none` when not reloading.
    #[test]
    fn status_report_tracks_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        let before = state.status_report();
        assert_eq!(before.state, "idle");
        assert_eq!(before.generation, 0);
        assert_eq!(before.files, None);

        state.ensure_loading();
        wait_ready(&state);

        let after = state.status_report();
        assert_eq!(after.state, "ready");
        assert!(after.generation >= 1, "generation bumped on build");
        assert_eq!(after.files, Some(1), "one resident .bsl");
        assert_eq!(after.reload, "none");
        assert!(after.error.is_none());
        // elapsed_ms is cleared once ready.
        assert!(after.elapsed_ms.is_none());
    }

    /// The production `catch_build` fold: an `Ok` build passes through, an `Err` becomes a
    /// message, and a PANIC is folded into `Err` (so the caller publishes `Failed` instead
    /// of leaving a dead thread with the status pinned at `Loading`).
    #[test]
    fn catch_build_folds_ok_err_and_panic() {
        let ok: Result<i32, String> = DiagnosticsState::catch_build(|| Ok(42));
        assert_eq!(ok, Ok(42));

        let err = DiagnosticsState::catch_build(|| -> anyhow::Result<i32> {
            anyhow::bail!("plain build error")
        });
        assert_eq!(err, Err("plain build error".to_owned()));

        let panicked = DiagnosticsState::catch_build(|| -> anyhow::Result<i32> {
            panic!("synthetic build panic")
        });
        let msg = panicked.unwrap_err();
        assert!(msg.contains("panicked") && msg.contains("synthetic build panic"), "{msg}");
    }

    /// End-to-end: a loader that publishes via `catch_build`'s `Err` path lands in
    /// `Failed` with `loading_since` cleared (no stale `elapsed_ms`), never stuck `Loading`.
    #[test]
    fn failed_build_clears_loading_since_and_is_visible() {
        let err = DiagnosticsState::catch_build(|| -> anyhow::Result<()> { panic!("boom") });
        // Simulate run_load's Err arm publishing the failure.
        let state = DiagnosticsState::for_workspace(std::env::temp_dir());
        {
            let mut inner = lock_recover(&state.inner);
            inner.loading_since = Some(Instant::now());
            inner.status = DiagnosticsStatus::Loading;
            // The exact publication run_load performs on Err.
            inner.loading_since = None;
            inner.status = DiagnosticsStatus::Failed(err.unwrap_err());
        }
        let report = state.status_report();
        assert_eq!(report.state, "failed");
        assert!(report.error.as_deref().unwrap().contains("boom"));
        assert!(report.elapsed_ms.is_none(), "loading_since cleared on failure");
    }

    /// Adding a metadata `.xml` point-refreshes the substrate in place: the new object
    /// resolves without a full db rebuild (no reload kicked), and the generation bumps.
    #[test]
    fn xml_add_point_refreshes_substrate_without_full_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();

        let module = module_path(root, "Сервер");
        assert!(!catalog_resolves(&state, &module), "catalog absent before the add");

        std::thread::sleep(Duration::from_millis(10));
        write_catalog(root, "Товары", 9);

        assert!(catalog_resolves(&state, &module), "added catalog resolves after point-refresh");
        assert_eq!(
            state.status_report().reload,
            "none",
            "no full rebuild was kicked for an XML add"
        );
        assert!(state.generation() > gen0, "the point-refresh bumps the generation");
        assert!(matches!(state.status(), DiagnosticsStatus::Ready { .. }), "stays Ready, no churn");
    }

    /// Removing a metadata `.xml` tombstones the object through a point-refresh — it no
    /// longer resolves — with no full rebuild.
    #[test]
    fn xml_remove_point_refreshes_substrate_without_full_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write_catalog(root, "Товары", 9);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();

        let module = module_path(root, "Сервер");
        assert!(catalog_resolves(&state, &module), "catalog present before the remove");

        std::thread::sleep(Duration::from_millis(10));
        std::fs::remove_file(root.join("Catalogs/Товары.xml")).unwrap();

        assert!(
            !catalog_resolves(&state, &module),
            "removed catalog tombstoned after point-refresh"
        );
        assert_eq!(
            state.status_report().reload,
            "none",
            "no full rebuild was kicked for an XML remove"
        );
        assert!(state.generation() > gen0, "the point-refresh bumps the generation");
    }

    /// Editing a metadata `.xml` re-reads only that object; the resident stays in place
    /// (no full rebuild) and its diagnostics equal a cold build over the mutated tree.
    #[test]
    fn xml_edit_point_refreshes_and_matches_fresh_build() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write_catalog(root, "Товары", 9);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();

        let module = module_path(root, "Сервер");
        std::thread::sleep(Duration::from_millis(10));
        write_catalog(root, "Товары", 12); // content edit → the object's revision moves

        // A read triggers the synchronous point-refresh.
        let _ = state.read(|_, _| ());
        assert_eq!(
            state.status_report().reload,
            "none",
            "no full rebuild was kicked for an XML edit"
        );
        assert!(state.generation() > gen0, "the edit is detected and applied in place");

        // A cold resident over the same on-disk tree must agree diagnostic-for-diagnostic.
        let fresh = DiagnosticsState::for_workspace(root.to_path_buf());
        fresh.ensure_loading();
        wait_ready(&fresh);
        assert_eq!(
            module_diag_fingerprint(&state, &module),
            module_diag_fingerprint(&fresh, &module),
            "point-refreshed diagnostics must equal a cold build over the mutated tree"
        );
    }

    /// An analyzer-config edit is NOT a metadata point-refresh: it still forces a full
    /// rebuild, re-deriving the effective config (something only a rebuild does). Proven
    /// by the resident picking up the flipped `Typo` setting after the edit.
    #[test]
    fn analyzer_config_edit_still_full_rebuilds() {
        use ide::DiagnosticCode;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = false\n");

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let disabled0 = state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo));
        assert!(matches!(disabled0, ResidentOutcome::Ready(true, _)), "initial toml disables Typo");

        std::thread::sleep(Duration::from_millis(10));
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = true\n");

        // The reload runs off-thread; poll until the re-derived config lands.
        let mut reloaded = false;
        for _ in 0..200 {
            if let ResidentOutcome::Ready(false, _) =
                state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo))
            {
                reloaded = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(reloaded, "a config edit full-rebuilds and re-derives the effective config");
    }

    /// A metadata `.xml` that `discover_*` does NOT enroll as a composing file
    /// (`Configuration.xml` here — a whole-config `load_from_directory` would re-read it)
    /// must still invalidate the coarse Channel-2 `load_configuration` memo via an
    /// unconditional config-revision bump, without a full rebuild. Observed directly
    /// through the config-root revision the `load_configuration` query keys on.
    #[test]
    fn non_enrolled_xml_edit_bumps_channel2_without_full_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(root, "Configuration.xml", "<Configuration><Name>Конфа</Name></Configuration>");

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);

        let module = module_path(root, "Сервер");
        let rev0 = state.read(|r, _| r.db.config_root_revision_for_path(&module));
        let ResidentOutcome::Ready(rev0, _) = rev0 else { panic!("expected Ready") };

        std::thread::sleep(Duration::from_millis(10));
        write(root, "Configuration.xml", "<Configuration><Name>Другая</Name></Configuration>");

        // A read triggers the synchronous point-refresh; the non-enrolled edit still bumps
        // the config revision even though no per-MDO composing file moved.
        let rev1 = state.read(|r, _| r.db.config_root_revision_for_path(&module));
        let ResidentOutcome::Ready(rev1, _) = rev1 else { panic!("expected Ready") };

        assert_eq!(
            state.status_report().reload,
            "none",
            "a non-enrolled XML edit is a point-refresh, not a full rebuild"
        );
        assert!(rev1 > rev0, "the config-root revision the Channel-2 memo keys on must bump");
    }

    /// A metadata subtree that is a symlink to a directory OUTSIDE the config root: the
    /// canonical (scan) path of its XML resolves outside the root, so the point-refresh
    /// cannot express the drift. Editing such an XML must still reach the resident — the
    /// pre-classification routes it to a full rebuild instead of silently forgetting it.
    #[cfg(unix)]
    #[test]
    fn symlinked_subtree_outside_root_xml_edit_is_not_lost() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let root = base.join("ws");
        std::fs::create_dir_all(&root).unwrap();
        // Real common-module content lives OUTSIDE the workspace root, reached only via a
        // symlinked `CommonModules` directory.
        let real = base.join("real");
        write_common_module(&real, "Сервер", true, "&НаСервере\nФункция Ч() Экспорт КонецФункции");
        std::os::unix::fs::symlink(real.join("CommonModules"), root.join("CommonModules")).unwrap();

        let mut state = DiagnosticsState::for_workspace(root.clone());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);

        let module = root.join("CommonModules/Сервер/Ext/Module.bsl");
        assert_eq!(module_is_server(&state, &module), Some(true), "starts server-side");

        // Flip Server→false via the descriptor XML only (no body edit). Its canonical path
        // is outside the root, so the point-refresh cannot own it.
        std::thread::sleep(Duration::from_millis(10));
        write_common_module_xml(&real, "Сервер", false);

        // The full rebuild is async; poll until the edit lands.
        let mut flipped = false;
        for _ in 0..300 {
            let _ = state.read(|_, _| ());
            if module_is_server(&state, &module) == Some(false) {
                flipped = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(flipped, "an XML edit under a symlinked-outside subtree must not be lost");
    }

    /// A metadata subtree that is a symlink to another directory INSIDE the config root:
    /// the canonical path stays under the root (so the point-refresh owns it), but the
    /// discovery join keeps the symlink unresolved. Editing the XML must re-read the file
    /// in place (via `enroll_refresh`'s canonicalise-on-miss) — a point-refresh, not a
    /// full rebuild.
    #[cfg(unix)]
    #[test]
    fn symlinked_subtree_inside_root_xml_edit_point_refreshes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Real modules live under `root/RealCM`; `root/CommonModules` is a symlink to it —
        // both inside the root, so canonical paths stay under the root.
        let realcm = root.join("RealCM");
        std::fs::create_dir_all(&realcm).unwrap();
        write_common_module(
            &realcm,
            "Сервер",
            true,
            "&НаСервере\nФункция Ч() Экспорт КонецФункции",
        );
        std::os::unix::fs::symlink(realcm.join("CommonModules"), root.join("CommonModules"))
            .unwrap();

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);

        let module = root.join("CommonModules/Сервер/Ext/Module.bsl");
        assert_eq!(module_is_server(&state, &module), Some(true), "starts server-side");

        std::thread::sleep(Duration::from_millis(10));
        write_common_module_xml(&realcm, "Сервер", false);

        // A read triggers the synchronous point-refresh; no full rebuild.
        let _ = state.read(|_, _| ());
        assert_eq!(
            state.status_report().reload,
            "none",
            "an in-root symlinked XML edit is a point-refresh, not a full rebuild"
        );
        assert_eq!(
            module_is_server(&state, &module),
            Some(false),
            "enroll_refresh must re-read the edited XML through the canonicalise-on-miss path"
        );
    }

    /// The `metadata object` tool path (`object_from_db` over the resident substrate) sees
    /// a newly-added catalog through the point-refresh — no full db rebuild, generation
    /// bumped — and never loads the whole configuration (the resolver is substrate-only).
    #[test]
    fn metadata_object_finds_added_catalog_without_full_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();

        let found = |state: &DiagnosticsState| {
            let out = state.read(|r, _| {
                crate::tools::metadata::object_from_db(r.db(), "Catalog", "Товары").is_ok()
            });
            match out {
                ResidentOutcome::Ready(v, _) => v,
                _ => panic!("expected Ready"),
            }
        };

        assert!(!found(&state), "catalog absent before the add");

        std::thread::sleep(Duration::from_millis(10));
        write_catalog(root, "Товары", 9);

        assert!(found(&state), "the metadata object tool finds the added catalog");
        assert_eq!(state.status_report().reload, "none", "no full db rebuild for an object add");
        assert!(state.generation() > gen0, "the point-refresh bumped the generation");
    }

    /// The idle-eviction contract for metadata reads: after the resident is evicted, a read
    /// re-triggers the build and degrades to a "loading" outcome (or Ready once rebuilt) —
    /// NEVER a hard `Disabled`/`Failed` error. The tool maps `Loading` to a retry envelope.
    #[test]
    fn metadata_read_after_eviction_is_loading_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.eviction_after = Duration::from_millis(50);
        state.ensure_loading();
        wait_ready(&state);

        for _ in 0..300 {
            if state.status() == DiagnosticsStatus::Idle {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(state.status(), DiagnosticsStatus::Idle, "resident evicted after idle");

        // A metadata read after eviction: re-trigger the build and read. It is Loading (still
        // rebuilding) or Ready (rebuilt fast) — the tool turns Loading into a retry envelope,
        // never surfacing a hard "not loaded" error.
        state.ensure_loading();
        let out = state
            .read(|r, _| crate::tools::metadata::object_from_db(r.db(), "Catalog", "X").is_ok());
        assert!(
            matches!(out, ResidentOutcome::Loading | ResidentOutcome::Ready(_, _)),
            "an evicted metadata read must be loading or ready, never a hard error",
        );
    }

    /// The `metadata object` miss retry drops the throttle cache to force a re-scan, but
    /// only when the last scan is older than [`FORCE_RESCAN_FLOOR`] — so a loop of
    /// genuinely-absent lookups cannot stat-walk the workspace faster than that floor
    /// (the retired MetadataCache's storm guard). Exercised with a synthetic past
    /// `Instant`, so it is deterministic and needs no real sleep.
    #[test]
    fn force_rescan_is_storm_guarded_by_the_floor() {
        let state = DiagnosticsState::for_workspace(std::env::temp_dir());

        // A fresh scan (just now) must NOT be force-cleared — the storm guard.
        *lock_recover(&state.scan) = Some(ScanCache {
            at: Instant::now(),
            stats: Vec::new(),
            config_fp: 0,
            baseline_epoch: 0,
        });
        state.force_rescan();
        assert!(
            lock_recover(&state.scan).is_some(),
            "a scan within the floor is kept, so repeated misses cannot hammer the FS",
        );

        // A scan older than the floor IS cleared, so the next read re-scans and can pick up
        // a just-added object.
        let stale_at = Instant::now()
            .checked_sub(FORCE_RESCAN_FLOOR + Duration::from_millis(50))
            .expect("a valid past instant");
        *lock_recover(&state.scan) =
            Some(ScanCache { at: stale_at, stats: Vec::new(), config_fp: 0, baseline_epoch: 0 });
        state.force_rescan();
        assert!(
            lock_recover(&state.scan).is_none(),
            "a scan older than the floor is force-cleared so the retry re-scans",
        );
    }

    // --- Event-driven drift (W2): the change hub feeds a drain-on-read path. ---

    /// A `.bsl` body edit reaches the resident through the hub drain, and the healthy hot
    /// path performs NO workspace scan — the whole point of the event-driven path.
    #[test]
    fn event_driven_body_edit_lands_via_drain_without_scan() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);
        assert_eq!(state.scan_count(), 0, "the cold build does not go through the throttled scan");

        let gen0 = raw_generation(&state);
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            module_path(root, "Сервер"),
            "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции\n",
        )
        .unwrap();

        assert!(wait_for_apply(&state, gen0), "the body edit must be applied via drain");
        assert_eq!(state.scan_count(), 0, "the event-driven hot path performs no scan");

        let text = state.read(|resident, _| {
            let fid = resident.file_id_for(&module_path(root, "Сервер")).unwrap();
            resident.analysis().file_text(fid)
        });
        match text {
            ResidentOutcome::Ready(t, _) => {
                assert!(t.contains("Возврат 1"), "edited text resident")
            }
            _ => panic!("expected Ready"),
        }
    }

    /// A metadata `.xml` edit is delivered through the drain and point-refreshes the
    /// substrate in place (no full rebuild), again with no scan on the hot path.
    #[test]
    fn event_driven_xml_edit_lands_via_drain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let gen0 = raw_generation(&state);
        std::thread::sleep(Duration::from_millis(10));
        // Flip the common module's server flag: a pure `.xml` edit (no body change).
        write_common_module_xml(root, "Сервер", false);

        assert!(wait_for_apply(&state, gen0), "the xml edit must be applied via drain");
        assert_eq!(
            state.status_report().reload,
            "none",
            "an xml edit is a point-refresh, not a full rebuild"
        );
        assert_eq!(state.scan_count(), 0, "the event-driven hot path performs no scan");
    }

    /// A degraded hub falls back to exactly today's throttled scan path: the edit is still
    /// applied, but through a scan (parity with the pre-hub behaviour).
    #[test]
    fn degraded_hub_reconciles_via_scan_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);
        // Force the scan fallback.
        hub.degrade_external();
        assert!(matches!(hub.health(), Health::Degraded(_)));

        let gen0 = raw_generation(&state);
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            module_path(root, "Сервер"),
            "&НаСервере\nФункция Считать() Экспорт Возврат 2; КонецФункции\n",
        )
        .unwrap();

        assert!(wait_for_apply(&state, gen0), "a degraded hub still applies the edit via scan");
        assert!(state.scan_count() > 0, "the degraded path uses the scan, matching today");
    }

    /// A consumer that stopped draining owes its own reconcile, and it used to be charged
    /// to everyone: the shared verdict stayed degraded for as long as that cursor was
    /// silent, so these diagnostics answered every read with a full workspace scan for the
    /// rest of the daemon's life.
    ///
    /// The counter covers BOTH health questions on this path — the drain/scan choice and
    /// the freshness fallback — because either one scanning shows up in it. An
    /// implementation that moved only one of them off the shared verdict fails here.
    #[test]
    fn a_foreign_cursors_debt_does_not_cost_diagnostics_a_scan() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        // A stranger subscribes and never drains; everyone is asked to reconcile. These
        // diagnostics answer for THEMSELVES — that debt is genuinely theirs — and the
        // stranger's stays outstanding for ever after.
        let _stranger = hub.subscribe();
        hub.degrade_external();
        // Paid through the reconcile tick, not through a read: on the scan path a read
        // never drains, so the debt would otherwise outlive the reason for it.
        state.reconcile_tick();

        let scans = state.scan_count();
        let _ = state.read(|_resident, _| ());
        assert_eq!(
            state.scan_count(),
            scans,
            "somebody else's outstanding reconcile is not these diagnostics' to pay for"
        );
    }

    /// The other half, and the one that keeps the first honest: a hub that cannot deliver
    /// at all leaves nothing to trust, however clean this consumer's own cursor is.
    /// Without this leg, an unconditional fast path passes the test above while quietly
    /// serving whatever the event stream never delivered.
    #[test]
    fn a_hub_that_cannot_deliver_still_sends_diagnostics_to_a_scan() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let hub = WorkspaceChangeHub::start_with_unstartable_thread(vec![
            crate::change_hub::WatchTarget::recursive(root.to_path_buf()),
        ]);
        let mut state = DiagnosticsState::for_workspace(root.to_path_buf()).with_change_hub(hub);
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);

        let scans = state.scan_count();
        let _ = state.read(|_resident, _| ());
        assert!(
            state.scan_count() > scans,
            "a hub that will never deliver leaves the diagnostics nothing to trust"
        );
    }

    /// The reconciler/watchdog: a change the event stream failed to deliver (simulated by
    /// draining the cursor without applying) is caught by the periodic scan, which applies
    /// the drift AND degrades the hub so reads revert to scanning until it recovers.
    #[test]
    fn reconciler_catches_undelivered_drift_and_degrades() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let mut observer = hub.subscribe();
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            module_path(root, "Сервер"),
            "&НаСервере\nФункция Считать() Экспорт Возврат 3; КонецФункции\n",
        )
        .unwrap();
        // Confirm the hub delivered the change (so the diagnostics cursor has it too)...
        assert!(wait_for_delivery(&hub, &mut observer, "Module.bsl"), "hub delivered the edit");
        // ...then simulate a lossy sink dropping it: consume the cursor without applying.
        state.drain_and_discard_cursor();

        let gen0 = raw_generation(&state);
        assert_eq!(hub.health(), Health::Healthy, "still healthy before the reconcile");
        state.reconcile_tick();

        assert!(raw_generation(&state) > gen0, "the reconciler applied the missed drift");
        assert_eq!(
            hub.health().label(),
            "degraded:reconcile-miss",
            "a delivered-but-undrained miss degrades the hub to the scan fallback",
        );
    }

    /// An analyzer-config edit delivered through the drain is structural: it forces a full
    /// rebuild that re-derives the effective config, exactly like the scan path.
    #[test]
    fn event_driven_config_edit_full_rebuilds() {
        use ide::DiagnosticCode;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = false\n");

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);
        let disabled0 = state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo));
        assert!(matches!(disabled0, ResidentOutcome::Ready(true, _)), "initial toml disables Typo");

        std::thread::sleep(Duration::from_millis(10));
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = true\n");

        let mut reloaded = false;
        for _ in 0..300 {
            if let ResidentOutcome::Ready(false, _) =
                state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo))
            {
                reloaded = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(reloaded, "a config edit via drain full-rebuilds and re-derives the config");
    }

    /// After a full rebuild the cursor is re-subscribed, so a change landing AFTER the
    /// rebuild is applied to the fresh resident (the drain path survives a rebuild).
    #[test]
    fn events_after_rebuild_apply_to_the_new_resident() {
        use ide::DiagnosticCode;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = false\n");

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        // Force a full rebuild via a config edit and wait for the fresh resident.
        std::thread::sleep(Duration::from_millis(10));
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = true\n");
        let mut reloaded = false;
        for _ in 0..300 {
            if let ResidentOutcome::Ready(false, _) =
                state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo))
            {
                reloaded = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(reloaded, "the config rebuild completed");

        // A body edit AFTER the rebuild must reach the freshly-built resident via drain.
        let gen0 = raw_generation(&state);
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            module_path(root, "Сервер"),
            "&НаСервере\nФункция Считать() Экспорт Возврат 42; КонецФункции\n",
        )
        .unwrap();
        assert!(wait_for_apply(&state, gen0), "post-rebuild edits apply to the new resident");
        let text = state.read(|r, _| {
            let fid = r.file_id_for(&module_path(root, "Сервер")).unwrap();
            r.analysis().file_text(fid)
        });
        match text {
            ResidentOutcome::Ready(t, _) => {
                assert!(t.contains("Возврат 42"), "new resident edited")
            }
            _ => panic!("expected Ready"),
        }
    }

    /// A rebuild starts by taking a fresh cursor, and the rebuild can fail — in which case
    /// the OLD resident keeps serving requests. So an outstanding reconcile has to survive
    /// the swap: settling it against a baseline that was never taken would leave the stale
    /// resident looking healthy, with the events it missed already reclaimed.
    #[test]
    fn a_rebuild_that_takes_a_fresh_cursor_keeps_the_reconcile_it_owed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);
        assert!(state.has_hub_cursor(), "a built resident holds a cursor");

        hub.degrade_external();
        state.resubscribe_cursor();

        let cursor = lock_recover(&state.hub_cursor).expect("the resident still holds a cursor");
        assert!(
            hub.drain(cursor).rescan_required,
            "the debt belongs to the resident, not to the cursor it happened to hold",
        );
    }

    /// Idle eviction releases the hub cursor so an evicted resident does not pin the
    /// accumulator against reclamation.
    #[test]
    fn eviction_releases_hub_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (mut state, _hub) = state_with_hub(root);
        state.eviction_after = Duration::from_millis(50);
        state.ensure_loading();
        wait_ready(&state);
        assert!(state.has_hub_cursor(), "a built resident holds a cursor");

        for _ in 0..300 {
            if state.status() == DiagnosticsStatus::Idle {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(state.status(), DiagnosticsStatus::Idle, "resident evicted after idle");
        assert!(!state.has_hub_cursor(), "eviction drops the cursor");
    }

    /// `status_report` surfaces the hub view so an agent can tell an event-driven serve
    /// from a scan fallback.
    #[test]
    fn status_report_exposes_watch_mode() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let watch = state.status_report().watch.expect("a hub-backed profile reports watch");
        assert_eq!(watch.mode, "event-driven");
        assert_eq!(watch.health, "healthy");

        hub.degrade_external();
        let watch = state.status_report().watch.expect("watch report present");
        assert_eq!(watch.mode, "scan-fallback", "a degraded hub reports the scan fallback");
    }

    /// An edit that lands WHILE a full rebuild is in flight must not be dropped: the drain
    /// leaves it pending (rather than draining-then-bailing on the reload) and applies it to
    /// the fresh resident once the rebuild finishes — without waiting for the reconciler.
    /// With the old drain-before-reload-check order this test fails (the edit is lost).
    #[test]
    fn edit_during_rebuild_applies_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        // Simulate a full rebuild in flight.
        lock_recover(&state.inner).reload = ReloadState::Running;

        let mut observer = hub.subscribe();
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            module_path(root, "Сервер"),
            "&НаСервере\nФункция Считать() Экспорт Возврат 11; КонецФункции\n",
        )
        .unwrap();
        assert!(wait_for_delivery(&hub, &mut observer, "Module.bsl"), "hub delivered the edit");

        // A read during the rebuild must NOT drain/apply (else the edit is lost).
        let gen0 = raw_generation(&state);
        let _ = state.read(|_, _| ());
        assert_eq!(raw_generation(&state), gen0, "no apply while a rebuild is in flight");

        // The rebuild finishes.
        lock_recover(&state.inner).reload = ReloadState::Idle;

        // The still-pending edit now applies to the (current) resident on the next read.
        assert!(wait_for_apply(&state, gen0), "the pending edit applies once the rebuild ends");
        let text = state.read(|r, _| {
            let fid = r.file_id_for(&module_path(root, "Сервер")).unwrap();
            r.analysis().file_text(fid)
        });
        match text {
            ResidentOutcome::Ready(t, _) => assert!(t.contains("Возврат 11"), "edit applied"),
            _ => panic!("expected Ready"),
        }
    }

    /// A legitimate edit that lands DURING the reconciler's scan (delivered to the cursor,
    /// just after its first drain) must NOT be counted as a lossy-backend miss: the second
    /// drain covers it, so the hub stays Healthy. With the old single-drain reconciler this
    /// fails (the scan sees drift and degrades).
    #[test]
    fn reconciler_does_not_degrade_a_late_delivered_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        // The probe fires between the reconciler's first drain and its scan: it writes the
        // edit and waits for the hub to deliver it into the accumulator (so the diagnostics
        // cursor holds it for the reconciler's second drain).
        let probe_root = root.to_path_buf();
        let probe_hub = hub.clone();
        state.set_reconcile_probe(move || {
            fs::write(
                probe_root.join("CommonModules/Сервер/Ext/Module.bsl"),
                "&НаСервере\nФункция Считать() Экспорт Возврат 13; КонецФункции\n",
            )
            .unwrap();
            let mut obs = probe_hub.subscribe();
            for _ in 0..300 {
                let batch = probe_hub.drain(obs);
                obs = batch.cursor;
                if batch.entries.iter().any(|e| e.raw.to_string_lossy().contains("Module.bsl")) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });

        let gen0 = raw_generation(&state);
        state.reconcile_tick();

        assert!(raw_generation(&state) > gen0, "the late edit is still applied");
        assert_eq!(
            hub.health(),
            Health::Healthy,
            "an edit delivered during the scan is not a miss and must not degrade",
        );
    }

    /// The containment rule the miss check leans on, pinned where it lives: an end-to-end
    /// deletion cannot pin it, because a directory removed through the filesystem also
    /// delivers an event per file, and those exact paths would answer for the descendants
    /// even if containment did nothing.
    mod delivered_coverage {
        use super::*;

        fn subtree_removal(path: &str) -> ChangeEntry {
            ChangeEntry {
                canonical: PathBuf::from(path),
                raw: PathBuf::from(path),
                kind: ChangeKind::SubtreeRemoved,
                seq: 1,
            }
        }

        #[test]
        fn a_vanished_directory_answers_for_its_descendants() {
            let delivered = DeliveredPaths::of(&[subtree_removal("/w/Dir")]);
            assert!(delivered.covers_removal("/w/Dir/A.bsl"));
            assert!(delivered.covers_removal("/w/Dir/Deep/B.bsl"));
            assert!(delivered.covers_removal("/w/Dir"), "and for itself");
        }

        #[test]
        fn a_namesake_directory_is_not_covered() {
            let delivered = DeliveredPaths::of(&[subtree_removal("/w/Dir")]);
            assert!(!delivered.covers_removal("/w/Dir2/A.bsl"));
        }

        /// The hub cannot tell a vanished directory from a vanished file when the name has a
        /// dot in it, so `Dir.v1` arrives as an ordinary removal while the scan still lists
        /// every file that was inside. Counting only the explicit kind would call all of
        /// them missed and charge every consumer of the hub a reconcile for it.
        #[test]
        fn a_dotted_directory_still_answers_for_its_descendants() {
            let delivered = DeliveredPaths::of(&[ChangeEntry {
                canonical: PathBuf::from("/w/Dir.v1"),
                raw: PathBuf::from("/w/Dir.v1"),
                kind: ChangeKind::MaybeRemoved,
                seq: 1,
            }]);
            let gone = WorkspaceDiff {
                added: vec![],
                removed: vec!["/w/Dir.v1/A.bsl".to_owned(), "/w/Dir.v1/B.bsl".to_owned()],
                modified: vec![],
            };
            assert!(
                !has_undelivered_drift(&gone, &delivered),
                "the removal of a dotted directory accounts for the files under it",
            );
        }

        /// An event has a direction. A directory reported gone accounts for what was under
        /// it disappearing — not for a file that appeared there afterwards, whose own event
        /// the watcher may well have lost. Checked through the miss decision itself, so it
        /// pins which rule that decision applies, not merely that both rules exist.
        #[test]
        fn a_vanished_directory_does_not_answer_for_what_appears_after_it() {
            let delivered = DeliveredPaths::of(&[subtree_removal("/w/Dir")]);
            let gone = WorkspaceDiff {
                added: vec![],
                removed: vec!["/w/Dir/A.bsl".to_owned()],
                modified: vec![],
            };
            assert!(
                !has_undelivered_drift(&gone, &delivered),
                "the vanished directory accounts for its descendants disappearing",
            );

            let reborn = WorkspaceDiff {
                added: vec!["/w/Dir/New.bsl".to_owned()],
                removed: vec![],
                modified: vec![],
            };
            assert!(
                has_undelivered_drift(&reborn, &delivered),
                "but not for a file that appeared there afterwards",
            );
        }

        /// Only a vanished DIRECTORY stands for what is under it. An ordinary change to a
        /// path covers that path alone — otherwise an edit inside a directory would vouch
        /// for files nobody reported.
        #[test]
        fn an_ordinary_entry_covers_only_itself() {
            let delivered = DeliveredPaths::of(&[ChangeEntry {
                canonical: PathBuf::from("/w/Dir"),
                raw: PathBuf::from("/w/Dir"),
                kind: ChangeKind::MaybeChanged,
                seq: 1,
            }]);
            assert!(delivered.covers("/w/Dir"));
            assert!(!delivered.covers("/w/Dir/A.bsl"));
        }
    }

    /// A batch that also demands a full reconcile still DELIVERED its paths, and the hub
    /// keeps them on every input but a channel overflow. Counting them as missed answers a
    /// reconcile with another one — charged to every consumer of the hub, not just this one.
    #[test]
    fn a_path_delivered_alongside_a_rescan_is_not_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        // The probe fires between the reconciler's first drain and its scan: it edits a
        // module, waits for the hub to hold it, then opens a reconcile window that does NOT
        // clear entries — the shape nine of the hub's ten inputs have.
        let probe_root = root.to_path_buf();
        let probe_hub = hub.clone();
        let after_probe = Arc::new(Mutex::new(0u64));
        let probe_seen = Arc::clone(&after_probe);
        state.set_reconcile_probe(move || {
            fs::write(
                probe_root.join("CommonModules/Сервер/Ext/Module.bsl"),
                "&НаСервере\nФункция Считать() Экспорт Возврат 17; КонецФункции\n",
            )
            .unwrap();
            let mut obs = probe_hub.subscribe();
            for _ in 0..300 {
                let batch = probe_hub.drain(obs);
                obs = batch.cursor;
                if batch.entries.iter().any(|e| e.raw.to_string_lossy().contains("Module.bsl")) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            probe_hub.unsubscribe(obs);
            probe_hub.degrade_external();
            *lock_recover(&probe_seen) = probe_hub.rescan_request_count();
        });

        state.reconcile_tick();

        assert_eq!(
            hub.rescan_request_count(),
            *lock_recover(&after_probe),
            "the reconciler asked for no reconcile of its own: the path was delivered",
        );
    }

    /// A snapshot and the baseline it is compared against must describe the same world.
    /// Once the baseline moves, their diff runs BACKWARDS: a file added since the snapshot
    /// was taken is absent from it, so it reads as a deletion and is applied as one.
    #[test]
    fn a_scan_taken_before_the_baseline_moved_is_not_applied() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        // The snapshot, taken while the new module does not exist yet.
        let snapshot = state.throttled_scan(root).expect("a scan");

        // The baseline moves: the module is created and applied from the event stream.
        write_common_module(root, "Новый", true, "&НаСервере\nФункция Н() Экспорт КонецФункции");
        let added = module_path(root, "Новый");
        state.apply_drained_entries(&[ChangeEntry {
            canonical: added.clone(),
            raw: added.clone(),
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        }]);
        assert!(
            matches!(
                state.read(|r, _| r.file_id_for(&added).is_some()),
                ResidentOutcome::Ready(true, _)
            ),
            "the new module is in the resident before the stale scan is applied",
        );

        // The stale snapshot now describes a world without that module.
        let changes = {
            let inner = lock_recover(&state.inner);
            classify_changes(&inner.stats, &snapshot.stats)
        };
        assert!(
            changes.removed.iter().any(|p| p.contains("Новый")),
            "the stale snapshot really does read the new module as deleted",
        );
        state.apply_scan_drift(&changes, false, &snapshot);

        assert!(
            matches!(
                state.read(|r, _| r.file_id_for(&added).is_some()),
                ResidentOutcome::Ready(true, _)
            ),
            "a snapshot older than the baseline is refused, not applied backwards",
        );
    }

    /// Refusing a snapshot answers nothing: whatever made this scan run — a reconcile debt
    /// the drain already cleared, a consumed force — is spent. A healthy cursor then sends
    /// every following read down the drain path, so the drift the hub lost would sit
    /// unapplied, and unreported, until the watchdog tick.
    #[test]
    fn a_refused_scan_re_arms_the_obligation_to_scan() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let snapshot = state.throttled_scan(root).expect("a scan");
        write_common_module(
            root,
            "Двигатель",
            true,
            "&НаСервере\nФункция Д() Экспорт КонецФункции",
        );
        let moved = module_path(root, "Двигатель");
        state.apply_drained_entries(&[ChangeEntry {
            canonical: moved.clone(),
            raw: moved,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        }]);

        let changes = {
            let inner = lock_recover(&state.inner);
            classify_changes(&inner.stats, &snapshot.stats)
        };
        state.apply_scan_drift(&changes, false, &snapshot);

        let scans = state.scan_count();
        let _ = state.read(|_, _| ());
        assert!(
            state.scan_count() > scans,
            "the refused snapshot leaves the obligation to scan standing",
        );
    }

    /// Re-arming alone does not make the next poll walk. `throttled_scan` caches whatever it
    /// produced, so a walk that STARTED before the baseline moved lands its snapshot in the
    /// cache after it — and every read inside the drift interval is then handed the same
    /// snapshot and refuses it again. The epoch only grows, so it can never become
    /// applicable: the refusal has to drop it.
    #[test]
    fn a_refused_snapshot_does_not_stay_in_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (mut state, _hub) = state_with_hub(root);
        state.drift_interval = Duration::from_secs(60);
        state.ensure_loading();
        wait_ready(&state);

        let snapshot = state.throttled_scan(root).expect("a scan");
        write_common_module(root, "Обгон", true, "&НаСервере\nФункция О() Экспорт КонецФункции");
        let overtaken = module_path(root, "Обгон");
        state.apply_drained_entries(&[ChangeEntry {
            canonical: overtaken.clone(),
            raw: overtaken,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        }]);

        // A walk that began before the move finishing after it: its snapshot is cached
        // carrying the older epoch.
        *lock_recover(&state.scan) = Some(ScanCache {
            at: Instant::now(),
            stats: snapshot.stats.clone(),
            config_fp: snapshot.config_fp,
            baseline_epoch: snapshot.baseline_epoch,
        });

        // Set directly rather than through `force_rescan`, whose storm guard declines to arm
        // anything while the cache is fresh — which is exactly the state under test.
        state.force_scan.store(true, Ordering::SeqCst);
        let _ = state.read(|_, _| ());
        let scans = state.scan_count();
        let _ = state.read(|_, _| ());

        assert!(
            state.scan_count() > scans,
            "the refused snapshot is gone, so the re-armed scan actually walks",
        );
    }

    /// The largest baseline move there is: a rebuild replaces `stats` wholesale. A snapshot
    /// from before it would rebase the freshly built baseline back onto its own older view.
    #[test]
    fn a_scan_taken_before_a_rebuild_is_not_applied_to_the_new_resident() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let snapshot = state.throttled_scan(root).expect("a scan");
        write_common_module(root, "После", true, "&НаСервере\nФункция П() Экспорт КонецФункции");

        state.kick_full_reload();
        for _ in 0..300 {
            if lock_recover(&state.inner).reload != ReloadState::Running {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        wait_ready(&state);
        let built = module_path(root, "После");
        assert!(
            matches!(
                state.read(|r, _| r.file_id_for(&built).is_some()),
                ResidentOutcome::Ready(true, _)
            ),
            "the rebuild picked the module up",
        );

        let changes = {
            let inner = lock_recover(&state.inner);
            classify_changes(&inner.stats, &snapshot.stats)
        };
        state.apply_scan_drift(&changes, false, &snapshot);

        assert!(
            matches!(
                state.read(|r, _| r.file_id_for(&built).is_some()),
                ResidentOutcome::Ready(true, _)
            ),
            "a rebuild's baseline is not rebased onto a snapshot that predates it",
        );
    }

    /// The epoch check guards the FILE diff, which is meaningless against a moved baseline.
    /// A config change is not a diff — it says the analyzer setup changed, which staleness
    /// cannot make untrue — and the rebuild it triggers reads the world afresh anyway.
    /// Gating it too would strand config drift until the next reconcile tick, since the miss
    /// check counts file paths only and a healthy read never scans.
    #[test]
    fn a_config_change_survives_a_snapshot_the_epoch_check_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let snapshot = state.throttled_scan(root).expect("a scan");
        // Move the baseline out from under the snapshot.
        write_common_module(root, "Сдвиг", true, "&НаСервере\nФункция С() Экспорт КонецФункции");
        let moved = module_path(root, "Сдвиг");
        state.apply_drained_entries(&[ChangeEntry {
            canonical: moved.clone(),
            raw: moved,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        }]);
        lock_recover(&state.inner).reload = ReloadState::Idle;

        let no_file_drift =
            WorkspaceDiff { added: Vec::new(), removed: Vec::new(), modified: Vec::new() };
        state.apply_scan_drift(&no_file_drift, true, &snapshot);

        assert_ne!(
            lock_recover(&state.inner).reload,
            ReloadState::Idle,
            "config drift is answered by a rebuild even when the snapshot is stale",
        );
    }

    /// Applying entries moves the baseline, which leaves every cached snapshot describing
    /// the world before the move. The cache must go with it, or the next scan inside the
    /// drift interval is served a snapshot the baseline has already outrun.
    #[test]
    fn applying_entries_drops_the_scan_cache() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (mut state, _hub) = state_with_hub(root);
        state.drift_interval = Duration::from_secs(60);
        state.ensure_loading();
        wait_ready(&state);
        state.throttled_scan(root).expect("a scan warms the cache");

        write_common_module(root, "Свежий", true, "&НаСервере\nФункция Св() Экспорт КонецФункции");
        let fresh = module_path(root, "Свежий");
        let scans = state.scan_count();
        state.apply_drained_entries(&[ChangeEntry {
            canonical: fresh.clone(),
            raw: fresh,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        }]);
        state.throttled_scan(root).expect("a scan");

        assert!(
            state.scan_count() > scans,
            "the next scan walks instead of reusing a snapshot the baseline outran",
        );
    }

    /// A read whose drain comes back demanding a reconcile answers with a scan — but that
    /// scan is served from a cache up to `drift_interval` old, which can predate the very
    /// entries the batch delivered. Applying them is what keeps the read from waiting for
    /// the cache to cool.
    #[test]
    fn a_read_applies_the_entries_delivered_with_a_rescan_demand() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (mut state, hub) = state_with_hub(root);
        state.drift_interval = Duration::from_secs(60);
        state.ensure_loading();
        wait_ready(&state);
        // Warm the scan cache, so the scan the rescan branch performs is served from a
        // snapshot older than the file the probe is about to create.
        state.force_rescan();
        let _ = state.read(|_, _| ());

        // The debt is raised INSIDE the window between the health decision and the drain:
        // raised any earlier, the read would take the scan path and never drain at all.
        let probe_root = root.to_path_buf();
        let probe_hub = hub.clone();
        state.set_pre_drain_probe(move || {
            write_common_module(
                &probe_root,
                "Внезапный",
                true,
                "&НаСервере\nФункция Внезапная() Экспорт КонецФункции",
            );
            let mut obs = probe_hub.subscribe();
            for _ in 0..300 {
                let batch = probe_hub.drain(obs);
                obs = batch.cursor;
                if batch.entries.iter().any(|e| e.raw.to_string_lossy().contains("Внезапный"))
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            probe_hub.unsubscribe(obs);
            probe_hub.degrade_external();
        });

        let _ = state.read(|_, _| ());

        let sudden = module_path(root, "Внезапный");
        let found = state.read(|resident, _| resident.file_id_for(&sudden).is_some());
        assert!(
            matches!(found, ResidentOutcome::Ready(true, _)),
            "the entries delivered with the rescan demand are applied, not dropped",
        );
    }

    /// The scan is what covers whatever the hub DROPPED, so it runs whether or not the
    /// batch also carried entries. An empty batch is the case where nothing else would.
    #[test]
    fn a_read_still_scans_when_the_rescan_demand_carries_no_entries() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let probe_hub = hub.clone();
        state.set_pre_drain_probe(move || probe_hub.degrade_external());

        let scans = state.scan_count();
        let _ = state.read(|_, _| ());

        assert!(
            state.scan_count() > scans,
            "a reconcile demand is answered by a walk even with nothing to apply",
        );
    }

    /// The tick clears its scan cache ONCE, and the snapshot it then takes immediately
    /// becomes the warm cache the second drain's scan is served from. An event delivered
    /// after that snapshot is therefore in neither the snapshot nor step 4's diff: without
    /// applying the batch's own entries it waits for the cache to cool.
    #[test]
    fn an_event_delivered_after_the_scan_is_applied_in_the_same_tick() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (mut state, hub) = state_with_hub(root);
        // A warm cache is the whole point: at the test harness's zero interval the second
        // drain's scan would be a fresh walk that sees the late file by itself.
        state.drift_interval = Duration::from_secs(60);
        state.ensure_loading();
        wait_ready(&state);

        // Drift that exists BEFORE the scan, so the tick does not return on an empty diff
        // and never reaches the second drain at all.
        lock_recover(&state.inner).stats.insert(root.join("Vanished.bsl").display().to_string(), 1);

        let probe_root = root.to_path_buf();
        let probe_hub = hub.clone();
        state.set_post_scan_probe(move || {
            write_common_module(
                &probe_root,
                "Поздний",
                true,
                "&НаСервере\nФункция Поздняя() Экспорт КонецФункции",
            );
            let mut obs = probe_hub.subscribe();
            for _ in 0..300 {
                let batch = probe_hub.drain(obs);
                obs = batch.cursor;
                if batch.entries.iter().any(|e| e.raw.to_string_lossy().contains("Поздний"))
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            probe_hub.unsubscribe(obs);
            probe_hub.degrade_external();
        });

        state.reconcile_tick();

        let late = module_path(root, "Поздний");
        // The BASELINE is the observable for this path, not the resident. Step 4 applies the
        // diff computed at step 2, and removals from the resident come only from that diff's
        // own list — the late file is not in it, so the resident keeps it either way. What
        // step 4 does move is the baseline, wholesale, onto its pre-move snapshot.
        assert!(
            lock_recover(&state.inner).stats.keys().any(|p| p.contains("Поздний")),
            "the late file is in the baseline: the stale snapshot did not rebase it away",
        );
        let found = state.read(|resident, _| resident.file_id_for(&late).is_some());
        assert!(
            matches!(found, ResidentOutcome::Ready(true, _)),
            "and it is in the resident, not waiting for the cache to cool",
        );
    }

    /// The drain names a vanished directory and nothing else — the descendants it stood for
    /// are exactly what can no longer be enumerated, while the scan lists every one of them.
    /// Matching delivered paths by equality alone therefore calls every descendant a miss.
    #[test]
    fn the_descendants_of_a_delivered_vanished_directory_are_not_missed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_common_module(root, "Первый", true, "&НаСервере\nФункция А() Экспорт КонецФункции");
        write_common_module(root, "Второй", true, "&НаСервере\nФункция Б() Экспорт КонецФункции");

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let probe_root = root.to_path_buf();
        let probe_hub = hub.clone();
        let after_probe = Arc::new(Mutex::new(0u64));
        let probe_seen = Arc::clone(&after_probe);
        state.set_reconcile_probe(move || {
            fs::remove_dir_all(probe_root.join("CommonModules")).unwrap();
            let mut obs = probe_hub.subscribe();
            for _ in 0..300 {
                let batch = probe_hub.drain(obs);
                obs = batch.cursor;
                if batch.entries.iter().any(|e| e.kind == ChangeKind::SubtreeRemoved) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            probe_hub.unsubscribe(obs);
            probe_hub.degrade_external();
            *lock_recover(&probe_seen) = probe_hub.rescan_request_count();
        });

        state.reconcile_tick();

        assert_eq!(
            hub.rescan_request_count(),
            *lock_recover(&after_probe),
            "the vanished directory covers the files that went with it",
        );
    }

    /// A `bsl-analyzer.toml` in a SUBDIRECTORY is not the analyzer config (which lives at the
    /// workspace root): the drain must ignore it, matching the scan path's `config_files_fingerprint`
    /// which only fingerprints `root.join(name)`. A subtree toml edit is not a rebuild trigger.
    #[test]
    fn subdir_config_file_is_not_config_drift() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        // A toml deep in the tree, NOT the root analyzer config.
        write(root, "CommonModules/Сервер/bsl-analyzer.toml", "[diagnostics]\n");

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let subdir_toml = root.join("CommonModules/Сервер/bsl-analyzer.toml");
        let canonical = subdir_toml.canonicalize().unwrap_or(subdir_toml);
        let entry = ChangeEntry {
            canonical: canonical.clone(),
            raw: canonical,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };

        let gen0 = raw_generation(&state);
        // Feeding the subdir toml as drift must NOT kick a full rebuild.
        state.apply_drained_entries(&[entry]);
        assert_eq!(
            state.status_report().reload,
            "none",
            "a toml outside the workspace root is not analyzer-config drift",
        );
        assert_eq!(raw_generation(&state), gen0, "no structural rebuild for a subtree toml");
    }

    /// A `.bsl` edit in an EXTENSION root (disjoint from the config source root) is delivered
    /// through the drain, because the hub watches every scan root — not just the source one.
    /// Without extension coverage this drift would be invisible until the 90s reconciler.
    #[test]
    fn extension_root_edit_lands_via_drain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Nested layout: config source under `src/cf`, an extension auto-discovered under
        // `src/cfe/*` (both need a `Configuration.xml` to be recognised).
        let cf = root.join("src/cf");
        fs::create_dir_all(&cf).unwrap();
        fs::write(cf.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_common_module(&cf, "Сервер", true, "&НаСервере\nФункция Ч() Экспорт КонецФункции");
        let ext = root.join("src/cfe/Расш");
        fs::create_dir_all(&ext).unwrap();
        fs::write(ext.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_common_module(
            &ext,
            "РасшМодуль",
            true,
            "&НаСервере\nФункция Р() Экспорт КонецФункции",
        );

        // Build the hub over the SAME targets production arms (source + extensions,
        // plus the workspace root non-recursively) — a narrower boot set would be
        // upgraded by the resident's post-publish re-arm, and that one-time rescan
        // debt is exactly what this zero-scan assertion must not see.
        let project = project_model::Project::new(root).expect("valid test project");
        let mut roots = vec![project.source_path().to_path_buf()];
        roots.extend(project.extension_paths().iter().map(|(_, p)| p.clone()));
        assert!(roots.len() >= 2, "the extension root must be discovered: {roots:?}");
        let hub =
            WorkspaceChangeHub::start_targets(crate::change_hub::watch_targets_for(root, &roots));
        assert!(hub.wait_until_watching(Duration::from_secs(5)), "the hub must arm");

        let mut state =
            DiagnosticsState::for_workspace(root.to_path_buf()).with_change_hub(hub.clone());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);

        let ext_module = ext.join("CommonModules/РасшМодуль/Ext/Module.bsl");
        let resident = state.read(|r, _| r.file_id_for(&ext_module).is_some());
        assert!(
            matches!(resident, ResidentOutcome::Ready(true, _)),
            "the extension module must be resident",
        );

        let gen0 = raw_generation(&state);
        std::thread::sleep(Duration::from_millis(10));
        fs::write(&ext_module, "&НаСервере\nФункция Р() Экспорт Возврат 9; КонецФункции\n")
            .unwrap();

        assert!(wait_for_apply(&state, gen0), "an extension-root edit is delivered via drain");
        assert_eq!(state.scan_count(), 0, "the event-driven path performs no scan");
    }

    /// `BSL_MCP_RECONCILE_SECS` clamping: `0` and garbage fall back to the default; small
    /// positive values are floored so the sweeper cannot busy-loop; valid values pass through.
    #[test]
    fn reconcile_interval_clamps_bad_env() {
        assert_eq!(clamp_reconcile_interval(None), RECONCILE_INTERVAL, "unset/garbage → default");
        assert_eq!(clamp_reconcile_interval(Some(0)), RECONCILE_INTERVAL, "zero → default");
        assert_eq!(clamp_reconcile_interval(Some(1)), MIN_RECONCILE_INTERVAL, "floored");
        assert_eq!(clamp_reconcile_interval(Some(4)), MIN_RECONCILE_INTERVAL, "floored");
        assert_eq!(clamp_reconcile_interval(Some(5)), Duration::from_secs(5), "at the floor");
        assert_eq!(clamp_reconcile_interval(Some(120)), Duration::from_secs(120), "passthrough");
        // Unparseable env text becomes `None` before clamping.
        assert_eq!("nonsense".parse::<u64>().ok(), None);
    }

    /// A delivered file outside the scan universe (an editor temp file) is a no-op for the
    /// diagnostics drain: it touches no resident input and triggers no scan on the healthy
    /// hot path. `apply_drained_entries` already ignores non-`.bsl`/`.xml`/config paths.
    #[test]
    fn non_scan_file_delivery_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let mut observer = hub.subscribe();
        std::thread::sleep(Duration::from_millis(10));
        fs::write(root.join("CommonModules/Сервер/Ext/Module.bsl.tmp"), "editor swap").unwrap();
        assert!(wait_for_delivery(&hub, &mut observer, ".tmp"), "hub delivered the temp file");

        let gen0 = raw_generation(&state);
        // A read drains the diagnostics cursor (which holds the .tmp), and must apply nothing.
        let _ = state.read(|_, _| ());
        assert_eq!(raw_generation(&state), gen0, "a non-scan file does not move the resident");
        assert_eq!(state.scan_count(), 0, "and triggers no scan on the healthy path");
    }
}
