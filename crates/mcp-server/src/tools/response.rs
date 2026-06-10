//! Response-shaping helpers shared by the agent-facing tools.

use rmcp::model::CallToolResult;
use serde_json::Value;

/// Emit `body` as the MCP `structuredContent` field. rmcp mirrors the value as a
/// compact JSON text block for clients without structured-output support;
/// structured-aware hosts read `structuredContent` and ignore the mirror, so the
/// payload reaches a model exactly once either way.
pub fn structured(body: Value) -> CallToolResult {
    CallToolResult::structured(body)
}
