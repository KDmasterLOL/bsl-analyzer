//! Metadata tools: configuration tree, object structure, forms.

use crate::state::SharedState;
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;

/// Returns configuration metadata tree — categories and object names.
pub fn get_metadata_tree(
    _state: &SharedState,
    _filter: Option<String>,
) -> Result<CallToolResult, McpError> {
    // TODO: implement in Step 2
    Ok(CallToolResult::success(vec![Content::text("not implemented yet")]))
}

/// Returns detailed structure of a metadata object.
pub fn get_object_structure(
    _state: &SharedState,
    _object_type: &str,
    _object_name: &str,
) -> Result<CallToolResult, McpError> {
    // TODO: implement in Step 2
    Ok(CallToolResult::success(vec![Content::text("not implemented yet")]))
}

/// Returns general configuration info.
pub fn get_configuration_info(_state: &SharedState) -> Result<CallToolResult, McpError> {
    // TODO: implement in Step 2
    Ok(CallToolResult::success(vec![Content::text("not implemented yet")]))
}

/// Returns form structure for a metadata object.
pub fn get_form_structure(
    _state: &SharedState,
    _object_type: &str,
    _object_name: &str,
    _form_name: Option<&str>,
) -> Result<CallToolResult, McpError> {
    // TODO: implement in Step 2
    Ok(CallToolResult::success(vec![Content::text("not implemented yet")]))
}
