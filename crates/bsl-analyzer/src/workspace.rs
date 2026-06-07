use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{anyhow, Result};
use lsp_types::Url;
use vfs::{loader, FileId, VfsPath};

use crate::global_state::GlobalState;

/// What a [`GlobalState::process_changes`] batch did, so the caller can decide
/// whether already-open documents need re-analysis and re-publishing.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChangeOutcome {
    /// A project config file (`bsl-analyzer.toml` / `.json`) changed, triggering
    /// a full project reload.
    pub config_file_changed: bool,
    /// A change was applied that can affect the analysis of *other* documents — a
    /// metadata XML edit or any `.bsl` source content (add / modify / delete). Open
    /// documents must be re-analyzed even though their own buffers did not change
    /// (e.g. files pulled in by `git pull` while editing).
    pub affects_open_documents: bool,
}

impl GlobalState {
    pub fn init_empty_source_root(&mut self) {
        use base_db::{SourceDatabase, SourceRoot, SourceRootId};

        let db = self.analysis_host.raw_database_mut();
        let source_root_id = SourceRootId(0);

        let file_set = vfs::file_set::FileSet::new();
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);

        tracing::debug!("initialized empty SourceRoot(0) before event loop");
    }

    pub fn set_workspace_root(&mut self, root: PathBuf) {
        let start = Instant::now();
        tracing::info!(?root, "setting workspace root");

        let project = project_model::Project::new(&root);

        let source_path = project.source_path().to_path_buf();
        let extensions: Vec<(String, PathBuf)> = project.extension_paths().to_vec();

        tracing::info!(
            ?source_path,
            extensions = extensions.len(),
            configuration_found = project.configuration_path().is_some(),
            "loaded project, scanning source path"
        );

        self.workspace_root = Some(root.clone());
        self.project = Some(project);

        {
            let mut all_paths: Vec<(Option<String>, std::path::PathBuf)> = Vec::new();
            all_paths.push((None, source_path.clone()));
            for (name, ext_path) in &extensions {
                all_paths.push((Some(name.clone()), ext_path.clone()));
            }
            self.analysis_host.request_cancellation();
            self.analysis_host.raw_database_mut().set_all_config_paths(all_paths);
        }

        self.update_diagnostics_config();
        self.update_features_config();

        self.vfs_progress_config_version += 1;

        let config_files: Vec<paths::AbsPathBuf> =
            ["bsl-analyzer.toml", ".bsl-analyzer.json", ".bsl-language-server.json"]
                .iter()
                .map(|name| root.join(name))
                .filter(|p| p.exists())
                .map(paths::AbsPathBuf::assert_utf8)
                .collect();

        let mut include = vec![paths::AbsPathBuf::assert_utf8(source_path)];

        for (name, ext_path) in &extensions {
            tracing::info!(name = %name, path = %ext_path.display(), "adding extension to VFS scan");
            include.push(paths::AbsPathBuf::assert_utf8(ext_path.clone()));
        }

        let mut load_entries = vec![loader::Entry::Directories(loader::Directories {
            extensions: project_model::SOURCE_EXTENSIONS.iter().map(|s| (*s).to_string()).collect(),
            include,
            exclude: vec![
                paths::AbsPathBuf::assert_utf8(root.join(".git")),
                paths::AbsPathBuf::assert_utf8(root.join("build")),
                paths::AbsPathBuf::assert_utf8(root.join(".vscode")),
            ],
            rules: vec![loader::FileRule {
                extensions: project_model::METADATA_WATCHED_EXTENSIONS
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
                load_mode: loader::LoadMode::WatchOnly,
            }],
        })];

        let watch = if config_files.is_empty() {
            vec![0]
        } else {
            load_entries.push(loader::Entry::Files(config_files));
            vec![0, 1]
        };

        self.loader.set_config(loader::Config {
            load: load_entries,
            watch,
            version: self.vfs_progress_config_version,
        });

        tracing::info!(
            elapsed_ms = start.elapsed().as_millis() as u64,
            "set_workspace_root complete (loader running async)",
        );
    }

    pub fn process_changes(&mut self, suppress_metadata_bump: bool) -> ChangeOutcome {
        use base_db::SourceDatabase;

        let start = Instant::now();
        let take_start = Instant::now();
        let changed_files = self.vfs.write().take_changes();
        let vfs_take_elapsed_ms = take_start.elapsed().as_millis() as u64;
        if changed_files.is_empty() {
            tracing::debug!(vfs_take_elapsed_ms, "process_changes: no VFS changes");
            return ChangeOutcome::default();
        }

        let file_count = changed_files.len();
        tracing::info!(file_count, vfs_take_elapsed_ms, "processing VFS changes");

        self.analysis_host.request_cancellation();

        let db = self.analysis_host.raw_database_mut();
        let source_root_id = base_db::SourceRootId(0);

        let source_root_input = db.source_root_input(source_root_id);
        let source_root = source_root_input.root(db);
        let mut file_set = source_root.file_set().clone();
        let mut file_set_modified = false;
        let mut config_file_changed = false;
        let mut bsl_source_changed = false;
        let mut changed_metadata_paths: Vec<std::path::PathBuf> = Vec::new();

        for file in changed_files {
            let text = match file.change {
                vfs::Change::Create(content, _) | vfs::Change::Modify(content, _) => Some(content),
                vfs::Change::Delete => None,
            };

            db.set_file_source_root(file.file_id, source_root_id);

            let is_bsl_path = {
                let vfs = self.vfs.read();
                let path = vfs.file_path(file.file_id);
                let path_path = path.as_path();
                let file_name = path_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if matches!(
                    file_name,
                    "bsl-analyzer.toml" | ".bsl-analyzer.json" | ".bsl-language-server.json"
                ) {
                    tracing::info!(path = %path_path.display(), "config file changed");
                    config_file_changed = true;
                }
                if project_model::is_metadata_path(path_path) {
                    tracing::info!(path = %path_path.display(), "metadata XML file changed");
                    changed_metadata_paths.push(path_path.to_path_buf());
                }
                project_model::is_bsl_source_path(path_path)
            };

            if is_bsl_path && text.is_none() {
                if file_set.path_for_file(&file.file_id).is_some() {
                    file_set.remove(file.file_id);
                    db.set_file_text(file.file_id, "");
                    file_set_modified = true;
                    bsl_source_changed = true;
                    tracing::warn!(
                        file_id = file.file_id.0,
                        "BSL file evicted from FileSet (deleted or unreadable); FileTextInput tombstoned",
                    );
                }
                continue;
            }

            if file_set.path_for_file(&file.file_id).is_none() {
                let vfs = self.vfs.read();
                let path = vfs.file_path(file.file_id);
                file_set.insert(file.file_id, path.clone());
                drop(vfs);
                file_set_modified = true;

                tracing::debug!(
                    file_id = file.file_id.0,
                    "added file to FileSet during process_changes"
                );
            }

            if let Some(text) = text {
                let store_in_salsa = ide_db::is_bsl_source(&file_set, file.file_id);
                if store_in_salsa {
                    // Open editor buffers are the source of truth for unsaved
                    // content, so they stay a resident overlay. Closed files are
                    // recorded by content revision only and re-read from disk on
                    // demand (LRU-evictable), so a whole workspace's closed-file
                    // text is not pinned in memory. Open-ness is keyed by `FileId`
                    // (resolved at didOpen) to avoid URL-encoding misclassification.
                    let is_open = self.open_files.contains(&file.file_id);
                    tracing::debug!(
                        file_id = file.file_id.0,
                        text_len = text.len(),
                        is_open,
                        "process_changes: file text"
                    );
                    if is_open {
                        db.set_file_text(file.file_id, &text);
                    } else {
                        db.set_file_revision_from_disk(
                            file.file_id,
                            base_db::content_revision(&text),
                        );
                    }
                    bsl_source_changed = true;
                }
            }
        }

        if file_set_modified {
            let updated_source_root = base_db::SourceRoot::new_local(file_set);
            db.set_source_root(source_root_id, updated_source_root);
        }

        if config_file_changed {
            if suppress_metadata_bump {
                tracing::debug!("suppressing project reload during initial sync");
            } else {
                self.reload_project_config();
            }
        }

        if !changed_metadata_paths.is_empty() {
            if suppress_metadata_bump {
                tracing::debug!("suppressing metadata revision bump during initial sync");
            } else {
                let db = self.analysis_host.raw_database_mut();
                for path in &changed_metadata_paths {
                    tracing::info!(path = %path.display(), "bumping config revision after XML change");
                    db.bump_config_for_path(path);
                }
            }
        }

        tracing::info!(
            file_count,
            vfs_take_elapsed_ms,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "process_changes complete",
        );

        // A suppressed batch (initial sync) neither reloaded the project nor bumped
        // metadata, so it must not claim those as observable changes.
        let metadata_changed = !suppress_metadata_bump && !changed_metadata_paths.is_empty();
        ChangeOutcome {
            config_file_changed: !suppress_metadata_bump && config_file_changed,
            affects_open_documents: bsl_source_changed || metadata_changed,
        }
    }

    /// Handle a removed directory subtree (delivered when a watch backend reports
    /// only the directory, not each child). Tombstones every loaded, closed file
    /// under one of `removed`, and invalidates the metadata of the owning config
    /// roots (a removed directory may have held metadata XML that, when coalesced,
    /// never arrived as its own event). Open files are left untouched — their
    /// editor buffer is authoritative. Returns whether open documents should be
    /// re-analyzed.
    pub fn remove_directories(&mut self, removed: &[paths::AbsPathBuf]) -> bool {
        use base_db::{SourceDatabase, SourceRootId};

        if removed.is_empty() {
            return false;
        }

        let mut descendants: Vec<VfsPath> = Vec::new();
        {
            let db = self.analysis_host.raw_database();
            let source_root = db.source_root_input(SourceRootId(0)).root(db);
            let file_set = source_root.file_set();
            for file_id in file_set.iter() {
                if self.open_files.contains(&file_id) {
                    continue;
                }
                let Some(vfs_path) = file_set.path_for_file(&file_id) else { continue };
                if removed.iter().any(|dir| vfs_path.as_path().starts_with(dir)) {
                    descendants.push(vfs_path.clone());
                }
            }
        }

        if !descendants.is_empty() {
            let mut vfs = self.vfs.write();
            for vfs_path in &descendants {
                vfs.set_file_contents(vfs_path.clone(), None);
            }
        }

        self.analysis_host.request_cancellation();
        self.analysis_host
            .raw_database_mut()
            .bump_config_for_paths(removed.iter().map(|p| p.as_ref()));

        // Apply the tombstones. We always invalidated a config root above, which
        // `ChangeOutcome` does not reflect, so a refresh is warranted regardless.
        self.process_changes(false);
        true
    }

    pub fn reload_project_config(&mut self) -> bool {
        let Some(root) = self.workspace_root.clone() else {
            return false;
        };
        tracing::info!("reloading project config");
        self.set_workspace_root(root);
        self.prune_stale_workspace_files();
        true
    }

    fn prune_stale_workspace_files(&mut self) {
        use base_db::{SourceDatabase, SourceRoot, SourceRootId};

        let allowed_roots = self.workspace_allowed_roots();
        if allowed_roots.is_empty() {
            return;
        }
        let open_paths = self.open_doc_paths();

        let source_root_id = SourceRootId(0);
        let mut new_file_set = vfs::file_set::FileSet::new();
        let mut dropped = 0usize;
        {
            let db = self.analysis_host.raw_database();
            let source_root_input = db.source_root_input(source_root_id);
            let source_root = source_root_input.root(db);
            let file_set = source_root.file_set();

            for file_id in file_set.iter() {
                let Some(vfs_path) = file_set.path_for_file(&file_id) else { continue };
                if path_in_workspace(vfs_path.as_path(), &allowed_roots, &open_paths) {
                    new_file_set.insert(file_id, vfs_path.clone());
                } else {
                    dropped += 1;
                }
            }
        }

        if dropped == 0 {
            return;
        }

        tracing::info!(dropped, "pruning stale files from SourceRoot after workspace reconfig");

        self.analysis_host.request_cancellation();
        let new_source_root = SourceRoot::new_local(new_file_set);
        self.analysis_host.raw_database_mut().set_source_root(source_root_id, new_source_root);
    }

    fn workspace_allowed_roots(&self) -> Vec<PathBuf> {
        let Some(project) = self.project.as_ref() else { return Vec::new() };
        let mut roots = vec![project.source_path().to_path_buf()];
        for (_name, ext_path) in project.extension_paths() {
            roots.push(ext_path.clone());
        }
        roots
    }

    fn open_doc_paths(&self) -> HashSet<PathBuf> {
        self.mem_docs.uris().into_iter().filter_map(|u| u.to_file_path().ok()).collect()
    }

    /// Whether the editor currently has this path open as a document. Checks the
    /// `FileId` open-set (the authority `process_changes` uses to choose overlay
    /// vs disk) as well as the URL-keyed buffer set, because the file-watcher's
    /// URL encoding can differ from the client's didOpen URL — a mismatch would
    /// otherwise let a disk-sourced change overwrite an open file's unsaved
    /// overlay.
    pub fn is_open_document_path(&self, std_path: &Path, vfs_path: &VfsPath) -> bool {
        let by_url =
            Url::from_file_path(std_path).map(|url| self.mem_docs.contains(&url)).unwrap_or(false);
        let by_id = self
            .vfs
            .read()
            .file_id(vfs_path)
            .is_some_and(|file_id| self.open_files.contains(&file_id));
        by_url || by_id
    }

    pub fn opened_document_uris(&self) -> Vec<Url> {
        self.mem_docs.uris()
    }

    pub fn init_source_root(&mut self) {
        use base_db::{SourceDatabase, SourceRoot, BSL_SOURCE_ROOT};

        let start = Instant::now();

        let allowed_roots = self.workspace_allowed_roots();
        let open_paths = self.open_doc_paths();

        let vfs = self.vfs.read();

        // Rebuild root(0) FRESH from the current VFS, `.bsl` sources only. A fresh
        // rebuild (no merge with the previous set) means a renamed/removed file's
        // stale entry cannot linger across a reload, and excluding metadata XML
        // keeps it out of the BSL iterators that scan root(0). Metadata composing
        // files live in the dedicated metadata root(1), owned by
        // [`bootstrap_metadata_substrate`].
        let mut bsl_file_set = vfs::file_set::FileSet::new();

        let mut vfs_files_skipped = 0;

        for file_id_raw in 0..vfs.num_file_ids() {
            let file_id = vfs::FileId(file_id_raw);
            if !vfs.exists(file_id) {
                continue;
            }
            let path = vfs.file_path(file_id);
            if !path_in_workspace(path.as_path(), &allowed_roots, &open_paths) {
                vfs_files_skipped += 1;
                continue;
            }

            if project_model::is_bsl_source_path(path.as_path()) {
                bsl_file_set.insert(file_id, path.clone());
            } else {
                vfs_files_skipped += 1;
            }
        }

        let bsl_files = bsl_file_set.len();
        drop(vfs);

        if bsl_files == 0 {
            tracing::warn!(
                elapsed_ms = start.elapsed().as_millis() as u64,
                "no .bsl files in VFS during init_source_root",
            );
            return;
        }

        let db = self.analysis_host.raw_database_mut();

        db.set_source_root(BSL_SOURCE_ROOT, SourceRoot::new_local(bsl_file_set));

        let bsl_ids: Vec<_> = db.source_root_input(BSL_SOURCE_ROOT).root(db).iter().collect();
        for file_id in bsl_ids {
            db.set_file_source_root(file_id, BSL_SOURCE_ROOT);
        }

        tracing::info!(
            bsl_files,
            vfs_files_skipped,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "rebuilt root(0) from VFS (.bsl only, fresh)",
        );
    }

    /// Build the metadata Salsa substrate from the filesystem: for each config
    /// root, discover its content-parsed MDOs, intern their composing files as
    /// versioned VFS inputs in the dedicated metadata root(1), record each file's
    /// on-disk content revision (text read on demand, not retained), and set the
    /// per-root structure listing that `resolve_metadata_object` reads. The walk is
    /// filesystem-authoritative (it does not enumerate VFS WatchOnly entries);
    /// `alloc_file_id` is idempotent, so it reuses the FileId already interned for a
    /// watched path. Runs after `init_source_root` and re-runs on reload.
    pub fn bootstrap_metadata_substrate(&mut self) {
        use base_db::{SourceDatabase, SourceRoot, METADATA_SOURCE_ROOT};
        use ide_db::metadata::MdoEntry;

        let start = Instant::now();

        let config_paths = self.analysis_host.raw_database().all_config_paths();
        if config_paths.is_empty() {
            return;
        }

        let mut metadata_file_set = vfs::file_set::FileSet::new();
        let mut revisions: Vec<(FileId, u64)> = Vec::new();
        let mut listings: Vec<(String, Vec<MdoEntry>)> = Vec::new();

        {
            let mut vfs = self.vfs.write();
            for (_, root_path) in &config_paths {
                let discovered = bsl_metadata::discover_metadata_structure(root_path);
                let mut entries = Vec::with_capacity(discovered.len());
                for d in discovered {
                    let Some(main) = enroll_metadata_file(
                        &mut vfs,
                        &d.main,
                        &mut metadata_file_set,
                        &mut revisions,
                    ) else {
                        continue;
                    };
                    let predefined = d.predefined.as_ref().and_then(|p| {
                        enroll_metadata_file(&mut vfs, p, &mut metadata_file_set, &mut revisions)
                    });
                    entries.push(MdoEntry { kind: d.mdo_type, name: d.name, main, predefined });
                }
                listings.push((root_path.to_string_lossy().to_string(), entries));
            }
        }

        let mdo_count: usize = listings.iter().map(|(_, e)| e.len()).sum();
        let file_count = revisions.len();

        let db = self.analysis_host.raw_database_mut();
        db.set_source_root(METADATA_SOURCE_ROOT, SourceRoot::new_local(metadata_file_set));
        for (fid, _) in &revisions {
            db.set_file_source_root(*fid, METADATA_SOURCE_ROOT);
        }
        for (fid, revision) in &revisions {
            db.set_file_revision_from_disk(*fid, *revision);
        }
        for (root, entries) in listings {
            db.set_metadata_listing(&root, entries);
        }

        tracing::info!(
            mdo_count,
            file_count,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "bootstrapped metadata substrate",
        );
    }

    pub fn warm_metadata_cache(&mut self) {
        let Some(ref project) = self.project else {
            tracing::debug!("no project, skipping metadata warmup");
            return;
        };

        let Some(config_path) = project.configuration_path() else {
            tracing::debug!("no configuration path, skipping metadata warmup");
            return;
        };

        let _span = tracing::info_span!("warm_metadata_cache", ?config_path).entered();
        let start = Instant::now();

        let db = self.analysis_host.raw_database();
        let path_input = ide_db::metadata::intern_configuration_path(
            db,
            &config_path.to_string_lossy(),
            db.config_root_revision_for_path(config_path),
        );

        let config = ide_db::metadata::load_configuration(db, path_input);

        tracing::info!(
            common_modules = config.common_modules().len(),
            metadata_objects = config.metadata_objects().len(),
            registers = config.registers().len(),
            "metadata cache warmed"
        );

        let extension_paths: Vec<_> = project.extension_paths().to_vec();
        for (name, ext_path) in &extension_paths {
            let ext_path_input = ide_db::metadata::intern_configuration_path(
                db,
                &ext_path.to_string_lossy(),
                db.config_root_revision_for_path(ext_path),
            );
            let ext_config = ide_db::metadata::load_configuration(db, ext_path_input);
            tracing::info!(
                extension = %name,
                common_modules = ext_config.common_modules().len(),
                metadata_objects = ext_config.metadata_objects().len(),
                "extension metadata cache warmed"
            );
        }

        tracing::info!(
            elapsed_ms = start.elapsed().as_millis() as u64,
            "warm_metadata_cache complete",
        );
    }

    pub fn vfs_file_for_url(&mut self, url: &Url) -> Result<FileId> {
        let path = url.to_file_path().map_err(|_| anyhow!("Invalid file URL: {}", url))?;
        if !project_model::is_bsl_source_path(&path) {
            return Err(anyhow!("File is not BSL, LSP unsupported: {}", url));
        }

        let vfs_path = VfsPath::new(path);

        let mut vfs = self.vfs.write();

        if let Some(file_id) = vfs.file_id(&vfs_path) {
            Ok(file_id)
        } else {
            Ok(vfs.alloc_file_id(vfs_path))
        }
    }

    pub fn url_for_file_id(&self, file_id: FileId) -> Result<Url> {
        let vfs = self.vfs.read();
        let path = vfs.file_path(file_id);

        let std_path = path.as_path();

        Url::from_file_path(std_path)
            .map_err(|_| anyhow!("Failed to convert path to URL: {:?}", std_path))
    }
}

/// Intern a metadata composing file's path to a stable [`FileId`], add it to the
/// metadata file set, and record its on-disk content revision. Returns `None` when
/// the file cannot be read (discovered then vanished), so the caller drops it from
/// the MDO. `alloc_file_id` is idempotent: an already-watched path keeps its id.
fn enroll_metadata_file(
    vfs: &mut vfs::Vfs,
    path: &Path,
    file_set: &mut vfs::file_set::FileSet,
    revisions: &mut Vec<(FileId, u64)>,
) -> Option<FileId> {
    let text = base_db::read_disk_text(path).ok()?;
    let vfs_path = VfsPath::new(path.to_path_buf());
    let file_id = vfs.alloc_file_id(vfs_path.clone());
    file_set.insert(file_id, vfs_path);
    revisions.push((file_id, base_db::content_revision(&text)));
    Some(file_id)
}

fn path_in_workspace(
    path: &Path,
    allowed_roots: &[PathBuf],
    open_paths: &HashSet<PathBuf>,
) -> bool {
    if allowed_roots.is_empty() {
        return true;
    }
    if allowed_roots.iter().any(|root| path.starts_with(root)) {
        return true;
    }
    open_paths.contains(path)
}
