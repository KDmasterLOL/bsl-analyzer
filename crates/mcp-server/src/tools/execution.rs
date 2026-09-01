use crate::state::SharedState;
use crate::tools::response::text_within_budget;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::ErrorData as McpError;

/// Platform output (a run's context block, a syntax-error listing, an evaluated value) has no
/// size of its own, so every body that carries it goes out through the output budget.
const BUDGET_NOTE: &str =
    "\n-- вывод усечён под max_output_tokens; повысьте бюджет или сократите код/выражение --\n";

fn require_onec_connection(
    state: &SharedState,
    connection: Option<&str>,
) -> Result<crate::OnecConnection, McpError> {
    state.onec_connection(connection).map_err(|e| McpError::invalid_params(e, None))
}

pub async fn check_syntax(
    state: &SharedState,
    code: &str,
    connection: Option<&str>,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let selected = require_onec_connection(state, connection)?;

    if code.trim().is_empty() {
        return Err(McpError::invalid_params("Пустой код", None));
    }

    let request = onec_client::CheckSyntaxRequest { code: code.to_string() };

    let result = selected.client().check_syntax(&request).await.map_err(|e| {
        McpError::internal_error(format!("Ошибка проверки синтаксиса в 1С: {e}"), None)
    })?;

    if result.valid {
        Ok(CallToolResult::success(vec![ContentBlock::text("✓ Синтаксис корректен")]))
    } else {
        let error = result.error.unwrap_or_default();
        Ok(text_within_budget(
            format!("✗ Ошибка синтаксиса:\n{error}"),
            max_output_tokens,
            BUDGET_NOTE,
        ))
    }
}

pub async fn execute_code(
    state: &SharedState,
    code: &str,
    connection: Option<&str>,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let selected = require_onec_connection(state, connection)?;
    if !selected.allow_execute() {
        return Err(McpError::invalid_params("BSL run is disabled for this 1C connection", None));
    }

    if code.trim().is_empty() {
        return Err(McpError::invalid_params("Пустой код", None));
    }

    let request = onec_client::ExecuteRequest { code: code.to_string() };

    let result =
        selected.client().execute_code(&request).await.map_err(|e| {
            McpError::internal_error(format!("Ошибка выполнения кода в 1С: {e}"), None)
        })?;

    let mut out = if result.success {
        "✓ Код выполнен успешно".to_string()
    } else {
        let error = result.error.unwrap_or_default();
        format!("✗ Ошибка выполнения:\n{error}")
    };

    if let Some(ms) = result.duration_ms {
        out.push_str(&format!("\nВремя: {ms} мс"));
    }

    if let Some(ref ctx) = result.context {
        if !ctx.is_empty() {
            out.push_str("\n\n## Контекст\n");
            for (key, value) in ctx {
                let v = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                out.push_str(&format!("- **{key}**: {v}\n"));
            }
        }
    }

    Ok(text_within_budget(out, max_output_tokens, BUDGET_NOTE))
}

pub async fn eval_expression(
    state: &SharedState,
    expression: &str,
    connection: Option<&str>,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let selected = require_onec_connection(state, connection)?;
    if !selected.allow_execute() {
        return Err(McpError::invalid_params("BSL eval is disabled for this 1C connection", None));
    }

    if expression.trim().is_empty() {
        return Err(McpError::invalid_params("Пустое выражение", None));
    }

    let request = onec_client::EvalRequest { expression: expression.to_string() };

    let result = selected.client().eval_expression(&request).await.map_err(|e| {
        McpError::internal_error(format!("Ошибка вычисления выражения в 1С: {e}"), None)
    })?;

    if result.success {
        let value = match &result.result {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => "Неопределено".to_string(),
        };
        let type_name = result.type_name.unwrap_or_default();
        Ok(CallToolResult::success(vec![ContentBlock::text(format_eval_result(
            &value,
            &type_name,
            max_output_tokens,
        ))]))
    } else {
        let error = result.error.unwrap_or_default();
        Ok(text_within_budget(
            format!("✗ Ошибка вычисления:\n{error}"),
            max_output_tokens,
            BUDGET_NOTE,
        ))
    }
}

/// A serialized value (a structure, a value table) has no bound of its own, so it goes out
/// through the budget. The value alone is clipped, with the type line and the note reserved
/// out of the budget first: a clipped value must not cost the agent the one line that says
/// what it was looking at.
fn format_eval_result(value: &str, type_name: &str, max_output_tokens: usize) -> String {
    let tail = format!("\nТип: {type_name}");
    // Everything but the value itself — the label, the type line, the note — is reserved out
    // of the budget, so the composed body stays inside it.
    let reserved = (format!("✓ Результат: {tail}").len() + BUDGET_NOTE.len()).div_ceil(4);
    let mut clipped = value.to_string();
    let cut = crate::tools::response::truncate_text_to_budget(
        &mut clipped,
        max_output_tokens.saturating_sub(reserved).max(1),
        " …",
    );
    let mut out = format!("✓ Результат: {clipped}{tail}");
    // Hard ceiling on the composed body: an absurdly long type name can blow the budget even
    // when the value itself fits.
    let ceiling_hit =
        crate::tools::response::truncate_text_to_budget(&mut out, max_output_tokens, BUDGET_NOTE);
    if cut && !ceiling_hit {
        out.push_str(BUDGET_NOTE);
    }
    out
}

#[cfg(test)]
fn test_shared_state() -> SharedState {
    SharedState::shared()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_check_syntax_empty_code() {
        let state = test_shared_state();
        let result = check_syntax(&state, "", None, 6000).await;
        assert!(result.is_err(), "empty code should fail");
    }

    #[tokio::test]
    async fn test_check_syntax_whitespace() {
        let state = test_shared_state();
        let result = check_syntax(&state, "   ", None, 6000).await;
        assert!(result.is_err(), "whitespace-only code should fail");
    }

    #[tokio::test]
    async fn test_check_syntax_no_client() {
        let state = test_shared_state();
        let result = check_syntax(&state, "а = 1;", None, 6000).await;
        assert!(result.is_err(), "should fail without onec client");
    }

    #[tokio::test]
    async fn test_execute_code_empty() {
        let state = test_shared_state();
        let result = execute_code(&state, "", None, 6000).await;
        assert!(result.is_err(), "empty code should fail");
    }

    #[tokio::test]
    async fn test_execute_code_no_client() {
        let state = test_shared_state();
        let result = execute_code(&state, "Сообщить(1);", None, 6000).await;
        assert!(result.is_err(), "should fail without onec client");
    }

    #[tokio::test]
    async fn test_eval_expression_empty() {
        let state = test_shared_state();
        let result = eval_expression(&state, "", None, 6000).await;
        assert!(result.is_err(), "empty expression should fail");
    }

    #[tokio::test]
    async fn test_eval_expression_no_client() {
        let state = test_shared_state();
        let result = eval_expression(&state, "1 + 1", None, 6000).await;
        assert!(result.is_err(), "should fail without onec client");
    }

    #[test]
    fn eval_result_within_budget_is_untouched() {
        let out = format_eval_result("42", "Число", 6000);
        assert_eq!(out, "✓ Результат: 42\nТип: Число");
    }

    #[test]
    fn eval_clips_a_huge_value_but_never_the_type_line() {
        let out = format_eval_result(&"я".repeat(10_000), "ТаблицаЗначений", 200);
        assert!(out.contains("\nТип: ТаблицаЗначений"), "type line must survive: {out}");
        assert!(out.ends_with(BUDGET_NOTE), "must say it clipped: {out}");
        assert!(out.len() <= 200 * 4, "must stay inside the budget: {}", out.len());
    }
}
