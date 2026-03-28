use naparnik::{NaparnikApi, NaparnikHttpClient};
use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;
use tokio::sync::Mutex;
use tracing::debug;

static CLIENT: Mutex<Option<NaparnikHttpClient>> = Mutex::const_new(None);

pub async fn its_help(question: &str) -> Result<CallToolResult, McpError> {
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

    Ok(CallToolResult::success(vec![rmcp::model::Content::text(answer.text)]))
}
