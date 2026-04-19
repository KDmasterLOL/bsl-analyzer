//! Global state for the LSP server.
//!
//! This module defines the main state structures for the bsl-analyzer LSP server.
//!
//! ## Module structure
//!
//! - **`global_state`** (this file) — `GlobalState` struct, `new()`, `snapshot()`, LSP transport
//! - **`analysis_host`** — `AnalysisHost` wrapper around Salsa database
//! - **`workspace`** — VFS, source roots, file loading, metadata warming
//! - **`diagnostics_state`** — Diagnostics config management

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use base_db::DiagnosticsConfigInput;
use crossbeam_channel::{Receiver, Sender};
use ide::Analysis;
use lsp_server::{Message, ReqQueue, Response};
use lsp_types::Url;
use parking_lot::RwLock;
use project_model::Project;
use vfs::loader::Handle;
use vfs::{loader, Vfs};

use crate::analysis_host::AnalysisHost;
use crate::lsp::Progress;
use crate::mem_docs::MemDocs;
use crate::task_pool;

/// Task results from background threads.
#[derive(Debug)]
pub enum Task {
    /// Dependency preloading completed for a file.
    DependenciesPreloaded { file_id: vfs::FileId, count: usize },
    /// Diagnostics computed in background thread.
    DiagnosticsReady { uri: Url, diagnostics: Vec<lsp_types::Diagnostic>, generation: u64 },
    /// Diagnostics cancelled (Salsa query was interrupted).
    DiagnosticsCancelled { generation: u64 },
    /// Request to preload external files discovered during semantic highlighting.
    PreloadExternalFiles { files: Vec<vfs::FileId> },
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

    /// Task pool for background tasks with result channel.
    pub task_pool: task_pool::Handle<Task>,

    /// Counter for generating unique LSP request IDs.
    pub(crate) next_request_id: AtomicI32,

    /// Current diagnostics configuration (hashable, for Salsa caching).
    pub(crate) diagnostics_config: DiagnosticsConfigInput,

    /// Monotonically increasing generation counter for background diagnostics.
    pub diagnostics_generation: u64,

    /// URI of the most recently changed file pending diagnostics scheduling.
    pub pending_diagnostics_uri: Option<Url>,

    /// Cancellation tokens for in-flight diagnostics computations, keyed by URI.
    /// Cancelling a token lets the associated background worker unwind cooperatively
    /// at the next Salsa query boundary, even when no write bumps the global revision.
    pub diagnostics_tokens: HashMap<Url, salsa::CancellationToken>,

    /// Cancellation tokens for in-flight cache-warming (preload) tasks, keyed by
    /// the head `FileId` that triggered the preload. Used to abort stale warming
    /// when the user closes a file or re-triggers preload for the same head.
    pub preload_tokens: HashMap<vfs::FileId, salsa::CancellationToken>,

    /// Last time progress was reported to the client.
    pub last_progress_report: std::time::Instant,

    /// Buffer for VFS files during initial loading.
    pub pending_vfs_files: Vec<(paths::AbsPathBuf, Option<Vec<u8>>)>,
}

impl GlobalState {
    /// Creates a new GlobalState with the given sender.
    pub fn new(sender: Sender<Message>) -> Self {
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
            diagnostics_tokens: HashMap::new(),
            preload_tokens: HashMap::new(),
            last_progress_report: std::time::Instant::now(),
            pending_vfs_files: Vec::new(),
        }
    }

    // ========================================================================
    // LSP transport helpers
    // ========================================================================

    /// Generates a unique request ID for LSP requests.
    fn next_request_id(&self) -> lsp_server::RequestId {
        lsp_server::RequestId::from(self.next_request_id.fetch_add(1, Ordering::SeqCst))
    }

    /// Reports progress to the LSP client using WorkDoneProgress protocol.
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

    /// Sends a response to the client.
    pub fn respond(&mut self, response: Response) {
        if let Err(e) = self.sender.send(response.into()) {
            tracing::error!("Failed to send response: {}", e);
        }
    }

    /// Requests the client to refresh semantic tokens for all open files.
    pub fn request_semantic_tokens_refresh(&mut self) {
        use lsp_server::Request;

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

    /// Creates an immutable snapshot for thread-safe access.
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
}

/// Immutable snapshot of GlobalState for thread-safe access.
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
    pub task_sender: Sender<Task>,
}

impl GlobalStateSnapshot {
    /// Gets the FileId for a URL.
    pub fn file_id_for_url(&self, url: &Url) -> anyhow::Result<vfs::FileId> {
        let path = url.to_file_path().map_err(|_| anyhow::anyhow!("Invalid file URL: {}", url))?;

        let vfs_path = vfs::VfsPath::new(path);

        let vfs = self.vfs.read();
        vfs.file_id(&vfs_path).ok_or_else(|| anyhow::anyhow!("File not in VFS: {}", url))
    }

    /// Gets the URL for a FileId.
    pub fn url_for_file_id(&self, file_id: vfs::FileId) -> anyhow::Result<Url> {
        let vfs = self.vfs.read();
        let path = vfs.file_path(file_id);

        let std_path = path.as_path();

        Url::from_file_path(std_path)
            .map_err(|_| anyhow::anyhow!("Failed to convert path to URL: {:?}", std_path))
    }
}

#[cfg(test)]
mod vfs_race_tests {
    use super::*;

    #[test]
    fn test_empty_source_root_init() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();

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

        state.init_empty_source_root();

        let uri = lsp_types::Url::parse("file:///user.bsl").unwrap();
        let file_id = state.vfs_file_for_url(&uri).unwrap();

        {
            let vfs_path = vfs::VfsPath::new(uri.to_file_path().unwrap());
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(vfs_path, Some(Arc::from("Процедура Test() КонецПроцедуры")));
        }

        state.process_changes();

        let text1 = {
            use base_db::SourceDatabase;
            let db = state.analysis_host.raw_database_mut();
            db.file_text_input(file_id).text(db)
        };
        assert!(text1.contains("Test"));

        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                vfs::VfsPath::new("/loader.bsl"),
                Some(Arc::from("// Loader file")),
            );
        }

        state.process_changes();
        state.init_source_root();

        let text2 = {
            use base_db::SourceDatabase;
            let db = state.analysis_host.raw_database_mut();
            db.file_text_input(file_id).text(db)
        };
        assert!(text2.contains("Test"));
        assert_eq!(text1, text2, "file lost after merge!");
    }
}
