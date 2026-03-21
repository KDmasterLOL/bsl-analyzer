//! MCP (Model Context Protocol) server for bsl-analyzer.
//!
//! Exposes 1C:Enterprise metadata and platform knowledge as MCP tools
//! for AI agents. Complements LSP (which handles code analysis) with
//! capabilities LSP doesn't cover: metadata browsing, platform docs,
//! ad-hoc query validation.

mod state;
mod tools;

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
    name: String,
    /// Имя типа для поиска метода в контексте типа (например type_name="Массив", name="Добавить")
    type_name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct FindDocsParams {
    /// Текст для поиска по справке платформы 1С: имена типов, методов, функций.
    /// Используйте точные имена и токены из справочной документации.
    /// Примеры: "Массив", "HTTPСоединение", "НачатьТранзакцию"
    query: String,
    /// Максимальное количество результатов (по умолчанию 10, максимум 50)
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct SearchDocsParams {
    /// Описание искомой функциональности платформы на естественном языке.
    /// Примеры: "как записать файл на диск", "работа с HTTP запросами",
    /// "сортировка массива", "текущая дата и время"
    query: String,
    /// Максимальное количество результатов (по умолчанию 10, максимум 50)
    limit: Option<usize>,
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
struct CheckSyntaxParams {
    /// BSL-код для проверки синтаксиса.
    /// Нельзя объявлять Функция/Процедура — только операторы, выражения, условия, циклы.
    code: String,
}

#[derive(Deserialize, JsonSchema)]
struct ExecuteCodeParams {
    /// BSL-код для выполнения (операторы). Код выполняется через Выполнить().
    /// Для возврата данных пиши в переменную Контекст: Контекст.Вставить("ключ", значение).
    /// Типы значений Контекст, сохраняющие структуру в JSON: Строка, Число, Булево, Дата,
    /// Неопределено, Структура, Массив, Соответствие (и Фиксированные варианты).
    /// Ссылки, объекты, ТаблицаЗначений и др. будут приведены к строке через Строка().
    /// Нельзя объявлять Функция/Процедура — только операторы: присваивания, вызовы, циклы, условия.
    code: String,
}

#[derive(Deserialize, JsonSchema)]
struct EvalExpressionParams {
    /// BSL-выражение для вычисления. Выражение вычисляется через Вычислить() и возвращает результат.
    /// Нельзя объявлять Функция/Процедура. Только выражения, возвращающие значение:
    /// ТекущаяДата(), 1+1, Справочники.Номенклатура.НайтиПоНаименованию("Товар")
    expression: String,
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
    /// Точный поиск по имени: name="Массив" или name="Добавить", type_name="Массив".
    /// Для полнотекстового поиска используй find_docs, для семантического — search_docs.
    #[tool(name = "bsl_syntax_help", annotations(read_only_hint = true))]
    async fn bsl_syntax_help(
        &self,
        params: Parameters<SyntaxHelpParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::platform::bsl_syntax_help(&params.0.name, params.0.type_name.as_deref())
    }

    /// Проверить синтаксис запроса 1С (SDBL) без выполнения. Найдёт ошибки в ВЫБРАТЬ/SELECT.
    #[tool(name = "validate_query", annotations(read_only_hint = true))]
    async fn validate_query(
        &self,
        params: Parameters<ValidateQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::query::validate_query(&self.state, &params.0.query).await
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

    /// Проверить синтаксис BSL-кода без выполнения. Код компилируется платформой 1С, но не запускается.
    /// ОГРАНИЧЕНИЯ: нельзя объявлять Функция/Процедура — только операторы, условия, циклы, присваивания.
    #[tool(name = "check_syntax", annotations(read_only_hint = true))]
    async fn check_syntax(
        &self,
        params: Parameters<CheckSyntaxParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::execution::check_syntax(&self.state, &params.0.code).await
    }

    /// Выполнить BSL-код (операторы) в реальной базе 1С. Код выполняется через Выполнить().
    /// Для возврата данных используй переменную Контекст: Контекст.Вставить("ключ", значение).
    /// Содержимое Контекст возвращается в ответе. Требует подключения (--onec-url).
    /// ОГРАНИЧЕНИЯ: нельзя объявлять Функция/Процедура — только операторы.
    #[tool(name = "execute_code")]
    async fn execute_code(
        &self,
        params: Parameters<ExecuteCodeParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::execution::execute_code(&self.state, &params.0.code).await
    }

    /// Вычислить BSL-выражение в реальной базе 1С и получить результат. Выражение вычисляется
    /// через Вычислить(). Используй для получения значений: ТекущаяДата(), 1+1,
    /// Справочники.Номенклатура.НайтиПоНаименованию("Товар"). Требует подключения (--onec-url).
    /// ОГРАНИЧЕНИЯ: нельзя объявлять Функция/Процедура — только выражения.
    #[tool(name = "eval_expression")]
    async fn eval_expression(
        &self,
        params: Parameters<EvalExpressionParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::execution::eval_expression(&self.state, &params.0.expression).await
    }

    /// Статус поискового индекса: количество файлов, чанков, режим (FTS/семантика),
    /// прогресс текущей индексации (процент, батчи).
    #[tool(name = "search_status", annotations(read_only_hint = true))]
    async fn search_status(&self) -> Result<CallToolResult, McpError> {
        let engine = self.state.search_engine().clone();
        let progress = self.state.index_progress().clone();
        tokio::task::spawn_blocking(move || tools::search::search_status(&engine, &progress))
            .await
            .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
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

    /// Поиск по справке платформы 1С по точному тексту: имена типов, методов, функций.
    /// Быстрый лексический поиск по документации встроенных типов и глобальных функций.
    /// Используй когда знаешь точное имя из справки.
    #[tool(name = "find_docs", annotations(read_only_hint = true))]
    async fn find_docs(
        &self,
        params: Parameters<FindDocsParams>,
    ) -> Result<CallToolResult, McpError> {
        let engine = self.state.search_engine().clone();
        let query = params.0.query;
        let limit = params.0.limit.unwrap_or(10).min(50);
        tokio::task::spawn_blocking(move || tools::search::find_docs(&engine, &query, limit))
            .await
            .map_err(|e| McpError::internal_error(format!("Task error: {e}"), None))?
    }

    /// Семантический поиск по справке платформы 1С — поиск по смыслу.
    /// Опиши что ищешь на естественном языке: "как записать файл", "работа с HTTP".
    /// Используй когда не знаешь точных имён, а знаешь только что нужно.
    /// Требует работающий сервис эмбеддингов (EMBEDDING_URL).
    #[tool(name = "search_docs", annotations(read_only_hint = true))]
    async fn search_docs(
        &self,
        params: Parameters<SearchDocsParams>,
    ) -> Result<CallToolResult, McpError> {
        let engine = self.state.search_engine().clone();
        let query = params.0.query;
        let limit = params.0.limit.unwrap_or(10).min(50);
        tokio::task::spawn_blocking(move || tools::search::search_docs(&engine, &query, limit))
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
             platform API reference, SDBL query validation, code execution, \
             and code search (full-text and semantic)."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = rmcp::model::Implementation::from_build_env();
        info
    }
}
