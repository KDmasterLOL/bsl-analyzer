use bsl_platform::PlatformDataInner;
use bsl_search::{
    fingerprint_documents, BaselineRef, CorpusId, Document, ExternalBaselineAdapter,
    ExternalBaselineBackend, ExternalBaselineConfig, IndexedDocument, ResolvedView, SearchEngine,
    SnapshotCatalog, SnapshotContentStore,
};
use project_model::{
    ProjectConfig, SearchBaselineBackend, SearchBaselineConfig, SearchBaselineTargetConfig,
    SearchPostgresConfig,
};
use std::collections::HashSet;
use std::env;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BaselineSnapshotDocuments {
    pub snapshot_id: String,
    pub fingerprint: Option<String>,
    pub documents: Vec<IndexedDocument>,
}

#[derive(Debug, Clone)]
pub(crate) struct BaselineRuntime {
    pub configured_baseline: ConfiguredBaselineStatus,
    pub external_baseline: Option<Arc<ExternalBaselineSource>>,
}

impl BaselineRuntime {
    pub(crate) fn workspace(project_config: &ProjectConfig) -> Self {
        Self::for_corpus(
            CorpusId::WorkspaceCode,
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
            project_config.map(|config| &config.search.baseline),
            "BSL_SEARCH_REFERENCE",
            &["BSL_SEARCH_REFERENCE_PG_URL", "BSL_SEARCH_BASELINE_PG_URL"],
            &["BSL_SEARCH_REFERENCE_PG_SCHEMA", "BSL_SEARCH_BASELINE_PG_SCHEMA"],
            project_config.is_none(),
        )
    }

    fn for_corpus(
        corpus: CorpusId,
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
        let baseline = baseline_ref_from_config(corpus.clone(), selection_prefix, baseline_target);
        let selection = baseline_description(&baseline);

        let Some(connection) = resolve_connection(connection_keys, postgres) else {
            return Self {
                configured_baseline: ConfiguredBaselineStatus {
                    backend: "postgres",
                    selection,
                    issue: Some(format!(
                        "connection string is not configured; set search.baseline.postgres.url or {}",
                        connection_keys.join(", ")
                    )),
                },
                external_baseline: None,
            };
        };

        let mut config = ExternalBaselineConfig::postgres(connection);
        if let Some(schema) = resolve_schema(schema_keys, postgres) {
            config = config.with_schema(schema);
        }

        match ExternalBaselineSource::new(config, baseline) {
            Ok(source) => {
                tracing::info!(corpus = %corpus, "external baseline source configured");
                Self {
                    configured_baseline: ConfiguredBaselineStatus {
                        backend: "postgres",
                        selection,
                        issue: None,
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
    project_config: &ProjectConfig,
) -> BaselineConfigDiagnostics {
    let workspace = BaselineRuntime::workspace(project_config);
    let reference = BaselineRuntime::reference(Some(project_config));

    BaselineConfigDiagnostics { workspace: workspace.summary(), reference: reference.summary() }
}

#[derive(Debug)]
pub(crate) struct ExternalBaselineSource {
    adapter: ExternalBaselineAdapter,
    baseline: BaselineRef,
}

impl ExternalBaselineSource {
    pub(crate) fn new(
        config: ExternalBaselineConfig,
        baseline: BaselineRef,
    ) -> Result<Self, bsl_search::SearchError> {
        let adapter = ExternalBaselineAdapter::new(config)?;
        Ok(Self { adapter, baseline })
    }

    pub(crate) fn probe_status(&self) -> ExternalBaselineStatus {
        let backend = match self.adapter.config().backend {
            ExternalBaselineBackend::Postgres => "postgres",
        };
        let schema =
            self.adapter.config().schema.clone().unwrap_or_else(|| "bsl_search".to_owned());
        let selection = baseline_description(&self.baseline);

        match self.adapter.resolve_baseline(&self.baseline) {
            Ok(Some(snapshot)) => match self.adapter.load_snapshot_documents(&snapshot) {
                Ok(documents) => {
                    let files: HashSet<&str> =
                        documents.iter().map(|document| document.path.as_str()).collect();
                    ExternalBaselineStatus {
                        backend,
                        schema,
                        selection,
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
                    state: ExternalBaselineState::Error(error.to_string()),
                },
            },
            Ok(None) => ExternalBaselineStatus {
                backend,
                schema,
                selection,
                state: ExternalBaselineState::Missing,
            },
            Err(error) => ExternalBaselineStatus {
                backend,
                schema,
                selection,
                state: ExternalBaselineState::Error(error.to_string()),
            },
        }
    }

    pub(crate) fn resolve_workspace_view(
        &self,
        engine: &SearchEngine,
    ) -> Result<Option<ResolvedView>, bsl_search::SearchError> {
        engine.resolve_workspace_code_view_with(
            self.baseline.clone(),
            self.adapter.clone(),
            self.adapter.clone(),
        )
    }

    pub(crate) fn resolve_reference_view(
        &self,
    ) -> Result<Option<ResolvedView>, bsl_search::SearchError> {
        let Some(snapshot) = self.adapter.resolve_baseline(&self.baseline)? else {
            return Ok(None);
        };
        let documents = self.adapter.load_snapshot_documents(&snapshot)?;
        let baseline = BaselineRef::for_snapshot(snapshot.corpus.clone(), snapshot.id.0.clone());
        Ok(Some(ResolvedView::new(baseline, documents)))
    }

    pub(crate) fn load_workspace_snapshot_documents(
        &self,
    ) -> Result<Option<BaselineSnapshotDocuments>, bsl_search::SearchError> {
        if !matches!(self.baseline.corpus, CorpusId::WorkspaceCode) {
            return Ok(None);
        }
        self.load_snapshot_documents()
    }

    pub(crate) fn load_reference_snapshot_documents(
        &self,
    ) -> Result<Option<BaselineSnapshotDocuments>, bsl_search::SearchError> {
        if !matches!(self.baseline.corpus, CorpusId::Reference) {
            return Ok(None);
        }
        self.load_snapshot_documents()
    }

    fn load_snapshot_documents(
        &self,
    ) -> Result<Option<BaselineSnapshotDocuments>, bsl_search::SearchError> {
        let Some(snapshot) = self.adapter.resolve_baseline(&self.baseline)? else {
            return Ok(None);
        };
        let documents = self.adapter.load_snapshot_documents(&snapshot)?;
        Ok(Some(BaselineSnapshotDocuments {
            snapshot_id: snapshot.id.0,
            fingerprint: snapshot.fingerprint,
            documents,
        }))
    }

    pub(crate) fn corpus(&self) -> &CorpusId {
        &self.baseline.corpus
    }

    pub(crate) fn local_reference_fingerprint(&self) -> Option<String> {
        if !matches!(self.baseline.corpus, CorpusId::Reference) {
            return None;
        }
        Some(fingerprint_documents(&platform_reference_documents()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExternalBaselineStatus {
    pub backend: &'static str,
    pub schema: String,
    pub selection: String,
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
        let runtime = BaselineRuntime::workspace(&ProjectConfig::default());

        assert_eq!(
            runtime.configured_baseline,
            ConfiguredBaselineStatus {
                backend: "sqlite",
                selection: "local workspace index".to_owned(),
                issue: None,
            }
        );
        assert!(runtime.external_baseline.is_none());
    }

    #[test]
    fn workspace_uses_postgres_config_selection() {
        let runtime = BaselineRuntime::workspace(&ProjectConfig {
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
        });

        assert!(runtime.external_baseline.is_some());
        assert_eq!(runtime.configured_baseline.backend, "postgres");
        assert_eq!(runtime.configured_baseline.selection, "branch main");
        assert!(runtime.configured_baseline.issue.is_none());
    }

    #[test]
    fn workspace_reports_missing_postgres_connection() {
        let runtime = BaselineRuntime::workspace(&ProjectConfig {
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
        });

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
    fn project_baseline_diagnostics_returns_workspace_and_reference_summaries() {
        let diagnostics = resolve_project_baseline_diagnostics(&ProjectConfig {
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
        });

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
