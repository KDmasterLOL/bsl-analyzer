use crate::tools::response::text_within_budget;
use naparnik::{NaparnikApi, NaparnikHttpClient};
use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;
use tokio::sync::Mutex;
use tracing::debug;

static CLIENT: Mutex<Option<NaparnikHttpClient>> = Mutex::const_new(None);

/// The knowledge base answers at whatever length the document warrants, so the answer goes
/// out through the output budget.
const BUDGET_NOTE: &str =
    "\n-- ответ усечён под max_output_tokens; повысьте бюджет или задайте более узкий вопрос --\n";

pub async fn its_help(
    question: &str,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let mut guard = CLIENT.lock().await;
    if guard.is_none() {
        let client = NaparnikHttpClient::from_env()
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        *guard = Some(client);
    }
    let client = guard.as_ref().unwrap();

    debug!(question, "its_help request");

    let answer = client
        .ask_its(question)
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(text_within_budget(answer.text, max_output_tokens, BUDGET_NOTE))
}
