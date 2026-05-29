use crate::state::SharedState;
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use std::collections::HashMap;
use std::fmt::Write;

pub async fn validate_query(state: &SharedState, query: &str) -> Result<CallToolResult, McpError> {
    if query.trim().is_empty() {
        return Err(McpError::invalid_params("Пустой запрос", None));
    }

    if let Some(client) = state.onec_client() {
        return validate_query_remote(client, query).await;
    }

    validate_query_local(query)
}

async fn validate_query_remote(
    client: &onec_client::Client,
    query: &str,
) -> Result<CallToolResult, McpError> {
    let request = onec_client::ValidateQueryRequest { query: query.to_string() };

    let result = client.validate_query(&request).await.map_err(|e| {
        McpError::internal_error(format!("Ошибка проверки запроса в 1С: {e}"), None)
    })?;

    if result.valid {
        Ok(CallToolResult::success(vec![Content::text("✓ Запрос синтаксически корректен")]))
    } else {
        let mut out = "✗ Ошибки в запросе:\n\n".to_string();
        for err in &result.errors {
            let _ = writeln!(out, "- {err}");
        }
        Ok(CallToolResult::success(vec![Content::text(out)]))
    }
}

fn validate_query_local(query: &str) -> Result<CallToolResult, McpError> {
    let parse = parser::parse_sdbl(query);
    let root = parse.syntax_node();

    let error_nodes: Vec<_> = root
        .descendants()
        .filter(|node| {
            matches!(node.kind(), syntax::SyntaxKind::ERROR | syntax::SyntaxKind::SDBL_ERROR)
        })
        .collect();

    if error_nodes.is_empty() {
        Ok(CallToolResult::success(vec![Content::text("✓ Запрос синтаксически корректен")]))
    } else {
        let mut out = format!("✗ Найдено ошибок: {}\n\n", error_nodes.len());
        for node in &error_nodes {
            let range = node.text_range();
            let start = u32::from(range.start()) as usize;
            let end = u32::from(range.end()) as usize;
            let fragment = query.get(start..end).unwrap_or("…");
            if fragment.trim().is_empty() {
                let _ = writeln!(out, "- [{}] неожиданный конец выражения", start);
            } else {
                let _ =
                    writeln!(out, "- [{}..{}] неожиданный фрагмент: `{}`", start, end, fragment);
            }
        }
        Ok(CallToolResult::success(vec![Content::text(out)]))
    }
}

const DEFAULT_QUERY_LIMIT: u32 = 100;
const MAX_QUERY_LIMIT: u32 = 1000;

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

    let _ = write!(out, "|");
    for col in &result.columns {
        let _ = write!(out, " {col} |");
    }
    out.push('\n');

    let _ = write!(out, "|");
    for _ in &result.columns {
        let _ = write!(out, "-----|");
    }
    out.push('\n');

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
        let _state = test_shared_state();
        let result = validate_query_local("ВЫБРАТЬ 1").unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✓"), "valid query should pass");
    }

    #[test]
    fn test_validate_query_garbage_input() {
        let result = validate_query_local("}{}{}{").unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✗"), "garbage input should produce errors");
    }

    #[test]
    fn test_validate_query_not_a_query() {
        let result = validate_query_local("это вообще не запрос").unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✗"), "arbitrary text should produce errors");
    }

    #[test]
    fn test_validate_query_incomplete_where() {
        let result =
            validate_query_local("ВЫБРАТЬ Наименование ИЗ Справочник.Номенклатура ГДЕ").unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✗"), "incomplete WHERE should produce errors");
    }

    #[test]
    fn test_validate_query_empty() {
        let state = test_shared_state();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(validate_query(&state, ""));
        assert!(result.is_err(), "empty query should fail");
    }

    #[test]
    fn test_validate_query_whitespace() {
        let state = test_shared_state();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(validate_query(&state, "   "));
        assert!(result.is_err(), "whitespace-only query should fail");
    }

    #[test]
    fn test_validate_query_select_with_fields() {
        let result =
            validate_query_local("ВЫБРАТЬ Ссылка, Наименование ИЗ Справочник.Номенклатура")
                .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✓"), "select with fields should pass");
    }

    #[test]
    fn test_validate_query_english() {
        let result = validate_query_local("SELECT 1").unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✓"), "English SELECT should pass");
    }
}
