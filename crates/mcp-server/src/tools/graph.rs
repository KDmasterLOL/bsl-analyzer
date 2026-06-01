//! Agent-facing call-graph tool actions over [`ide::Analysis::graph_*`].
//!
//! These run on a blocking task with a Salsa snapshot. Domain errors (not found,
//! malformed id) are returned in-band as structured JSON so the agent can react,
//! rather than as transport errors. Each result is wrapped in a freshness
//! [`envelope`] so the agent knows the revision the answer was computed at and
//! whether the workspace has drifted on disk since.

use std::path::Path;

use ide::{Analysis, Direction, GraphDetail, NeighborsParams};
use rmcp::model::{CallToolResult, Content};
use serde::Serialize;
use serde_json::{json, Value};

use crate::graph::{Freshness, GRAPH_SOURCE_ROOT};
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

pub fn overview(analysis: &Analysis, workspace_root: Option<&Path>, top: usize) -> Value {
    let overview = analysis.graph_overview(GRAPH_SOURCE_ROOT, workspace_root, top);
    to_value(&overview)
}

pub fn node(
    analysis: &Analysis,
    workspace_root: Option<&Path>,
    id: &str,
    detail: GraphDetail,
) -> Value {
    match analysis.graph_node(GRAPH_SOURCE_ROOT, workspace_root, id, detail) {
        Ok(mut result) => {
            redact_opt(&mut result.node.source);
            to_value(&result)
        }
        Err(err) => to_value(&err),
    }
}

pub fn neighbors(
    analysis: &Analysis,
    workspace_root: Option<&Path>,
    params: &NeighborsParams<'_>,
) -> Value {
    match analysis.graph_neighbors(GRAPH_SOURCE_ROOT, workspace_root, params) {
        Ok(mut result) => {
            redact_opt(&mut result.root.source);
            for node in &mut result.nodes {
                redact_opt(&mut node.source);
            }
            to_value(&result)
        }
        Err(err) => to_value(&err),
    }
}

pub fn source(
    analysis: &Analysis,
    workspace_root: Option<&Path>,
    ids: &[String],
    max_output_tokens: usize,
) -> Value {
    let mut result =
        analysis.graph_source(GRAPH_SOURCE_ROOT, workspace_root, ids, max_output_tokens);
    for item in &mut result.items {
        redact_opt(&mut item.source);
    }
    to_value(&result)
}

/// Wrap an action result in the freshness envelope. `revision` is the snapshot
/// generation the answer was computed at; `stale` flags on-disk drift since then;
/// `reload` is the background-reindex state (`none` / `running` / `failed`).
pub fn envelope(freshness: Freshness, result: Value) -> CallToolResult {
    let body = json!({
        "revision": freshness.revision,
        "stale": freshness.stale,
        "reload": freshness.reload,
        "result": result,
    });
    let text = serde_json::to_string_pretty(&body)
        .unwrap_or_else(|e| format!("{{\"error\":\"serialize\",\"detail\":\"{e}\"}}"));
    CallToolResult::success(vec![Content::text(text)])
}

/// Static graph schema for cold-start discovery.
pub fn schema() -> CallToolResult {
    let schema = json!({
        "schema_version": "1",
        "actions": ["overview", "schema", "node", "source", "neighbors", "callers", "callees"],
        "node_kinds": ["method", "module", "mdo"],
        "edge_kinds": ["call", "manager_creates", "manager_access", "query_ref"],
        "provenance": ["resolved", "inferred", "visibility_blocked", "unresolved"],
        "dispatch": ["client", "server"],
        "envelope": {
            "revision": "u64 — snapshot generation the answer was computed at",
            "stale": "bool — workspace drifted on disk since this snapshot",
            "reload": "none | running | failed — background re-index state",
            "result": "the action's payload (or an {error} object)"
        },
        "id_format": {
            "method_common": "method/common/<Module>/<Method>",
            "method_manager": "method/manager/<MdoEnglish>/<Object>/<Method>",
            "method_object": "method/object/<MdoEnglish>/<Object>/<Method>",
            "method_record_set": "method/recordset/<MdoEnglish>/<Object>/<Method>",
            "module": "module/common/<Module>",
            "mdo": "mdo/<MdoEnglish>/<ObjectName>",
            "path_fallback": "method/file/<relpath>::<Method>"
        }
    });
    let text = serde_json::to_string_pretty(&schema)
        .unwrap_or_else(|e| format!("{{\"error\":\"serialize\",\"detail\":\"{e}\"}}"));
    CallToolResult::success(vec![Content::text(text)])
}

fn to_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value)
        .unwrap_or_else(|e| json!({ "error": "serialize", "detail": e.to_string() }))
}

fn redact_opt(source: &mut Option<String>) {
    if let Some(text) = source {
        *text = redact_secrets(text);
    }
}
