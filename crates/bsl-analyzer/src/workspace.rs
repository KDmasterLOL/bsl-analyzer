//! Workspace management — VFS, source roots, file loading, metadata.
//!
//! These methods on `GlobalState` handle:
//! - VFS initialization and file loading
//! - Source root management (init, merge after loader)
//! - Workspace root setup and project config
//! - Metadata cache warming
//! - File ↔ URL resolution

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use lsp_types::Url;
use vfs::{loader, FileId, VfsPath};

use crate::global_state::GlobalState;

impl GlobalState {
    /// Initialize an empty SourceRoot(0) before event loop starts.
    ///
    /// Prevents race conditions where files are opened via LSP before
    /// VFS loader finishes. SourceRoot will be populated later by
    /// process_changes() and updated by init_source_root().
    pub fn init_empty_source_root(&mut self) {
        use base_db::{SourceDatabase, SourceRoot, SourceRootId};

        let db = self.analysis_host.raw_database_mut();
        let source_root_id = SourceRootId(0);

        let file_set = vfs::file_set::FileSet::new();
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);

        tracing::debug!("initialized empty SourceRoot(0) before event loop");
    }

    /// Sets the workspace root and loads project configuration.
    pub fn set_workspace_root(&mut self, root: PathBuf) {
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

        // Register all configuration paths (main + extensions) in the database
        {
            let mut all_paths: Vec<(Option<String>, std::path::PathBuf)> = Vec::new();
            all_paths.push((None, source_path.clone()));
            for (name, ext_path) in &extensions {
                all_paths.push((Some(name.clone()), ext_path.clone()));
            }
            self.analysis_host.raw_database().set_all_config_paths(all_paths);
        }

        // Update diagnostics config from project settings
        self.update_diagnostics_config();

        // Configure VFS loader to scan source path in background thread
        self.vfs_progress_config_version += 1;

        // Config files to watch for changes
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
            extensions: vec!["bsl".to_string(), "os".to_string(), "xml".to_string()],
            include,
            exclude: vec![
                paths::AbsPathBuf::assert_utf8(root.join(".git")),
                paths::AbsPathBuf::assert_utf8(root.join("build")),
                paths::AbsPathBuf::assert_utf8(root.join(".vscode")),
            ],
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
    }

    /// Process VFS changes and sync to Salsa database.
    ///
    /// This method:
    /// 1. Takes all pending changes from VFS
    /// 2. Applies them to the Salsa database
    /// 3. Ensures files are mapped to SourceRoot and added to FileSet
    /// 4. Returns (has_changes, config_changed)
    pub fn process_changes(&mut self) -> (bool, bool) {
        use base_db::SourceDatabase;

        let changed_files = self.vfs.write().take_changes();
        if changed_files.is_empty() {
            return (false, false);
        }

        tracing::info!(file_count = changed_files.len(), "processing VFS changes");

        // Wake outstanding Salsa snapshots before invoking any setter. Pairs
        // with the short-lock pattern in `base_db::Files::set_*` to prevent
        // the DashMap/Salsa ABBA described in `Files`'s doc-comment.
        self.analysis_host.request_cancellation();

        let db = self.analysis_host.raw_database_mut();
        let source_root_id = base_db::SourceRootId(0);

        let source_root_input = db.source_root_input(source_root_id);
        let source_root = source_root_input.root(db);
        let mut file_set = source_root.file_set().clone();
        let mut file_set_modified = false;
        let mut config_file_changed = false;
        let mut metadata_xml_changed = false;

        for file in changed_files {
            let text = match file.change {
                vfs::Change::Create(content, _) | vfs::Change::Modify(content, _) => Some(content),
                vfs::Change::Delete => None,
            };

            db.set_file_source_root(file.file_id, source_root_id);

            // Check if this is a config file change
            {
                let vfs = self.vfs.read();
                let path = vfs.file_path(file.file_id);
                let path_str = path.as_path().to_string_lossy();
                if path_str.ends_with(".bsl-analyzer.json")
                    || path_str.ends_with(".bsl-language-server.json")
                {
                    tracing::info!(path = %path_str, "config file changed");
                    config_file_changed = true;
                }
                if !metadata_xml_changed && path_str.ends_with(".xml") {
                    tracing::info!(path = %path_str, "metadata XML file changed");
                    metadata_xml_changed = true;
                }
            }

            // Ensure file is in SourceRoot's FileSet
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
                let path_str = {
                    let vfs = self.vfs.read();
                    format!("{:?}", vfs.file_path(file.file_id))
                };
                tracing::debug!(
                    file_id = file.file_id.0,
                    path = %path_str,
                    text_len = text.len(),
                    "process_changes: set_file_text (invalidates Salsa cache)"
                );
                db.set_file_text(file.file_id, &text);
            }
        }

        if file_set_modified {
            let updated_source_root = base_db::SourceRoot::new_local(file_set);
            db.set_source_root(source_root_id, updated_source_root);
        }

        if config_file_changed {
            self.reload_project_config();
        }

        if metadata_xml_changed {
            tracing::info!("bumping metadata version after XML change");
            self.analysis_host.raw_database().bump_metadata_version();
        }

        (true, config_file_changed)
    }

    /// Reloads project config from disk and updates diagnostics config.
    pub fn reload_project_config(&mut self) -> bool {
        if let Some(root) = self.workspace_root.clone() {
            tracing::info!("reloading project config");
            let project = project_model::Project::new(&root);
            self.project = Some(project);
            self.update_diagnostics_config();
            true
        } else {
            false
        }
    }

    /// Returns URIs of all currently opened documents.
    pub fn opened_document_uris(&self) -> Vec<Url> {
        self.mem_docs.uris()
    }

    /// Initialize or update SourceRoot after VFS loading completes.
    ///
    /// Merges VFS-loaded files with existing SourceRoot to preserve
    /// files opened via LSP before loader finished.
    pub fn init_source_root(&mut self) {
        use base_db::{SourceDatabase, SourceRoot, SourceRootId};

        let source_root_id = SourceRootId(0);
        let vfs = self.vfs.read();

        let db = self.analysis_host.raw_database_mut();
        let existing_source_root = db.source_root_input(source_root_id);
        let mut file_set = existing_source_root.root(db).file_set().clone();

        let mut vfs_files_added = 0;

        for file_id_raw in 0..vfs.num_file_ids() {
            let file_id = vfs::FileId(file_id_raw);
            if vfs.exists(file_id) {
                let path = vfs.file_path(file_id);

                if file_set.path_for_file(&file_id).is_none() {
                    vfs_files_added += 1;
                }
                file_set.insert(file_id, path.clone());
            }
        }

        let total_files = file_set.len();
        drop(vfs);

        if total_files == 0 {
            tracing::warn!("no files in VFS during init_source_root");
            return;
        }

        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(source_root_id, source_root);

        let source_root_input = db.source_root_input(source_root_id);
        let indexed_files: Vec<_> = source_root_input.root(db).iter().collect();

        for file_id in indexed_files {
            db.set_file_source_root(file_id, source_root_id);
        }

        tracing::info!(total_files, vfs_files_added, "updated SourceRoot with VFS files (merged)");
    }

    /// Eagerly load metadata to warm Salsa cache.
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

        let db = self.analysis_host.raw_database();
        let path_input = ide_db::metadata::ConfigurationPathInput::new(
            db,
            config_path.to_string_lossy().into_owned(),
            db.metadata_version(),
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
            let ext_path_input = ide_db::metadata::ConfigurationPathInput::new(
                db,
                ext_path.to_string_lossy().into_owned(),
                db.metadata_version(),
            );
            let ext_config = ide_db::metadata::load_configuration(db, ext_path_input);
            tracing::info!(
                extension = %name,
                common_modules = ext_config.common_modules().len(),
                metadata_objects = ext_config.metadata_objects().len(),
                "extension metadata cache warmed"
            );
        }
    }

    /// Gets or creates a FileId for the given URL.
    pub fn vfs_file_for_url(&mut self, url: &Url) -> Result<FileId> {
        let path = url.to_file_path().map_err(|_| anyhow!("Invalid file URL: {}", url))?;

        let vfs_path = VfsPath::new(path);

        let mut vfs = self.vfs.write();

        if let Some(file_id) = vfs.file_id(&vfs_path) {
            Ok(file_id)
        } else {
            Ok(vfs.alloc_file_id(vfs_path))
        }
    }

    /// Gets the URL for a FileId.
    pub fn url_for_file_id(&self, file_id: FileId) -> Result<Url> {
        let vfs = self.vfs.read();
        let path = vfs.file_path(file_id);

        let std_path = path.as_path();

        Url::from_file_path(std_path)
            .map_err(|_| anyhow!("Failed to convert path to URL: {:?}", std_path))
    }
}
