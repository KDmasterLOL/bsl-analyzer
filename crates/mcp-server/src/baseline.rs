use bsl_platform::PlatformDataInner;
use bsl_search::{
    fingerprint_documents, BaselineRef, CorpusId, Document, ExternalBaselineAdapter,
    ExternalBaselineBackend, ExternalBaselineConfig, IndexedDocument, ResolvedView,
    SnapshotCatalog, SnapshotContentStore, WorkspaceBaselineManifest,
};
use project_model::{
    current_git_branch, evaluate_workspace_baseline_support_now, parse_timestamp_utc,
    resolve_postgres_url, resolve_workspace_branch_policy, PostgresAccessMode, ProjectConfig,
    ResolvedWorkspaceBaselineSupport, SearchBaselineBackend, SearchBaselineConfig,
    SearchBaselinePolicyConfig, SearchBaselineSupportState, SearchBaselineTargetConfig,
    SearchPostgresConfig,
};
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::RwLock as StdRwLock;
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineResolutionSummary {
    pub backend: String,
    pub selection: String,
    pub issue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineConfigDiagnostics {
    pub workspace: BaselineResolutionSummary,
    pub reference: BaselineResolutionSummary,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BaselineSnapshotDocuments {
    pub snapshot_id: String,
    pub fingerprint: Option<String>,
    pub documents: Vec<IndexedDocument>,
    pub shared_embeddings: HashMap<String, Vec<f32>>,
}

#[derive(Debug, Clone)]
pub(crate) struct BaselineRuntime {
    pub configured_baseline: ConfiguredBaselineStatus,
    pub external_baseline: Option<Arc<ExternalBaselineService>>,
}

#[derive(Debug)]
pub(crate) struct ExternalBaselineService {
    corpus: CorpusId,
    schema: String,
    selection: String,
    local_reference_fingerprint: Option<String>,
    sender: mpsc::Sender<BaselineServiceRequest>,
    worker: StdMutex<Option<JoinHandle<()>>>,
    closed: AtomicBool,
}

#[derive(Debug)]
enum BaselineServiceRequest {
    ProbeStatus {
        reply: mpsc::Sender<ExternalBaselineStatus>,
    },
    ResolveSnapshot {
        reply: mpsc::Sender<
            Result<Option<(BaselineRef, bsl_search::Snapshot)>, bsl_search::SearchError>,
        >,
    },
    LexicalSearch {
        snapshot_id: String,
        query: String,
        collection: Option<String>,
        limit: usize,
        reply: mpsc::Sender<Result<Vec<bsl_search::LexicalHit>, bsl_search::SearchError>>,
    },
    SemanticSearch {
        snapshot_id: String,
        query_embedding: Vec<f32>,
        model_id: String,
        dimension: usize,
        collection: Option<String>,
        limit: usize,
        reply: mpsc::Sender<Result<Vec<bsl_search::SemanticHit>, bsl_search::SearchError>>,
    },
    LoadReferenceSnapshotDocuments {
        model_id: Option<String>,
        dimension: Option<usize>,
        reply: mpsc::Sender<Result<Option<BaselineSnapshotDocuments>, bsl_search::SearchError>>,
    },
    LoadBaselineManifest {
        snapshot_id: String,
        reply: mpsc::Sender<Result<WorkspaceBaselineManifest, bsl_search::SearchError>>,
    },
    EmbeddingIdentity {
        reply: mpsc::Sender<Result<Option<(String, usize)>, bsl_search::SearchError>>,
    },
    Shutdown {
        reply: mpsc::Sender<()>,
    },
}

impl BaselineRuntime {
    pub(crate) fn workspace(project_root: Option<&Path>, project_config: &ProjectConfig) -> Self {
        Self::for_corpus(
            CorpusId::WorkspaceCode,
            project_root,
            Some(&project_config.search.baseline),
            "BSL_SEARCH_BASELINE",
            &["BSL_SEARCH_BASELINE_PG_SCHEMA"],
            false,
        )
    }

    pub(crate) fn reference(project_config: Option<&ProjectConfig>) -> Self {
        Self::for_corpus(
            CorpusId::Reference,
            None,
            project_config.map(|config| &config.search.baseline),
            "BSL_SEARCH_REFERENCE",
            &["BSL_SEARCH_REFERENCE_PG_SCHEMA", "BSL_SEARCH_BASELINE_PG_SCHEMA"],
            project_config.is_none(),
        )
    }

    #[allow(clippy::too_many_arguments, reason = "private resolution chain uses all inputs")]
    fn for_corpus(
        corpus: CorpusId,
        project_root: Option<&Path>,
        project_config: Option<&SearchBaselineConfig>,
        selection_prefix: &str,
        schema_keys: &[&str],
        allow_env_backend_without_config: bool,
    ) -> Self {
        let configured_backend = project_config.map(|config| config.backend.clone());
        let use_postgres = matches!(configured_backend, Some(SearchBaselineBackend::Postgres));
        if !use_postgres && !allow_env_backend_without_config {
            return Self {
                configured_baseline: ConfiguredBaselineStatus {
                    backend: "sqlite",
                    selection: local_baseline_description(&corpus),
                    issue: None,
                    support: None,
                },
                external_baseline: None,
            };
        }

        let default_target = SearchBaselineTargetConfig::default();
        let baseline_target = project_config
            .map(|config| match corpus {
                CorpusId::WorkspaceCode => &config.workspace_code,
                CorpusId::Reference => &config.reference,
                CorpusId::Custom(_) => &config.workspace_code,
            })
            .unwrap_or(&default_target);
        let explicit_baseline =
            baseline_ref_from_config(corpus.clone(), selection_prefix, baseline_target);
        let (baselines, selection) = resolve_baseline_selection(
            &corpus,
            project_root,
            selection_prefix,
            baseline_target,
            &explicit_baseline,
        );

        let default_postgres = SearchPostgresConfig::default();
        let postgres = project_config.map(|config| &config.postgres).unwrap_or(&default_postgres);

        if !postgres.is_configured() {
            if use_postgres {
                return Self {
                    configured_baseline: ConfiguredBaselineStatus {
                        backend: "postgres",
                        selection,
                        issue: Some(
                            "search.baseline.postgres is not configured; set host, dbname, schema, vault_role_base, and credential_helper.program"
                                .to_owned(),
                        ),
                        support: None,
                    },
                    external_baseline: None,
                };
            }

            return Self {
                configured_baseline: ConfiguredBaselineStatus {
                    backend: "sqlite",
                    selection: local_baseline_description(&corpus),
                    issue: None,
                    support: None,
                },
                external_baseline: None,
            };
        }

        let connection = match resolve_postgres_url(postgres, PostgresAccessMode::Reader) {
            Ok(resolved) => resolved.url,
            Err(error) => {
                return Self {
                    configured_baseline: ConfiguredBaselineStatus {
                        backend: "postgres",
                        selection,
                        issue: Some(format!(
                            "failed to resolve PostgreSQL reader credentials: {error}"
                        )),
                        support: None,
                    },
                    external_baseline: None,
                };
            }
        };

        let schema = resolve_schema(schema_keys, postgres);

        let context = RefreshContext {
            postgres: postgres.clone(),
            baselines: baselines.clone(),
            selection: selection.clone(),
            schema_keys: schema_keys.iter().map(|s| s.to_string()).collect(),
        };

        match RefreshableExternalBaselineSource::new(connection, schema.clone(), context) {
            Ok(source) => {
                let support = if matches!(corpus, CorpusId::WorkspaceCode) {
                    resolve_workspace_support_status(project_root, &baseline_target.policy, &source)
                } else {
                    None
                };
                let service = ExternalBaselineService::spawn(source);
                tracing::info!(corpus = %corpus, "refreshable external baseline source configured");
                Self {
                    configured_baseline: ConfiguredBaselineStatus {
                        backend: "postgres",
                        selection,
                        issue: None,
                        support,
                    },
                    external_baseline: Some(Arc::new(service)),
                }
            }
            Err(error) => {
                tracing::warn!(corpus = %corpus, "failed to configure refreshable external baseline source: {error}");
                Self {
                    configured_baseline: ConfiguredBaselineStatus {
                        backend: "postgres",
                        selection,
                        issue: Some(error.to_string()),
                        support: None,
                    },
                    external_baseline: None,
                }
            }
        }
    }

    fn summary(&self) -> BaselineResolutionSummary {
        BaselineResolutionSummary {
            backend: self.configured_baseline.backend.to_owned(),
            selection: self.configured_baseline.selection.clone(),
            issue: self.configured_baseline.issue.clone(),
        }
    }
}

pub fn resolve_project_baseline_diagnostics(
    project_root: Option<&Path>,
    project_config: &ProjectConfig,
) -> BaselineConfigDiagnostics {
    let workspace = BaselineRuntime::workspace(project_root, project_config).summary();
    let reference = BaselineRuntime::reference(Some(project_config)).summary();
    BaselineConfigDiagnostics { workspace, reference }
}

impl ExternalBaselineService {
    fn spawn(source: RefreshableExternalBaselineSource) -> Self {
        let corpus = source.corpus().clone();
        let schema = source._schema_for_status();
        let selection = source._selection();
        let local_reference_fingerprint = source.local_reference_fingerprint();
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name(format!("baseline-service-{}", corpus.as_str()))
            .spawn(move || Self::worker_loop(source, receiver))
            .expect("failed to spawn external baseline service worker");

        Self {
            corpus,
            schema,
            selection,
            local_reference_fingerprint,
            sender,
            worker: StdMutex::new(Some(worker)),
            closed: AtomicBool::new(false),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(source: RefreshableExternalBaselineSource) -> Arc<Self> {
        Arc::new(Self::spawn(source))
    }

    fn worker_loop(
        source: RefreshableExternalBaselineSource,
        receiver: mpsc::Receiver<BaselineServiceRequest>,
    ) {
        while let Ok(request) = receiver.recv() {
            match request {
                BaselineServiceRequest::ProbeStatus { reply } => {
                    let _ = reply.send(source.probe_status());
                }
                BaselineServiceRequest::ResolveSnapshot { reply } => {
                    let _ = reply.send(source.resolve_snapshot());
                }
                BaselineServiceRequest::LexicalSearch {
                    snapshot_id,
                    query,
                    collection,
                    limit,
                    reply,
                } => {
                    let result =
                        source.lexical_search(&snapshot_id, &query, collection.as_deref(), limit);
                    let _ = reply.send(result);
                }
                BaselineServiceRequest::SemanticSearch {
                    snapshot_id,
                    query_embedding,
                    model_id,
                    dimension,
                    collection,
                    limit,
                    reply,
                } => {
                    let result = source.semantic_search(
                        &snapshot_id,
                        &query_embedding,
                        &model_id,
                        dimension,
                        collection.as_deref(),
                        limit,
                    );
                    let _ = reply.send(result);
                }
                BaselineServiceRequest::LoadReferenceSnapshotDocuments {
                    model_id,
                    dimension,
                    reply,
                } => {
                    let result =
                        source.load_reference_snapshot_documents(model_id.as_deref(), dimension);
                    let _ = reply.send(result);
                }
                BaselineServiceRequest::LoadBaselineManifest { snapshot_id, reply } => {
                    let result = source.load_baseline_manifest(&snapshot_id);
                    let _ = reply.send(result);
                }
                BaselineServiceRequest::EmbeddingIdentity { reply } => {
                    let _ = reply.send(source.embedding_identity());
                }
                BaselineServiceRequest::Shutdown { reply } => {
                    let _ = reply.send(());
                    break;
                }
            }
        }
    }

    fn request<R>(
        &self,
        build: impl FnOnce(mpsc::Sender<R>) -> BaselineServiceRequest,
    ) -> Result<R, bsl_search::SearchError>
    where
        R: Send + 'static,
    {
        if self.closed.load(Ordering::Acquire) {
            return Err(service_closed_error(&self.corpus));
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender.send(build(reply_tx)).map_err(|_| service_closed_error(&self.corpus))?;
        reply_rx.recv().map_err(|_| service_closed_error(&self.corpus))
    }

    pub(crate) fn lexical_search(
        &self,
        snapshot_id: &str,
        query: &str,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<bsl_search::LexicalHit>, bsl_search::SearchError> {
        self.request(|reply| BaselineServiceRequest::LexicalSearch {
            snapshot_id: snapshot_id.to_owned(),
            query: query.to_owned(),
            collection: collection.map(ToOwned::to_owned),
            limit,
            reply,
        })?
    }

    pub(crate) fn semantic_search(
        &self,
        snapshot_id: &str,
        query_embedding: &[f32],
        model_id: &str,
        dimension: usize,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<bsl_search::SemanticHit>, bsl_search::SearchError> {
        self.request(|reply| BaselineServiceRequest::SemanticSearch {
            snapshot_id: snapshot_id.to_owned(),
            query_embedding: query_embedding.to_vec(),
            model_id: model_id.to_owned(),
            dimension,
            collection: collection.map(ToOwned::to_owned),
            limit,
            reply,
        })?
    }

    pub(crate) fn probe_status(&self) -> ExternalBaselineStatus {
        self.request(|reply| BaselineServiceRequest::ProbeStatus { reply }).unwrap_or_else(
            |error| ExternalBaselineStatus {
                backend: "postgres",
                schema: self.schema.clone(),
                selection: self.selection.clone(),
                resolved: None,
                state: ExternalBaselineState::Error(error.to_string()),
            },
        )
    }

    pub(crate) fn resolve_reference_view(
        &self,
    ) -> Result<Option<ResolvedView>, bsl_search::SearchError> {
        let Some(snapshot) = self.load_reference_snapshot_documents(None, None)? else {
            return Ok(None);
        };
        let baseline = BaselineRef::for_snapshot(self.corpus.clone(), snapshot.snapshot_id);
        Ok(Some(ResolvedView::new(baseline, snapshot.documents)))
    }

    pub(crate) fn load_reference_snapshot_documents(
        &self,
        model_id: Option<&str>,
        dimension: Option<usize>,
    ) -> Result<Option<BaselineSnapshotDocuments>, bsl_search::SearchError> {
        self.request(|reply| BaselineServiceRequest::LoadReferenceSnapshotDocuments {
            model_id: model_id.map(ToOwned::to_owned),
            dimension,
            reply,
        })?
    }

    pub(crate) fn load_baseline_manifest(
        &self,
        snapshot_id: &str,
    ) -> Result<WorkspaceBaselineManifest, bsl_search::SearchError> {
        self.request(|reply| BaselineServiceRequest::LoadBaselineManifest {
            snapshot_id: snapshot_id.to_owned(),
            reply,
        })?
    }

    pub fn embedding_identity(&self) -> Result<Option<(String, usize)>, bsl_search::SearchError> {
        self.request(|reply| BaselineServiceRequest::EmbeddingIdentity { reply })?
    }

    pub(crate) fn corpus(&self) -> CorpusId {
        self.corpus.clone()
    }

    pub(crate) fn local_reference_fingerprint(&self) -> Option<String> {
        self.local_reference_fingerprint.clone()
    }

    pub(crate) fn resolve_snapshot(
        &self,
    ) -> Result<Option<(BaselineRef, bsl_search::Snapshot)>, bsl_search::SearchError> {
        self.request(|reply| BaselineServiceRequest::ResolveSnapshot { reply })?
    }

    pub(crate) fn shutdown(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }

        let (reply_tx, reply_rx) = mpsc::channel();
        let acknowledged = match self
            .sender
            .send(BaselineServiceRequest::Shutdown { reply: reply_tx })
        {
            Ok(()) => match reply_rx.recv_timeout(shutdown_ack_timeout()) {
                Ok(()) => true,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    tracing::warn!(
                        corpus = %self.corpus,
                        timeout_ms = shutdown_ack_timeout().as_millis(),
                        "external baseline service shutdown timed out waiting for worker acknowledgement"
                    );
                    false
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    tracing::warn!(
                        corpus = %self.corpus,
                        "external baseline service shutdown acknowledgement channel disconnected"
                    );
                    false
                }
            },
            Err(_) => {
                tracing::warn!(
                    corpus = %self.corpus,
                    "external baseline service shutdown request could not be sent"
                );
                false
            }
        };

        if let Ok(mut worker) = self.worker.lock() {
            if let Some(handle) = worker.take() {
                if acknowledged || handle.is_finished() {
                    let _ = handle.join();
                } else {
                    drop(handle);
                }
            }
        }
    }
}

impl Drop for ExternalBaselineService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn service_closed_error(corpus: &CorpusId) -> bsl_search::SearchError {
    bsl_search::SearchError::ExternalBaseline(format!(
        "baseline_service_closed: external baseline service for {} is not available",
        corpus.as_str()
    ))
}

#[cfg(test)]
fn shutdown_ack_timeout() -> Duration {
    Duration::from_millis(100)
}

#[cfg(not(test))]
fn shutdown_ack_timeout() -> Duration {
    Duration::from_secs(2)
}

#[derive(Debug)]
pub(crate) struct ExternalBaselineSource {
    adapter: ExternalBaselineAdapter,
    baselines: Vec<BaselineRef>,
    selection: String,
}

#[derive(Debug)]
struct RefreshContext {
    postgres: SearchPostgresConfig,
    baselines: Vec<BaselineRef>,
    selection: String,
    schema_keys: Vec<String>,
}

/// A baseline's recorded embedding identity: model name and vector dimension.
type EmbeddingIdentity = (String, usize);

/// Memoized embedding identity keyed by `refresh_generation`. Outer `Option` = not yet
/// populated; inner `Option` = the recorded identity (`None` when the baseline has none).
type EmbeddingIdentityCache = StdMutex<Option<(usize, Option<EmbeddingIdentity>)>>;

#[derive(Debug)]
pub(crate) struct RefreshableExternalBaselineSource {
    inner: StdRwLock<ExternalBaselineSource>,
    context: RefreshContext,
    refresh_generation: AtomicUsize,
    refresh_lock: StdMutex<()>,
    /// Invalidated by `refresh_inner` whenever credentials are refreshed (the generation bumps).
    embedding_identity_cache: EmbeddingIdentityCache,
}

impl RefreshableExternalBaselineSource {
    fn new(
        connection_url: String,
        schema: Option<String>,
        context: RefreshContext,
    ) -> Result<Self, bsl_search::SearchError> {
        let mut config = ExternalBaselineConfig::postgres(connection_url);
        if let Some(schema) = schema {
            config = config.with_schema(schema);
        }
        let inner = StdRwLock::new(ExternalBaselineSource::new_with_candidates(
            config,
            context.baselines.clone(),
            context.selection.clone(),
        )?);
        Ok(Self {
            inner,
            context,
            refresh_generation: AtomicUsize::new(0),
            refresh_lock: StdMutex::new(()),
            embedding_identity_cache: StdMutex::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        config: ExternalBaselineConfig,
        baseline: BaselineRef,
    ) -> Result<Self, bsl_search::SearchError> {
        let selection = baseline_description(&baseline);
        let inner = StdRwLock::new(ExternalBaselineSource::new_with_candidates(
            config,
            vec![baseline.clone()],
            selection.clone(),
        )?);
        let context = RefreshContext {
            postgres: SearchPostgresConfig::default(),
            baselines: vec![baseline],
            selection,
            schema_keys: vec![],
        };
        Ok(Self {
            inner,
            context,
            refresh_generation: AtomicUsize::new(0),
            embedding_identity_cache: StdMutex::new(None),
            refresh_lock: StdMutex::new(()),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_refresh_context(
        config: ExternalBaselineConfig,
        baseline: BaselineRef,
        postgres: SearchPostgresConfig,
    ) -> Result<Self, bsl_search::SearchError> {
        let selection = baseline_description(&baseline);
        let inner = StdRwLock::new(ExternalBaselineSource::new_with_candidates(
            config,
            vec![baseline.clone()],
            selection.clone(),
        )?);
        let context =
            RefreshContext { postgres, baselines: vec![baseline], selection, schema_keys: vec![] };
        Ok(Self {
            inner,
            context,
            refresh_generation: AtomicUsize::new(0),
            embedding_identity_cache: StdMutex::new(None),
            refresh_lock: StdMutex::new(()),
        })
    }

    fn run_with_refresh<F, T>(&self, operation: F) -> Result<T, RefreshOrTerminalError>
    where
        F: Fn(&ExternalBaselineSource) -> Result<T, bsl_search::SearchError>,
    {
        let first_error = {
            let reader = self.inner.read().expect("baseline source lock poisoned");
            match operation(&reader) {
                Ok(value) => return Ok(value),
                Err(error) => {
                    if !error.is_retryable() {
                        return Err(RefreshOrTerminalError::Terminal(error));
                    }
                    error
                }
            }
        };

        let generation_before = self.refresh_generation.load(Ordering::Acquire);
        let _refresh_guard = self.refresh_lock.lock().expect("baseline refresh lock poisoned");

        if self.refresh_generation.load(Ordering::Acquire) != generation_before {
            let reader = self.inner.read().expect("baseline source lock poisoned");
            return match operation(&reader) {
                Ok(value) => Ok(value),
                Err(error) => {
                    tracing::warn!(
                        generation = generation_before,
                        "refreshable external baseline source retry after concurrent refresh failed: {error}"
                    );
                    if error.is_retryable() {
                        Err(RefreshOrTerminalError::Terminal(
                            RefreshAttemptError::RetryExhausted { source: error }
                                .into_search_error(),
                        ))
                    } else {
                        Err(RefreshOrTerminalError::Terminal(error))
                    }
                }
            };
        }

        match self.refresh_inner() {
            Ok(()) => {
                let reader = self.inner.read().expect("baseline source lock poisoned");
                match operation(&reader) {
                    Ok(value) => {
                        tracing::info!(
                            generation = generation_before,
                            "refreshable external baseline source recovered after credential refresh"
                        );
                        Ok(value)
                    }
                    Err(error) => {
                        tracing::warn!(
                            generation = generation_before,
                            "refreshable external baseline source retry after refresh failed: {error}"
                        );
                        if error.is_retryable() {
                            Err(RefreshOrTerminalError::Terminal(
                                RefreshAttemptError::RetryExhausted { source: error }
                                    .into_search_error(),
                            ))
                        } else {
                            Err(RefreshOrTerminalError::Terminal(error))
                        }
                    }
                }
            }
            Err(refresh_err) => {
                tracing::warn!(
                    generation = generation_before,
                    first_error = %first_error,
                    "refreshable external baseline source re-resolve failed: {refresh_err}"
                );
                Err(RefreshOrTerminalError::Terminal(refresh_err.into_search_error()))
            }
        }
    }

    fn refresh_inner(&self) -> Result<(), RefreshAttemptError> {
        let resolved = resolve_postgres_url(&self.context.postgres, PostgresAccessMode::Reader)
            .map_err(RefreshAttemptError::Resolve)?;
        let schema = resolve_schema_vec(&self.context.schema_keys, &self.context.postgres);

        let mut fresh_config = ExternalBaselineConfig::postgres(resolved.url);
        if let Some(schema) = schema {
            fresh_config = fresh_config.with_schema(schema);
        }

        let fresh_source = ExternalBaselineSource::new_with_candidates(
            fresh_config,
            self.context.baselines.clone(),
            self.context.selection.clone(),
        )
        .map_err(RefreshAttemptError::Build)?;

        {
            let mut writer = self.inner.write().expect("baseline source lock poisoned");
            *writer = fresh_source;
        }

        let old = self.refresh_generation.fetch_add(1, Ordering::SeqCst);
        tracing::info!(
            old_generation = old,
            new_generation = old + 1,
            "refreshable external baseline source credentials refreshed"
        );

        Ok(())
    }

    fn delegate<F, T>(&self, operation: F) -> Result<T, bsl_search::SearchError>
    where
        F: Fn(&ExternalBaselineSource) -> Result<T, bsl_search::SearchError>,
    {
        self.run_with_refresh(operation).map_err(|e| match e {
            RefreshOrTerminalError::Terminal(err) => err,
        })
    }

    pub(crate) fn lexical_search(
        &self,
        snapshot_id: &str,
        query: &str,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<bsl_search::LexicalHit>, bsl_search::SearchError> {
        self.delegate(|source| source.lexical_search(snapshot_id, query, collection, limit))
    }

    pub(crate) fn semantic_search(
        &self,
        snapshot_id: &str,
        query_embedding: &[f32],
        model_id: &str,
        dimension: usize,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<bsl_search::SemanticHit>, bsl_search::SearchError> {
        self.delegate(|source| {
            source.semantic_search(
                snapshot_id,
                query_embedding,
                model_id,
                dimension,
                collection,
                limit,
            )
        })
    }

    pub(crate) fn probe_status(&self) -> ExternalBaselineStatus {
        let (backend, schema, selection) = {
            let reader = self.inner.read().expect("baseline source lock poisoned");
            let backend = match reader.adapter.config().backend {
                ExternalBaselineBackend::Postgres => "postgres",
            };
            let schema =
                reader.adapter.config().schema.clone().unwrap_or_else(|| "bsl_search".to_owned());
            (backend, schema, reader.selection.clone())
        };

        match self.run_with_refresh(|source| source.probe_status_result()) {
            Ok(status) => status,
            Err(RefreshOrTerminalError::Terminal(error)) => ExternalBaselineStatus {
                backend,
                schema,
                selection,
                resolved: None,
                state: ExternalBaselineState::Error(error.to_string()),
            },
        }
    }

    pub(crate) fn load_reference_snapshot_documents(
        &self,
        model_id: Option<&str>,
        dimension: Option<usize>,
    ) -> Result<Option<BaselineSnapshotDocuments>, bsl_search::SearchError> {
        self.delegate(|source| source.load_reference_snapshot_documents(model_id, dimension))
    }

    pub(crate) fn load_baseline_manifest(
        &self,
        snapshot_id: &str,
    ) -> Result<WorkspaceBaselineManifest, bsl_search::SearchError> {
        self.delegate(|source| source.load_baseline_manifest(snapshot_id))
    }

    pub(crate) fn corpus(&self) -> CorpusId {
        let reader = self.inner.read().expect("baseline source lock poisoned");
        reader.corpus().clone()
    }

    pub(crate) fn snapshot_details(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<bsl_search::BaselineSnapshotDetails>, bsl_search::SearchError> {
        self.delegate(|source| source.snapshot_details(snapshot_id))
    }

    pub(crate) fn local_reference_fingerprint(&self) -> Option<String> {
        let reader = self.inner.read().expect("baseline source lock poisoned");
        reader.local_reference_fingerprint()
    }

    pub(crate) fn resolve_snapshot(
        &self,
    ) -> Result<Option<(BaselineRef, bsl_search::Snapshot)>, bsl_search::SearchError> {
        self.delegate(|source| source.resolve_snapshot())
    }

    pub(crate) fn _selection(&self) -> String {
        self.context.selection.clone()
    }

    pub(crate) fn _schema_for_status(&self) -> String {
        let reader = self.inner.read().expect("baseline source lock poisoned");
        reader.adapter.config().schema.clone().unwrap_or_else(|| "bsl_search".to_owned())
    }

    fn embedding_identity(&self) -> Result<Option<(String, usize)>, bsl_search::SearchError> {
        // The identity is immutable for a baseline, so memoize it and re-read only after a
        // refresh swaps the adapter (keyed by `refresh_generation`). This keeps the per-query
        // semantic path off the DB. A read error is not cached, so a transient failure recovers
        // on the next call. `refresh_inner` only takes `inner.write()` (never this cache lock),
        // so there is no lock-ordering cycle with the read lock taken below.
        let generation = self.refresh_generation.load(Ordering::Acquire);
        let mut cache =
            self.embedding_identity_cache.lock().expect("embedding identity cache poisoned");
        if let Some((cached_generation, value)) = cache.as_ref() {
            if *cached_generation == generation {
                return Ok(value.clone());
            }
        }
        let value = {
            let reader = self.inner.read().expect("baseline source lock poisoned");
            reader.adapter.read_embedding_identity()?
        };
        *cache = Some((generation, value.clone()));
        Ok(value)
    }
}

enum RefreshOrTerminalError {
    Terminal(bsl_search::SearchError),
}

#[derive(Debug)]
enum RefreshAttemptError {
    Resolve(project_model::ResolvePostgresUrlError),
    Build(bsl_search::SearchError),
    RetryExhausted { source: bsl_search::SearchError },
}

impl RefreshAttemptError {
    fn into_search_error(self) -> bsl_search::SearchError {
        match self {
            Self::Resolve(error) => bsl_search::SearchError::ExternalBaseline(format!(
                "{}: {error}",
                resolve_reason_code(&error)
            )),
            Self::Build(error) => error,
            Self::RetryExhausted { source } => bsl_search::SearchError::ExternalBaseline(format!(
                "refresh_retry_exhausted: {source}"
            )),
        }
    }
}

impl std::fmt::Display for RefreshAttemptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resolve(err) => write!(f, "resolve: {err}"),
            Self::Build(err) => write!(f, "build: {err}"),
            Self::RetryExhausted { source } => write!(f, "retry exhausted: {source}"),
        }
    }
}

fn resolve_reason_code(error: &project_model::ResolvePostgresUrlError) -> &'static str {
    use project_model::ResolvePostgresUrlError;

    match error {
        ResolvePostgresUrlError::MissingField(_)
        | ResolvePostgresUrlError::MissingCredentialHelper => "missing_config",
        ResolvePostgresUrlError::HelperSpawn { .. } => "helper_spawn_failed",
        ResolvePostgresUrlError::HelperTimeout { .. } => "helper_timeout",
        ResolvePostgresUrlError::HelperProtocol { .. } => "helper_protocol_error",
        ResolvePostgresUrlError::HelperRejected { .. } => "helper_rejected",
        ResolvePostgresUrlError::UnsupportedUrlScheme(_)
        | ResolvePostgresUrlError::InvalidResolvedUrl(_) => "helper_protocol_error",
        ResolvePostgresUrlError::TargetMismatch { .. } => "resolved_target_mismatch",
    }
}

impl ExternalBaselineSource {
    #[cfg(test)]
    pub(crate) fn new(
        config: ExternalBaselineConfig,
        baseline: BaselineRef,
    ) -> Result<Self, bsl_search::SearchError> {
        let selection = baseline_description(&baseline);
        Self::new_with_candidates(config, vec![baseline], selection)
    }

    pub(crate) fn new_with_candidates(
        config: ExternalBaselineConfig,
        baselines: Vec<BaselineRef>,
        selection: String,
    ) -> Result<Self, bsl_search::SearchError> {
        let adapter = ExternalBaselineAdapter::new(config)?;
        Ok(Self { adapter, baselines, selection })
    }

    pub(crate) fn lexical_search(
        &self,
        snapshot_id: &str,
        query: &str,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<bsl_search::LexicalHit>, bsl_search::SearchError> {
        self.adapter.lexical_search_baseline(snapshot_id, query, collection, limit)
    }

    pub(crate) fn semantic_search(
        &self,
        snapshot_id: &str,
        query_embedding: &[f32],
        model_id: &str,
        dimension: usize,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<bsl_search::SemanticHit>, bsl_search::SearchError> {
        self.adapter.semantic_search_baseline(
            snapshot_id,
            query_embedding,
            model_id,
            dimension,
            collection,
            limit,
        )
    }

    #[cfg(test)]
    pub(crate) fn probe_status(&self) -> ExternalBaselineStatus {
        match self.probe_status_result() {
            Ok(status) => status,
            Err(error) => ExternalBaselineStatus {
                backend: match self.adapter.config().backend {
                    ExternalBaselineBackend::Postgres => "postgres",
                },
                schema: self
                    .adapter
                    .config()
                    .schema
                    .clone()
                    .unwrap_or_else(|| "bsl_search".to_owned()),
                selection: self.selection.clone(),
                resolved: None,
                state: ExternalBaselineState::Error(error.to_string()),
            },
        }
    }

    fn probe_status_result(&self) -> Result<ExternalBaselineStatus, bsl_search::SearchError> {
        let backend = match self.adapter.config().backend {
            ExternalBaselineBackend::Postgres => "postgres",
        };
        let schema =
            self.adapter.config().schema.clone().unwrap_or_else(|| "bsl_search".to_owned());
        let selection = self.selection.clone();

        match self.resolve_snapshot()? {
            Some((resolved_baseline, snapshot)) => {
                // Counts only — do NOT load the serving rows. `load_snapshot_documents` pulls the
                // entire baseline corpus from Postgres (~228K rows) just to count it, which on
                // the status path runs under the engine lock and stalls `search status` past the
                // client timeout. `snapshot_details` returns aggregated counts from
                // snapshot/file-object metadata (O(files), not O(serving rows)) instead.
                let snapshot_id_str = snapshot.id.0.clone();
                let (documents, files) = match self.adapter.snapshot_details(&snapshot_id_str)? {
                    Some(details) => (details.snapshot.documents, details.snapshot.files),
                    None => {
                        // The snapshot resolved above but its detail row was not found —
                        // a brief metadata race. Report 0/0 (counts unavailable, not an empty
                        // baseline) rather than failing the whole status call.
                        tracing::warn!(
                            snapshot_id = %snapshot_id_str,
                            "probe_status: snapshot_details returned None for a resolved snapshot; \
                             reporting counts as 0 (metadata race)"
                        );
                        (0, 0)
                    }
                };
                Ok(ExternalBaselineStatus {
                    backend,
                    schema,
                    selection,
                    resolved: Some(baseline_description(&resolved_baseline)),
                    state: ExternalBaselineState::Ready {
                        snapshot_id: snapshot.id.0,
                        fingerprint: snapshot.fingerprint,
                        documents,
                        files,
                    },
                })
            }
            None => Ok(ExternalBaselineStatus {
                backend,
                schema,
                selection,
                resolved: None,
                state: ExternalBaselineState::Missing,
            }),
        }
    }

    pub(crate) fn snapshot_details(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<bsl_search::BaselineSnapshotDetails>, bsl_search::SearchError> {
        self.adapter.snapshot_details(snapshot_id)
    }

    pub(crate) fn load_reference_snapshot_documents(
        &self,
        model_id: Option<&str>,
        dimension: Option<usize>,
    ) -> Result<Option<BaselineSnapshotDocuments>, bsl_search::SearchError> {
        if !matches!(self.corpus(), CorpusId::Reference) {
            return Ok(None);
        }
        self.load_snapshot_documents(model_id, dimension)
    }

    fn load_snapshot_documents(
        &self,
        model_id: Option<&str>,
        dimension: Option<usize>,
    ) -> Result<Option<BaselineSnapshotDocuments>, bsl_search::SearchError> {
        let Some((_, snapshot)) = self.resolve_snapshot()? else {
            return Ok(None);
        };
        let documents = self.adapter.load_snapshot_documents(&snapshot)?;
        let shared_embeddings = if let (Some(model_id), Some(dimension)) = (model_id, dimension) {
            let embedding_keys = documents
                .iter()
                .map(bsl_search::semantic_key_for_indexed_document)
                .collect::<Vec<_>>();
            self.adapter.load_embeddings(&embedding_keys, model_id, dimension)?
        } else {
            HashMap::new()
        };
        Ok(Some(BaselineSnapshotDocuments {
            snapshot_id: snapshot.id.0,
            fingerprint: snapshot.fingerprint,
            documents,
            shared_embeddings,
        }))
    }

    pub(crate) fn load_baseline_manifest(
        &self,
        snapshot_id: &str,
    ) -> Result<WorkspaceBaselineManifest, bsl_search::SearchError> {
        self.adapter.load_baseline_manifest(snapshot_id)
    }

    pub(crate) fn corpus(&self) -> &CorpusId {
        &self.baselines[0].corpus
    }

    pub(crate) fn local_reference_fingerprint(&self) -> Option<String> {
        if !matches!(self.corpus(), CorpusId::Reference) {
            return None;
        }
        Some(fingerprint_documents(&platform_reference_documents()))
    }

    pub(crate) fn resolve_snapshot(
        &self,
    ) -> Result<Option<(BaselineRef, bsl_search::Snapshot)>, bsl_search::SearchError> {
        for baseline in &self.baselines {
            if let Some(snapshot) = self.adapter.resolve_baseline(baseline)? {
                return Ok(Some((baseline.clone(), snapshot)));
            }
        }
        Ok(None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExternalBaselineStatus {
    pub backend: &'static str,
    pub schema: String,
    pub selection: String,
    pub resolved: Option<String>,
    pub state: ExternalBaselineState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExternalBaselineState {
    Ready { snapshot_id: String, fingerprint: Option<String>, documents: usize, files: usize },
    Missing,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfiguredBaselineStatus {
    pub backend: &'static str,
    pub selection: String,
    pub issue: Option<String>,
    pub support: Option<ResolvedWorkspaceBaselineSupport>,
}

impl ConfiguredBaselineStatus {
    pub fn search_is_expired(&self) -> bool {
        self.support
            .as_ref()
            .is_some_and(|support| matches!(support.state, SearchBaselineSupportState::Expired))
    }
}

pub(crate) fn baseline_description(baseline: &BaselineRef) -> String {
    if let Some(snapshot_id) = &baseline.snapshot_id {
        return format!("snapshot {}", snapshot_id.0);
    }
    if let (Some(branch), Some(commit)) = (&baseline.branch, &baseline.commit) {
        return format!("branch {branch} @ {commit}");
    }
    if let Some(branch) = &baseline.branch {
        return format!("branch {branch}");
    }
    if let Some(commit) = &baseline.commit {
        return format!("commit {commit}");
    }
    format!("latest {}", baseline.corpus.as_str())
}

fn local_baseline_description(corpus: &CorpusId) -> String {
    match corpus {
        CorpusId::WorkspaceCode => "local workspace index".to_owned(),
        CorpusId::Reference => "local reference index".to_owned(),
        CorpusId::Custom(id) => format!("local {id} index"),
    }
}

fn baseline_ref_from_config(
    corpus: CorpusId,
    selection_prefix: &str,
    target: &SearchBaselineTargetConfig,
) -> BaselineRef {
    BaselineRef {
        corpus,
        snapshot_id: env::var(format!("{selection_prefix}_SNAPSHOT_ID"))
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| target.snapshot_id.clone())
            .map(bsl_search::SnapshotId::new),
        branch: env::var(format!("{selection_prefix}_BRANCH"))
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| target.branch.clone()),
        commit: env::var(format!("{selection_prefix}_COMMIT"))
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| target.commit.clone()),
    }
}

fn resolve_baseline_selection(
    corpus: &CorpusId,
    project_root: Option<&Path>,
    selection_prefix: &str,
    target: &SearchBaselineTargetConfig,
    explicit_baseline: &BaselineRef,
) -> (Vec<BaselineRef>, String) {
    if baseline_has_explicit_selection(explicit_baseline) {
        return (vec![explicit_baseline.clone()], baseline_description(explicit_baseline));
    }

    if matches!(corpus, CorpusId::WorkspaceCode) && target.policy.is_configured() {
        if let Some(policy_selection) =
            resolve_workspace_policy_selection(project_root, &target.policy)
        {
            let baselines = policy_selection
                .candidate_branches()
                .into_iter()
                .map(|branch| BaselineRef {
                    corpus: corpus.clone(),
                    snapshot_id: None,
                    branch: Some(branch),
                    commit: None,
                })
                .collect::<Vec<_>>();
            return (baselines, policy_selection.selection_description());
        }
    }

    let baseline = baseline_ref_from_config(corpus.clone(), selection_prefix, target);
    (vec![baseline.clone()], baseline_description(&baseline))
}

fn baseline_has_explicit_selection(baseline: &BaselineRef) -> bool {
    baseline.snapshot_id.is_some() || baseline.branch.is_some() || baseline.commit.is_some()
}

fn resolve_workspace_policy_selection(
    project_root: Option<&Path>,
    policy: &SearchBaselinePolicyConfig,
) -> Option<project_model::ResolvedWorkspaceBranchPolicy> {
    let workspace_branch = resolve_workspace_branch(project_root);
    resolve_workspace_branch_policy(policy, workspace_branch.as_deref())
}

fn resolve_workspace_branch(project_root: Option<&Path>) -> Option<String> {
    project_root
        .and_then(current_git_branch)
        .or_else(|| resolve_env_value(&["CI_COMMIT_BRANCH", "CI_COMMIT_REF_NAME"]))
}

fn resolve_workspace_support_status(
    project_root: Option<&Path>,
    policy: &SearchBaselinePolicyConfig,
    source: &RefreshableExternalBaselineSource,
) -> Option<ResolvedWorkspaceBaselineSupport> {
    if !policy.is_configured() {
        return None;
    }

    let workspace_branch = resolve_workspace_branch(project_root);
    let (resolved_baseline, snapshot) = source.resolve_snapshot().ok().flatten()?;
    let details = source.snapshot_details(&snapshot.id.0).ok().flatten()?;
    let snapshot_created_at = parse_timestamp_utc(&details.snapshot.created_at);
    let selected_branch =
        resolved_baseline.branch.as_deref().or(details.snapshot.branch.as_deref());

    evaluate_workspace_baseline_support_now(
        policy,
        workspace_branch.as_deref(),
        selected_branch,
        snapshot_created_at,
    )
}

fn resolve_schema(schema_keys: &[&str], postgres: &SearchPostgresConfig) -> Option<String> {
    resolve_env_value(schema_keys)
        .filter(|v| !v.trim().is_empty())
        .or_else(|| postgres.schema.clone())
}

fn resolve_schema_vec(schema_keys: &[String], postgres: &SearchPostgresConfig) -> Option<String> {
    resolve_env_value_from_vec(schema_keys)
        .filter(|v| !v.trim().is_empty())
        .or_else(|| postgres.schema.clone())
}

fn resolve_env_value_from_vec(keys: &[String]) -> Option<String> {
    keys.iter().find_map(|key| env::var(key.as_str()).ok().filter(|value| !value.trim().is_empty()))
}

fn resolve_env_value(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| env::var(key).ok().filter(|value| !value.trim().is_empty()))
}

fn platform_reference_documents() -> Vec<Document> {
    let platform = PlatformDataInner::instance();
    let mut documents = Vec::new();

    for ty in platform.all_types() {
        let methods = platform.get_type_methods(&ty.name);
        let method_list: String = methods
            .iter()
            .map(|method| format!("{} / {}", method.name, method.english_name))
            .collect::<Vec<_>>()
            .join(", ");

        documents.push(Document {
            title: format!("{} / {}", ty.name, ty.english_name),
            body: format!("Тип: {} / {}\nМетоды: {method_list}", ty.name, ty.english_name),
            kind: "type".to_owned(),
        });
    }

    for method in platform.all_methods() {
        let mut body = format!(
            "Тип: {}\nМетод: {} / {}\n",
            method.type_name, method.name, method.english_name
        );
        if let Some(ref ret) = method.return_type {
            body.push_str(&format!("Возвращает: {ret}\n"));
        }
        if let Some(docs) = platform.get_method_docs(method.id) {
            if !docs.syntax.is_empty() {
                body.push_str(&format!("Синтаксис: {}\n", docs.syntax));
            }
            if !docs.description.is_empty() {
                body.push_str(&format!("Описание: {}\n", docs.description));
            }
            for param in &docs.params {
                body.push_str(&format!("Параметр {}: {}\n", param.name, param.description));
            }
            for example in &docs.examples {
                body.push_str(&format!("Пример: {}\n", example.code));
            }
        }
        documents.push(Document {
            title: format!(
                "{}.{} / {}.{}",
                method.type_name, method.name, method.type_name, method.english_name
            ),
            body,
            kind: "method".to_owned(),
        });
    }

    for func in platform.all_global_functions() {
        let mut body = format!("Глобальная функция: {} / {}\n", func.name, func.english_name);
        if let Some(ref ret) = func.return_type {
            body.push_str(&format!("Возвращает: {ret}\n"));
        }
        if let Some(docs) = platform.get_global_function_docs(func.id) {
            if !docs.syntax.is_empty() {
                body.push_str(&format!("Синтаксис: {}\n", docs.syntax));
            }
            if !docs.description.is_empty() {
                body.push_str(&format!("Описание: {}\n", docs.description));
            }
            for param in &docs.params {
                body.push_str(&format!("Параметр {}: {}\n", param.name, param.description));
            }
        }
        documents.push(Document {
            title: format!("{} / {}", func.name, func.english_name),
            body,
            kind: "global_function".to_owned(),
        });
    }

    documents
}

#[cfg(test)]
mod tests {
    use super::{
        baseline_description, resolve_project_baseline_diagnostics, BaselineRuntime,
        BaselineServiceRequest, ConfiguredBaselineStatus, ExternalBaselineService,
        ExternalBaselineSource, ExternalBaselineState, ExternalBaselineStatus,
        RefreshOrTerminalError, RefreshableExternalBaselineSource,
    };
    use bsl_search::{BaselineRef, CorpusId, ExternalBaselineConfig, SearchError};
    use project_model::{
        ProjectConfig, SearchBaselineBackend, SearchBaselineConfig, SearchBaselineTargetConfig,
        SearchConfig, SearchPostgresConfig, SearchPostgresCredentialHelperConfig,
    };
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    #[test]
    fn baseline_description_prefers_snapshot_id() {
        let baseline = BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "snapshot-1");
        assert_eq!(baseline_description(&baseline), "snapshot snapshot-1");
    }

    #[test]
    fn external_baseline_probe_reports_connection_errors() {
        let source = ExternalBaselineSource::new(
            ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
            BaselineRef {
                corpus: CorpusId::WorkspaceCode,
                snapshot_id: None,
                branch: Some("main".to_owned()),
                commit: None,
            },
        )
        .unwrap();

        let status = source.probe_status();
        assert_eq!(status.backend, "postgres");
        assert!(matches!(status.state, ExternalBaselineState::Error(_)));
    }

    #[test]
    fn workspace_uses_sqlite_when_search_backend_is_default() {
        let runtime = BaselineRuntime::workspace(None, &ProjectConfig::default());

        assert_eq!(
            runtime.configured_baseline,
            ConfiguredBaselineStatus {
                backend: "sqlite",
                selection: "local workspace index".to_owned(),
                issue: None,
                support: None,
            }
        );
        assert!(runtime.external_baseline.is_none());
    }

    #[test]
    fn workspace_reports_error_when_postgres_backend_lacks_postgres_config() {
        let runtime = BaselineRuntime::workspace(
            None,
            &ProjectConfig {
                search: SearchConfig {
                    baseline: SearchBaselineConfig {
                        backend: SearchBaselineBackend::Postgres,
                        ..SearchBaselineConfig::default()
                    },
                },
                ..ProjectConfig::default()
            },
        );

        assert_eq!(runtime.configured_baseline.backend, "postgres");
        assert_eq!(runtime.configured_baseline.selection, "latest workspace-code");
        assert_eq!(
            runtime.configured_baseline.issue.as_deref(),
            Some(
                "search.baseline.postgres is not configured; set host, dbname, schema, vault_role_base, and credential_helper.program"
            )
        );
        assert!(runtime.external_baseline.is_none());
    }

    #[test]
    fn workspace_uses_postgres_when_helper_configured() {
        let runtime = BaselineRuntime::workspace(
            None,
            &ProjectConfig {
                search: SearchConfig {
                    baseline: SearchBaselineConfig {
                        backend: SearchBaselineBackend::Postgres,
                        postgres: SearchPostgresConfig {
                            host: Some("pg-central.company.com".to_owned()),
                            port: Some(5432),
                            dbname: Some("bsl_search".to_owned()),
                            schema: Some("corp_search".to_owned()),
                            vault_role_base: Some("prod/search/bsl-analyzer".to_owned()),
                            credential_helper: SearchPostgresCredentialHelperConfig {
                                program: Some("echo".to_owned()),
                                args: vec![],
                            },
                        },
                        workspace_code: SearchBaselineTargetConfig {
                            branch: Some("main".to_owned()),
                            ..SearchBaselineTargetConfig::default()
                        },
                        ..SearchBaselineConfig::default()
                    },
                },
                ..ProjectConfig::default()
            },
        );

        assert_eq!(runtime.configured_baseline.backend, "postgres");
        assert_eq!(runtime.configured_baseline.selection, "branch main");
        assert!(runtime.configured_baseline.issue.is_some());
        assert!(runtime.external_baseline.is_none());
    }

    #[test]
    fn workspace_reports_missing_credential_helper() {
        let runtime = BaselineRuntime::workspace(
            None,
            &ProjectConfig {
                search: SearchConfig {
                    baseline: SearchBaselineConfig {
                        backend: SearchBaselineBackend::Postgres,
                        postgres: SearchPostgresConfig {
                            host: Some("pg-central.company.com".to_owned()),
                            dbname: Some("bsl_search".to_owned()),
                            port: None,
                            schema: Some("bsl_search".to_owned()),
                            vault_role_base: Some("prod/search/bsl-analyzer".to_owned()),
                            credential_helper: SearchPostgresCredentialHelperConfig {
                                program: None,
                                args: vec![],
                            },
                        },
                        workspace_code: SearchBaselineTargetConfig {
                            branch: Some("main".to_owned()),
                            ..SearchBaselineTargetConfig::default()
                        },
                        ..SearchBaselineConfig::default()
                    },
                },
                ..ProjectConfig::default()
            },
        );

        assert_eq!(runtime.configured_baseline.backend, "postgres");
        assert_eq!(runtime.configured_baseline.selection, "branch main");
        assert!(runtime
            .configured_baseline
            .issue
            .as_deref()
            .is_some_and(|issue| issue.contains("credential_helper")));
        assert!(runtime.external_baseline.is_none());
    }

    #[test]
    fn workspace_policy_selection_uses_branch_chain() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/feature/demo\n").unwrap();

        let postgres_config = build_dummy_postgres_config();

        let runtime = BaselineRuntime::workspace(
            Some(dir.path()),
            &ProjectConfig {
                search: SearchConfig {
                    baseline: SearchBaselineConfig {
                        backend: SearchBaselineBackend::Postgres,
                        postgres: postgres_config,
                        workspace_code: serde_json::from_value(serde_json::json!({
                            "policy": {
                                "publishBranches": ["vendor", "develop"],
                                "branches": [
                                    {
                                        "match": "feature/*",
                                        "selectBranch": "develop",
                                        "fallbackBranch": "vendor"
                                    },
                                    {
                                        "match": "*",
                                        "selectBranch": "develop",
                                        "fallbackBranch": "vendor"
                                    }
                                ]
                            }
                        }))
                        .unwrap(),
                        ..SearchBaselineConfig::default()
                    },
                },
                ..ProjectConfig::default()
            },
        );

        assert_eq!(runtime.configured_baseline.backend, "postgres");
        assert_eq!(
            runtime.configured_baseline.selection,
            "workspace branch feature/demo -> branch develop -> branch vendor"
        );
        assert!(runtime.external_baseline.is_none());
    }

    #[test]
    fn project_baseline_diagnostics_returns_workspace_and_reference_summaries() {
        let postgres_config = build_dummy_postgres_config();

        let diagnostics = resolve_project_baseline_diagnostics(
            None,
            &ProjectConfig {
                search: SearchConfig {
                    baseline: SearchBaselineConfig {
                        backend: SearchBaselineBackend::Postgres,
                        postgres: postgres_config,
                        workspace_code: SearchBaselineTargetConfig {
                            branch: Some("main".to_owned()),
                            ..SearchBaselineTargetConfig::default()
                        },
                        reference: SearchBaselineTargetConfig {
                            snapshot_id: Some("reference:0.1.104".to_owned()),
                            ..SearchBaselineTargetConfig::default()
                        },
                        ..SearchBaselineConfig::default()
                    },
                },
                ..ProjectConfig::default()
            },
        );

        assert_eq!(diagnostics.workspace.backend, "postgres");
        assert_eq!(diagnostics.workspace.selection, "branch main");
        assert!(diagnostics.workspace.issue.is_some());
        assert_eq!(diagnostics.reference.backend, "postgres");
        assert_eq!(diagnostics.reference.selection, "snapshot reference:0.1.104");
        assert!(diagnostics.reference.issue.is_some());
    }

    fn build_dummy_postgres_config() -> SearchPostgresConfig {
        SearchPostgresConfig {
            host: Some("pg-central.company.com".to_owned()),
            port: Some(5432),
            dbname: Some("bsl_search".to_owned()),
            schema: Some("corp_search".to_owned()),
            vault_role_base: Some("prod/search/bsl-analyzer".to_owned()),
            credential_helper: SearchPostgresCredentialHelperConfig {
                program: Some("echo".to_owned()),
                args: vec![],
            },
        }
    }

    #[test]
    fn refreshable_source_constructs_from_test_config() {
        let source = RefreshableExternalBaselineSource::for_test(
            ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
            BaselineRef {
                corpus: CorpusId::WorkspaceCode,
                snapshot_id: None,
                branch: Some("main".to_owned()),
                commit: None,
            },
        )
        .unwrap();

        assert!(matches!(source.corpus(), CorpusId::WorkspaceCode));
    }

    #[test]
    fn refreshable_source_probe_status_reports_connection_error() {
        let source = RefreshableExternalBaselineSource::for_test(
            ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
            BaselineRef {
                corpus: CorpusId::Reference,
                snapshot_id: Some(bsl_search::SnapshotId::new("ref:0.1.0")),
                branch: None,
                commit: None,
            },
        )
        .unwrap();

        let status = source.probe_status();
        assert_eq!(status.backend, "postgres");
        assert!(matches!(status.state, ExternalBaselineState::Error(_)));
    }

    #[test]
    fn refreshable_source_probe_status_refreshes_retryable_failures() {
        let postgres = SearchPostgresConfig {
            host: Some("127.0.0.1".to_owned()),
            port: Some(1),
            dbname: Some("bsl_search".to_owned()),
            schema: Some("bsl_search".to_owned()),
            vault_role_base: Some("prod/search/bsl-analyzer".to_owned()),
            credential_helper: SearchPostgresCredentialHelperConfig {
                program: Some("python3".to_owned()),
                args: vec![
                    "-c".to_owned(),
                    "import json; print(json.dumps({'protocol':'bsl-analyzer.postgres-helper.v1','ok':True,'url':'postgres://127.0.0.1:1/bsl_search'}))"
                        .to_owned(),
                ],
            },
        };
        let source = RefreshableExternalBaselineSource::for_test_with_refresh_context(
            ExternalBaselineConfig::postgres("postgres://127.0.0.1:1/bsl_search"),
            BaselineRef {
                corpus: CorpusId::Reference,
                snapshot_id: Some(bsl_search::SnapshotId::new("ref:0.1.0")),
                branch: None,
                commit: None,
            },
            postgres,
        )
        .unwrap();

        let generation_before = source.refresh_generation.load(Ordering::SeqCst);
        let status = source.probe_status();

        assert!(matches!(status.state, ExternalBaselineState::Error(_)));
        assert_eq!(source.refresh_generation.load(Ordering::SeqCst), generation_before + 1);
    }

    #[test]
    fn refreshable_source_terminal_error_does_not_trigger_refresh() {
        let source = RefreshableExternalBaselineSource::for_test(
            ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
            BaselineRef {
                corpus: CorpusId::WorkspaceCode,
                snapshot_id: None,
                branch: Some("main".to_owned()),
                commit: None,
            },
        )
        .unwrap();

        let generation_before = source.refresh_generation.load(Ordering::SeqCst);
        let result: Result<(), RefreshOrTerminalError> = source.run_with_refresh(|_| {
            Err(SearchError::StorageNotInitialized { schema: "bsl_search".to_owned() })
        });

        assert!(matches!(
            result,
            Err(RefreshOrTerminalError::Terminal(SearchError::StorageNotInitialized { .. }))
        ));
        assert_eq!(source.refresh_generation.load(Ordering::SeqCst), generation_before);
    }

    #[test]
    fn refreshable_source_retryable_error_refresh_failure_surfaces_missing_config() {
        let source = RefreshableExternalBaselineSource::for_test(
            ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
            BaselineRef {
                corpus: CorpusId::WorkspaceCode,
                snapshot_id: None,
                branch: Some("main".to_owned()),
                commit: None,
            },
        )
        .unwrap();

        let generation_before = source.refresh_generation.load(Ordering::SeqCst);
        let result: Result<(), RefreshOrTerminalError> = source.run_with_refresh(|_| {
            Err(SearchError::from(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "refused",
            )))
        });

        match result {
            Err(RefreshOrTerminalError::Terminal(SearchError::ExternalBaseline(message))) => {
                assert!(message.starts_with("missing_config:"), "unexpected message: {message}");
            }
            Err(RefreshOrTerminalError::Terminal(other)) => {
                panic!("expected missing_config terminal error, got {other}");
            }
            Ok(()) => panic!("expected refresh attempt to fail"),
        }
        assert_eq!(source.refresh_generation.load(Ordering::SeqCst), generation_before);
    }

    #[test]
    fn refreshable_source_preserves_non_terminal_fallback_error_after_refresh() {
        let postgres = SearchPostgresConfig {
            host: Some("127.0.0.1".to_owned()),
            port: Some(1),
            dbname: Some("bsl_search".to_owned()),
            schema: Some("bsl_search".to_owned()),
            vault_role_base: Some("prod/search/bsl-analyzer".to_owned()),
            credential_helper: SearchPostgresCredentialHelperConfig {
                program: Some("python3".to_owned()),
                args: vec![
                    "-c".to_owned(),
                    "import json; print(json.dumps({'protocol':'bsl-analyzer.postgres-helper.v1','ok':True,'url':'postgres://127.0.0.1:1/bsl_search'}))"
                        .to_owned(),
                ],
            },
        };
        let source = RefreshableExternalBaselineSource::for_test_with_refresh_context(
            ExternalBaselineConfig::postgres("postgres://127.0.0.1:1/bsl_search"),
            BaselineRef {
                corpus: CorpusId::Reference,
                snapshot_id: Some(bsl_search::SnapshotId::new("ref:0.1.0")),
                branch: None,
                commit: None,
            },
            postgres,
        )
        .unwrap();

        let calls = std::sync::atomic::AtomicUsize::new(0);
        let result: Result<(), RefreshOrTerminalError> =
            source.run_with_refresh(|_| match calls.fetch_add(1, Ordering::SeqCst) {
                0 => Err(SearchError::from(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    "refused",
                ))),
                _ => Err(SearchError::ExternalBaseline(
                    "serving_lexical_unavailable: serving_lexical is empty".to_owned(),
                )),
            });

        match result {
            Err(RefreshOrTerminalError::Terminal(SearchError::ExternalBaseline(message))) => {
                assert!(
                    message.starts_with("serving_lexical_unavailable:"),
                    "unexpected message: {message}"
                );
            }
            Err(RefreshOrTerminalError::Terminal(other)) => {
                panic!("expected serving_lexical_unavailable error, got {other}");
            }
            Ok(()) => panic!("expected refresh attempt to surface fallback-worthy error"),
        }
    }

    #[test]
    fn refreshable_source_resolve_snapshot_returns_error_for_unreachable_host() {
        let source = RefreshableExternalBaselineSource::for_test(
            ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
            BaselineRef {
                corpus: CorpusId::Reference,
                snapshot_id: Some(bsl_search::SnapshotId::new("nonexistent:0.1.0")),
                branch: None,
                commit: None,
            },
        )
        .unwrap();

        let result = source.resolve_snapshot();

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("credential refresh")
                || err_msg.contains("postgres")
                || err_msg.contains("missing_config"),
            "expected wrapper to surface error, got: {err_msg}"
        );
    }

    #[test]
    fn external_baseline_service_shutdown_times_out_without_blocking_future_requests() {
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("baseline-service-test-timeout".to_owned())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    match request {
                        BaselineServiceRequest::ProbeStatus { reply } => {
                            std::thread::sleep(Duration::from_millis(250));
                            let _ = reply.send(ExternalBaselineStatus {
                                backend: "postgres",
                                schema: "test".to_owned(),
                                selection: "test".to_owned(),
                                resolved: None,
                                state: ExternalBaselineState::Ready {
                                    snapshot_id: "snapshot:test".to_owned(),
                                    fingerprint: None,
                                    documents: 0,
                                    files: 0,
                                },
                            });
                        }
                        BaselineServiceRequest::Shutdown { reply } => {
                            let _ = reply.send(());
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .unwrap();
        let service = Arc::new(ExternalBaselineService {
            corpus: CorpusId::WorkspaceCode,
            schema: "test".to_owned(),
            selection: "test".to_owned(),
            local_reference_fingerprint: None,
            sender,
            worker: StdMutex::new(Some(worker)),
            closed: AtomicBool::new(false),
        });

        let probe_service = Arc::clone(&service);
        let probe_thread = std::thread::spawn(move || {
            let _ = probe_service.probe_status();
        });
        std::thread::sleep(Duration::from_millis(20));

        let started = Instant::now();
        service.shutdown();
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "shutdown blocked for {:?}",
            started.elapsed()
        );

        let error = service.resolve_snapshot().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("baseline_service_closed: external baseline service for workspace-code"),
            "unexpected error: {error}"
        );

        probe_thread.join().unwrap();
    }
}
