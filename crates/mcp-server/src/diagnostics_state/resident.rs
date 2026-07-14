use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ide::{Analysis, RootDatabaseImpl};
use vfs::{FileId, Vfs, VfsPath};

use super::workspace_sweep::{CodeAggregate, SweepOptions, WorkspaceSweep};

/// Adapts the resident's owned [`Vfs`] to the lock-neutral [`ide_host_core::VfsWrite`]
/// the shared metadata policy expects. The resident is only ever touched while the
/// caller holds the state mutex (the db is `!Sync`), so a single-threaded `RefCell`
/// gives the interning critical section its interior mutability without a second lock —
/// the same discipline the LSP's `parking_lot`-locked adapter has, minus the lock.
pub(super) struct ResidentVfs(pub(super) RefCell<Vfs>);

impl ide_host_core::VfsWrite for ResidentVfs {
    fn with_write<R>(&self, f: impl FnOnce(&mut Vfs) -> R) -> R {
        f(&mut self.0.borrow_mut())
    }
}

/// The built resident database plus the path→FileId index needed to resolve a request
/// path to the Salsa input it set. Held behind the [`std::sync::Mutex`]; reads borrow it,
/// a reload mutates `db` in place.
pub(crate) struct DiagnosticsResident {
    pub(super) db: RootDatabaseImpl,
    /// The VFS pre-seeded with the resident's `.bsl` FileIds and grown by the metadata
    /// bootstrap with the metadata-XML ids. Kept alongside the db so a drift-driven
    /// substrate refresh can intern new composing files onto the same id space without
    /// rebuilding it.
    pub(super) vfs: ResidentVfs,
    /// Canonical-path string → FileId for every resident `.bsl`.
    pub(super) by_path: HashMap<String, FileId>,
    /// The project's effective diagnostics settings, loaded from `bsl-analyzer.toml` /
    /// `.bsl-analyzer.json` the same way LSP and CLI do — so `file`/`workspace` honour
    /// the project's disabled rules and thresholds, not analyzer defaults.
    pub(super) config: ide::DiagnosticsConfig,
    /// The workspace root the resident was built against — the SAME root the graph build
    /// uses (`source_dir`), so an absolute finding path strips to the graph encoder's rel
    /// and the `method/file/<rel>::<name>` graph bridge resolves.
    pub(super) workspace_root: PathBuf,
}

impl DiagnosticsResident {
    /// Resolve a request path to the resident FileId, canonicalising it the same way
    /// the loader did. A relative path is resolved against the workspace root (not the
    /// process CWD), so `diagnostics file` works regardless of where the server was
    /// started. `None` when the path is not a resident workspace `.bsl`.
    pub(crate) fn file_id_for(&self, path: &Path) -> Option<FileId> {
        let resolved;
        let abs: &Path = if path.is_absolute() {
            path
        } else {
            resolved = self.workspace_root.join(path);
            &resolved
        };
        self.by_path.get(&canonical_key(abs)).copied()
    }

    /// The workspace root the resident was built against (the graph's `source_dir`),
    /// used to bridge findings to durable `method/file/<rel>::<name>` graph ids.
    pub(crate) fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// An `Analysis` view over a cloned db handle. The clone shares the Salsa storage
    /// (memo/LRU cache), and is dropped before the read guard is released.
    pub(crate) fn analysis(&self) -> Analysis {
        Analysis::from_database(self.db.clone())
    }

    /// The resident Salsa database, for the `metadata` tool's root-scoped metadata
    /// reads (`resolve_*_across_roots` point-lookups and the Channel-2
    /// `configuration_for_root` header/enumeration). Borrowed under the state lock, so
    /// the borrow cannot outlive the read and a reload can never alias it.
    pub(crate) fn db(&self) -> &RootDatabaseImpl {
        &self.db
    }

    pub(crate) fn file_count(&self) -> usize {
        self.by_path.len()
    }

    /// The project's effective diagnostics config, the single source of truth shared
    /// with LSP and CLI. `file` and `workspace` analyse against this, never defaults.
    pub(crate) fn config(&self) -> &ide::DiagnosticsConfig {
        &self.config
    }

    /// Workspace-wide diagnostics aggregated per code (the `workspace` action). Runs
    /// rayon over per-worker db clones (shared Salsa storage, the CLI `analyze`
    /// discipline). The caller MUST hold the state lock for the whole sweep so no
    /// reload mutates the master db mid-flight — that would cancel the cloned queries.
    /// Bounded by `opts.max_files` over a stable FileId order, so a cap is deterministic.
    pub(crate) fn workspace_aggregates(
        &self,
        config: &ide::DiagnosticsConfig,
        opts: &SweepOptions,
    ) -> WorkspaceSweep {
        use rayon::prelude::*;
        use std::collections::HashSet;

        let mut files: Vec<FileId> = self.by_path.values().copied().collect();
        files.sort_by_key(|f| f.0);
        let files_total = files.len();
        let truncated = files_total > opts.max_files;
        let swept = &files[..opts.max_files.min(files_total)];

        // Per file: the (code, bucket) of each diagnostic. Each rayon worker owns a db
        // clone; queries run in parallel on the shared, unmutated Salsa storage.
        let per_file: Vec<Vec<(String, ide::SeverityBucket)>> = swept
            .par_iter()
            .map_with(self.db.clone(), |db, &file_id| {
                let analysis = Analysis::from_database(db.clone());
                analysis
                    .diagnostics(file_id, config)
                    .iter()
                    .map(|d| (d.code.as_str().to_string(), ide::SeverityBucket::from(d.severity)))
                    .collect()
            })
            .collect();

        // Fold: code -> (bucket, total count, files-affected). All occurrences of a code
        // share a bucket under one config, so first-seen is representative.
        let mut map: HashMap<String, (ide::SeverityBucket, usize, usize)> = HashMap::new();
        for file_diags in &per_file {
            let mut seen_here: HashSet<&str> = HashSet::new();
            for (code, bucket) in file_diags {
                let entry = map.entry(code.clone()).or_insert((*bucket, 0, 0));
                entry.1 += 1;
                if seen_here.insert(code.as_str()) {
                    entry.2 += 1;
                }
            }
        }

        let mut aggregates: Vec<CodeAggregate> = map
            .into_iter()
            .filter(|(_, (bucket, _, _))| *bucket >= opts.min_severity)
            .filter(|(code, _)| opts.codes.is_empty() || opts.codes.iter().any(|c| c == code))
            .map(|(code, (severity, count, files_affected))| CodeAggregate {
                code,
                severity,
                count,
                files_affected,
            })
            .collect();
        // Most-severe first, then most-frequent, then code for a stable order.
        aggregates.sort_by(|a, b| {
            b.severity.cmp(&a.severity).then(b.count.cmp(&a.count)).then(a.code.cmp(&b.code))
        });

        WorkspaceSweep { aggregates, files_swept: swept.len(), files_total, truncated }
    }
}

/// Canonicalise a path to the same key the loader indexed by (`enumerate_bsl_files`
/// canonicalises, falling back to the raw path). Lets a request path in any form
/// resolve to the resident FileId.
pub(super) fn canonical_key(path: &Path) -> String {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned()
}

/// Apply drifted XML metadata + modified BSL bodies to the resident under an
/// already-held lock, shared by the scan and event-driven drift paths so both mutate
/// the resident identically. Returns `(needs_rebuild, moved)`: a full rebuild is
/// needed when an XML path resolves outside every config root (a symlink the
/// point-refresh cannot express) or a modified `.bsl` has no resident FileId (the file
/// universe moved); `moved` is whether any Salsa input actually changed. `fp_of` yields
/// the on-disk fingerprint of a `modified_bsl` path so an already-current body is
/// skipped. The caller owns the drift-baseline update (full rebase vs incremental).
pub(super) fn apply_resident_changes(
    resident: &mut DiagnosticsResident,
    xml_paths: &[PathBuf],
    added_bsl: &[String],
    modified_bsl: &[String],
    removed_bsl: &[String],
    fp_of: impl Fn(&str) -> Option<u64>,
    stats: &HashMap<String, u64>,
) -> (bool, bool) {
    use base_db::{SourceDatabase, SourceRoot};
    use ide_host_core::{set_file_text_source, FileTextSource, VfsWrite};

    // Pre-classification: an XML path resolving outside every registered config root is
    // drift the point-refresh cannot express — `refresh_metadata_substrate` gates its
    // re-discovery on `changed.starts_with(root)`, so it would silently no-op. Bail to a
    // full rebuild, which re-reads through the discovery joins, symlinks and all.
    let config_roots = resident.db.all_config_paths();
    let xml_outside_roots =
        xml_paths.iter().any(|p| !config_roots.iter().any(|(_, root)| p.starts_with(root)));
    if xml_outside_roots {
        return (true, false);
    }

    let mut moved = false;

    // (1) Reconcile the file universe FIRST — mirrors the LSP's `process_changes`
    // discipline (one FileSet clone, per-file inputs, one `set_source_root`), so the
    // substrate refresh below resolves module back-links through an up-to-date VFS and
    // root. Per-file ordering matters: the source-root + content-revision inputs are
    // registered BEFORE the file becomes visible through the FileSet
    // (`file_text_query` panics on a visible file with no revision).
    let mut file_set_modified = false;
    let mut file_set = {
        let db = &resident.db;
        db.source_root_input(crate::graph::input::GRAPH_SOURCE_ROOT).root(db).file_set().clone()
    };
    for path in added_bsl {
        // Vanished again before we got here (create+delete coalesced apart): the
        // removal pass — or the next drift window — settles it.
        if fp_of(path).is_none() && !Path::new(path).is_file() {
            continue;
        }
        let vfs_path = VfsPath::new(path.clone());
        let file_id = resident.vfs.with_write(|vfs| vfs.alloc_file_id(vfs_path.clone()));
        if let Some(&known) = resident.by_path.get(path.as_str()) {
            if known != file_id {
                // The path is already registered under a different id — an aliasing
                // (symlink/canonicalisation) case registration cannot express safely.
                return (true, moved);
            }
        }
        resident.db.set_file_source_root(file_id, crate::graph::input::GRAPH_SOURCE_ROOT);
        match base_db::read_disk_text(Path::new(path)) {
            Ok(text) => {
                set_file_text_source(&mut resident.db, file_id, FileTextSource::Disk(&text))
            }
            Err(_) => set_file_text_source(&mut resident.db, file_id, FileTextSource::Tombstone),
        }
        if file_set.path_for_file(&file_id).is_none() {
            file_set.insert(file_id, vfs_path);
            file_set_modified = true;
        }
        // The classifier's `key` IS the canonical by_path spelling (both come from the
        // scan-universe canonicalisation), so insert it verbatim — re-canonicalising
        // here could diverge on a path that vanished between classify and apply.
        resident.by_path.insert(path.clone(), file_id);
        moved = true;
    }
    for path in removed_bsl {
        // Never indexed → nothing to unregister (an untracked removal is not drift).
        let Some(&file_id) = resident.by_path.get(path.as_str()) else { continue };
        set_file_text_source(&mut resident.db, file_id, FileTextSource::Tombstone);
        if file_set.path_for_file(&file_id).is_some() {
            file_set.remove(file_id);
            file_set_modified = true;
        }
        resident.by_path.remove(path.as_str());
        moved = true;
    }
    if file_set_modified {
        resident.db.set_source_root(
            crate::graph::input::GRAPH_SOURCE_ROOT,
            SourceRoot::new_local(file_set),
        );
    }

    // (2) Refresh the per-MDO substrate. Beside the drifted `.xml`, a created or
    // deleted common-module/service body changes its listing's `module_file`
    // reverse-index entry (the body is ordinary source, so it never flows through the
    // metadata-XML path) — include those bodies in the same re-discovery, exactly as
    // the LSP does. The config-revision bump stays `.xml`-only: a body add/remove does
    // not change the whole-config metadata content.
    let structural_listing_bodies: Vec<PathBuf> = added_bsl
        .iter()
        .chain(removed_bsl)
        .map(PathBuf::from)
        .filter(|p| project_model::is_substrate_listed_body_path(p))
        .filter(|p| config_roots.iter().any(|(_, root)| p.starts_with(root)))
        .collect();
    if !xml_paths.is_empty() || !structural_listing_bodies.is_empty() {
        let mut refresh: Vec<PathBuf> = xml_paths.to_vec();
        refresh.extend(structural_listing_bodies);
        ide_host_core::refresh_metadata_substrate(&mut resident.db, &resident.vfs, &refresh);
        if !xml_paths.is_empty() {
            resident.db.bump_config_for_paths(xml_paths.iter().map(|p| p.as_path()));
        }
        moved = true;
    }

    // `.bsl` bodies: disk-backed re-key. A body already at its on-disk fingerprint (a
    // racing caller beat us) is skipped.
    for path in modified_bsl {
        let Some(fp) = fp_of(path) else { continue };
        if stats.get(path).copied() == Some(fp) {
            continue;
        }
        let Some(&file_id) = resident.by_path.get(path) else {
            return (true, moved); // a modified `.bsl` we never indexed → structural
        };
        match base_db::read_disk_text(Path::new(path)) {
            Ok(text) => {
                set_file_text_source(&mut resident.db, file_id, FileTextSource::Disk(&text))
            }
            // Unreadable now: an empty overlay so a later query yields `""` instead of
            // panicking on the disk re-read, matching the load path.
            Err(_) => set_file_text_source(&mut resident.db, file_id, FileTextSource::Tombstone),
        }
        moved = true;
    }

    (false, moved)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{module_path, sample_workspace, wait_ready, write};
    use super::super::{DiagnosticsState, ResidentOutcome};
    use super::*;
    use ide::DiagnosticsConfig;

    /// First use builds the resident db over the workspace and resolves a request
    /// path to a FileId, then computes diagnostics for it.
    #[test]
    fn builds_resident_and_serves_file_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let path = module_path(root, "Сервер");
        let out = state.read(|resident, _| {
            let file_id = resident.file_id_for(&path).expect("path resolves to a resident FileId");
            resident.analysis().diagnostics(file_id, &DiagnosticsConfig::default()).len()
        });
        match out {
            ResidentOutcome::Ready(_, _) => {}
            _ => panic!("expected Ready outcome from a loaded db"),
        }
    }

    /// The resident is disk-backed: a workspace file is registered by content revision,
    /// not pinned as a `FileTextInput` overlay, so `file_text_query` re-reads it from disk
    /// under the LRU cap. This is what keeps a whole-workspace resident from OOMing. The
    /// file's text must still be queryable (diagnostics ran above), it just must not be
    /// held resident as a salsa input.
    #[test]
    fn resident_text_is_disk_backed_not_pinned() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let path = module_path(root, "Сервер");
        let out = state.read(|resident, _| {
            let file_id = resident.file_id_for(&path).expect("path resolves to a resident FileId");
            let pinned = resident.db.try_file_text(file_id).is_some();
            let len = resident.analysis().file_text(file_id).len();
            (pinned, len)
        });
        match out {
            ResidentOutcome::Ready((pinned, len), _) => {
                assert!(!pinned, "workspace file must be disk-backed, not pinned as an overlay");
                assert!(len > 0, "disk-backed text must still be readable on demand");
            }
            _ => panic!("expected Ready outcome"),
        }
    }

    /// The resident loads the project's `bsl-analyzer.toml` and exposes it as the
    /// effective config, so `file`/`workspace` honour the same disabled rules and tuned
    /// thresholds as LSP and CLI — not analyzer defaults.
    #[test]
    fn resident_config_reflects_project_toml() {
        use ide::DiagnosticCode;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(
            root,
            "bsl-analyzer.toml",
            "[source]\nroot = \".\"\n\n\
             [diagnostics.parameters]\n\
             Typo = false\n\n\
             [diagnostics.parameters.LineLength]\n\
             maxLineLength = 200\n",
        );

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let out = state.read(|resident, _| {
            let config = resident.config();
            (
                config.is_disabled(DiagnosticCode::Typo),
                config.get_int(DiagnosticCode::LineLength, "maxLineLength"),
            )
        });
        match out {
            ResidentOutcome::Ready((typo_disabled, line_len), _) => {
                assert!(typo_disabled, "project toml disables Typo");
                assert_eq!(line_len, Some(200), "project toml sets the LineLength threshold");
            }
            _ => panic!("expected Ready outcome"),
        }
    }

    /// A `diagnostics file` request may pass a workspace-relative path; it must resolve
    /// against the workspace root, not the process CWD.
    #[test]
    fn file_id_resolves_relative_path_against_workspace_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let rel = Path::new("CommonModules/Сервер/Ext/Module.bsl");
        let abs = module_path(root, "Сервер");
        let found = state.read(|resident, _| {
            (resident.file_id_for(rel).is_some(), resident.file_id_for(&abs).is_some())
        });
        match found {
            ResidentOutcome::Ready((rel_ok, abs_ok), _) => {
                assert!(rel_ok, "relative path resolves against the workspace root");
                assert!(abs_ok, "absolute path still resolves");
            }
            _ => panic!("expected Ready"),
        }
    }

    /// The resident's metadata substrate resolves a common module's `Ext/Module.bsl`
    /// back to the SAME FileId the resident indexed for it. This guards the seeding
    /// invariant: the VFS is pre-seeded with the resident's `.bsl` ids before the
    /// bootstrap interns the metadata XML on top, so the reverse index carries the
    /// resident's own id. Were the ids unseeded, the bootstrap would drop the back-link
    /// and `common_module_for_file_id` would return `None`.
    #[test]
    fn resident_substrate_backlinks_common_module_to_its_own_file_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let module = module_path(root, "Сервер");
        let project = project_model::Project::new(root);
        let config_root = project
            .source_path()
            .canonicalize()
            .unwrap_or_else(|_| project.source_path().to_path_buf());
        let root_key = config_root.to_string_lossy().into_owned();

        let out = state.read(|resident, _| {
            let file_id = resident.file_id_for(&module).expect("module .bsl resolves to a FileId");
            let listing_present = resident.db.metadata_listing(&root_key).is_some();
            let resolved = resident.db.common_module_for_file_id(file_id).is_some();
            (listing_present, resolved)
        });
        match out {
            ResidentOutcome::Ready((listing_present, resolved), _) => {
                assert!(
                    listing_present,
                    "the metadata substrate must be bootstrapped for the config root"
                );
                assert!(
                    resolved,
                    "the substrate must resolve the common module through the resident's own id"
                );
            }
            _ => panic!("expected Ready outcome from a loaded db"),
        }
    }

    /// A symlink inside the config tree must not drop the common module's back-link.
    #[cfg(unix)]
    #[test]
    fn resident_substrate_backlinks_common_module_through_symlinked_dir() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let root = base.join("ws");
        std::fs::create_dir_all(&root).unwrap();
        let real = base.join("real");
        super::super::test_support::write_common_module(
            &real,
            "Сервер",
            true,
            "&НаСервере\nФункция Ч() Экспорт КонецФункции",
        );
        std::os::unix::fs::symlink(real.join("CommonModules"), root.join("CommonModules")).unwrap();

        let state = DiagnosticsState::for_workspace(root.clone());
        state.ensure_loading();
        wait_ready(&state);

        let module = root.join("CommonModules/Сервер/Ext/Module.bsl");
        let out = state.read(|resident, _| {
            let file_id = resident.file_id_for(&module).expect("module .bsl resolves to a FileId");
            resident.db.common_module_for_file_id(file_id).is_some()
        });
        match out {
            ResidentOutcome::Ready(resolved, _) => assert!(
                resolved,
                "back-link must resolve through a symlinked config subtree via the canonicalising fallback"
            ),
            _ => panic!("expected Ready outcome from a loaded db"),
        }
    }
}
