//! MCP (Model Context Protocol) server for bsl-analyzer.
//!
//! Exposes 1C:Enterprise metadata and platform knowledge as MCP tools
//! for AI agents. Complements LSP (which handles code analysis) with
//! capabilities LSP doesn't cover: metadata browsing, platform docs,
//! ad-hoc query validation.

mod baseline;
mod state;
mod tools;

pub use baseline::{
    resolve_project_baseline_diagnostics, BaselineConfigDiagnostics, BaselineResolutionSummary,
};
pub use state::SharedState;

/// Start MCP server on stdio (stdin/stdout).
///
/// This is the standard MCP transport — the host IDE spawns the process
/// and communicates via JSON-RPC over stdin/stdout.
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

// -- Parameter types for consolidated tools --

#[derive(Deserialize, JsonSchema)]
struct MetadataParams {
    /// Действие:
    /// - "info" — общая информация о конфигурации (название, UUID, количество объектов)
    /// - "tree" — дерево объектов по категориям (с filter — объекты категории, без — сводка)
    /// - "object" — структура объекта: реквизиты, ТЧ, измерения, ресурсы (требует object_type + object_name)
    /// - "form" — структура управляемой формы: элементы, команды, обработчики (требует object_type + object_name, form_name — опционально)
    action: String,
    /// Категория метаданных для фильтрации (action=tree, на русском): Справочники, Документы,
    /// Перечисления, Обработки, Отчеты, РегистрыСведений, РегистрыНакопления, ОбщиеМодули и др.
    filter: Option<String>,
    /// Тип объекта на английском (action=object, form): Document, Catalog, InformationRegister,
    /// AccumulationRegister, Enum, DataProcessor, Report, CommonModule и др.
    object_type: Option<String>,
    /// Имя объекта метаданных, например РеализацияТоваровУслуг (action=object, form)
    object_name: Option<String>,
    /// Имя формы (action=form, необязательно — без него возвращается список форм)
    form_name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SearchParams {
    /// Действие:
    /// - "find_code" — полнотекстовый поиск по коду (точные имена процедур, переменных, вызовов API)
    /// - "search_code" — семантический поиск по коду на естественном языке (требует EMBEDDING_URL)
    /// - "find_docs" — полнотекстовый поиск по справке платформы (точные имена типов, методов)
    /// - "search_docs" — семантический поиск по справке на естественном языке (требует EMBEDDING_URL)
    /// - "status" — статус поискового индекса (количество файлов, чанков, прогресс)
    action: String,
    /// Текст запроса.
    /// Для find_code/find_docs: точные имена и токены ("ОбработкаПроведения", "Массив").
    /// Для search_code/search_docs: описание на естественном языке ("обработка проведения документа").
    /// Не требуется для action=status.
    query: Option<String>,
    /// Максимальное количество результатов (по умолчанию 10, максимум 50)
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct SyntaxHelpParams {
    /// Имя типа, метода или глобальной функции платформы (русское или английское).
    name: String,
    /// Имя типа для поиска метода в контексте типа (например type_name="Массив", name="Добавить")
    type_name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct QueryParams {
    /// Действие:
    /// - "validate" — проверить синтаксис запроса без выполнения (через СхемаЗапроса если доступен --onec-url, иначе локально парсером)
    /// - "execute" — выполнить запрос и получить данные (только ВЫБРАТЬ/SELECT, требует --onec-url)
    action: String,
    /// Текст запроса на языке запросов 1С (SDBL). Параметры через &ИмяПараметра.
    query: String,
    /// Максимальное количество строк результата (action=execute, по умолчанию 100, максимум 1000)
    limit: Option<u32>,
    /// Параметры запроса в виде пар ключ-значение (action=execute). Ключ — имя параметра без амперсанда.
    parameters: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Deserialize, JsonSchema)]
struct ExecuteParams {
    /// Действие:
    /// - "check" — проверить синтаксис BSL-кода без выполнения (компиляция платформой, но не запуск)
    /// - "run" — выполнить BSL-код (операторы) через Выполнить(). Для возврата данных:
    ///   Контекст.Вставить("ключ", значение). Содержимое Контекст возвращается в ответе.
    /// - "eval" — вычислить BSL-выражение через Вычислить() и вернуть результат.
    ///   Примеры: ТекущаяДата(), 1+1, Справочники.Номенклатура.НайтиПоНаименованию("Товар")
    ///
    /// ОГРАНИЧЕНИЯ: нельзя объявлять Функция/Процедура — только операторы, выражения, условия, циклы.
    /// action=run и action=eval требуют подключения к живой базе (--onec-url).
    action: String,
    /// BSL-код (action=check, run) или BSL-выражение (action=eval).
    code: String,
}

#[derive(Deserialize, JsonSchema)]
struct ItsHelpParams {
    /// Вопрос на русском языке. Примеры:
    /// - "Как правильно реализовать обработку проведения документа?"
    /// - "Стандарт структуры модуля по ИТС"
    /// - "Когда использовать ПовторноеИспользование НаВремяСеанса?"
    /// - "Ошибка 'Поле объекта не обнаружено' — причины и решения"
    question: String,
}

#[derive(Deserialize, JsonSchema)]
struct DebugParams {
    /// Действие:
    /// - "attach" — подключиться к серверу отладки (требует host, infobase; port по умолчанию 1550)
    /// - "disconnect" — отключиться от сервера отладки
    /// - "set_breakpoint" — установить точку останова (требует module, line; condition опционально)
    /// - "remove_breakpoint" — удалить точку останова (требует module, line)
    /// - "continue" — продолжить выполнение после остановки
    /// - "step" — пошаговое выполнение (требует direction: "next"/"in"/"out")
    /// - "wait_stop" — ожидать остановку на breakpoint/исключении (timeout_secs по умолчанию 30)
    /// - "stack_trace" — получить стек вызовов остановленной программы
    /// - "locals" — получить локальные переменные (stack_level по умолчанию 0)
    /// - "eval" — вычислить выражение в контексте остановленной программы (требует expression)
    ///
    /// ВАЖНО: если код запущен через execute(action=run) и остановился на breakpoint,
    /// execute зависнет до вызова debug(action=continue). Запускай код через curl или в фоне.
    action: String,
    /// Хост сервера отладки 1С (action=attach)
    host: Option<String>,
    /// Порт сервера отладки (action=attach, по умолчанию 1550)
    port: Option<u16>,
    /// Имя информационной базы (action=attach)
    infobase: Option<String>,
    /// Корневой каталог конфигурации для маппинга модулей на файлы (action=attach)
    config_root: Option<String>,
    /// Расширения: список пар ["имя", "путь_к_каталогу"] для маппинга модулей расширений (action=attach)
    #[serde(default)]
    extensions: Vec<[String; 2]>,
    /// Типы целей автоподключения (action=attach). По умолчанию: Client, Server, HTTPService.
    /// Допустимые: Client, ManagedClient, WEBClient, COMConnector, Server, ServerEmulation,
    /// WEBService, HTTPService, OData, JOB, JobFileMode, MobileClient, MobileServer, MobileManagedClient.
    #[serde(default)]
    auto_attach: Vec<String>,
    /// Имя модуля (action=set_breakpoint, remove_breakpoint).
    /// Например "ОбщийМодуль.ОбщегоНазначения" или "Справочник.Номенклатура.МодульОбъекта".
    /// Сокращённый формат поддерживается — суффикс .Модуль/.МодульОбъекта добавляется автоматически.
    module: Option<String>,
    /// Номер строки (action=set_breakpoint, remove_breakpoint)
    line: Option<u32>,
    /// Условие остановки — BSL-выражение (action=set_breakpoint, опционально)
    condition: Option<String>,
    /// Направление шага (action=step): "next" (через), "in" (внутрь), "out" (наружу)
    direction: Option<String>,
    /// Таймаут ожидания в секундах (action=wait_stop, по умолчанию 30)
    timeout_secs: Option<u64>,
    /// Уровень стека, 0 = текущий фрейм (action=locals, eval; по умолчанию 0)
    stack_level: Option<u32>,
    /// BSL-выражение для вычисления в контексте breakpoint (action=eval).
    /// Для вычисления без отладчика используй execute(action=eval).
    expression: Option<String>,
}

fn default_debug_port() -> u16 {
    1550
}

/// Helper to extract a required field from Option, returning McpError if missing.
fn require<T>(val: Option<T>, field: &str, action: &str) -> Result<T, McpError> {
    val.ok_or_else(|| {
        McpError::invalid_params(format!("'{field}' is required for action '{action}'"), None)
    })
}

/// MCP server exposing bsl-analyzer capabilities as tools.
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

    /// Метаданные конфигурации 1С: общая информация, дерево объектов по категориям,
    /// структура объекта (реквизиты, ТЧ, измерения, ресурсы), структура формы.
    /// action: info | tree | object | form
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

    /// Поиск по коду и справке платформы 1С.
    /// find_code — полнотекстовый по коду. search_code — семантический по коду.
    /// action: find_code | search_code | status
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
                let configured_baseline = self.state.configured_baseline();
                let external_baseline = self.state.external_baseline();
                tokio::task::spawn_blocking(move || {
                    tools::search::search_status(
                        &engine,
                        &progress,
                        &semantic_runtime,
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
                let configured_baseline = self.state.configured_baseline();
                let external_baseline = self.state.external_baseline();
                let action = p.action.clone();
                tokio::task::spawn_blocking(move || match action.as_str() {
                    "find_code" => tools::search::find_code(
                        &engine,
                        configured_baseline.as_ref(),
                        external_baseline,
                        &query,
                        limit,
                    ),
                    "search_code" => tools::search::search_code(
                        &engine,
                        &semantic_runtime,
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

    /// Запросы 1С (SDBL): проверка синтаксиса или выполнение SELECT с получением данных.
    /// action: validate | execute
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

    /// Выполнение BSL-кода в реальной базе 1С: проверка синтаксиса, выполнение операторов, вычисление выражений.
    /// action=run и action=eval требуют подключения (--onec-url).
    /// action: check | run | eval
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

    /// Отладчик 1С: подключение к серверу отладки, точки останова, пошаговое выполнение,
    /// просмотр стека и переменных, вычисление выражений в контексте breakpoint.
    /// action: attach | disconnect | set_breakpoint | remove_breakpoint | continue | step | wait_stop | stack_trace | locals | eval
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
    /// Поиск по справке платформы 1С.
    /// find_docs — полнотекстовый, search_docs — семантический.
    /// action: find_docs | search_docs | status
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
                let external_baseline = self.state.external_baseline();
                let action = p.action.clone();
                tokio::task::spawn_blocking(move || match action.as_str() {
                    "find_docs" => {
                        tools::search::find_docs(&engine, external_baseline.clone(), &query, limit)
                    }
                    "search_docs" => {
                        tools::search::search_docs(&engine, external_baseline, &query, limit)
                    }
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

    /// Справка по типам, методам и глобальным функциям платформы 1С.
    /// Точный поиск по имени. Для полнотекстового/семантического поиска используй search.
    #[tool(name = "syntax_help", annotations(read_only_hint = true))]
    async fn syntax_help(
        &self,
        params: Parameters<SyntaxHelpParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::platform::bsl_syntax_help(&params.0.name, params.0.type_name.as_deref())
    }

    /// Вопрос эксперту по 1С:Предприятие через ИТС (1С:Напарник).
    /// Используйте для: стандартов разработки ИТС, паттернов БСП,
    /// методических рекомендаций, типовых решений, диагностики ошибок.
    /// НЕ используйте для: сигнатур методов платформы (→ syntax_help),
    /// поиска в коде конфигурации (→ search).
    /// Требует NAPARNIK_TOKEN. Latency: 5-20 секунд.
    #[tool(name = "its_help", annotations(read_only_hint = true))]
    async fn its_help(
        &self,
        params: Parameters<ItsHelpParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::its_help::its_help(&params.0.question).await
    }
}

#[tool_handler]
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
