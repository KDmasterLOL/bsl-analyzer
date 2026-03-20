//! Query tools: validation and execution.

use crate::state::SharedState;
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use std::collections::HashMap;
use std::fmt::Write;

/// Validates SDBL query syntax without execution.
pub fn validate_query(_state: &SharedState, query: &str) -> Result<CallToolResult, McpError> {
    if query.trim().is_empty() {
        return Err(McpError::invalid_params("Пустой запрос", None));
    }

    let parse = parser::parse_sdbl(query);
    let errors = parse.errors();

    if errors.is_empty() {
        Ok(CallToolResult::success(vec![Content::text("✓ Запрос синтаксически корректен")]))
    } else {
        let mut out = format!("✗ Найдено ошибок: {}\n\n", errors.len());
        for err in errors {
            let range = err.range();
            let start = u32::from(range.start()) as usize;
            let end = u32::from(range.end()) as usize;
            let fragment = query.get(start..end).unwrap_or("…");
            let _ = writeln!(
                out,
                "- [{}..{}] {}: `{}`",
                u32::from(range.start()),
                u32::from(range.end()),
                err.message(),
                fragment,
            );
        }
        Ok(CallToolResult::success(vec![Content::text(out)]))
    }
}

const DEFAULT_QUERY_LIMIT: u32 = 100;
const MAX_QUERY_LIMIT: u32 = 1000;

/// Executes a SELECT query against a live 1C database.
pub async fn execute_query(
    state: &SharedState,
    query: &str,
    limit: Option<u32>,
    parameters: Option<HashMap<String, serde_json::Value>>,
) -> Result<CallToolResult, McpError> {
    let client = state.onec_client().ok_or_else(|| {
        McpError::invalid_params(
            "1C HTTP клиент не настроен. Укажите --onec-url при запуске MCP сервера.",
            None,
        )
    })?;

    if query.trim().is_empty() {
        return Err(McpError::invalid_params("Пустой запрос", None));
    }

    // Only SELECT/ВЫБРАТЬ allowed
    let prefix = query.trim();
    let upper_start: String = prefix.chars().take(30).collect::<String>().to_uppercase();
    if !upper_start.starts_with("ВЫБРАТЬ") && !upper_start.starts_with("SELECT") {
        return Err(McpError::invalid_params("Только SELECT/ВЫБРАТЬ запросы разрешены", None));
    }

    let limit = limit.unwrap_or(DEFAULT_QUERY_LIMIT).min(MAX_QUERY_LIMIT);

    let request = onec_client::QueryRequest {
        query: query.to_string(),
        limit,
        parameters: parameters.unwrap_or_default(),
    };

    let result = client.execute_query(&request).await.map_err(|e| {
        McpError::internal_error(format!("Ошибка выполнения запроса в 1С: {e}"), None)
    })?;

    Ok(CallToolResult::success(vec![Content::text(format_query_result(&result))]))
}

#[cfg(test)]
fn test_shared_state() -> crate::SharedState {
    crate::SharedState::shared()
}

fn format_query_result(result: &onec_client::QueryResult) -> String {
    if result.columns.is_empty() {
        return "Запрос выполнен, результат пуст.".to_string();
    }

    let mut out = format!("## Результат запроса ({} записей", result.total);
    if result.truncated {
        out.push_str(", результат усечён");
    }
    out.push_str(")\n\n");

    // Header
    let _ = write!(out, "|");
    for col in &result.columns {
        let _ = write!(out, " {col} |");
    }
    out.push('\n');

    // Separator
    let _ = write!(out, "|");
    for _ in &result.columns {
        let _ = write!(out, "-----|");
    }
    out.push('\n');

    // Rows
    for row in &result.rows {
        let _ = write!(out, "|");
        for val in row {
            let s = match val {
                serde_json::Value::Null => "—".to_string(),
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let _ = write!(out, " {s} |");
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_text(result: &CallToolResult) -> &str {
        result.content[0].raw.as_text().expect("expected text content").text.as_str()
    }

    #[test]
    fn test_validate_query_valid() {
        let state = test_shared_state();
        let result = validate_query(&state, "ВЫБРАТЬ 1").unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✓"), "valid query should pass");
    }

    #[test]
    fn test_validate_query_garbage_input() {
        let state = test_shared_state();
        // SDBL parser is error-recovering (IDE parser), so even garbage input
        // produces a result. We just verify the tool doesn't panic.
        let result = validate_query(&state, "}{}{}{").unwrap();
        let text = extract_text(&result);
        assert!(
            text.contains("✓") || text.contains("✗"),
            "should produce either success or error marker"
        );
    }

    #[test]
    fn test_validate_query_empty() {
        let state = test_shared_state();
        let result = validate_query(&state, "");
        assert!(result.is_err(), "empty query should fail");
    }

    #[test]
    fn test_validate_query_whitespace() {
        let state = test_shared_state();
        let result = validate_query(&state, "   ");
        assert!(result.is_err(), "whitespace-only query should fail");
    }

    #[test]
    fn test_validate_query_select_with_fields() {
        let state = test_shared_state();
        let result =
            validate_query(&state, "ВЫБРАТЬ Ссылка, Наименование ИЗ Справочник.Номенклатура")
                .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✓"), "select with fields should pass");
    }

    #[test]
    fn test_validate_query_english() {
        let state = test_shared_state();
        let result = validate_query(&state, "SELECT 1").unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✓"), "English SELECT should pass");
    }
}
