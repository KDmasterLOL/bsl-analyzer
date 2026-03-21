//! Global state for the LSP server.
//!
//! This module defines the main state structures for the bsl-analyzer LSP server.

use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use base_db::{DiagnosticsConfigId, DiagnosticsConfigInput};
use crossbeam_channel::{Receiver, Sender};
use ide::Analysis;
use ide::RootDatabaseImpl;
use lsp_server::{Message, ReqQueue, Response};
use lsp_types::Url;
use parking_lot::RwLock;
use project_model::Project;
use vfs::loader::Handle;
use vfs::{loader, FileId, Vfs, VfsPath};

use crate::lsp::Progress;
use crate::mem_docs::MemDocs;
use crate::task_pool;

/// Task results from background threads.
#[derive(Debug)]
pub enum Task {
    /// Dependency preloading completed for a file.
    DependenciesPreloaded { file_id: FileId, count: usize },
    /// Diagnostics computed in background thread.
    DiagnosticsReady { uri: Url, diagnostics: Vec<lsp_types::Diagnostic>, generation: u64 },
    /// Diagnostics cancelled (Salsa query was interrupted).
    DiagnosticsCancelled { generation: u64 },
    /// Request to preload external files discovered during semantic highlighting.
    /// This enables faster goto_definition by warming caches before navigation.
    PreloadExternalFiles { files: Vec<FileId> },
}

/// The main state of the LSP server (mutable, main thread only).
///
/// This struct holds all the mutable state needed by the server:
/// - LSP communication channel
/// - Salsa database (via AnalysisHost wrapper)
/// - Virtual file system
/// - In-memory document tracking
/// - Request queue
///
/// # Thread Safety
/// GlobalState is not Send/Sync because it contains the mutable Salsa database.
/// For concurrent access, use `snapshot()` to create a `GlobalStateSnapshot`.
pub struct GlobalState {
    /// Sender for LSP messages to the client.
    pub sender: Sender<Message>,

    /// Queue for tracking pending requests.
    pub req_queue: ReqQueue<(), ()>,

    /// The Salsa database for analysis (wrapped in AnalysisHost).
    /// This is the mutable version - only accessible on main thread.
    pub analysis_host: AnalysisHost,

    /// Virtual file system for managing file contents.
    pub vfs: Arc<RwLock<Vfs>>,

    /// In-memory tracking of opened documents.
    pub mem_docs: MemDocs,

    /// Workspace root directory (from LSP initialize).
    pub workspace_root: Option<PathBuf>,

    /// Project configuration (loaded from .bsl-analyzer.json or .bsl-language-server.json config files).
    pub project: Option<Project>,

    /// Whether shutdown has been requested.
    pub shutdown_requested: bool,

    /// Handle to VFS loader thread for background file loading.
    pub loader: Box<dyn loader::Handle>,

    /// Receiver for loader messages (file loaded/changed/progress).
    pub loader_receiver: Receiver<loader::Message>,

    /// VFS loading progress state (config version counter).
    pub vfs_progress_config_version: u32,

    /// Whether VFS has finished initial loading.
    pub vfs_done: bool,

    /// Task pool for background tasks with result channel.
    pub task_pool: task_pool::Handle<Task>,

    /// Counter for generating unique LSP request IDs.
    next_request_id: AtomicI32,

    /// Current diagnostics configuration (hashable, for Salsa caching).
    ///
    /// This is the normalized config used for Salsa queries. When config file
    /// changes, this is updated and all diagnostic caches are invalidated.
    diagnostics_config: DiagnosticsConfigInput,

    /// Monotonically increasing generation counter for background diagnostics.
    /// Used to discard stale results when a newer change has been made.
    pub diagnostics_generation: u64,

    /// URI of the most recently changed file pending diagnostics scheduling.
    /// Set by `handle_did_change`, consumed after event loop drains all messages.
    pub pending_diagnostics_uri: Option<Url>,

    /// Last time progress was reported to the client.
    /// Used for throttling to avoid overwhelming the UI.
    pub last_progress_report: std::time::Instant,

    /// Buffer for VFS files during initial loading.
    /// Files are accumulated here and processed all at once after VFS Finished.
    pub pending_vfs_files: Vec<(paths::AbsPathBuf, Option<Vec<u8>>)>,
}

impl GlobalState {
    /// Creates a new GlobalState with the given sender.
    pub fn new(sender: Sender<Message>) -> Self {
        // Create loader thread for background file loading
        let (loader_sender, loader_receiver) = crossbeam_channel::unbounded();
        let loader = vfs_notify::NotifyHandle::spawn(loader_sender);

        Self {
            sender,
            req_queue: ReqQueue::default(),
            analysis_host: AnalysisHost::default(),
            vfs: Arc::new(RwLock::new(Vfs::default())),
            mem_docs: MemDocs::default(),
            workspace_root: None,
            project: None,
            shutdown_requested: false,
            loader: Box::new(loader),
            loader_receiver,
            vfs_progress_config_version: 0,
            vfs_done: false,
            task_pool: task_pool::TaskPool::new_with_handle(),
            next_request_id: AtomicI32::new(1),
            diagnostics_config: DiagnosticsConfigInput::new(),
            diagnostics_generation: 0,
            pending_diagnostics_uri: None,
            last_progress_report: std::time::Instant::now(),
            pending_vfs_files: Vec::new(),
        }
    }

    /// Gets the Salsa-interned diagnostics config ID.
    ///
    /// This ID is used in the `file_diagnostics_query` for Salsa caching.
    /// The same config produces the same ID (Salsa interning).
    pub fn diagnostics_config_id(&self) -> DiagnosticsConfigId<'_> {
        DiagnosticsConfigId::new(self.analysis_host.raw_database(), self.diagnostics_config.clone())
    }

    /// Gets a reference to the current diagnostics config.
    pub fn diagnostics_config(&self) -> &DiagnosticsConfigInput {
        &self.diagnostics_config
    }

    /// Updates diagnostics config from project settings.
    ///
    /// Called when project is loaded or config file changes.
    /// This invalidates all cached diagnostics (new config ID = new hash).
    pub fn update_diagnostics_config(&mut self) {
        self.diagnostics_config =
            self.project.as_ref().map(Self::config_from_project).unwrap_or_default();

        tracing::info!(
            disabled_count = self.diagnostics_config.disabled.len(),
            enabled_count = self.diagnostics_config.enabled.len(),
            params_count = self.diagnostics_config.parameters.len(),
            "updated diagnostics config"
        );

        if !self.diagnostics_config.disabled.is_empty() {
            tracing::debug!(
                disabled = ?self.diagnostics_config.disabled,
                "disabled diagnostics from config"
            );
        }
    }

    /// Converts project diagnostics config to hashable DiagnosticsConfigInput.
    ///
    /// Deserializes the raw JSON into `ide::DiagnosticsConfig`,
    /// then converts to the Salsa-compatible `DiagnosticsConfigInput`.
    fn config_from_project(project: &Project) -> DiagnosticsConfigInput {
        // Deserialize JSON into ide::DiagnosticsConfig
        let config: ide::DiagnosticsConfig = match serde_json::from_value(
            project.config.diagnostics.clone(),
        ) {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!(error = %e, "failed to deserialize diagnostics config, using defaults");
                ide::DiagnosticsConfig::default()
            }
        };

        // Convert DiagnosticCode enums back to strings for Salsa hashing
        let disabled: Vec<String> = config.disabled.iter().map(|code| code.to_string()).collect();
        let enabled: Vec<String> = config.enabled.iter().map(|code| code.to_string()).collect();

        // Convert parameters HashMap<DiagnosticCode, Value> to Vec<(String, String)>
        let parameters: Vec<(String, String)> = config
            .parameters
            .iter()
            .map(|(code, value)| {
                (code.to_string(), serde_json::to_string(value).unwrap_or_default())
            })
            .collect();

        DiagnosticsConfigInput::from_raw(
            disabled,
            enabled,
            parameters,
            config.ordinary_app_support,
            config.dataflow_max_iterations,
        )
    }

    /// Generates a unique request ID for LSP requests.
    fn next_request_id(&self) -> lsp_server::RequestId {
        lsp_server::RequestId::from(self.next_request_id.fetch_add(1, Ordering::SeqCst))
    }

    /// Reports progress to the LSP client using WorkDoneProgress protocol.
    ///
    /// This sends progress notifications that VS Code displays as a progress bar.
    pub fn report_progress(
        &self,
        title: &str,
        state: Progress,
        message: Option<String>,
        fraction: Option<f64>,
    ) {
        let percentage = fraction.map(|f| (f * 100.0) as u32);
        let token = lsp_types::ProgressToken::String(format!("bslAnalyzer/{title}"));

        let work_done_progress = match state {
            Progress::Begin => {
                // Create progress token first
                let params = lsp_types::WorkDoneProgressCreateParams { token: token.clone() };
                let request = lsp_server::Request::new(
                    self.next_request_id(),
                    "window/workDoneProgress/create".to_string(),
                    params,
                );
                self.sender.send(request.into()).ok();

                lsp_types::WorkDoneProgress::Begin(lsp_types::WorkDoneProgressBegin {
                    title: title.into(),
                    cancellable: Some(false),
                    message,
                    percentage,
                })
            }
            Progress::Report => {
                lsp_types::WorkDoneProgress::Report(lsp_types::WorkDoneProgressReport {
                    cancellable: Some(false),
                    message,
                    percentage,
                })
            }
            Progress::End => {
                lsp_types::WorkDoneProgress::End(lsp_types::WorkDoneProgressEnd { message })
            }
        };

        let notification = lsp_server::Notification::new(
            "$/progress".to_string(),
            lsp_types::ProgressParams {
                token,
                value: lsp_types::ProgressParamsValue::WorkDone(work_done_progress),
            },
        );
        self.sender.send(notification.into()).ok();
    }

    /// Initialize an empty SourceRoot(0) before event loop starts.
    ///
    /// Prevents race conditions where files are opened via LSP before
    /// VFS loader finishes. SourceRoot will be populated later by
    /// process_changes() and updated by init_source_root().
    ///
    /// This matches the pattern used in tests and ensures that
    /// process_changes() can always safely call set_file_source_root().
    pub fn init_empty_source_root(&mut self) {
        use base_db::{SourceDatabase, SourceRoot, SourceRootId};

        let db = self.analysis_host.raw_database_mut();
        let source_root_id = SourceRootId(0);

        // Create empty FileSet (will be populated by process_changes)
        let file_set = vfs::file_set::FileSet::new();
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);

        tracing::debug!("initialized empty SourceRoot(0) before event loop");
    }

    /// Sets the workspace root and loads project configuration.
    pub fn set_workspace_root(&mut self, root: PathBuf) {
        tracing::info!(?root, "setting workspace root");

        // Load project configuration from workspace root
        let project = Project::new(&root);

        // Get source path (configuration directory or project root)
        let source_path = project.source_path().to_path_buf();

        tracing::info!(
            ?source_path,
            configuration_found = project.configuration_path().is_some(),
            "loaded project, scanning source path"
        );

        self.workspace_root = Some(root.clone());
        self.project = Some(project);

        // Update diagnostics config from project settings
        self.update_diagnostics_config();

        // Configure VFS loader to scan source path in background thread
        self.vfs_progress_config_version += 1;

        // Config files to watch for changes
        let config_files: Vec<paths::AbsPathBuf> =
            [".bsl-analyzer.json", ".bsl-language-server.json"]
                .iter()
                .map(|name| root.join(name))
                .filter(|p| p.exists())
                .map(paths::AbsPathBuf::assert_utf8)
                .collect();

        let mut load_entries = vec![loader::Entry::Directories(loader::Directories {
            extensions: vec!["bsl".to_string(), "os".to_string(), "xml".to_string()],
            include: vec![paths::AbsPathBuf::assert_utf8(source_path)],
            exclude: vec![
                paths::AbsPathBuf::assert_utf8(root.join(".git")),
                paths::AbsPathBuf::assert_utf8(root.join("build")),
                paths::AbsPathBuf::assert_utf8(root.join(".vscode")),
            ],
        })];

        // Add config files as separate entry (index 1) and watch them
        let watch = if config_files.is_empty() {
            vec![0] // Watch only directories
        } else {
            load_entries.push(loader::Entry::Files(config_files));
            vec![0, 1] // Watch directories (0) and config files (1)
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
    ///
    /// Should be called after receiving loader messages or LSP file changes.
    pub fn process_changes(&mut self) -> (bool, bool) {
        use base_db::SourceDatabase;

        let changed_files = self.vfs.write().take_changes();
        if changed_files.is_empty() {
            return (false, false);
        }

        tracing::info!(file_count = changed_files.len(), "processing VFS changes");

        let db = self.analysis_host.raw_database_mut();
        let source_root_id = base_db::SourceRootId(0);

        // Get current SourceRoot and FileSet
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

            // Map file to SourceRoot in database
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
                // This invalidates Salsa cache for this file!
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

        // Update SourceRoot if FileSet changed
        if file_set_modified {
            let updated_source_root = base_db::SourceRoot::new_local(file_set);
            db.set_source_root(source_root_id, updated_source_root);
        }

        // Reload project config if config file changed
        // This invalidates all diagnostic caches (new config ID = new hash)
        if config_file_changed {
            if let Some(root) = self.workspace_root.clone() {
                tracing::info!("reloading project config after config file change");
                let project = Project::new(&root);
                self.project = Some(project);
                self.update_diagnostics_config();
            }
        }

        // Bump metadata version if XML files changed.
        // This invalidates Salsa cache for load_configuration by producing a new
        // ConfigurationPathInput interned key with an incremented version field.
        if metadata_xml_changed {
            tracing::info!("bumping metadata version after XML change");
            self.analysis_host.raw_database().bump_metadata_version();
        }

        (true, config_file_changed)
    }

    /// Returns URIs of all currently opened documents (for re-running diagnostics).
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

        // Get existing SourceRoot (may contain LSP-opened files)
        let db = self.analysis_host.raw_database_mut();
        let existing_source_root = db.source_root_input(source_root_id);
        let mut file_set = existing_source_root.root(db).file_set().clone();

        let mut vfs_files_added = 0;

        // Collect all VFS-loaded files
        for file_id_raw in 0..vfs.num_file_ids() {
            let file_id = vfs::FileId(file_id_raw);
            if vfs.exists(file_id) {
                let path = vfs.file_path(file_id);

                // Track new files (not already in FileSet)
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

        // Update SourceRoot with merged FileSet
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(source_root_id, source_root);

        // Update file→source_root mappings
        let source_root_input = db.source_root_input(source_root_id);
        let indexed_files: Vec<_> = source_root_input.root(db).iter().collect();

        for file_id in indexed_files {
            db.set_file_source_root(file_id, source_root_id);
        }

        tracing::info!(total_files, vfs_files_added, "updated SourceRoot with VFS files (merged)");
    }

    /// Eagerly load metadata to warm Salsa cache.
    ///
    /// This prevents the delay on first file open by loading metadata
    /// immediately after VFS loading completes.
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

        // This call warms the Salsa cache - subsequent calls will be instant
        let config = ide_db::metadata::load_configuration(db, path_input);

        tracing::info!(
            common_modules = config.common_modules().len(),
            metadata_objects = config.metadata_objects().len(),
            registers = config.registers().len(),
            "metadata cache warmed"
        );
    }

    /// Creates an immutable snapshot for thread-safe access.
    ///
    /// This is a cheap operation (Arc clone + generation bump in Salsa).
    /// The snapshot can be sent to worker threads safely.
    pub fn snapshot(&self) -> GlobalStateSnapshot {
        GlobalStateSnapshot {
            analysis: self.analysis_host.analysis(),
            vfs: Arc::clone(&self.vfs),
            mem_docs: self.mem_docs.clone(),
            workspace_root: self.workspace_root.clone(),
            project: self.project.clone(),
            diagnostics_config: self.diagnostics_config.clone(),
            vfs_done: self.vfs_done,
            task_sender: self.task_pool.pool.sender.clone(),
        }
    }

    /// Sends a response to the client.
    pub fn respond(&mut self, response: Response) {
        if let Err(e) = self.sender.send(response.into()) {
            tracing::error!("Failed to send response: {}", e);
        }
    }

    /// Requests the client to refresh semantic tokens for all open files.
    ///
    /// This sends a `workspace/semanticTokens/refresh` request to the client,
    /// asking it to re-request semantic tokens. Used after VFS loading completes
    /// when we may have returned empty tokens earlier.
    pub fn request_semantic_tokens_refresh(&mut self) {
        use lsp_server::Request;
        use std::sync::atomic::Ordering;

        let id = self.next_request_id.fetch_add(1, Ordering::SeqCst);
        let request = Request::new(
            lsp_server::RequestId::from(id),
            "workspace/semanticTokens/refresh".to_string(),
            (),
        );

        if let Err(e) = self.sender.send(request.into()) {
            tracing::error!("Failed to send semantic tokens refresh request: {}", e);
        } else {
            tracing::info!("Requested client to refresh semantic tokens");
        }
    }

    /// Gets or creates a FileId for the given URL.
    ///
    /// This method:
    /// 1. Converts URL to VfsPath
    /// 2. Looks up or allocates FileId in VFS
    ///
    /// # Errors
    /// Returns an error if the URL is invalid (not a file:// URL).
    pub fn vfs_file_for_url(&mut self, url: &Url) -> Result<FileId> {
        let path = url.to_file_path().map_err(|_| anyhow!("Invalid file URL: {}", url))?;

        let vfs_path = VfsPath::new(path);

        let mut vfs = self.vfs.write();

        // Try to find existing FileId, or allocate new one
        if let Some(file_id) = vfs.file_id(&vfs_path) {
            Ok(file_id)
        } else {
            // Allocate new FileId (VFS handles path storage internally)
            Ok(vfs.alloc_file_id(vfs_path))
        }
    }

    /// Gets the URL for a FileId.
    ///
    /// # Errors
    /// Returns an error if the path cannot be converted to URL.
    pub fn url_for_file_id(&self, file_id: FileId) -> Result<Url> {
        let vfs = self.vfs.read();
        let path = vfs.file_path(file_id);

        let std_path = path.as_path();

        Url::from_file_path(std_path)
            .map_err(|_| anyhow!("Failed to convert path to URL: {:?}", std_path))
    }
}

/// Immutable snapshot of GlobalState for thread-safe access.
///
/// This struct contains Arc-wrapped versions of the state,
/// allowing it to be safely sent to worker threads.
pub struct GlobalStateSnapshot {
    /// Immutable analysis API (Salsa snapshot).
    pub analysis: Analysis,

    /// Virtual file system (read-only access).
    pub vfs: Arc<RwLock<Vfs>>,

    /// In-memory document tracking (clone).
    pub mem_docs: MemDocs,

    /// Workspace root directory.
    pub workspace_root: Option<PathBuf>,

    /// Project configuration.
    pub project: Option<Project>,

    /// Current diagnostics configuration (for code action handler).
    pub diagnostics_config: DiagnosticsConfigInput,

    /// Whether VFS loading has completed.
    pub vfs_done: bool,

    /// Task sender for triggering background work from handlers.
    /// Used for preloading external files discovered during semantic highlighting.
    pub task_sender: Sender<Task>,
}

impl GlobalStateSnapshot {
    /// Gets the FileId for a URL.
    ///
    /// # Errors
    /// Returns an error if the URL is invalid or not in VFS.
    pub fn file_id_for_url(&self, url: &Url) -> Result<FileId> {
        let path = url.to_file_path().map_err(|_| anyhow!("Invalid file URL: {}", url))?;

        let vfs_path = VfsPath::new(path);

        let vfs = self.vfs.read();
        vfs.file_id(&vfs_path).ok_or_else(|| anyhow!("File not in VFS: {}", url))
    }

    /// Gets the URL for a FileId.
    ///
    /// # Errors
    /// Returns an error if the path cannot be converted to URL.
    pub fn url_for_file_id(&self, file_id: FileId) -> Result<Url> {
        let vfs = self.vfs.read();
        let path = vfs.file_path(file_id);

        let std_path = path.as_path();

        Url::from_file_path(std_path)
            .map_err(|_| anyhow!("Failed to convert path to URL: {:?}", std_path))
    }
}

/// Wrapper around the mutable Salsa database.
///
/// AnalysisHost provides controlled access to the database,
/// allowing snapshots for concurrent queries.
#[derive(Default)]
pub struct AnalysisHost {
    db: RootDatabaseImpl,
}

impl AnalysisHost {
    /// Creates an Analysis snapshot for queries.
    ///
    /// This is a cheap operation that creates an immutable view of the database.
    /// Note: Salsa 0.25+ uses the database directly without explicit snapshots.
    pub fn analysis(&self) -> Analysis {
        // In Salsa 0.25+, the database itself provides snapshot semantics
        // We create a new Analysis referencing the current database state
        Analysis::from_database(self.db.clone())
    }

    /// Gets immutable access to the database.
    ///
    /// Used for Salsa interned types (DiagnosticsConfigId) and queries.
    pub fn raw_database(&self) -> &RootDatabaseImpl {
        &self.db
    }

    /// Gets mutable access to the database.
    ///
    /// Use this to apply changes (file updates, config changes, etc.).
    pub fn raw_database_mut(&mut self) -> &mut RootDatabaseImpl {
        &mut self.db
    }
}

#[cfg(test)]
mod vfs_race_tests {
    use super::*;
    use base_db::SourceDatabase;
    use std::sync::Arc;

    #[test]
    fn test_empty_source_root_init() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();

        // Verify SourceRoot(0) exists and is empty
        use base_db::{SourceDatabase, SourceRootId};
        let db = state.analysis_host.raw_database_mut();
        let sr = db.source_root_input(SourceRootId(0));
        assert!(!sr.root(db).is_library);
        assert_eq!(sr.root(db).file_set().len(), 0);
    }

    #[test]
    fn test_lsp_before_loader() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);

        // Phase 1: Early init
        state.init_empty_source_root();

        // Phase 2: LSP opens file (simulate didOpen)
        let uri = lsp_types::Url::parse("file:///user.bsl").unwrap();
        let file_id = state.vfs_file_for_url(&uri).unwrap();

        {
            let vfs_path = vfs::VfsPath::new(uri.to_file_path().unwrap());
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(vfs_path, Some(Arc::from("Процедура Test() КонецПроцедуры")));
        }

        state.process_changes();

        // Verify file accessible
        let text1 = {
            let db = state.analysis_host.raw_database_mut();
            db.file_text_input(file_id).text(db)
        };
        assert!(text1.contains("Test"));

        // Phase 3: Loader finishes (simulate VFS loading other files)
        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                vfs::VfsPath::new("/loader.bsl"),
                Some(Arc::from("// Loader file")),
            );
        }

        state.process_changes();
        state.init_source_root();

        // Phase 4: Original file still accessible after merge
        let text2 = {
            let db = state.analysis_host.raw_database_mut();
            db.file_text_input(file_id).text(db)
        };
        assert!(text2.contains("Test"));
        assert_eq!(text1, text2, "file lost after merge!");
    }
}
