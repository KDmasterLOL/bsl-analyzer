//! Query validation tool.

use crate::state::SharedState;
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;

/// Validates SDBL query syntax without execution.
pub fn validate_query(_state: &SharedState, _query: &str) -> Result<CallToolResult, McpError> {
    // TODO: implement in Step 3
    Ok(CallToolResult::success(vec![Content::text("not implemented yet")]))
}
