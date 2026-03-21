//! MCP (Model Context Protocol) server for bsl-analyzer.
//!
//! Exposes 1C:Enterprise metadata and platform knowledge as MCP tools
//! for AI agents. Complements LSP (which handles code analysis) with
//! capabilities LSP doesn't cover: metadata browsing, platform docs,
//! ad-hoc query validation.

mod state;
mod tools;
mod transport;

pub use state::SharedState;
pub use transport::serve_unix_socket;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;

// -- Parameter types for tools --

#[derive(Deserialize, JsonSchema)]
struct MetadataTreeParams {
    /// Категория метаданных для фильтрации: Справочники, Документы, Перечисления,
    /// Обработки, Отчеты, РегистрыСведений, РегистрыНакопления, ОбщиеМодули и др.
    /// Если не указан — возвращаются все категории с количеством объектов.
    filter: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ObjectStructureParams {
    /// Тип объекта метаданных: Document, Catalog, InformationRegister, AccumulationRegister, Enum и др.
    object_type: String,
    /// Имя объекта метаданных, например РеализацияТоваровУслуг
    object_name: String,
}

#[derive(Deserialize, JsonSchema)]
struct FormStructureParams {
    /// Тип объекта: Document, Catalog, DataProcessor, Report и т.д.
    object_type: String,
    /// Имя объекта метаданных
    object_name: String,
    /// Имя формы (если не указано — возвращается список форм объекта)
    form_name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SyntaxHelpParams {
    /// Имя типа, метода или глобальной функции платформы (русское или английское).
    /// Не используется при полнотекстовом поиске (query).
    name: Option<String>,
    /// Имя типа для поиска метода в контексте типа (например type_name="Массив", name="Добавить")
    type_name: Option<String>,
    /// Полнотекстовый поиск по описаниям, именам и параметрам.
    /// Например: "сортировка массива", "текущая дата", "HTTP запрос".
    /// Поддерживает морфологию русского языка.
    query: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ValidateQueryParams {
    /// Текст запроса на языке запросов 1С для проверки
    query: String,
}

#[derive(Deserialize, JsonSchema)]
struct ExecuteQueryParams {
    /// Текст запроса на языке запросов 1С. Только ВЫБРАТЬ/SELECT.
    /// Параметры указывай через &ИмяПараметра.
    query: String,
    /// Максимальное количество строк результата (по умолчанию 100, максимум 1000)
    limit: Option<u32>,
    /// Параметры запроса в виде пар ключ-значение.
    /// Ключ — имя параметра без амперсанда.
    parameters: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Deserialize, JsonSchema)]
struct DebugAttachParams {
    /// Хост сервера отладки 1С
    host: String,
    /// Порт сервера отладки (по умолчанию 1550)
    #[serde(default = "default_debug_port")]
    port: u16,
    /// Имя информационной базы
    infobase: String,
    /// Корневой каталог конфигурации (для маппинга модулей на файлы)
    config_root: Option<String>,
}

fn default_debug_port() -> u16 {
    1550
}

#[derive(Deserialize, JsonSchema)]
struct DebugBreakpointParams {
    /// Имя модуля (например "ОбщийМодуль.МойМодуль" или "Справочник.Номенклатура.МодульОбъекта")
    module: String,
    /// Номер строки
    line: u32,
    /// Условие остановки (BSL-выражение)
    condition: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct DebugRemoveBreakpointParams {
    /// Имя модуля
    module: String,
    /// Номер строки
    line: u32,
}

#[derive(Deserialize, JsonSchema)]
struct DebugStepParams {
    /// Действие: "next" (шаг через), "in" (шаг внутрь), "out" (шаг наружу)
    action: String,
}

#[derive(Deserialize, JsonSchema)]
struct DebugWaitStopParams {
    /// Таймаут ожидания в секундах (по умолчанию 30)
    timeout_secs: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
struct DebugLocalsParams {
    /// Уровень стека (0 = текущий фрейм, по умолчанию 0)
    stack_level: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct DebugEvalParams {
    /// BSL-выражение для вычисления
    expression: String,
    /// Уровень стека (0 = текущий фрейм)
    stack_level: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct FindCodeParams {
    /// Текст для поиска: имя процедуры, вызов API, переменная, строковый литерал.
    /// Используйте точные имена и токены из кода.
    /// Примеры: "ОбработкаПроведения", "СообщитьПользователю", "ТекущаяДата"
    query: String,
    /// Максимальное количество результатов (по умолчанию 10, максимум 50)
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct SearchCodeParams {
    /// Описание искомого кода на естественном языке.
    /// Примеры: "обработка проведения документа", "получение остатков товаров",
    /// "проверка прав доступа пользователя", "отправка HTTP запроса"
    query: String,
    /// Максимальное количество результатов (по умолчанию 10, максимум 50)
    limit: Option<usize>,
}

/// MCP server exposing bsl-analyzer capabilities as tools.
#[derive(Clone)]
pub struct McpServer {
    state: SharedState,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl McpServer {
    pub fn new(state: SharedState) -> Self {
        Self { state, tool_router: Self::tool_router() }
    }

    /// Список всех объектов конфигурации 1С по категориям: справочники, документы,
    /// регистры, перечисления, обработки и т.д.
    /// Без фильтра — сводка (категории и количество), с filter — полный перечень объектов категории.
    #[tool(name = "get_metadata_tree", annotations(read_only_hint = true))]
    async fn get_metadata_tree(
        &self,
        params: Parameters<MetadataTreeParams>,
    ) -> Result<CallToolResult, McpError> {
        let config = self
            .state
            .configuration()
            .await
            .ok_or_else(|| McpError::invalid_params("Configuration not loaded", None))?;
        tools::metadata::get_metadata_tree(&config, params.0.filter)
    }

    /// Получить реквизиты, табличные части, измерения, ресурсы и типы полей объекта метаданных 1С.
    #[tool(name = "get_object_structure", annotations(read_only_hint = true))]
    async fn get_object_structure(
        &self,
        params: Parameters<ObjectStructureParams>,
    ) -> Result<CallToolResult, McpError> {
        let config = self
            .state
            .configuration()
            .await
            .ok_or_else(|| McpError::invalid_params("Configuration not loaded", None))?;
        tools::metadata::get_object_structure(&config, &params.0.object_type, &params.0.object_name)
    }

    /// Получить общую информацию о конфигурации 1С: название, UUID, количество объектов.
    #[tool(name = "get_configuration_info", annotations(read_only_hint = true))]
    async fn get_configuration_info(&self) -> Result<CallToolResult, McpError> {
        let config = self
            .state
            .configuration()
            .await
            .ok_or_else(|| McpError::invalid_params("Configuration not loaded", None))?;
        tools::metadata::get_configuration_info(&config)
    }

    /// Получить структуру управляемой формы объекта 1С: элементы интерфейса, команды, обработчики событий.
    #[tool(name = "get_form_structure", annotations(read_only_hint = true))]
    async fn get_form_structure(
        &self,
        params: Parameters<FormStructureParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::metadata::get_form_structure(
            self.state.workspace_root().map(|p| p.as_path()),
            &params.0.object_type,
            &params.0.object_name,
            params.0.form_name.as_deref(),
        )
    }

    /// Справка по типам, методам и глобальным функциям платформы 1С.
    /// Два режима:
    /// 1) Точный поиск: name="Массив" или name="Добавить", type_name="Массив"
    /// 2) Полнотекстовый поиск: query="сортировка массива" — ищет по описаниям с морфологией
    #[tool(name = "bsl_syntax_help", annotations(read_only_hint = true))]
    async fn bsl_syntax_help(
        &self,
        params: Parameters<SyntaxHelpParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(ref query) = params.0.query {
            return tools::platform::bsl_syntax_search(query);
        }
        let name = params.0.name.as_deref().unwrap_or_default();
        if name.is_empty() {
            return Err(McpError::invalid_params(
                "Укажите name для точного поиска или query для полнотекстового",
                None,
            ));
        }
        tools::platform::bsl_syntax_help(name, params.0.type_name.as_deref())
    }

    /// Проверить синтаксис запроса 1С (SDBL) без выполнения. Найдёт ошибки в ВЫБРАТЬ/SELECT.
    #[tool(name = "validate_query", annotations(read_only_hint = true))]
    async fn validate_query(
        &self,
        params: Parameters<ValidateQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::query::validate_query(&self.state, &params.0.query)
    }

    /// Выполнить запрос на языке 1С (ВЫБРАТЬ/SELECT) и получить данные из базы.
    /// Требует подключения к живой базе 1С (--onec-url).
    #[tool(name = "execute_query", annotations(read_only_hint = true))]
    async fn execute_query(
        &self,
        params: Parameters<ExecuteQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::query::execute_query(
            &self.state,
            &params.0.query,
            params.0.limit,
            params.0.parameters,
        )
        .await
    }

    /// Поиск кода 1С по точному тексту: имена процедур, вызовы API, переменные, строковые литералы.
    /// Быстрый лексический поиск по всем проиндексированным BSL файлам конфигурации.
    /// Используй когда знаешь точное имя или токен из кода.
    #[tool(name = "find_code", annotations(read_only_hint = true))]
    async fn find_code(
        &self,
        params: Parameters<FindCodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let engine = self.state.search_engine().clone();
        let query = params.0.query;
        let limit = params.0.limit.unwrap_or(10).min(50);
        tokio::task::spawn_blocking(move || tools::search::find_code(&engine, &query, limit))
            .await
            .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// Семантический поиск кода 1С — поиск по смыслу на естественном языке.
    /// Опиши что делает искомый код, не нужно знать точные имена.
    /// Используй когда не знаешь точных имён, а знаешь только назначение кода.
    /// Требует работающий сервис эмбеддингов (EMBEDDING_URL).
    #[tool(name = "search_code", annotations(read_only_hint = true))]
    async fn search_code(
        &self,
        params: Parameters<SearchCodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let engine = self.state.search_engine().clone();
        let query = params.0.query;
        let limit = params.0.limit.unwrap_or(10).min(50);
        tokio::task::spawn_blocking(move || tools::search::search_code(&engine, &query, limit))
            .await
            .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// Подключиться к серверу отладки 1С. Начинает сеанс отладки.
    #[tool(name = "debug_attach")]
    async fn debug_attach(
        &self,
        params: Parameters<DebugAttachParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let session = self.state.debug_session().clone();
        tokio::task::spawn_blocking(move || {
            tools::debug::debug_attach(
                &session,
                &p.host,
                p.port,
                &p.infobase,
                p.config_root.as_deref(),
            )
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// Отключиться от сервера отладки 1С. Завершает сеанс отладки.
    #[tool(name = "debug_disconnect")]
    async fn debug_disconnect(&self) -> Result<CallToolResult, McpError> {
        let session = self.state.debug_session().clone();
        tokio::task::spawn_blocking(move || tools::debug::debug_disconnect(&session))
            .await
            .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// Установить точку останова в модуле 1С.
    #[tool(name = "debug_set_breakpoint")]
    async fn debug_set_breakpoint(
        &self,
        params: Parameters<DebugBreakpointParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let session = self.state.debug_session().clone();
        tokio::task::spawn_blocking(move || {
            tools::debug::debug_set_breakpoint(&session, &p.module, p.line, p.condition.as_deref())
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// Удалить точку останова.
    #[tool(name = "debug_remove_breakpoint")]
    async fn debug_remove_breakpoint(
        &self,
        params: Parameters<DebugRemoveBreakpointParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let session = self.state.debug_session().clone();
        tokio::task::spawn_blocking(move || {
            tools::debug::debug_remove_breakpoint(&session, &p.module, p.line)
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// Продолжить выполнение программы после остановки.
    #[tool(name = "debug_continue")]
    async fn debug_continue(&self) -> Result<CallToolResult, McpError> {
        let session = self.state.debug_session().clone();
        tokio::task::spawn_blocking(move || tools::debug::debug_continue(&session))
            .await
            .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// Пошаговое выполнение: next (через), in (внутрь), out (наружу).
    #[tool(name = "debug_step")]
    async fn debug_step(
        &self,
        params: Parameters<DebugStepParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let session = self.state.debug_session().clone();
        tokio::task::spawn_blocking(move || tools::debug::debug_step(&session, &p.action))
            .await
            .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// Ожидать остановку программы (точка останова, исключение, шаг).
    /// Блокирует до наступления события или таймаута.
    #[tool(name = "debug_wait_stop")]
    async fn debug_wait_stop(
        &self,
        params: Parameters<DebugWaitStopParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let session = self.state.debug_session().clone();
        tokio::task::spawn_blocking(move || tools::debug::debug_wait_stop(&session, p.timeout_secs))
            .await
            .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// Получить стек вызовов остановленной программы.
    #[tool(name = "debug_stack_trace", annotations(read_only_hint = true))]
    async fn debug_stack_trace(&self) -> Result<CallToolResult, McpError> {
        let session = self.state.debug_session().clone();
        tokio::task::spawn_blocking(move || tools::debug::debug_stack_trace(&session))
            .await
            .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// Получить локальные переменные на указанном уровне стека.
    #[tool(name = "debug_locals", annotations(read_only_hint = true))]
    async fn debug_locals(
        &self,
        params: Parameters<DebugLocalsParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let session = self.state.debug_session().clone();
        tokio::task::spawn_blocking(move || tools::debug::debug_locals(&session, p.stack_level))
            .await
            .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// Вычислить BSL-выражение в контексте остановленной программы.
    #[tool(name = "debug_eval", annotations(read_only_hint = true))]
    async fn debug_eval(
        &self,
        params: Parameters<DebugEvalParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = params.0;
        let session = self.state.debug_session().clone();
        tokio::task::spawn_blocking(move || {
            tools::debug::debug_eval(&session, &p.expression, p.stack_level)
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "BSL Analyzer MCP server. Provides 1C:Enterprise metadata browsing, \
             platform API reference, SDBL query validation, and code search \
             (full-text and semantic)."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = rmcp::model::Implementation::from_build_env();
        info
    }
}
