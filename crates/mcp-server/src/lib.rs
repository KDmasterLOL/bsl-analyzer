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
    /// Имя типа, метода или глобальной функции платформы (русское или английское)
    name: String,
    /// Имя типа для поиска метода в контексте типа (например type_name="Массив", name="Добавить")
    type_name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ValidateQueryParams {
    /// Текст запроса на языке запросов 1С для проверки
    query: String,
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
        tools::metadata::get_metadata_tree(&self.state, params.0.filter)
    }

    /// Получить реквизиты, табличные части, измерения, ресурсы и типы полей объекта метаданных 1С.
    #[tool(name = "get_object_structure", annotations(read_only_hint = true))]
    async fn get_object_structure(
        &self,
        params: Parameters<ObjectStructureParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::metadata::get_object_structure(
            &self.state,
            &params.0.object_type,
            &params.0.object_name,
        )
    }

    /// Получить общую информацию о конфигурации 1С: название, UUID, количество объектов.
    #[tool(name = "get_configuration_info", annotations(read_only_hint = true))]
    async fn get_configuration_info(&self) -> Result<CallToolResult, McpError> {
        tools::metadata::get_configuration_info(&self.state)
    }

    /// Получить структуру управляемой формы объекта 1С: элементы интерфейса, команды, обработчики событий.
    #[tool(name = "get_form_structure", annotations(read_only_hint = true))]
    async fn get_form_structure(
        &self,
        params: Parameters<FormStructureParams>,
    ) -> Result<CallToolResult, McpError> {
        tools::metadata::get_form_structure(
            &self.state,
            &params.0.object_type,
            &params.0.object_name,
            params.0.form_name.as_deref(),
        )
    }

    /// Справка по типам, методам и глобальным функциям платформы 1С.
    /// Принимает имя типа (Массив, Строка), метода (Добавить, Найти) или глобальной функции (СтрДлина, Формат).
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
        tools::query::validate_query(&self.state, &params.0.query)
    }
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "BSL Analyzer MCP server. Provides 1C:Enterprise metadata browsing, \
             platform API reference, and SDBL query validation."
                .into(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = rmcp::model::Implementation::from_build_env();
        info
    }
}
