//! Global state for the LSP server.
//!
//! This module defines the main state structures for the bsl-analyzer LSP server,
//! following the rust-analyzer architecture pattern.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use crossbeam_channel::{Receiver, Sender};
use ide::Analysis;
use ide_db::RootDatabaseImpl;
use lsp_server::{Message, ReqQueue, Response};
use lsp_types::Url;
use parking_lot::RwLock;
use project_model::Project;
use vfs::loader::Handle;
use vfs::{loader, FileId, Vfs, VfsPath};

use crate::mem_docs::MemDocs;

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

    /// Project configuration (loaded from .bsl-analyzer.json or .bsl-language-server.json).
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
        }
    }

    /// Sets the workspace root and loads project configuration.
    pub fn set_workspace_root(&mut self, root: PathBuf) {
        tracing::info!(?root, "setting workspace root");

        // Load project configuration from workspace root
        let project = Project::new(&root);

        tracing::info!(
            configuration_root = ?project.config.configuration_root,
            "loaded project configuration"
        );

        self.workspace_root = Some(root.clone());
        self.project = Some(project);

        // Configure VFS loader to scan workspace in background thread
        self.vfs_progress_config_version += 1;
        self.loader.set_config(loader::Config {
            load: vec![loader::Entry::Directories(loader::Directories {
                extensions: vec!["bsl".to_string(), "os".to_string()],
                include: vec![paths::AbsPathBuf::assert_utf8(root.clone())],
                exclude: vec![
                    paths::AbsPathBuf::assert_utf8(root.join(".git")),
                    paths::AbsPathBuf::assert_utf8(root.join("build")),
                    paths::AbsPathBuf::assert_utf8(root.join(".vscode")),
                ],
            })],
            watch: vec![0], // Watch first entry for file changes
            version: self.vfs_progress_config_version,
        });
    }

    /// Process VFS changes and sync to Salsa database.
    ///
    /// This method:
    /// 1. Takes all pending changes from VFS
    /// 2. Applies them to the Salsa database
    /// 3. Ensures files are mapped to SourceRoot
    /// 4. Returns true if any changes were processed
    ///
    /// Should be called after receiving loader messages or LSP file changes.
    pub fn process_changes(&mut self) -> bool {
        use base_db::SourceDatabase;

        let changed_files = self.vfs.write().take_changes();
        if changed_files.is_empty() {
            return false;
        }

        tracing::info!(file_count = changed_files.len(), "processing VFS changes");

        let db = self.analysis_host.raw_database_mut();
        let source_root_id = base_db::SourceRootId(0);

        for file in changed_files {
            let text = match file.change {
                vfs::Change::Create(content, _) | vfs::Change::Modify(content, _) => Some(content),
                vfs::Change::Delete => None,
            };

            // Ensure file is mapped to SourceRoot (important for files opened via LSP)
            db.set_file_source_root(file.file_id, source_root_id);

            if let Some(text) = text {
                db.set_file_text(file.file_id, &text);
            }
        }

        true
    }

    /// Initialize SourceRoot after VFS loading completes.
    ///
    /// This creates a single SourceRoot containing all files loaded by the VFS loader.
    /// Should be called once after vfs_done becomes true.
    pub fn init_source_root(&mut self) {
        use base_db::SourceDatabase;

        // Collect all files that exist in VFS by checking which files have been registered
        // We need to iterate through all FileIds and check if they exist
        let vfs = self.vfs.read();
        let mut file_set = vfs::file_set::FileSet::new();

        // Find the maximum FileId by checking what's been allocated
        // We'll iterate through FileIds and check if they exist
        for file_id_raw in 0..10000u32 {
            // Reasonable upper limit
            let file_id = FileId(file_id_raw);
            if vfs.exists(file_id) {
                let path = vfs.file_path(file_id);
                file_set.insert(file_id, path.clone());
            }
        }

        let file_count = file_set.len();
        drop(vfs);

        if file_count == 0 {
            tracing::warn!("No files found in VFS during init_source_root");
            return;
        }

        // Create single source root for all files
        let db = self.analysis_host.raw_database_mut();
        let source_root_id = base_db::SourceRootId(0);
        let source_root = base_db::SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);

        // Set source root mapping for all files
        let source_root_input = db.source_root_input(source_root_id);
        let indexed_files: Vec<_> = source_root_input.root(db).iter().collect();

        for file_id in indexed_files {
            db.set_file_source_root(file_id, source_root_id);
        }

        tracing::info!(file_count, "initialized source root");
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
        }
    }

    /// Sends a response to the client.
    pub fn respond(&mut self, response: Response) {
        if let Err(e) = self.sender.send(response.into()) {
            tracing::error!("Failed to send response: {}", e);
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

    /// Gets mutable access to the database.
    ///
    /// Use this to apply changes (file updates, config changes, etc.).
    pub fn raw_database_mut(&mut self) -> &mut RootDatabaseImpl {
        &mut self.db
    }
}
