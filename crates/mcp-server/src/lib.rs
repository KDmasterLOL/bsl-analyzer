mod baseline;
mod cache;
mod diagnostics_state;
mod graph;
mod graph_db;
mod graph_query;
mod state;
mod tools;

pub use baseline::{
    resolve_project_baseline_diagnostics, BaselineConfigDiagnostics, BaselineResolutionSummary,
};
pub use cache::graph_db_path;
pub use graph_db::build_graph_database;
pub use graph_query::{GraphDb, GraphDbContextProvider};
pub use state::SharedState;
use state::WorkspaceSearchMode;

pub async fn serve_stdio(server: McpServer) -> anyhow::Result<()> {
    use rmcp::ServiceExt;
    let stdio = rmcp::transport::stdio();
    let session = server.serve(stdio).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    session.waiting().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

use crate::graph::GraphStatus;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpProfile {
    Workspace,
    Reference,
}

#[derive(Deserialize, JsonSchema)]
struct MetadataParams {
    action: String,
    filter: Option<String>,
    object_type: Option<String>,
    object_name: Option<String>,
    form_name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SearchParams {
    action: String,
    query: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct SyntaxHelpParams {
    name: String,
    type_name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct QueryParams {
    action: String,
    query: String,
    limit: Option<u32>,
    parameters: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Deserialize, JsonSchema)]
struct ExecuteParams {
    action: String,
    code: String,
}

#[derive(Deserialize, JsonSchema)]
struct GraphParams {
    /// overview | schema | node | source | neighbors | callers | callees
    action: String,
    /// Durable node id (required for node/neighbors/callers/callees).
    id: Option<String>,
    /// Durable node ids (required for `source`).
    #[serde(default)]
    ids: Vec<String>,
    /// Output budget for `source`, in tokens (~4 chars each; default 4000).
    max_output_tokens: Option<usize>,
    /// names | signatures | bodies (default: signatures).
    detail: Option<String>,
    /// in | out | both — only for `neighbors` (default: in).
    dir: Option<String>,
    /// Traversal depth for neighbors (default: 1).
    depth: Option<usize>,
    /// Server-side cap on returned neighbour nodes (default: 50).
    max_nodes: Option<usize>,
    /// Keep only edges with these provenances (resolved/inferred/visibility_blocked/unresolved).
    #[serde(default)]
    provenance: Vec<String>,
    /// How many top-centrality methods to include in `overview` (default: 20).
    top: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct DiagnosticsParams {
    /// catalog | schema | file
    action: String,
    /// `file`: absolute or workspace-relative `.bsl` path.
    path: Option<String>,
    /// `catalog`: narrow to these codes. `file`: keep only these codes.
    #[serde(default)]
    codes: Vec<String>,
    /// `catalog`: ru | en (default ru) — title language.
    locale: Option<String>,
    /// `file`: inclusive severity floor error|warning|info|hint (default warning).
    min_severity: Option<String>,
    /// `file`: 0-based first line to include (optional).
    range_start: Option<usize>,
    /// `file`: 0-based last line to include (optional).
    range_end: Option<usize>,
    /// `file`: concise | detailed (default concise).
    detail: Option<String>,
    /// `file`: cap on returned findings (default 200).
    max_findings: Option<usize>,
    /// `workspace`: cap on files swept (default 1000).
    max_files: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct ItsHelpParams {
    question: String,
}

#[derive(Deserialize, JsonSchema)]
struct DebugParams {
    action: String,
    host: Option<String>,
    port: Option<u16>,
    infobase: Option<String>,
    config_root: Option<String>,
    #[serde(default)]
    extensions: Vec<[String; 2]>,
    #[serde(default)]
    auto_attach: Vec<String>,
    module: Option<String>,
    line: Option<u32>,
    condition: Option<String>,
    direction: Option<String>,
    timeout_secs: Option<u64>,
    stack_level: Option<u32>,
    expression: Option<String>,
}

fn default_debug_port() -> u16 {
    1550
}

fn require<T>(val: Option<T>, field: &str, action: &str) -> Result<T, McpError> {
    val.ok_or_else(|| {
        McpError::invalid_params(format!("'{field}' is required for action '{action}'"), None)
    })
}

#[derive(Clone)]
pub struct McpServer {
    profile: McpProfile,
    state: SharedState,
    tool_router: ToolRouter<Self>,
}

#[tool_router(router = workspace_tool_router)]
impl McpServer {
    pub fn new(profile: McpProfile, state: SharedState) -> Self {
        let tool_router = match profile {
            McpProfile::Workspace => Self::workspace_tool_router(),
            McpProfile::Reference => Self::reference_tool_router(),
        };
        Self { profile, state, tool_router }
    }

    pub fn shutdown(&self) {
        self.state.shutdown();
    }

    #[tool(name = "metadata", annotations(read_only_hint = true))]
    async fn metadata(
        &self,
        params: Parameters<MetadataParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        match p.action.as_str() {
            "info" => {
                let config =
                    self.state.configuration().await.ok_or_else(|| {
                        McpError::invalid_params("Configuration not loaded", None)
                    })?;
                let extensions = self.state.extensions().await;
                tools::metadata::get_configuration_info(&config, &extensions)
            }
            "tree" => {
                let config =
                    self.state.configuration().await.ok_or_else(|| {
                        McpError::invalid_params("Configuration not loaded", None)
                    })?;
                let extensions = self.state.extensions().await;
                tools::metadata::get_metadata_tree(&config, &extensions, p.filter)
            }
            "object" => {
                let config =
                    self.state.configuration().await.ok_or_else(|| {
                        McpError::invalid_params("Configuration not loaded", None)
                    })?;
                let object_type = require(p.object_type, "object_type", "object")?;
                let object_name = require(p.object_name, "object_name", "object")?;
                tools::metadata::get_object_structure(&config, &object_type, &object_name)
            }
            "form" => {
                let object_type = require(p.object_type, "object_type", "form")?;
                let object_name = require(p.object_name, "object_name", "form")?;
                tools::metadata::get_form_structure(
                    self.state.workspace_root().map(|p| p.as_path()),
                    &object_type,
                    &object_name,
                    p.form_name.as_deref(),
                )
            }
            other => Err(McpError::invalid_params(
                format!("Unknown action '{other}'. Expected: info, tree, object, form"),
                None,
            )),
        }
    }

    #[tool(name = "search", annotations(read_only_hint = true))]
    async fn workspace_search(
        &self,
        params: Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        match p.action.as_str() {
            "status" => {
                let engine = self.state.search_engine().clone();
                let progress = self.state.index_progress().clone();
                let semantic_runtime = self.state.semantic_runtime();
                let workspace_search_mode = self.state.workspace_search_mode();
                let configured_baseline = self.state.configured_baseline();
                let external_baseline = self.state.external_baseline();
                tokio::task::spawn_blocking(move || {
                    tools::search::search_status(
                        &engine,
                        &progress,
                        &semantic_runtime,
                        workspace_search_mode,
                        configured_baseline,
                        external_baseline,
                    )
                })
                .await
                .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            "find_code" | "search_code" => {
                let query = require(p.query, "query", &p.action)?;
                let limit = p.limit.unwrap_or(10).min(50);
                let engine = self.state.search_engine().clone();
                let semantic_runtime = self.state.semantic_runtime();
                let workspace_search_mode = self.state.workspace_search_mode();
                let configured_baseline = self.state.configured_baseline();
                let external_baseline = self.state.external_baseline();
                let action = p.action.clone();
                tokio::task::spawn_blocking(move || match action.as_str() {
                    "find_code" => tools::search::find_code(
                        &engine,
                        workspace_search_mode,
                        configured_baseline.as_ref(),
                        external_baseline,
                        &query,
                        limit,
                    ),
                    "search_code" => tools::search::search_code(
                        &engine,
                        &semantic_runtime,
                        workspace_search_mode,
                        configured_baseline.as_ref(),
                        external_baseline,
                        &query,
                        limit,
                    ),
                    _ => unreachable!(),
                })
                .await
                .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            other => Err(McpError::invalid_params(
                format!("Unknown action '{other}'. Expected: find_code, search_code, status"),
                None,
            )),
        }
    }

    #[tool(name = "query", annotations(read_only_hint = true))]
    async fn query(&self, params: Parameters<QueryParams>) -> Result<CallToolResult, McpError> {
        let p = params.0;
        match p.action.as_str() {
            "validate" => tools::query::validate_query(&self.state, &p.query).await,
            "execute" => {
                tools::query::execute_query(&self.state, &p.query, p.limit, p.parameters).await
            }
            other => Err(McpError::invalid_params(
                format!("Unknown action '{other}'. Expected: validate, execute"),
                None,
            )),
        }
    }

    #[tool(name = "execute")]
    async fn execute(&self, params: Parameters<ExecuteParams>) -> Result<CallToolResult, McpError> {
        let p = params.0;
        match p.action.as_str() {
            "check" => tools::execution::check_syntax(&self.state, &p.code).await,
            "run" => tools::execution::execute_code(&self.state, &p.code).await,
            "eval" => tools::execution::eval_expression(&self.state, &p.code).await,
            other => Err(McpError::invalid_params(
                format!("Unknown action '{other}'. Expected: check, run, eval"),
                None,
            )),
        }
    }

    #[tool(name = "debug")]
    async fn debug(&self, params: Parameters<DebugParams>) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let session = self.state.debug_session().clone();

        match p.action.as_str() {
            "attach" => {
                let host = require(p.host, "host", "attach")?;
                let infobase = require(p.infobase, "infobase", "attach")?;
                let port = p.port.unwrap_or_else(default_debug_port);
                let workspace_root = self.state.workspace_root().cloned();
                let config_root = p.config_root;
                let extensions = p.extensions;
                let auto_attach = p.auto_attach;
                tokio::task::spawn_blocking(move || {
                    tools::debug::debug_attach(
                        &session,
                        tools::debug::AttachParams {
                            host: &host,
                            port,
                            infobase: &infobase,
                            config_root: config_root.as_deref(),
                            workspace_root: workspace_root.as_deref(),
                            extensions: &extensions,
                            auto_attach: &auto_attach,
                        },
                    )
                })
                .await
                .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            "disconnect" => {
                tokio::task::spawn_blocking(move || tools::debug::debug_disconnect(&session))
                    .await
                    .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            "set_breakpoint" => {
                let module = require(p.module, "module", "set_breakpoint")?;
                let line = require(p.line, "line", "set_breakpoint")?;
                let condition = p.condition;
                tokio::task::spawn_blocking(move || {
                    tools::debug::debug_set_breakpoint(
                        &session,
                        &module,
                        line,
                        condition.as_deref(),
                    )
                })
                .await
                .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            "remove_breakpoint" => {
                let module = require(p.module, "module", "remove_breakpoint")?;
                let line = require(p.line, "line", "remove_breakpoint")?;
                tokio::task::spawn_blocking(move || {
                    tools::debug::debug_remove_breakpoint(&session, &module, line)
                })
                .await
                .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            "continue" => {
                tokio::task::spawn_blocking(move || tools::debug::debug_continue(&session))
                    .await
                    .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            "step" => {
                let direction = require(p.direction, "direction", "step")?;
                tokio::task::spawn_blocking(move || tools::debug::debug_step(&session, &direction))
                    .await
                    .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            "wait_stop" => {
                let timeout_secs = p.timeout_secs;
                tokio::task::spawn_blocking(move || {
                    tools::debug::debug_wait_stop(&session, timeout_secs)
                })
                .await
                .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            "stack_trace" => {
                tokio::task::spawn_blocking(move || tools::debug::debug_stack_trace(&session))
                    .await
                    .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            "locals" => {
                let stack_level = p.stack_level;
                tokio::task::spawn_blocking(move || {
                    tools::debug::debug_locals(&session, stack_level)
                })
                .await
                .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            "eval" => {
                let expression = require(p.expression, "expression", "eval")?;
                let stack_level = p.stack_level;
                tokio::task::spawn_blocking(move || {
                    tools::debug::debug_eval(&session, &expression, stack_level)
                })
                .await
                .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            other => Err(McpError::invalid_params(
                format!(
                    "Unknown action '{other}'. Expected: attach, disconnect, set_breakpoint, \
                     remove_breakpoint, continue, step, wait_stop, stack_trace, locals, eval"
                ),
                None,
            )),
        }
    }

    #[tool(name = "graph", annotations(read_only_hint = true))]
    async fn graph(&self, params: Parameters<GraphParams>) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let graph = self.state.graph().clone();

        // `schema` is static and needs no loaded graph.
        if p.action == "schema" {
            return Ok(tools::graph::schema());
        }

        // Lazily trigger the background load on first use.
        graph.ensure_loading();

        match graph.status() {
            GraphStatus::Disabled => {
                return Err(McpError::invalid_params(
                    "graph is only available in the workspace profile",
                    None,
                ))
            }
            GraphStatus::Idle | GraphStatus::Loading => {
                return Ok(tools::graph::loading(Some(
                    "call graph is still indexing; retry shortly",
                )))
            }
            GraphStatus::Failed(msg) => {
                return Err(McpError::internal_error(format!("graph load failed: {msg}"), None))
            }
            GraphStatus::Ready { .. } => {}
        }

        let Some(snapshot) = graph.snapshot() else {
            return Ok(tools::graph::loading(None));
        };

        tokio::task::spawn_blocking(move || {
            let gdb = &snapshot.graph;
            let value = match p.action.as_str() {
                "overview" => tools::graph::overview(gdb, p.top.unwrap_or(20)),
                "node" => {
                    let id = require(p.id, "id", "node")?;
                    let detail = tools::graph::detail_from(p.detail.as_deref())
                        .map_err(|e| McpError::invalid_params(e, None))?;
                    tools::graph::node(gdb, &id, detail)
                }
                "source" => {
                    if p.ids.is_empty() {
                        return Err(McpError::invalid_params(
                            "'ids' is required (non-empty) for action 'source'",
                            None,
                        ));
                    }
                    let budget = p.max_output_tokens.unwrap_or(4000);
                    tools::graph::source(gdb, &p.ids, budget)
                }
                action @ ("neighbors" | "callers" | "callees") => {
                    let id = require(p.id, "id", action)?;
                    let dir = match action {
                        "callers" => ide::Direction::In,
                        "callees" => ide::Direction::Out,
                        _ => tools::graph::direction_from(p.dir.as_deref())
                            .map_err(|e| McpError::invalid_params(e, None))?,
                    };
                    let detail = tools::graph::detail_from(p.detail.as_deref())
                        .map_err(|e| McpError::invalid_params(e, None))?;
                    let neighbors = ide::NeighborsParams {
                        id: &id,
                        dir,
                        depth: p.depth.unwrap_or(1),
                        max_nodes: p.max_nodes.unwrap_or(50),
                        detail,
                        provenance_filter: p.provenance.clone(),
                    };
                    tools::graph::neighbors(gdb, &neighbors)
                }
                other => {
                    return Err(McpError::invalid_params(
                        format!(
                            "Unknown action '{other}'. Expected: overview, schema, node, source, \
                             neighbors, callers, callees"
                        ),
                        None,
                    ))
                }
            };
            // Stamp freshness relative to the snapshot that served this answer: the
            // scan may detect drift and kick a background reload, but `revision`
            // and `stale` describe the data actually returned above.
            let freshness = graph.freshness(&snapshot);
            Ok(tools::graph::envelope(freshness, value))
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    #[tool(name = "diagnostics", annotations(read_only_hint = true))]
    async fn diagnostics(
        &self,
        params: Parameters<DiagnosticsParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        match p.action.as_str() {
            // `catalog` and `schema` are static (compile-time metadata), so they need
            // no resident analysis database and answer in either profile.
            "schema" => Ok(tools::diagnostics::schema()),
            "catalog" => {
                let locale = match p.locale.as_deref() {
                    Some(s) => ide::Locale::from_config_str(s)
                        .map_err(|e| McpError::invalid_params(e.to_string(), None))?,
                    None => ide::Locale::default(),
                };
                Ok(tools::diagnostics::catalog(locale, &p.codes))
            }
            "file" => self.diagnostics_file(p).await,
            "workspace" => self.diagnostics_workspace(p).await,
            other => Err(McpError::invalid_params(
                format!("Unknown action '{other}'. Expected: catalog, schema, file, workspace"),
                None,
            )),
        }
    }

    /// The `diagnostics file` action: build/serve per-file findings from the resident
    /// analysis database, behind the lazy-load lifecycle and freshness envelope.
    async fn diagnostics_file(&self, p: DiagnosticsParams) -> Result<CallToolResult, McpError> {
        use crate::diagnostics_state::DiagnosticsStatus;
        use tools::diagnostics::{parse_min_severity, FileFilters};

        let diag = self.state.diagnostics().clone();
        let path = require(p.path, "path", "file")?;
        let path = std::path::PathBuf::from(path);

        diag.ensure_loading();
        match diag.status() {
            DiagnosticsStatus::Disabled => {
                return Err(McpError::invalid_params(
                    "diagnostics 'file' is only available in the workspace profile",
                    None,
                ))
            }
            DiagnosticsStatus::Failed(msg) => {
                return Err(McpError::internal_error(
                    format!("diagnostics database load failed: {msg}"),
                    None,
                ))
            }
            DiagnosticsStatus::Idle | DiagnosticsStatus::Loading => {
                return Ok(tools::diagnostics::loading())
            }
            DiagnosticsStatus::Ready { .. } => {}
        }

        let min_severity = parse_min_severity(p.min_severity.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let range = match (p.range_start, p.range_end) {
            (Some(s), Some(e)) => Some((s, e)),
            (Some(s), None) => Some((s, usize::MAX)),
            (None, Some(e)) => Some((0, e)),
            (None, None) => None,
        };
        let filters = FileFilters {
            min_severity,
            codes: p.codes,
            range,
            max_findings: p.max_findings.unwrap_or(tools::diagnostics::DEFAULT_MAX_FINDINGS),
            detailed: p.detail.as_deref() == Some("detailed"),
        };

        tokio::task::spawn_blocking(move || {
            // `generation` is supplied by `read` under the lock (so `result_id` describes
            // the exact resident state queried), and the freshness verdict is computed
            // under that same lock and returned alongside — the envelope is atomic.
            let outcome = diag.read(|resident, generation| {
                tools::diagnostics::file_findings(resident, &path, &filters, generation)
            });
            use crate::diagnostics_state::ResidentOutcome;
            match outcome {
                ResidentOutcome::Ready(result, freshness) => {
                    Ok(tools::diagnostics::envelope(freshness, result))
                }
                ResidentOutcome::Loading => Ok(tools::diagnostics::loading()),
                ResidentOutcome::Disabled => Err(McpError::invalid_params(
                    "diagnostics 'file' is only available in the workspace profile",
                    None,
                )),
                ResidentOutcome::Failed(msg) => {
                    Err(McpError::internal_error(format!("diagnostics database: {msg}"), None))
                }
            }
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// The `diagnostics workspace` action: an opt-in, bounded whole-config sweep that
    /// returns per-code aggregates only (no per-finding detail). The rayon sweep runs
    /// under the resident lock (so no reload mutates the db mid-sweep), which serialises
    /// other diagnostics calls for its duration — acceptable for a capped, opt-in pass.
    async fn diagnostics_workspace(
        &self,
        p: DiagnosticsParams,
    ) -> Result<CallToolResult, McpError> {
        use crate::diagnostics_state::{DiagnosticsStatus, ResidentOutcome, SweepOptions};
        use tools::diagnostics::{
            parse_min_severity, DEFAULT_MAX_SWEEP_FILES, MAX_SWEEP_FILES_CEILING,
        };

        let diag = self.state.diagnostics().clone();
        diag.ensure_loading();
        match diag.status() {
            DiagnosticsStatus::Disabled => {
                return Err(McpError::invalid_params(
                    "diagnostics 'workspace' is only available in the workspace profile",
                    None,
                ))
            }
            DiagnosticsStatus::Failed(msg) => {
                return Err(McpError::internal_error(
                    format!("diagnostics database load failed: {msg}"),
                    None,
                ))
            }
            DiagnosticsStatus::Idle | DiagnosticsStatus::Loading => {
                return Ok(tools::diagnostics::loading())
            }
            DiagnosticsStatus::Ready { .. } => {}
        }

        let min_severity = parse_min_severity(p.min_severity.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let opts = SweepOptions {
            min_severity,
            codes: p.codes,
            // Clamp to the ceiling: the sweep holds the resident lock throughout, so an
            // unbounded request would stall every other diagnostics call. A larger
            // config surfaces as `truncated` with the true `files_total`.
            max_files: p.max_files.unwrap_or(DEFAULT_MAX_SWEEP_FILES).min(MAX_SWEEP_FILES_CEILING),
        };

        tokio::task::spawn_blocking(move || {
            let outcome = diag.read(|resident, generation| {
                tools::diagnostics::workspace_findings(resident, &opts, generation)
            });
            match outcome {
                ResidentOutcome::Ready(result, freshness) => {
                    Ok(tools::diagnostics::envelope(freshness, result))
                }
                ResidentOutcome::Loading => Ok(tools::diagnostics::loading()),
                ResidentOutcome::Disabled => Err(McpError::invalid_params(
                    "diagnostics 'workspace' is only available in the workspace profile",
                    None,
                )),
                ResidentOutcome::Failed(msg) => {
                    Err(McpError::internal_error(format!("diagnostics database: {msg}"), None))
                }
            }
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }
}

#[tool_router(router = reference_tool_router)]
impl McpServer {
    #[tool(name = "search", annotations(read_only_hint = true))]
    async fn reference_search(
        &self,
        params: Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        match p.action.as_str() {
            "status" => {
                let engine = self.state.search_engine().clone();
                let progress = self.state.index_progress().clone();
                let semantic_runtime = self.state.semantic_runtime();
                let configured_baseline = self.state.configured_baseline();
                let external_baseline = self.state.external_baseline();
                tokio::task::spawn_blocking(move || {
                    tools::search::search_status(
                        &engine,
                        &progress,
                        &semantic_runtime,
                        WorkspaceSearchMode::SqliteLocal,
                        configured_baseline,
                        external_baseline,
                    )
                })
                .await
                .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            "find_docs" | "search_docs" => {
                let query = require(p.query, "query", &p.action)?;
                let limit = p.limit.unwrap_or(10).min(50);
                let engine = self.state.search_engine().clone();
                let configured_baseline = self.state.configured_baseline();
                let external_baseline = self.state.external_baseline();
                let action = p.action.clone();
                tokio::task::spawn_blocking(move || match action.as_str() {
                    "find_docs" => tools::search::find_docs(
                        &engine,
                        configured_baseline.as_ref(),
                        external_baseline.clone(),
                        &query,
                        limit,
                    ),
                    "search_docs" => tools::search::search_docs(
                        &engine,
                        configured_baseline.as_ref(),
                        external_baseline,
                        &query,
                        limit,
                    ),
                    _ => unreachable!(),
                })
                .await
                .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
            }
            other => Err(McpError::invalid_params(
                format!("Unknown action '{other}'. Expected: find_docs, search_docs, status"),
                None,
            )),
        }
    }

    #[tool(name = "syntax_help", annotations(read_only_hint = true))]
    async fn syntax_help(
        &self,
        params: Parameters<SyntaxHelpParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::platform::bsl_syntax_help(&params.0.name, params.0.type_name.as_deref())
    }

    #[tool(name = "its_help", annotations(read_only_hint = true))]
    async fn its_help(
        &self,
        params: Parameters<ItsHelpParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::its_help::its_help(&params.0.question).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(match self.profile {
            McpProfile::Workspace => {
                "BSL Analyzer workspace MCP server. Provides project metadata browsing, \
                 code search, a whole-config semantic call graph, semantic diagnostics, \
                 SDBL query validation, code execution and debugging. Prefer the `graph` tool \
                 over text search when you need call relationships: start with action 'overview' \
                 on an unfamiliar project, then 'node'/'callers'/'callees'/'neighbors' using the \
                 durable ids it returns. The `diagnostics` tool surfaces analyzer findings grep \
                 cannot (unreachable code, type mismatch, unresolved call); start with action \
                 'catalog' to discover the available codes. \
                 Tools: metadata, search, graph, diagnostics, query, execute, debug."
                    .into()
            }
            McpProfile::Reference => {
                "BSL Analyzer reference MCP server. Provides platform API reference, \
                 platform docs search and ITS expert help. \
                 Tools: search, syntax_help, its_help."
                    .into()
            }
        });
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = rmcp::model::Implementation::from_build_env();
        info
    }
}
