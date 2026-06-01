//! Agent-facing call-graph tool actions over [`ide::Analysis::graph_*`].
//!
//! These run on a blocking task with a Salsa snapshot. Domain errors (not found,
//! malformed id) are returned in-band as structured JSON so the agent can react,
//! rather than as transport errors.

use std::path::Path;

use ide::{Analysis, Direction, GraphDetail, NeighborsParams};
use rmcp::model::{CallToolResult, Content};
use serde::Serialize;
use serde_json::json;

use crate::graph::GRAPH_SOURCE_ROOT;
use crate::tools::redact::redact_secrets;

pub fn detail_from(s: Option<&str>) -> GraphDetail {
    match s {
        Some("names") => GraphDetail::Names,
        Some("bodies") => GraphDetail::Bodies,
        _ => GraphDetail::Signatures,
    }
}

pub fn direction_from(s: Option<&str>) -> Direction {
    match s {
        Some("out") => Direction::Out,
        Some("both") => Direction::Both,
        _ => Direction::In,
    }
}

pub fn overview(analysis: &Analysis, workspace_root: Option<&Path>, top: usize) -> CallToolResult {
    let overview = analysis.graph_overview(GRAPH_SOURCE_ROOT, workspace_root, top);
    json_result(&overview)
}

pub fn node(
    analysis: &Analysis,
    workspace_root: Option<&Path>,
    id: &str,
    detail: GraphDetail,
) -> CallToolResult {
    match analysis.graph_node(GRAPH_SOURCE_ROOT, workspace_root, id, detail) {
        Ok(mut result) => {
            redact_opt(&mut result.node.source);
            json_result(&result)
        }
        Err(err) => json_result(&err),
    }
}

pub fn neighbors(
    analysis: &Analysis,
    workspace_root: Option<&Path>,
    params: &NeighborsParams<'_>,
) -> CallToolResult {
    match analysis.graph_neighbors(GRAPH_SOURCE_ROOT, workspace_root, params) {
        Ok(mut result) => {
            redact_opt(&mut result.root.source);
            for node in &mut result.nodes {
                redact_opt(&mut node.source);
            }
            json_result(&result)
        }
        Err(err) => json_result(&err),
    }
}

pub fn source(
    analysis: &Analysis,
    workspace_root: Option<&Path>,
    ids: &[String],
    max_output_tokens: usize,
) -> CallToolResult {
    let mut result =
        analysis.graph_source(GRAPH_SOURCE_ROOT, workspace_root, ids, max_output_tokens);
    for item in &mut result.items {
        redact_opt(&mut item.source);
    }
    json_result(&result)
}

/// Static graph schema for cold-start discovery.
pub fn schema() -> CallToolResult {
    let schema = json!({
        "schema_version": "1",
        "actions": ["overview", "schema", "node", "source", "neighbors", "callers", "callees"],
        "node_kinds": ["method", "module"],
        "edge_kinds": ["call"],
        "provenance": ["resolved", "inferred", "visibility_blocked", "unresolved"],
        "dispatch": ["client", "server"],
        "id_format": {
            "method_common": "method/common/<Module>/<Method>",
            "method_manager": "method/manager/<MdoEnglish>/<Object>/<Method>",
            "method_object": "method/object/<MdoEnglish>/<Object>/<Method>",
            "method_record_set": "method/recordset/<MdoEnglish>/<Object>/<Method>",
            "module": "module/common/<Module>",
            "path_fallback": "method/file/<relpath>::<Method>"
        }
    });
    json_result(&schema)
}

fn json_result<T: Serialize>(value: &T) -> CallToolResult {
    let text = serde_json::to_string_pretty(value)
        .unwrap_or_else(|e| format!("{{\"error\":\"serialize\",\"detail\":\"{e}\"}}"));
    CallToolResult::success(vec![Content::text(text)])
}

fn redact_opt(source: &mut Option<String>) {
    if let Some(text) = source {
        *text = redact_secrets(text);
    }
}
