mod baseline;
mod state;
mod tools;

pub use baseline::{
    resolve_project_baseline_diagnostics, BaselineConfigDiagnostics, BaselineResolutionSummary,
};
pub use state::SharedState;
use state::WorkspaceSearchMode;

pub async fn serve_stdio(server: McpServer) -> anyhow::Result<()> {
    use rmcp::ServiceExt;
    let stdio = rmcp::transport::stdio();
    let session = server.serve(stdio).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    session.waiting().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

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
                 code search, SDBL query validation, code execution and debugging. \
                 Tools: metadata, search, query, execute, debug."
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
