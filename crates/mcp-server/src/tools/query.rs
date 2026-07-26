use crate::state::SharedState;
use crate::tools::response::{structured, text_within_budget};
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use serde_json::json;
use std::collections::HashMap;
use std::fmt::Write;

/// Static contract for cold-start discovery, mirroring `graph`/`diagnostics` schema so the
/// query tool is self-describing instead of revealing its actions only through an error.
pub fn schema() -> CallToolResult {
    structured(json!({
        "schema_version": "1",
        "actions": ["validate", "execute", "schema"],
        "validate": "syntax-check an SDBL query. Works offline via the local parser; with --onec-url it additionally runs live platform validation.",
        "execute": "run a SELECT query against the live 1C base (requires --onec-url). `limit` caps rows; `parameters` binds named query parameters.",
        "params": {
            "query": "the SDBL text (required for validate and execute)",
            "limit": "max rows for execute (optional)",
            "parameters": "object of name → value bindings for execute (optional)",
            "max_output_tokens": "output budget in tokens (~4 chars each) for execute, on top of `limit` — the rendered table is cut at a row boundary with a note (default 6000)"
        },
        "prerequisites": "validate needs nothing for offline syntax checks; execute (and live validation) need --onec-url / --onec-user / --onec-password"
    }))
}

/// A malformed query yields one diagnostic line per error node, so the listing is bounded by
/// the output budget like every other unbounded body.
const VALIDATE_NOTE: &str =
    "\n-- список ошибок усечён под max_output_tokens; исправьте показанные ошибки и повторите \
     проверку или повысьте бюджет --\n";

pub async fn validate_query(
    state: &SharedState,
    query: &str,
    connection: Option<&str>,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    if query.trim().is_empty() {
        return Err(McpError::invalid_params("Пустой запрос", None));
    }

    if connection.is_some() {
        let selected =
            state.onec_connection(connection).map_err(|e| McpError::invalid_params(e, None))?;
        return validate_query_remote(selected.client(), query, max_output_tokens).await;
    }
    if let Ok(selected) = state.onec_connection(None) {
        return validate_query_remote(selected.client(), query, max_output_tokens).await;
    }

    validate_query_local(query, max_output_tokens)
}

async fn validate_query_remote(
    client: &onec_client::Client,
    query: &str,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let request = onec_client::ValidateQueryRequest { query: query.to_string() };

    let result = client.validate_query(&request).await.map_err(|e| {
        McpError::internal_error(format!("Ошибка проверки запроса в 1С: {e}"), None)
    })?;

    if result.valid {
        Ok(CallToolResult::success(vec![Content::text("✓ Запрос синтаксически корректен")]))
    } else {
        Ok(render_validation_errors(&result.errors, max_output_tokens))
    }
}

/// The platform decides how many errors it reports and how long each one is, so the listing is
/// bounded by the output budget; the local parser's own listing goes through the same path.
fn render_validation_errors(errors: &[String], max_output_tokens: usize) -> CallToolResult {
    let mut out = "✗ Ошибки в запросе:\n\n".to_string();
    for err in errors {
        let _ = writeln!(out, "- {err}");
    }
    text_within_budget(out, max_output_tokens, VALIDATE_NOTE)
}

fn validate_query_local(query: &str, max_output_tokens: usize) -> Result<CallToolResult, McpError> {
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
        // The parser can emit overlapping ERROR/SDBL_ERROR nodes at the same offset (e.g. a
        // trailing `ГДЕ` yields two "unexpected end" nodes at the same position), which rendered
        // the identical diagnostic twice. Collapse byte-identical lines, preserving first order,
        // so the count and listing reflect distinct errors.
        let mut seen = std::collections::HashSet::new();
        let mut lines: Vec<String> = Vec::new();
        for node in &error_nodes {
            let range = node.text_range();
            let start = u32::from(range.start()) as usize;
            let end = u32::from(range.end()) as usize;
            let fragment = query.get(start..end).unwrap_or("…");
            let line = if fragment.trim().is_empty() {
                format!("- [{start}] неожиданный конец выражения")
            } else {
                format!("- [{start}..{end}] неожиданный фрагмент: `{fragment}`")
            };
            if seen.insert(line.clone()) {
                lines.push(line);
            }
        }

        let mut out = format!("✗ Найдено ошибок: {}\n\n", lines.len());
        for line in &lines {
            let _ = writeln!(out, "{line}");
        }
        Ok(text_within_budget(out, max_output_tokens, VALIDATE_NOTE))
    }
}

const DEFAULT_QUERY_LIMIT: u32 = 100;
const MAX_QUERY_LIMIT: u32 = 1000;

pub async fn execute_query(
    state: &SharedState,
    query: &str,
    limit: Option<u32>,
    parameters: Option<HashMap<String, serde_json::Value>>,
    connection: Option<&str>,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let selected =
        state.onec_connection(connection).map_err(|e| McpError::invalid_params(e, None))?;

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

    let result = selected.client().execute_query(&request).await.map_err(|e| {
        McpError::internal_error(format!("Ошибка выполнения запроса в 1С: {e}"), None)
    })?;

    Ok(render_query_result(&result, max_output_tokens))
}

/// A row cap bounds how MANY rows come back, nothing bounds how WIDE they are, so the
/// rendered table gets an output budget on top of `limit`. Truncation cuts at a line (row)
/// boundary, keeping the header and the leading rows.
fn render_query_result(
    result: &onec_client::QueryResult,
    max_output_tokens: usize,
) -> CallToolResult {
    let note = if result.truncated {
        // The row cap already fired: raising only the token budget stops at the same rows.
        "\n-- вывод усечён под max_output_tokens, и строки уже ограничены `limit`; сузьте выборку (меньше колонок/строк) либо поднимите ОБА: max_output_tokens и limit --\n"
    } else {
        "\n-- вывод усечён под max_output_tokens; сузьте выборку (меньше колонок/строк) или повысьте бюджет --\n"
    };
    text_within_budget(format_query_result(result), max_output_tokens, note)
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
            // A multiline string value would otherwise break the row into fragments that read
            // as separate (malformed) table rows — and would turn the budget's row-boundary
            // cut into a mid-row cut.
            let s = match val {
                serde_json::Value::Null => "—".to_string(),
                serde_json::Value::String(s) => s.replace(['\n', '\r'], " "),
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
    fn schema_advertises_every_action() {
        let result = schema();
        let body = result.structured_content.expect("schema is structured");
        let actions = body["actions"].as_array().expect("actions array");
        for action in ["validate", "execute", "schema"] {
            assert!(
                actions.iter().any(|a| a == action),
                "schema must advertise `{action}`: {body}",
            );
        }
    }

    #[test]
    fn test_validate_query_valid() {
        let _state = test_shared_state();
        let result = validate_query_local("ВЫБРАТЬ 1", 6000).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✓"), "valid query should pass");
    }

    #[test]
    fn test_validate_query_garbage_input() {
        let result = validate_query_local("}{}{}{", 6000).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✗"), "garbage input should produce errors");
    }

    #[test]
    fn test_validate_query_not_a_query() {
        let result = validate_query_local("это вообще не запрос", 6000).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✗"), "arbitrary text should produce errors");
    }

    #[test]
    fn test_validate_query_incomplete_where() {
        let result =
            validate_query_local("ВЫБРАТЬ Наименование ИЗ Справочник.Номенклатура ГДЕ", 6000)
                .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✗"), "incomplete WHERE should produce errors");
    }

    #[test]
    fn validate_does_not_report_the_same_error_twice() {
        // `ВЫБРАТЬ ИЗ ГДЕ` made the parser emit overlapping error nodes at the same offset,
        // which previously listed the identical diagnostic twice. Each rendered line must be
        // unique, and the reported count must match the listed lines.
        let result = validate_query_local("ВЫБРАТЬ ИЗ ГДЕ", 6000).unwrap();
        let text = extract_text(&result);
        let error_lines: Vec<&str> = text.lines().filter(|l| l.starts_with("- [")).collect();
        let mut unique = error_lines.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(error_lines.len(), unique.len(), "duplicate error line(s): {text}");
        assert!(
            text.contains(&format!("Найдено ошибок: {}", error_lines.len())),
            "reported count must match listed lines: {text}",
        );
    }

    #[test]
    fn test_validate_query_empty() {
        let state = test_shared_state();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(validate_query(&state, "", None, 6000));
        assert!(result.is_err(), "empty query should fail");
    }

    #[test]
    fn test_validate_query_whitespace() {
        let state = test_shared_state();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(validate_query(&state, "   ", None, 6000));
        assert!(result.is_err(), "whitespace-only query should fail");
    }

    #[test]
    fn test_validate_query_select_with_fields() {
        let result =
            validate_query_local("ВЫБРАТЬ Ссылка, Наименование ИЗ Справочник.Номенклатура", 6000)
                .unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✓"), "select with fields should pass");
    }

    fn wide_result(rows: usize, truncated: bool) -> onec_client::QueryResult {
        onec_client::QueryResult {
            columns: vec!["Ссылка".into(), "Наименование".into()],
            rows: (0..rows)
                .map(|i| {
                    vec![serde_json::json!(format!("row{i}")), serde_json::json!("ш".repeat(300))]
                })
                .collect(),
            total: rows as u32,
            truncated,
        }
    }

    #[test]
    fn platform_error_listing_is_bounded_by_the_budget() {
        let errors: Vec<String> =
            (0..200).map(|i| format!("Ошибка {i}: {}", "подробности ".repeat(10))).collect();
        let full = extract_text(&render_validation_errors(&errors, 100_000)).to_string();
        let clipped = render_validation_errors(&errors, 100);
        let clipped = extract_text(&clipped);
        assert!(clipped.len() < full.len(), "a 100-token budget must clip the listing");
        assert!(clipped.starts_with("✗ Ошибки в запросе:"), "the header must survive: {clipped}");
        assert!(clipped.contains("список ошибок усечён"), "must carry the note: {clipped}");
        assert!(clipped.len() <= 100 * 4, "must stay inside the budget: {}", clipped.len());
    }

    #[test]
    fn multiline_cell_never_breaks_a_table_row() {
        let result = onec_client::QueryResult {
            columns: vec!["Комментарий".into()],
            rows: vec![vec![serde_json::json!("первая\nвторая\r\nтретья")]],
            total: 1,
            truncated: false,
        };
        let text = extract_text(&render_query_result(&result, 6000)).to_string();
        // Header, separator and exactly one data row — the embedded newlines must not split it.
        assert_eq!(text.lines().filter(|l| l.starts_with('|')).count(), 3, "{text}");
        assert!(text.contains("| первая вторая  третья |"), "{text}");
    }

    #[test]
    fn execute_result_within_budget_is_untouched() {
        let result = render_query_result(&wide_result(2, false), 6000);
        let text = extract_text(&result);
        assert!(text.contains("row1"), "both rows must survive: {text}");
        assert!(!text.contains("усечён"), "nothing to note: {text}");
    }

    #[test]
    fn execute_result_over_budget_is_cut_at_a_row_boundary() {
        let result = render_query_result(&wide_result(200, false), 600);
        let text = extract_text(&result);
        assert!(text.contains("| Ссылка |"), "the header must survive: {text}");
        assert!(text.contains("row0"), "the leading rows must survive: {text}");
        assert!(text.contains("усечён под max_output_tokens"), "must carry the note: {text}");
        assert!(!text.contains("row199"), "the trailing rows must be dropped");
        // Truncation never leaves half a row before the note.
        let body = text.split("\n-- вывод усечён").next().unwrap();
        assert!(body.ends_with(" |\n"), "cut must land on a row boundary: {body:?}");
    }

    #[test]
    fn execute_note_says_raising_the_budget_alone_will_not_help_when_limit_also_capped() {
        let result = render_query_result(&wide_result(200, true), 600);
        let text = extract_text(&result);
        assert!(
            text.contains("ОБА: max_output_tokens и limit"),
            "note must name both caps: {text}"
        );
    }

    #[test]
    fn test_validate_query_english() {
        let result = validate_query_local("SELECT 1", 6000).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("✓"), "English SELECT should pass");
    }
}
