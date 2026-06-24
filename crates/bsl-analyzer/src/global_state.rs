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

/// How long background analysis must stay busy before the "Analyzing" indicator
/// appears. Fast per-file opens finish within this window and show nothing.
const ANALYSIS_PROGRESS_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(200);

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
    /// Debounce timer fired for a background-analysis busy burst. If the burst
    /// (`epoch`) is still the current one and still running, it promotes to a
    /// `window/workDoneProgress` "Analyzing" indicator; otherwise it is a no-op.
    AnalysisProgressTick {
        epoch: u64,
    },
    /// One background analysis job finished (returned normally or unwound).
    /// Posted by [`AnalysisGuard`] on drop so the in-flight counter is always
    /// balanced even if the job panicked before producing its result task.
    AnalysisJobFinished,
}

/// RAII token for one in-flight background analysis job. It is moved into the
/// spawned closure; whether the job returns normally or panics, dropping it posts
/// a [`Task::AnalysisJobFinished`] back to the event loop. This guarantees the
/// in-flight counter is decremented (and the "Analyzing" indicator cannot stick
/// open) without depending on the job's own result task being produced.
pub struct AnalysisGuard {
    sender: Sender<Task>,
}

impl Drop for AnalysisGuard {
    fn drop(&mut self) {
        let _ = self.sender.send(Task::AnalysisJobFinished);
    }
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
    /// Negotiated at `initialize`: whether the client honors `InsertTextMode`,
    /// so completion snippets may opt out of client-side re-indentation.
    pub supports_insert_text_mode_as_is: bool,
    /// Per-URI publish generation. Each scheduled diagnostics computation gets the
    /// next generation for ITS uri; a completed task publishes only if it is still
    /// the latest for that uri. Keyed per-uri (not a single global counter) so
    /// scheduling several documents in one batch — e.g. refreshing all open docs
    /// after an external change — does not let one document's newer generation
    /// discard another document's result.
    pub diagnostics_generation: HashMap<Url, u64>,
    /// Documents whose diagnostics should be (re)scheduled by the event loop
    /// once it finishes the current message: didChange debouncing lands here,
    /// and so does a schedule that found the task pool saturated — the loop
    /// retries when a worker frees a queue slot. Deduplicated by URI.
    pub pending_diagnostics_uris: Vec<Url>,

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

    /// High-water mark of process RSS observed while streaming the initial VFS
    /// load (before `vfs_done`). Sampled per loaded batch, just before that
    /// batch is drained into Salsa, so it captures the true streaming peak.
    pub boot_peak_rss_bytes: u64,

    /// High-water mark of source text buffered in `Vfs::changes` during the
    /// initial load, sampled per batch before it drains. With incremental
    /// draining this stays near one loader chunk; a regression that reverted to
    /// a single end-of-load flush would show it climb to the whole-corpus size.
    pub boot_peak_text_bytes: u64,

    /// Number of background analysis jobs (per-file dependency preload + type
    /// inference / diagnostics) currently in flight. Drives the debounced
    /// "Analyzing" work-done progress: it begins when this stays positive past
    /// the debounce and ends when it returns to zero.
    analysis_in_flight: u32,

    /// Whether the "Analyzing" work-done progress has an open Begin awaiting its
    /// End (i.e. the indicator is currently shown).
    analysis_progress_active: bool,

    /// Monotonic id of the current busy burst, bumped on each rising edge from
    /// zero in-flight. A debounce tick only promotes to a visible indicator if
    /// its captured epoch still matches, so a stale timer from an already-ended
    /// burst is ignored.
    analysis_epoch: u64,
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
            supports_insert_text_mode_as_is: false,
            diagnostics_generation: HashMap::new(),
            pending_diagnostics_uris: Vec::new(),
            diagnostics_tokens: HashMap::new(),
            preload_tokens: HashMap::new(),
            preload_external_tokens: HashMap::new(),
            request_tokens: HashMap::new(),
            last_progress_report: std::time::Instant::now(),
            skipped_bsl: FxHashSet::default(),
            degraded_files_count: 0,
            open_files: FxHashSet::default(),
            boot_peak_rss_bytes: 0,
            boot_peak_text_bytes: 0,
            analysis_in_flight: 0,
            analysis_progress_active: false,
            analysis_epoch: 0,
        }
    }

    /// Record that a background analysis job was just spawned. On the rising edge
    /// from idle it arms a one-shot debounce: a short-lived timer thread posts an
    /// [`Task::AnalysisProgressTick`] back to the event loop, so a burst that
    /// finishes within the debounce window never shows an indicator (no flicker),
    /// while a longer one promotes to a visible "Analyzing" progress.
    #[must_use = "the returned AnalysisGuard must be moved into the analysis job so \
                  it decrements the in-flight counter when the job ends"]
    pub fn note_analysis_spawned(&mut self) -> AnalysisGuard {
        self.analysis_in_flight += 1;
        if self.analysis_in_flight == 1 {
            self.analysis_epoch = self.analysis_epoch.wrapping_add(1);
            let epoch = self.analysis_epoch;
            let sender = self.task_pool.pool.sender.clone();
            if let Err(err) = std::thread::Builder::new()
                .name("bsl-analysis-debounce".to_owned())
                .spawn(move || {
                    std::thread::sleep(ANALYSIS_PROGRESS_DEBOUNCE);
                    let _ = sender.send(Task::AnalysisProgressTick { epoch });
                })
            {
                tracing::debug!(?err, "could not spawn analysis-progress debounce thread");
            }
        }
        AnalysisGuard { sender: self.task_pool.pool.sender.clone() }
    }

    /// Record that a background analysis job finished. When the last one drains,
    /// end the "Analyzing" indicator if it was shown.
    pub fn note_analysis_finished(&mut self) {
        self.analysis_in_flight = self.analysis_in_flight.saturating_sub(1);
        if self.analysis_in_flight == 0 && self.analysis_progress_active {
            self.analysis_progress_active = false;
            self.report_progress("Analyzing", Progress::End, None, None);
        }
    }

    /// Handle a debounce tick: show the "Analyzing" indicator only if this is
    /// still the current busy burst, work is still in flight, and nothing is
    /// shown yet.
    pub fn handle_analysis_progress_tick(&mut self, epoch: u64) {
        if epoch == self.analysis_epoch
            && self.analysis_in_flight > 0
            && !self.analysis_progress_active
        {
            self.analysis_progress_active = true;
            self.report_progress("Analyzing", Progress::Begin, Some("Analyzing…".to_owned()), None);
        }
    }

    /// Queue a document for diagnostics (re)scheduling at the bottom of the
    /// event loop, deduplicated by URI.
    pub fn enqueue_pending_diagnostics(&mut self, uri: Url) {
        if !self.pending_diagnostics_uris.contains(&uri) {
            self.pending_diagnostics_uris.push(uri);
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
    fn set_workspace_root_closes_load_gate_only_for_initial_load() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();
        assert!(state.analysis_host.raw_database().workspace_load_complete());

        let tmp = tempfile::tempdir().expect("tempdir");

        // Initial load (vfs_done = false): the gate closes until the finalize.
        state.set_workspace_root(tmp.path().to_path_buf());
        assert!(
            !state.analysis_host.raw_database().workspace_load_complete(),
            "the initial load must close the whole-config loader gate"
        );

        // A live reload (vfs_done = true) must not degrade running analysis.
        state.vfs_done = true;
        state.analysis_host.raw_database_mut().set_workspace_load_complete(true);
        state.set_workspace_root(tmp.path().to_path_buf());
        assert!(
            state.analysis_host.raw_database().workspace_load_complete(),
            "a live workspace reload must keep the gate open"
        );
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
    fn bootstrap_metadata_substrate_resolves_and_reloads_per_mdo() {
        use ide_db::metadata::resolve_metadata_object;

        fn catalog_xml(name: &str, uuid: &str) -> String {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="{uuid}">
        <Properties><Name>{name}</Name><CodeLength>9</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#
            )
        }

        let root = std::env::temp_dir().join(format!(
            "bsl_bootstrap_meta_{}_{}",
            std::process::id(),
            line!()
        ));
        let catalogs = root.join("Catalogs");
        std::fs::create_dir_all(&catalogs).unwrap();
        std::fs::write(
            catalogs.join("Справочник1.xml"),
            catalog_xml("Справочник1", "00000000-0000-0000-0000-000000000001"),
        )
        .unwrap();
        std::fs::write(
            catalogs.join("Товары.xml"),
            catalog_xml("Товары", "00000000-0000-0000-0000-000000000002"),
        )
        .unwrap();

        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();
        state.analysis_host.raw_database_mut().set_all_config_paths(vec![(None, root.clone())]);

        state.bootstrap_metadata_substrate();

        let root_key = root.to_string_lossy().to_string();
        let db = state.analysis_host.raw_database();
        let listing = db.metadata_listing(&root_key).expect("listing set for the config root");

        let c1 = resolve_metadata_object(
            db,
            listing,
            bsl_metadata::MdoType::Catalog,
            "Справочник1".to_string(),
        )
        .expect("Справочник1 resolves from disk via the bootstrap");
        assert_eq!(c1.name, "Справочник1");
        let tovary_before = resolve_metadata_object(
            db,
            listing,
            bsl_metadata::MdoType::Catalog,
            "Товары".to_string(),
        )
        .expect("Товары resolves");
        assert_eq!(tovary_before.name, "Товары");

        // Edit Товары on disk, re-run the bootstrap (mirrors a reload). Товары
        // re-parses; the sibling Справочник1 stays memoised (per-MDO granularity).
        std::fs::write(
            catalogs.join("Товары.xml"),
            catalog_xml("Товары", "00000000-0000-0000-0000-0000000000ff"),
        )
        .unwrap();
        state.bootstrap_metadata_substrate();

        let db = state.analysis_host.raw_database();
        let listing = db.metadata_listing(&root_key).unwrap();
        let c1_after = resolve_metadata_object(
            db,
            listing,
            bsl_metadata::MdoType::Catalog,
            "Справочник1".to_string(),
        )
        .unwrap();
        assert!(
            Arc::ptr_eq(&c1, &c1_after),
            "a content edit to Товары must not re-parse the sibling Справочник1"
        );
        let tovary_after = resolve_metadata_object(
            db,
            listing,
            bsl_metadata::MdoType::Catalog,
            "Товары".to_string(),
        )
        .unwrap();
        assert_eq!(tovary_after.name, "Товары");
        assert!(
            !Arc::ptr_eq(&tovary_before, &tovary_after),
            "Товары re-parses after its XML changed on disk"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn refresh_metadata_substrate_is_incremental_and_tracks_structure() {
        use ide_db::metadata::resolve_metadata_object;

        fn catalog_xml(name: &str, uuid: &str) -> String {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="{uuid}">
        <Properties><Name>{name}</Name><CodeLength>9</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#
            )
        }

        let cat = bsl_metadata::MdoType::Catalog;
        let root = std::env::temp_dir().join(format!(
            "bsl_refresh_meta_{}_{}",
            std::process::id(),
            line!()
        ));
        let catalogs = root.join("Catalogs");
        std::fs::create_dir_all(&catalogs).unwrap();
        let write = |name: &str, uuid: &str| {
            std::fs::write(catalogs.join(format!("{name}.xml")), catalog_xml(name, uuid)).unwrap()
        };
        write("Справочник1", "00000000-0000-0000-0000-000000000001");
        write("Товары", "00000000-0000-0000-0000-000000000002");

        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();
        state.analysis_host.raw_database_mut().set_all_config_paths(vec![(None, root.clone())]);
        state.bootstrap_metadata_substrate();

        let root_key = root.to_string_lossy().to_string();
        let resolve = |state: &GlobalState, name: &str| {
            let db = state.analysis_host.raw_database();
            let listing = db.metadata_listing(&root_key).unwrap();
            resolve_metadata_object(db, listing, cat, name.to_string())
        };

        let c1 = resolve(&state, "Справочник1").expect("Справочник1");
        let tovary = resolve(&state, "Товары").expect("Товары");

        // CONTENT edit to Товары: re-read only it. Товары re-parses; Справочник1
        // stays memoised (no full re-bootstrap, no sibling churn).
        write("Товары", "00000000-0000-0000-0000-0000000000ff");
        state.refresh_metadata_substrate(&[catalogs.join("Товары.xml")]);
        assert!(
            Arc::ptr_eq(&c1, &resolve(&state, "Справочник1").unwrap()),
            "a content edit to Товары must not re-resolve the sibling"
        );
        assert!(
            !Arc::ptr_eq(&tovary, &resolve(&state, "Товары").unwrap()),
            "Товары re-parses after its content changed"
        );
        let c1 = resolve(&state, "Справочник1").unwrap();

        // STRUCTURE add: a brand-new catalog appears and resolves; the absent-key
        // miss from before is invalidated through config_index.
        assert!(resolve(&state, "Услуги").is_none(), "Услуги absent before the add");
        write("Услуги", "00000000-0000-0000-0000-000000000003");
        state.refresh_metadata_substrate(&[catalogs.join("Услуги.xml")]);
        assert_eq!(resolve(&state, "Услуги").expect("Услуги after add").name, "Услуги");
        assert!(
            Arc::ptr_eq(&c1, &resolve(&state, "Справочник1").unwrap()),
            "a structure add must not re-resolve an untouched sibling"
        );

        // STRUCTURE remove: deleting a catalog tombstones it (resolve -> None).
        std::fs::remove_file(catalogs.join("Товары.xml")).unwrap();
        state.refresh_metadata_substrate(&[catalogs.join("Товары.xml")]);
        assert!(resolve(&state, "Товары").is_none(), "removed catalog resolves to None");
        assert_eq!(resolve(&state, "Услуги").expect("Услуги still present").name, "Услуги");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_metadata_object_for_file_matches_merged_visible_configuration() {
        use base_db::{SourceDatabase, SourceRoot, BSL_SOURCE_ROOT};
        use hir::ConfigsDatabase;

        let cat = bsl_metadata::MdoType::Catalog;
        let root = std::env::temp_dir().join(format!(
            "bsl_resolve_parity_{}_{}",
            std::process::id(),
            line!()
        ));
        let main_root = root.join("src/cf");
        let ext_root = root.join("src/cfe/X");
        std::fs::create_dir_all(main_root.join("Catalogs")).unwrap();
        std::fs::create_dir_all(ext_root.join("Catalogs")).unwrap();
        std::fs::write(main_root.join("Configuration.xml"), "<Configuration/>").unwrap();
        std::fs::write(ext_root.join("Configuration.xml"), "<Configuration/>").unwrap();
        // Same catalog in both roots; the extension adopts it (ObjectBelonging), so
        // the resolved object goes through the main + extension overlay path.
        std::fs::write(
            main_root.join("Catalogs/Номенклатура.xml"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000001">
        <Properties><Name>Номенклатура</Name><CodeLength>9</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#,
        )
        .unwrap();
        std::fs::write(
            ext_root.join("Catalogs/Номенклатура.xml"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000002">
        <Properties><ObjectBelonging>Adopted</ObjectBelonging><Name>Номенклатура</Name></Properties>
    </Catalog>
</MetaDataObject>"#,
        )
        .unwrap();

        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();
        state.analysis_host.raw_database_mut().set_all_config_paths(vec![
            (None, main_root.clone()),
            (Some("X".to_string()), ext_root.clone()),
        ]);
        state.bootstrap_metadata_substrate();

        // A .bsl file living in the extension's scope: it must see the merged object.
        // Allocate via the VFS *after* the bootstrap so its FileId does not collide
        // with the ones the bootstrap already interned for the catalog XMLs.
        let bsl_path = ext_root.join("CommonModules/М/Ext/Module.bsl");
        let bsl_vfs_path = vfs::VfsPath::new(bsl_path.to_string_lossy().as_ref());
        let file_id = state.vfs.write().alloc_file_id(bsl_vfs_path.clone());
        let mut file_set = vfs::file_set::FileSet::new();
        file_set.insert(file_id, bsl_vfs_path);
        let db = state.analysis_host.raw_database_mut();
        db.set_source_root(BSL_SOURCE_ROOT, SourceRoot::new_local(file_set));
        db.set_file_source_root(file_id, BSL_SOURCE_ROOT);
        db.set_file_text(file_id, "Процедура Т() КонецПроцедуры");

        let db = state.analysis_host.raw_database();
        let per_mdo = db
            .resolve_metadata_object_for_file(file_id, cat, "Номенклатура")
            .expect("per-MDO resolve finds the merged catalog");
        let whole = db.merged_visible_configuration(file_id).expect("merged config loads");
        let from_whole =
            whole.find_metadata_object(cat, "Номенклатура").expect("merged config has the catalog");

        assert_eq!(
            &*per_mdo, from_whole,
            "per-MDO resolve (with extension overlay) must equal the merged whole-config lookup"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn bootstrapped_common_module_resolves_by_name_and_by_body_file() {
        use base_db::{SourceDatabase, SourceRoot, BSL_SOURCE_ROOT};
        use bsl_metadata::traits::MdObject;

        let root = std::env::temp_dir().join(format!(
            "bsl_common_module_{}_{}",
            std::process::id(),
            line!()
        ));
        let cf = root.join("src/cf");
        let cm_dir = cf.join("CommonModules");
        std::fs::create_dir_all(cm_dir.join("МойМодуль/Ext")).unwrap();
        std::fs::write(cf.join("Configuration.xml"), "<Configuration/>").unwrap();
        std::fs::write(
            cm_dir.join("МойМодуль.xml"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <CommonModule uuid="00000000-0000-0000-0000-000000000021">
        <Properties><Name>МойМодуль</Name><Global>true</Global><Server>true</Server><Privileged>true</Privileged></Properties>
    </CommonModule>
</MetaDataObject>"#,
        )
        .unwrap();
        let bsl_path = cm_dir.join("МойМодуль/Ext/Module.bsl");
        std::fs::write(&bsl_path, "Функция Ф() Экспорт КонецФункции").unwrap();

        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();

        // Intern the module body into the VFS BEFORE the bootstrap, mirroring the
        // real boot order (`init_source_root` runs before
        // `bootstrap_metadata_substrate`). Discovery then resolves the body's FileId
        // through the same interner the analyzer uses — the path-normalization parity
        // the by-body reverse index depends on.
        let bsl_vfs_path = vfs::VfsPath::new(bsl_path.to_string_lossy().as_ref());
        let bsl_file = state.vfs.write().alloc_file_id(bsl_vfs_path.clone());
        {
            let mut file_set = vfs::file_set::FileSet::new();
            file_set.insert(bsl_file, bsl_vfs_path);
            let db = state.analysis_host.raw_database_mut();
            db.set_source_root(BSL_SOURCE_ROOT, SourceRoot::new_local(file_set));
            db.set_file_source_root(bsl_file, BSL_SOURCE_ROOT);
            db.set_file_text(bsl_file, "Функция Ф() Экспорт КонецФункции");
        }

        state.analysis_host.raw_database_mut().set_all_config_paths(vec![(None, cf.clone())]);
        state.bootstrap_metadata_substrate();

        let db = state.analysis_host.raw_database();

        // By-name resolution through the per-common-module substrate, flags intact.
        let by_name = db
            .resolve_common_module_for_file(bsl_file, "МойМодуль")
            .expect("common module resolves by name through the bootstrapped substrate");
        assert!(by_name.is_global() && by_name.is_privileged(), "metadata flags survive parsing");
        // Case-insensitive, like the whole-config lookup.
        assert!(db.resolve_common_module_for_file(bsl_file, "моймодуль").is_some());

        // By-body reverse index: the FileId the analyzer holds for Module.bsl maps
        // back to its common module — proves discovery interned the same path/FileId.
        let by_file = db
            .common_module_for_file_id(bsl_file)
            .expect("Module.bsl resolves to its common module via the reverse index");
        assert_eq!(by_file.name(), "МойМодуль");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn bootstrapped_common_module_scopes_base_everywhere_extension_private() {
        use base_db::{SourceDatabase, SourceRoot, BSL_SOURCE_ROOT};

        // Common-module visibility (same scoping as metadata objects): a main-config
        // common module is visible everywhere, but an extension's common module is
        // visible only within that extension — a sibling extension's modules are not.
        let root =
            std::env::temp_dir().join(format!("bsl_cm_xext_{}_{}", std::process::id(), line!()));
        let base = root.join("src/cf");
        let ext_a = root.join("src/cfe/A");
        let ext_b = root.join("src/cfe/B");
        for dir in [&base, &ext_a, &ext_b] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("Configuration.xml"), "<Configuration/>").unwrap();
        }
        let common_module = |dir: &std::path::Path, name: &str, uuid: &str| {
            std::fs::create_dir_all(dir.join(format!("CommonModules/{name}/Ext"))).unwrap();
            std::fs::write(
                dir.join(format!("CommonModules/{name}.xml")),
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <CommonModule uuid="{uuid}"><Properties><Name>{name}</Name><Server>true</Server></Properties></CommonModule>
</MetaDataObject>"#
                ),
            )
            .unwrap();
            std::fs::write(
                dir.join(format!("CommonModules/{name}/Ext/Module.bsl")),
                "Процедура П() Экспорт КонецПроцедуры",
            )
            .unwrap();
        };
        common_module(&base, "ОбщийБаза", "00000000-0000-0000-0000-000000000ba1");
        common_module(&ext_b, "ОбщийБ", "00000000-0000-0000-0000-0000000000b1");

        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();
        state.analysis_host.raw_database_mut().set_all_config_paths(vec![
            (None, base.clone()),
            (Some("A".to_string()), ext_a.clone()),
            (Some("B".to_string()), ext_b.clone()),
        ]);
        state.bootstrap_metadata_substrate();

        // A .bsl file living in extension A (after bootstrap, so its FileId does not
        // collide with the bootstrap-interned XML ids).
        let a_bsl = ext_a.join("CommonModules/М/Ext/Module.bsl");
        let a_vp = vfs::VfsPath::new(a_bsl.to_string_lossy().as_ref());
        let a_file = state.vfs.write().alloc_file_id(a_vp.clone());
        {
            let db = state.analysis_host.raw_database_mut();
            let mut fs = vfs::file_set::FileSet::new();
            fs.insert(a_file, a_vp);
            db.set_source_root(BSL_SOURCE_ROOT, SourceRoot::new_local(fs));
            db.set_file_source_root(a_file, BSL_SOURCE_ROOT);
            db.set_file_text(a_file, "Процедура Т() КонецПроцедуры");
        }

        let db = state.analysis_host.raw_database();
        assert!(
            db.resolve_common_module_for_file(a_file, "ОбщийБаза").is_some(),
            "a main-config common module must be visible to a file in extension A"
        );
        assert!(
            db.resolve_common_module_for_file(a_file, "ОбщийБ").is_none(),
            "a sibling extension B's common module must NOT be visible to a file in extension A"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_metadata_object_for_file_scopes_base_and_extensions() {
        // Pins the 1C visibility rules for per-MDO resolution:
        //  - a file in the base config sees only the base (extensions invisible);
        //  - a file in extension A sees base + A merged (A priority), never B;
        //  - extensions do not bleed into each other.
        let cat = bsl_metadata::MdoType::Catalog;
        let root = std::env::temp_dir().join(format!(
            "bsl_resolve_scope_{}_{}",
            std::process::id(),
            line!()
        ));
        let base = root.join("src/cf");
        let ext_a = root.join("src/cfe/A");
        let ext_b = root.join("src/cfe/B");
        for dir in [&base, &ext_a, &ext_b] {
            std::fs::create_dir_all(dir.join("Catalogs")).unwrap();
            std::fs::write(dir.join("Configuration.xml"), "<Configuration/>").unwrap();
        }
        let catalog = |name: &str, uuid: &str| {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="{uuid}"><Properties><Name>{name}</Name><CodeLength>9</CodeLength></Properties></Catalog>
</MetaDataObject>"#
            )
        };
        std::fs::write(
            base.join("Catalogs/Общий.xml"),
            catalog("Общий", "00000000-0000-0000-0000-000000000001"),
        )
        .unwrap();
        std::fs::write(
            ext_a.join("Catalogs/ТолькоА.xml"),
            catalog("ТолькоА", "00000000-0000-0000-0000-00000000000a"),
        )
        .unwrap();
        std::fs::write(
            ext_b.join("Catalogs/ТолькоБ.xml"),
            catalog("ТолькоБ", "00000000-0000-0000-0000-00000000000b"),
        )
        .unwrap();

        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();
        state.analysis_host.raw_database_mut().set_all_config_paths(vec![
            (None, base.clone()),
            (Some("A".to_string()), ext_a.clone()),
            (Some("B".to_string()), ext_b.clone()),
        ]);
        state.bootstrap_metadata_substrate();

        // Allocate one .bsl file per scope (after the bootstrap, so FileIds don't
        // collide with the catalog XML ids).
        let mut make_file = |dir: &std::path::Path| {
            use base_db::{SourceDatabase, SourceRoot, BSL_SOURCE_ROOT};
            let p = dir.join("CommonModules/М/Ext/Module.bsl");
            let vp = vfs::VfsPath::new(p.to_string_lossy().as_ref());
            let fid = state.vfs.write().alloc_file_id(vp.clone());
            let db = state.analysis_host.raw_database_mut();
            let sr = db.source_root_input(BSL_SOURCE_ROOT).root(db);
            let mut fs = sr.file_set().clone();
            fs.insert(fid, vp);
            db.set_source_root(BSL_SOURCE_ROOT, SourceRoot::new_local(fs));
            db.set_file_source_root(fid, BSL_SOURCE_ROOT);
            db.set_file_text(fid, "Процедура Т() КонецПроцедуры");
            fid
        };
        let base_file = make_file(&base);
        let a_file = make_file(&ext_a);
        let b_file = make_file(&ext_b);

        let db = state.analysis_host.raw_database();
        let r = |fid: vfs::FileId, name: &str| {
            db.resolve_metadata_object_for_file(fid, cat, name).map(|m| m.name.clone())
        };

        // Base file: only base objects, no extension objects.
        assert_eq!(r(base_file, "Общий").as_deref(), Some("Общий"));
        assert_eq!(r(base_file, "ТолькоА"), None, "base must not see extension A's object");
        assert_eq!(r(base_file, "ТолькоБ"), None, "base must not see extension B's object");

        // Extension A: base + A, never B.
        assert_eq!(r(a_file, "Общий").as_deref(), Some("Общий"), "A sees base objects");
        assert_eq!(r(a_file, "ТолькоА").as_deref(), Some("ТолькоА"), "A sees its own object");
        assert_eq!(r(a_file, "ТолькоБ"), None, "A must not see extension B's object");

        // Extension B: base + B, never A.
        assert_eq!(r(b_file, "ТолькоБ").as_deref(), Some("ТолькоБ"), "B sees its own object");
        assert_eq!(r(b_file, "ТолькоА"), None, "B must not see extension A's object");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn bootstrap_resolves_register_per_mdo() {
        use base_db::{SourceDatabase, SourceRoot, BSL_SOURCE_ROOT};

        // The real designer fixture (guaranteed-parseable register XML).
        let root = std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../bsl-metadata/fixtures/designer"
        ));

        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();
        state.analysis_host.raw_database_mut().set_all_config_paths(vec![(None, root.clone())]);
        state.bootstrap_metadata_substrate();

        let bsl_path = root.join("CommonModules/М/Ext/Module.bsl");
        let vp = vfs::VfsPath::new(bsl_path.to_string_lossy().as_ref());
        let fid = state.vfs.write().alloc_file_id(vp.clone());
        let db = state.analysis_host.raw_database_mut();
        let mut fs = vfs::file_set::FileSet::new();
        fs.insert(fid, vp);
        db.set_source_root(BSL_SOURCE_ROOT, SourceRoot::new_local(fs));
        db.set_file_source_root(fid, BSL_SOURCE_ROOT);
        db.set_file_text(fid, "Процедура Т() КонецПроцедуры");

        let db = state.analysis_host.raw_database();
        // Register resolves through the bootstrapped per-MDO path...
        let reg = db
            .resolve_register_for_file(
                fid,
                bsl_metadata::MdoType::InformationRegister,
                "РегистрСведений1",
            )
            .expect("register resolves per-MDO from the fixture");
        assert_eq!(reg.name(), "РегистрСведений1");
        // ...and an object kind does not resolve through the register query.
        assert!(
            db.resolve_register_for_file(fid, bsl_metadata::MdoType::Catalog, "Справочник1",)
                .is_none(),
            "a catalog must not resolve as a register"
        );
    }

    #[test]
    fn init_source_root_excludes_metadata_from_root0() {
        use base_db::{SourceDatabase, BSL_SOURCE_ROOT};

        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);
        state.init_empty_source_root();

        let bsl_a = "/proj/CommonModules/М/Ext/Module.bsl";
        let bsl_b = "/proj/Catalogs/Товары/Ext/ObjectModule.bsl";
        let xml_a = "/proj/Catalogs/Товары.xml";
        let xml_b = "/proj/Catalogs/Товары/Ext/Predefined.xml";
        {
            let mut vfs = state.vfs.write();
            for p in [bsl_a, bsl_b, xml_a, xml_b] {
                vfs.set_file_contents(vfs::VfsPath::new(p), Some(Arc::from("x")));
            }
        }
        state.process_changes(false);
        state.init_source_root();

        let db = state.analysis_host.raw_database_mut();
        let bsl_root = db.source_root_input(BSL_SOURCE_ROOT).root(db);

        let in_root = |root: &base_db::SourceRoot, p: &str| {
            root.file_set().file_for_path(&vfs::VfsPath::new(p)).is_some()
        };

        // root(0) holds the .bsl sources and never the metadata XML; metadata files
        // belong to the bootstrap-owned metadata root(1).
        assert!(in_root(&bsl_root, bsl_a) && in_root(&bsl_root, bsl_b), "bsl in root(0)");
        assert!(!in_root(&bsl_root, xml_a) && !in_root(&bsl_root, xml_b), "xml NOT in root(0)");
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

    fn progress_kinds(receiver: &Receiver<Message>) -> Vec<String> {
        let mut kinds = Vec::new();
        while let Ok(msg) = receiver.try_recv() {
            if let Message::Notification(not) = msg {
                if not.method == "$/progress" {
                    if let Some(kind) =
                        not.params.get("value").and_then(|v| v.get("kind")).and_then(|k| k.as_str())
                    {
                        kinds.push(kind.to_owned());
                    }
                }
            }
        }
        kinds
    }

    #[test]
    fn analysis_progress_shows_after_debounce_and_ends_when_idle() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);

        // Two concurrent jobs in flight; the rising edge armed one debounce epoch.
        // The guards model the in-flight jobs; completion is simulated below via
        // the same `note_analysis_finished` the guard's task triggers on the loop.
        let _guard_a = state.note_analysis_spawned();
        let _guard_b = state.note_analysis_spawned();
        let epoch = state.analysis_epoch;

        // A tick from a different burst must not show anything.
        state.handle_analysis_progress_tick(epoch.wrapping_sub(1));
        assert!(!state.analysis_progress_active, "stale-epoch tick must not show the indicator");

        // The current burst's tick promotes to a visible Begin.
        state.handle_analysis_progress_tick(epoch);
        assert!(state.analysis_progress_active);

        // A duplicate same-epoch tick must not re-Begin.
        state.handle_analysis_progress_tick(epoch);

        // The indicator stays while any job remains and ends only when the last drains.
        state.note_analysis_finished();
        assert!(state.analysis_progress_active, "indicator stays while work remains");
        state.note_analysis_finished();
        assert!(!state.analysis_progress_active, "indicator ends when the last job drains");

        // Exactly one Begin and one End, despite the duplicate tick.
        assert_eq!(progress_kinds(&receiver), vec!["begin".to_owned(), "end".to_owned()]);
    }

    #[test]
    fn analysis_progress_skips_indicator_for_fast_burst() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);

        // A burst that finishes before its debounce tick is delivered.
        let _guard = state.note_analysis_spawned();
        let epoch = state.analysis_epoch;
        state.note_analysis_finished();
        assert_eq!(state.analysis_in_flight, 0);

        // A `finished` while nothing is shown must not emit an End.
        state.handle_analysis_progress_tick(epoch);
        assert!(!state.analysis_progress_active);
        assert!(progress_kinds(&receiver).is_empty(), "a fast burst emits no progress");
    }

    #[test]
    fn analysis_progress_ignores_prior_burst_tick_after_new_burst() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut state = GlobalState::new(sender);

        // Burst A starts and finishes within its debounce (no indicator yet).
        let _guard_a = state.note_analysis_spawned();
        let epoch_a = state.analysis_epoch;
        state.note_analysis_finished();
        assert_eq!(state.analysis_in_flight, 0);

        // Burst B starts; its rising edge bumps the epoch.
        let _guard_b = state.note_analysis_spawned();
        let epoch_b = state.analysis_epoch;
        assert_ne!(epoch_a, epoch_b, "a new burst must get a fresh epoch");

        // A's late debounce tick must be ignored — it must not show B's indicator.
        state.handle_analysis_progress_tick(epoch_a);
        assert!(!state.analysis_progress_active, "stale prior-burst tick must not begin");

        // B's own tick shows the indicator; finishing B ends it.
        state.handle_analysis_progress_tick(epoch_b);
        assert!(state.analysis_progress_active);
        state.note_analysis_finished();
        assert!(!state.analysis_progress_active);

        assert_eq!(progress_kinds(&receiver), vec!["begin".to_owned(), "end".to_owned()]);
    }
}
