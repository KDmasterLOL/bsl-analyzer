use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use base_db::{DiagnosticsConfigInput, Locale};
use crossbeam_channel::{Receiver, Sender};
use ide::Analysis;
use lsp_server::{Message, ReqQueue, Response};
use lsp_types::Url;
use parking_lot::RwLock;
use project_model::Project;
use rustc_hash::FxHashSet;
use vfs::loader::Handle;
use vfs::{loader, Vfs};

use crate::analysis_host::AnalysisHost;
use crate::lsp::{PositionEncoding, Progress};
use crate::mem_docs::MemDocs;
use crate::task_pool;

#[derive(Debug)]
pub enum Task {
    DependenciesPreloaded {
        file_id: vfs::FileId,
        count: usize,
    },
    DiagnosticsReady {
        uri: Url,
        diagnostics: Vec<lsp_types::Diagnostic>,
        generation: u64,
        completed_at: std::time::Instant,
    },
    DiagnosticsCancelled {
        generation: u64,
        completed_at: std::time::Instant,
    },
    PreloadExternalFiles {
        files: Vec<vfs::FileId>,
    },
    RequestResult {
        response: Response,
    },
}

pub struct GlobalState {
    pub sender: Sender<Message>,
    pub req_queue: ReqQueue<(), ()>,
    pub analysis_host: AnalysisHost,
    pub vfs: Arc<RwLock<Vfs>>,
    pub mem_docs: MemDocs,
    pub workspace_root: Option<PathBuf>,
    pub project: Option<Project>,
    pub shutdown_requested: bool,

    pub loader_receiver: Receiver<loader::Message>,
    pub loader: Box<dyn loader::Handle>,
    pub vfs_progress_config_version: u32,
    pub vfs_done: bool,
    pub task_pool: task_pool::Handle<Task>,
    pub(crate) next_request_id: AtomicI32,
    pub(crate) diagnostics_config: DiagnosticsConfigInput,

    pub(crate) lsp_locale: Option<Locale>,
    pub position_encoding: PositionEncoding,
    /// Per-URI publish generation. Each scheduled diagnostics computation gets the
    /// next generation for ITS uri; a completed task publishes only if it is still
    /// the latest for that uri. Keyed per-uri (not a single global counter) so
    /// scheduling several documents in one batch — e.g. refreshing all open docs
    /// after an external change — does not let one document's newer generation
    /// discard another document's result.
    pub diagnostics_generation: HashMap<Url, u64>,
    pub pending_diagnostics_uri: Option<Url>,

    pub diagnostics_tokens: HashMap<Url, salsa::CancellationToken>,
    pub preload_tokens: HashMap<vfs::FileId, salsa::CancellationToken>,

    pub preload_external_tokens: HashMap<vfs::FileId, salsa::CancellationToken>,

    pub request_tokens: HashMap<lsp_server::RequestId, salsa::CancellationToken>,
    pub last_progress_report: std::time::Instant,

    pub skipped_bsl: FxHashSet<paths::AbsPathBuf>,

    pub degraded_files_count: usize,

    /// File ids of currently-open editor documents, resolved at didOpen time.
    /// `process_changes` keys text storage on this (overlay for open files,
    /// disk-backed revision for closed) — by `FileId`, not by `Url`, so client
    /// URL encoding/casing can't misclassify an open buffer as closed and route
    /// unsaved edits to a stale disk read.
    pub open_files: FxHashSet<vfs::FileId>,
}

impl GlobalState {
    pub fn new(sender: Sender<Message>) -> Self {
        let (loader_sender, loader_receiver) = crossbeam_channel::bounded(4);
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
            diagnostics_config: DiagnosticsConfigInput::from_raw(
                Vec::<String>::new(),
                Vec::<String>::new(),
                Vec::<(String, String)>::new(),
                false,
                hir::dataflow::DEFAULT_MAX_ITERATIONS,
                Locale::default(),
            ),
            lsp_locale: None,
            position_encoding: PositionEncoding::default(),
            diagnostics_generation: HashMap::new(),
            pending_diagnostics_uri: None,
            diagnostics_tokens: HashMap::new(),
            preload_tokens: HashMap::new(),
            preload_external_tokens: HashMap::new(),
            request_tokens: HashMap::new(),
            last_progress_report: std::time::Instant::now(),
            skipped_bsl: FxHashSet::default(),
            degraded_files_count: 0,
            open_files: FxHashSet::default(),
        }
    }

    pub fn assert_total_vfs_invariant(&self) -> usize {
        use base_db::SourceDatabase;

        let db = self.analysis_host.raw_database();
        let source_root_id = base_db::SourceRootId(0);
        let source_root_input = db.source_root_input(source_root_id);
        let source_root = source_root_input.root(db);
        let file_set = source_root.file_set();

        let mut violations = 0;
        for fid in file_set.iter() {
            if !ide_db::is_bsl_source(file_set, fid) {
                continue;
            }
            // "Loaded" now means a content revision is registered (disk-backed or
            // overlay), not that an overlay text is resident — closed files are
            // disk-backed and legitimately have no `FileTextInput`.
            if db.try_file_revision_input(fid).is_none() {
                tracing::warn!(file_id = fid.0, "BSL fid in SourceRoot has no content revision");
                violations += 1;
            }
        }
        violations
    }

    fn next_request_id(&self) -> lsp_server::RequestId {
        lsp_server::RequestId::from(self.next_request_id.fetch_add(1, Ordering::SeqCst))
    }

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

    pub fn respond(&mut self, response: Response) {
        if let Err(e) = self.sender.send(response.into()) {
            tracing::error!("Failed to send response: {}", e);
        }
    }

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

    pub fn snapshot(&self) -> GlobalStateSnapshot {
        GlobalStateSnapshot {
            analysis: self.analysis_host.analysis(),
            vfs: Arc::clone(&self.vfs),
            mem_docs: self.mem_docs.clone(),
            workspace_root: self.workspace_root.clone(),
            project: self.project.clone(),
            diagnostics_config: self.diagnostics_config.clone(),
            position_encoding: self.position_encoding,
            vfs_done: self.vfs_done,
            task_sender: self.task_pool.pool.sender.clone(),
        }
    }
}

pub struct GlobalStateSnapshot {
    pub analysis: Analysis,
    pub vfs: Arc<RwLock<Vfs>>,
    pub mem_docs: MemDocs,
    pub workspace_root: Option<PathBuf>,
    pub project: Option<Project>,
    pub diagnostics_config: DiagnosticsConfigInput,
    pub position_encoding: PositionEncoding,
    pub vfs_done: bool,
    pub task_sender: Sender<Task>,
}

impl GlobalStateSnapshot {
    pub fn file_id_for_url(&self, url: &Url) -> anyhow::Result<vfs::FileId> {
        let path = url.to_file_path().map_err(|_| anyhow::anyhow!("Invalid file URL: {}", url))?;
        if !project_model::is_bsl_source_path(&path) {
            return Err(anyhow::anyhow!("File is not BSL, request unsupported: {}", url));
        }

        let vfs_path = vfs::VfsPath::new(path);

        let vfs = self.vfs.read();
        vfs.file_id(&vfs_path).ok_or_else(|| anyhow::anyhow!("File not in VFS: {}", url))
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
        // Open document: text lives in the resident overlay, not on disk (this
        // synthetic file has no disk path for the disk-backed path to read).
        state.open_files.insert(file_id);

        {
            let vfs_path = vfs::VfsPath::new(uri.to_file_path().unwrap());
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(vfs_path, Some(Arc::from("Процедура Test() КонецПроцедуры")));
        }

        state.process_changes(false);

        let text1 = {
            use base_db::SourceDatabase;
            let db = state.analysis_host.raw_database_mut();
            db.file_text(file_id)
        };
        assert!(text1.contains("Test"));

        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                vfs::VfsPath::new("/loader.bsl"),
                Some(Arc::from("// Loader file")),
            );
        }

        state.process_changes(false);
        state.init_source_root();

        let text2 = {
            use base_db::SourceDatabase;
            let db = state.analysis_host.raw_database_mut();
            db.file_text(file_id)
        };
        assert!(text2.contains("Test"));
        assert_eq!(text1, text2, "file lost after merge!");
    }

    #[test]
    fn external_changes_report_affects_open_documents() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();

        // No pending VFS changes → nothing to refresh.
        let empty = state.process_changes(false);
        assert!(!empty.affects_open_documents);
        assert!(!empty.config_file_changed);

        // A closed .bsl file changing on disk affects analysis of other (open) docs.
        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                vfs::VfsPath::new("/cf/CommonModules/М/Ext/Module.bsl"),
                Some(Arc::from("Процедура А() Экспорт КонецПроцедуры")),
            );
        }
        let bsl = state.process_changes(false);
        assert!(bsl.affects_open_documents, "a .bsl change must mark open docs for refresh");
        assert!(!bsl.config_file_changed);

        // A metadata XML change likewise affects open docs (cross-config metadata).
        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                vfs::VfsPath::new("/cf/Catalogs/Товары.xml"),
                Some(Arc::from("<MetaDataObject/>")),
            );
        }
        let meta = state.process_changes(false);
        assert!(
            meta.affects_open_documents,
            "a metadata XML change must mark open docs for refresh"
        );
        assert!(!meta.config_file_changed);

        // A suppressed batch (initial sync) must not claim metadata changes.
        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                vfs::VfsPath::new("/cf/Catalogs/Услуги.xml"),
                Some(Arc::from("<MetaDataObject/>")),
            );
        }
        let suppressed = state.process_changes(true);
        assert!(
            !suppressed.affects_open_documents,
            "a suppressed initial-sync batch bumps nothing, so it reports no refresh"
        );
    }

    #[test]
    fn remove_directories_tombstones_descendants_only() {
        use base_db::{SourceDatabase, SourceRootId};

        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();

        let inside_a = "/proj/Catalogs/X/Ext/ObjectModule.bsl";
        let inside_b = "/proj/Catalogs/X/Forms/F/Ext/Form/Module.bsl";
        let outside = "/proj/Catalogs/Y/Ext/ObjectModule.bsl";
        {
            let mut vfs = state.vfs.write();
            for p in [inside_a, inside_b, outside] {
                vfs.set_file_contents(
                    vfs::VfsPath::new(p),
                    Some(Arc::from("Процедура А() КонецПроцедуры")),
                );
            }
        }
        state.process_changes(false);

        let in_file_set = |state: &GlobalState, p: &str| {
            let db = state.analysis_host.raw_database();
            let sr = db.source_root_input(SourceRootId(0)).root(db);
            sr.file_set().file_for_path(&vfs::VfsPath::new(p)).is_some()
        };
        assert!(in_file_set(&state, inside_a) && in_file_set(&state, outside), "baseline loaded");

        // Remove the directory subtree "Catalogs/X".
        let removed =
            vec![paths::AbsPathBuf::assert_utf8(std::path::PathBuf::from("/proj/Catalogs/X"))];
        let refreshed = state.remove_directories(&removed);

        assert!(refreshed, "a subtree removal should request an open-document refresh");
        assert!(!in_file_set(&state, inside_a), "descendant ObjectModule must be tombstoned");
        assert!(!in_file_set(&state, inside_b), "nested descendant must be tombstoned");
        assert!(in_file_set(&state, outside), "a sibling directory must be untouched");
    }

    #[test]
    fn is_open_document_path_detects_open_by_file_id() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();

        let p = "/proj/CommonModules/М/Ext/Module.bsl";
        let vfs_path = vfs::VfsPath::new(p);
        // Mark the file open by FileId ONLY (no mem_docs URL entry) — this models a
        // watcher URL that doesn't byte-match the client's didOpen URL.
        let file_id = state.vfs.write().alloc_file_id(vfs_path.clone());
        state.open_files.insert(file_id);

        assert!(
            state.is_open_document_path(std::path::Path::new(p), &vfs_path),
            "an open file must be recognized by its FileId even without a mem_docs URL"
        );

        let other = "/proj/CommonModules/Other/Ext/Module.bsl";
        assert!(
            !state.is_open_document_path(std::path::Path::new(other), &vfs::VfsPath::new(other)),
            "an unknown file is not open"
        );
    }

    #[test]
    fn closed_file_disk_backed_and_overlay_to_disk_transition() {
        use base_db::SourceDatabase;

        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("mod.bsl");
        std::fs::write(&path, "Процедура А() КонецПроцедуры").expect("write");

        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();

        let uri = Url::from_file_path(&path).unwrap();
        let file_id = state.vfs_file_for_url(&uri).unwrap();

        // 1) A closed file (not in open_files) is disk-backed: its text is read
        //    from disk on demand, not stored as a resident overlay.
        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                vfs::VfsPath::new(path.clone()),
                Some(Arc::from("Процедура А() КонецПроцедуры")),
            );
        }
        state.process_changes(false);
        {
            let db = state.analysis_host.raw_database_mut();
            assert!(db.try_file_text(file_id).is_none(), "closed file must have no overlay");
            assert_eq!(&*db.file_text(file_id), "Процедура А() КонецПроцедуры");
        }

        // 2) Opening it stores an authoritative overlay (an unsaved edit not yet
        //    on disk).
        state.open_files.insert(file_id);
        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                vfs::VfsPath::new(path.clone()),
                Some(Arc::from("Процедура Б() КонецПроцедуры")),
            );
        }
        state.process_changes(false);
        {
            let db = state.analysis_host.raw_database_mut();
            assert_eq!(&*db.file_text(file_id), "Процедура Б() КонецПроцедуры");
        }

        // 3) Disk changes under us, then the file closes: re-keying to disk must
        //    clear the stale overlay and read the new disk bytes WITHOUT a
        //    revision-mismatch panic (regression guard for the overlay-clear).
        std::fs::write(&path, "Процедура В() КонецПроцедуры").expect("rewrite");
        state.open_files.remove(&file_id);
        {
            let disk = std::fs::read_to_string(&path).unwrap();
            let db = state.analysis_host.raw_database_mut();
            db.set_file_revision_from_disk(file_id, base_db::content_revision(&disk));
        }
        {
            let db = state.analysis_host.raw_database_mut();
            assert!(
                db.try_file_text(file_id).is_none(),
                "overlay must be cleared on disk transition"
            );
            assert_eq!(&*db.file_text(file_id), "Процедура В() КонецПроцедуры");
        }
    }
}
