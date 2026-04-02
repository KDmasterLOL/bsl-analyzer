use bsl_platform::PlatformDataInner;
use bsl_search::{
    fingerprint_documents, BaselineRef, CorpusId, Document, ExternalBaselineAdapter,
    ExternalBaselineBackend, ExternalBaselineConfig, IndexedDocument, ResolvedView, SearchEngine,
    SnapshotCatalog, SnapshotContentStore,
};
use project_model::{
    current_git_branch, evaluate_workspace_baseline_support_now, parse_timestamp_utc,
    resolve_workspace_branch_policy, ProjectConfig, ResolvedWorkspaceBaselineSupport,
    SearchBaselineBackend, SearchBaselineConfig, SearchBaselinePolicyConfig,
    SearchBaselineSupportState, SearchBaselineTargetConfig, SearchPostgresConfig,
};
use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::sync::Arc;

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
    pub external_baseline: Option<Arc<ExternalBaselineSource>>,
}

impl BaselineRuntime {
    pub(crate) fn workspace(project_root: Option<&Path>, project_config: &ProjectConfig) -> Self {
        Self::for_corpus(
            CorpusId::WorkspaceCode,
            project_root,
            Some(&project_config.search.baseline),
            "BSL_SEARCH_BASELINE",
            &["BSL_SEARCH_BASELINE_PG_URL"],
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
            &["BSL_SEARCH_REFERENCE_PG_URL", "BSL_SEARCH_BASELINE_PG_URL"],
            &["BSL_SEARCH_REFERENCE_PG_SCHEMA", "BSL_SEARCH_BASELINE_PG_SCHEMA"],
            project_config.is_none(),
        )
    }

    fn for_corpus(
        corpus: CorpusId,
        project_root: Option<&Path>,
        project_config: Option<&SearchBaselineConfig>,
        selection_prefix: &str,
        connection_keys: &[&str],
        schema_keys: &[&str],
        allow_env_backend_without_config: bool,
    ) -> Self {
        let configured_backend = project_config.map(|config| config.backend.clone());
        let use_postgres = match configured_backend {
            Some(SearchBaselineBackend::Postgres) => true,
            Some(SearchBaselineBackend::Sqlite) => false,
            None => {
                allow_env_backend_without_config && resolve_env_value(connection_keys).is_some()
            }
        };

        if !use_postgres {
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

        let default_postgres = SearchPostgresConfig::default();
        let postgres = project_config.map(|config| &config.postgres).unwrap_or(&default_postgres);
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

        let Some(connection) = resolve_connection(connection_keys, postgres) else {
            return Self {
                configured_baseline: ConfiguredBaselineStatus {
                    backend: "postgres",
                    selection,
                    issue: Some(format!(
                        "connection string is not configured; set search.baseline.postgres.url or {}",
                        connection_keys.join(", ")
                    )),
                    support: None,
                },
                external_baseline: None,
            };
        };

        let mut config = ExternalBaselineConfig::postgres(connection);
        if let Some(schema) = resolve_schema(schema_keys, postgres) {
            config = config.with_schema(schema);
        }

        match ExternalBaselineSource::new_with_candidates(config, baselines, selection.clone()) {
            Ok(source) => {
                let support = if matches!(corpus, CorpusId::WorkspaceCode) {
                    resolve_workspace_support_status(project_root, &baseline_target.policy, &source)
                } else {
                    None
                };
                tracing::info!(corpus = %corpus, "external baseline source configured");
                Self {
                    configured_baseline: ConfiguredBaselineStatus {
                        backend: "postgres",
                        selection,
                        issue: None,
                        support,
                    },
                    external_baseline: Some(Arc::new(source)),
                }
            }
            Err(error) => {
                tracing::warn!(corpus = %corpus, "failed to configure external baseline source: {error}");
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
    let workspace = BaselineRuntime::workspace(project_root, project_config);
    let reference = BaselineRuntime::reference(Some(project_config));

    BaselineConfigDiagnostics { workspace: workspace.summary(), reference: reference.summary() }
}

#[derive(Debug)]
pub(crate) struct ExternalBaselineSource {
    adapter: ExternalBaselineAdapter,
    baselines: Vec<BaselineRef>,
    selection: String,
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

    pub(crate) fn adapter(&self) -> &ExternalBaselineAdapter {
        &self.adapter
    }

    pub(crate) fn probe_status(&self) -> ExternalBaselineStatus {
        let backend = match self.adapter.config().backend {
            ExternalBaselineBackend::Postgres => "postgres",
        };
        let schema =
            self.adapter.config().schema.clone().unwrap_or_else(|| "bsl_search".to_owned());
        let selection = self.selection.clone();

        match self.resolve_snapshot() {
            Ok(Some((resolved_baseline, snapshot))) => {
                match self.adapter.load_snapshot_documents(&snapshot) {
                    Ok(documents) => {
                        let files: HashSet<&str> =
                            documents.iter().map(|document| document.path.as_str()).collect();
                        ExternalBaselineStatus {
                            backend,
                            schema,
                            selection,
                            resolved: Some(baseline_description(&resolved_baseline)),
                            state: ExternalBaselineState::Ready {
                                snapshot_id: snapshot.id.0,
                                fingerprint: snapshot.fingerprint,
                                documents: documents.len(),
                                files: files.len(),
                            },
                        }
                    }
                    Err(error) => ExternalBaselineStatus {
                        backend,
                        schema,
                        selection,
                        resolved: Some(baseline_description(&resolved_baseline)),
                        state: ExternalBaselineState::Error(error.to_string()),
                    },
                }
            }
            Ok(None) => ExternalBaselineStatus {
                backend,
                schema,
                selection,
                resolved: None,
                state: ExternalBaselineState::Missing,
            },
            Err(error) => ExternalBaselineStatus {
                backend,
                schema,
                selection,
                resolved: None,
                state: ExternalBaselineState::Error(error.to_string()),
            },
        }
    }

    pub(crate) fn resolve_workspace_view(
        &self,
        engine: &SearchEngine,
    ) -> Result<Option<ResolvedView>, bsl_search::SearchError> {
        let Some((_, snapshot)) = self.resolve_snapshot()? else {
            return Ok(None);
        };
        engine.resolve_workspace_code_view_with(
            BaselineRef::for_snapshot(snapshot.corpus.clone(), snapshot.id.0.clone()),
            self.adapter.clone(),
            self.adapter.clone(),
        )
    }

    pub(crate) fn resolve_reference_view(
        &self,
    ) -> Result<Option<ResolvedView>, bsl_search::SearchError> {
        let Some((_, snapshot)) = self.resolve_snapshot()? else {
            return Ok(None);
        };
        let documents = self.adapter.load_snapshot_documents(&snapshot)?;
        let baseline = BaselineRef::for_snapshot(snapshot.corpus.clone(), snapshot.id.0.clone());
        Ok(Some(ResolvedView::new(baseline, documents)))
    }

    pub(crate) fn load_workspace_snapshot_documents(
        &self,
        model_id: Option<&str>,
        dimension: Option<usize>,
    ) -> Result<Option<BaselineSnapshotDocuments>, bsl_search::SearchError> {
        if !matches!(self.corpus(), CorpusId::WorkspaceCode) {
            return Ok(None);
        }
        self.load_snapshot_documents(model_id, dimension)
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
    source: &ExternalBaselineSource,
) -> Option<ResolvedWorkspaceBaselineSupport> {
    if !policy.is_configured() {
        return None;
    }

    let workspace_branch = resolve_workspace_branch(project_root);
    let (resolved_baseline, snapshot) = source.resolve_snapshot().ok().flatten()?;
    let details = source.adapter.snapshot_details(&snapshot.id.0).ok().flatten()?;
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

fn resolve_connection(connection_keys: &[&str], postgres: &SearchPostgresConfig) -> Option<String> {
    resolve_env_value(connection_keys).or_else(|| postgres.url.clone())
}

fn resolve_schema(schema_keys: &[&str], postgres: &SearchPostgresConfig) -> Option<String> {
    resolve_env_value(schema_keys).or_else(|| postgres.schema.clone())
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
        baseline_description, resolve_project_baseline_diagnostics, BaselineConfigDiagnostics,
        BaselineResolutionSummary, BaselineRuntime, ConfiguredBaselineStatus,
        ExternalBaselineSource, ExternalBaselineState,
    };
    use bsl_search::{BaselineRef, CorpusId, ExternalBaselineConfig};
    use project_model::{
        ProjectConfig, SearchBaselineBackend, SearchBaselineConfig, SearchBaselineTargetConfig,
        SearchConfig, SearchPostgresConfig,
    };
    use std::fs;
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
    fn workspace_uses_postgres_config_selection() {
        let runtime = BaselineRuntime::workspace(
            None,
            &ProjectConfig {
                search: SearchConfig {
                    baseline: SearchBaselineConfig {
                        backend: SearchBaselineBackend::Postgres,
                        postgres: SearchPostgresConfig {
                            url: Some("postgres://shared-search".to_owned()),
                            schema: Some("corp_search".to_owned()),
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

        assert!(runtime.external_baseline.is_some());
        assert_eq!(runtime.configured_baseline.backend, "postgres");
        assert_eq!(runtime.configured_baseline.selection, "branch main");
        assert!(runtime.configured_baseline.issue.is_none());
    }

    #[test]
    fn workspace_reports_missing_postgres_connection() {
        let runtime = BaselineRuntime::workspace(
            None,
            &ProjectConfig {
                search: SearchConfig {
                    baseline: SearchBaselineConfig {
                        backend: SearchBaselineBackend::Postgres,
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
            .is_some_and(|issue| issue.contains("search.baseline.postgres.url")));
        assert!(runtime.external_baseline.is_none());
    }

    #[test]
    fn workspace_policy_selection_uses_branch_chain() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/feature/demo\n").unwrap();

        let runtime = BaselineRuntime::workspace(
            Some(dir.path()),
            &ProjectConfig {
                search: SearchConfig {
                    baseline: SearchBaselineConfig {
                        backend: SearchBaselineBackend::Postgres,
                        postgres: SearchPostgresConfig {
                            url: Some("postgres://shared-search".to_owned()),
                            schema: Some("corp_search".to_owned()),
                        },
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

        assert!(runtime.external_baseline.is_some());
        assert_eq!(runtime.configured_baseline.backend, "postgres");
        assert_eq!(
            runtime.configured_baseline.selection,
            "workspace branch feature/demo -> branch develop -> branch vendor"
        );
        assert!(runtime.configured_baseline.issue.is_none());
    }

    #[test]
    fn project_baseline_diagnostics_returns_workspace_and_reference_summaries() {
        let diagnostics = resolve_project_baseline_diagnostics(
            None,
            &ProjectConfig {
                search: SearchConfig {
                    baseline: SearchBaselineConfig {
                        backend: SearchBaselineBackend::Postgres,
                        postgres: SearchPostgresConfig {
                            url: Some("postgres://shared-search".to_owned()),
                            schema: Some("corp_search".to_owned()),
                        },
                        workspace_code: SearchBaselineTargetConfig {
                            branch: Some("main".to_owned()),
                            ..SearchBaselineTargetConfig::default()
                        },
                        reference: SearchBaselineTargetConfig {
                            snapshot_id: Some("reference:0.1.104".to_owned()),
                            ..SearchBaselineTargetConfig::default()
                        },
                    },
                },
                ..ProjectConfig::default()
            },
        );

        assert_eq!(
            diagnostics,
            BaselineConfigDiagnostics {
                workspace: BaselineResolutionSummary {
                    backend: "postgres".to_owned(),
                    selection: "branch main".to_owned(),
                    issue: None,
                },
                reference: BaselineResolutionSummary {
                    backend: "postgres".to_owned(),
                    selection: "snapshot reference:0.1.104".to_owned(),
                    issue: None,
                },
            }
        );
    }
}
