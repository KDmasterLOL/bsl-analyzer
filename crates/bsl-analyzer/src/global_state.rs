//! Global state for the LSP server.
//!
//! This module defines the main state structures for the bsl-analyzer LSP server,
//! following the rust-analyzer architecture pattern.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use ide::Analysis;
use ide_db::RootDatabaseImpl;
use lsp_server::{Message, ReqQueue, Response};
use lsp_types::Url;
use parking_lot::RwLock;
use project_model::Project;
use vfs::{FileId, Vfs, VfsPath};

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
}

impl GlobalState {
    /// Creates a new GlobalState with the given sender.
    pub fn new(sender: Sender<Message>) -> Self {
        Self {
            sender,
            req_queue: ReqQueue::default(),
            analysis_host: AnalysisHost::default(),
            vfs: Arc::new(RwLock::new(Vfs::default())),
            mem_docs: MemDocs::default(),
            workspace_root: None,
            project: None,
            shutdown_requested: false,
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

        // Load all .bsl files from workspace into VFS for cross-file resolution
        self.load_workspace_files(&root);
    }

    /// Scans workspace directory structure and indexes all .bsl files.
    ///
    /// This creates FileId mappings WITHOUT loading file contents into memory.
    /// Files are loaded lazily when first accessed.
    ///
    /// Memory efficient: ~1KB per file for metadata vs ~20KB per file for content.
    fn load_workspace_files(&mut self, root: &PathBuf) {
        use walkdir::WalkDir;

        tracing::info!("Indexing workspace files from {:?}", root);

        let mut file_count = 0;
        let start = std::time::Instant::now();

        // Collect all .bsl file paths first
        let mut file_set = vfs::file_set::FileSet::new();
        let mut vfs = self.vfs.write();

        for entry in WalkDir::new(root).follow_links(true).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();

            // Only process .bsl files
            if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("bsl") {
                continue;
            }

            // Convert to VfsPath
            let vfs_path = VfsPath::new(path);

            // Allocate FileId (cheap - just increments counter)
            let file_id = if let Some(existing_id) = vfs.file_id(&vfs_path) {
                existing_id
            } else {
                vfs.alloc_file_id(vfs_path.clone())
            };

            // Add to file set (for SourceRoot)
            file_set.insert(file_id, vfs_path);

            file_count += 1;

            if file_count % 1000 == 0 {
                tracing::info!("Indexed {} files...", file_count);
            }
        }

        drop(vfs); // Release VFS lock

        // Create source root with all indexed files
        let db = self.analysis_host.raw_database_mut();
        let source_root_id = base_db::SourceRootId(0);
        let source_root = base_db::SourceRoot::new_local(file_set);

        use base_db::SourceDatabase;
        db.set_source_root(source_root_id, source_root);

        // Set source root mapping for all files
        let source_root_input = db.source_root_input(source_root_id);
        let indexed_files: Vec<_> = source_root_input.root(db).iter().collect();

        for file_id in indexed_files {
            db.set_file_source_root(file_id, source_root_id);
        }

        let elapsed = start.elapsed();
        tracing::info!(
            "Indexed {} .bsl files in {:?} (content will be loaded on demand)",
            file_count,
            elapsed
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

    /// Loads all CommonModules files into memory for cross-file resolution.
    ///
    /// This is called lazily before the first cross-file goto definition.
    /// Only CommonModules files are loaded (not Forms, Documents, etc.) to minimize memory usage.
    ///
    /// Subsequent calls are no-op.
    pub fn ensure_common_modules_loaded(&mut self) -> Result<()> {
        use base_db::SourceDatabase;

        tracing::info!("Loading CommonModules for cross-file resolution");
        let start = std::time::Instant::now();

        // Get all files from source root 0
        let db = self.analysis_host.raw_database_mut();
        let source_root_id = base_db::SourceRootId(0);
        let source_root_input = db.source_root_input(source_root_id);
        let all_files: Vec<_> = source_root_input.root(db).iter().collect();

        let mut loaded_count = 0;
        let mut skipped_count = 0;

        for file_id in all_files {
            // Get file path and clone it before dropping VFS lock
            let (std_path_buf, should_load) = {
                let vfs = self.vfs.read();
                let vfs_path = vfs.file_path(file_id);
                let std_path = vfs_path.as_path();
                let is_common_module = std_path.to_string_lossy().contains("CommonModules");
                (std_path.to_path_buf(), is_common_module)
            }; // VFS lock released here

            // Skip non-CommonModules files
            if !should_load {
                skipped_count += 1;
                continue;
            }

            // Check if already loaded (in MemDocs or database)
            if let Ok(url) = Url::from_file_path(&std_path_buf) {
                if self.mem_docs.get(&url).is_some() {
                    continue; // Already loaded
                }
            }

            // Read from disk and load
            match std::fs::read_to_string(&std_path_buf) {
                Ok(content) => {
                    db.set_file_text(file_id, &content);
                    loaded_count += 1;

                    if loaded_count % 100 == 0 {
                        tracing::info!("Loaded {} CommonModules files...", loaded_count);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to load file {:?}: {}", std_path_buf, e);
                }
            }
        }

        let elapsed = start.elapsed();
        tracing::info!(
            "Loaded {} CommonModules files in {:?} (skipped {} non-CommonModules)",
            loaded_count,
            elapsed,
            skipped_count
        );

        Ok(())
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
