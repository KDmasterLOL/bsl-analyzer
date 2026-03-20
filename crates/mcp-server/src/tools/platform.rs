//! Platform reference tool: BSL syntax help.

use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;

/// Returns documentation for a platform type, method, or global function.
pub fn bsl_syntax_help(_name: &str, _type_name: Option<&str>) -> Result<CallToolResult, McpError> {
    // TODO: implement in Step 3
    Ok(CallToolResult::success(vec![Content::text("not implemented yet")]))
}
