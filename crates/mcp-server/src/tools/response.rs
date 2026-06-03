//! Response-shaping helpers shared by the agent-facing tools.

use rmcp::model::{CallToolResult, Content};
use serde_json::Value;

/// Emit `body` as the MCP `structuredContent` field with a pretty-printed JSON
/// mirror in the text `content` block. Structured-aware clients (code-mode) get a
/// typed object to filter in-sandbox; plain clients still read the JSON text. The
/// payload is byte-identical between the two surfaces, so the agent-facing contract
/// is unchanged by carrying it in the native field as well.
pub fn structured(body: Value) -> CallToolResult {
    let text = serde_json::to_string_pretty(&body)
        .unwrap_or_else(|e| format!("{{\"error\":\"serialize\",\"detail\":\"{e}\"}}"));
    // `CallToolResult::structured` mirrors the value compactly; overwrite the text
    // block with the pretty form so plain-text clients keep the readable layout.
    let mut result = CallToolResult::structured(body);
    result.content = vec![Content::text(text)];
    result
}
