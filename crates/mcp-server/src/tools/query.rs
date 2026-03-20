//! Query validation tool.

use crate::state::SharedState;
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use std::fmt::Write;

/// Validates SDBL query syntax without execution.
///
/// Parses the query text and reports any syntax errors.
/// Does NOT validate against metadata (table/field names) — that requires
/// HIR lowering with Configuration, which will be added later.
pub fn validate_query(_state: &SharedState, query: &str) -> Result<CallToolResult, McpError> {
    if query.trim().is_empty() {
        return Err(McpError::invalid_params("Пустой запрос", None));
    }

    let parse = parser::parse_sdbl(query);
    let errors = parse.errors();

    if errors.is_empty() {
        Ok(CallToolResult::success(vec![Content::text("✓ Запрос синтаксически корректен")]))
    } else {
        let mut out = format!("✗ Найдено ошибок: {}\n\n", errors.len());
        for err in errors {
            let range = err.range();
            let start = u32::from(range.start()) as usize;
            let end = u32::from(range.end()) as usize;
            let fragment = query.get(start..end).unwrap_or("…");
            let _ = writeln!(
                out,
                "- [{}..{}] {}: `{}`",
                u32::from(range.start()),
                u32::from(range.end()),
                err.message(),
                fragment,
            );
        }
        Ok(CallToolResult::success(vec![Content::text(out)]))
    }
}
