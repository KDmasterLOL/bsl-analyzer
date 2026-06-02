//! Agent-facing call-graph tool actions over the on-disk [`GraphDb`].
//!
//! These run on a blocking task against a read-only SQLite handle. Domain errors
//! (not found, malformed id) are returned in-band as structured JSON so the agent
//! can react, rather than as transport errors; an infrastructure error (e.g. a
//! failed SQL read) surfaces as an `internal` error object. Each result is wrapped
//! in a freshness [`envelope`] so the agent knows the revision the answer was
//! computed at and whether the workspace has drifted on disk since.

use ide::{Direction, GraphDetail, NeighborsParams};
use rmcp::model::{CallToolResult, Content};
use serde::Serialize;
use serde_json::{json, Value};

use crate::graph::Freshness;
use crate::graph_query::GraphDb;
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

/// An infrastructure failure (e.g. a SQL read error) surfaced in-band so the agent
/// sees a structured error rather than a dropped tool call.
fn internal(e: anyhow::Error) -> Value {
    json!({ "error": "internal", "detail": e.to_string() })
}

pub fn overview(graph: &GraphDb, top: usize) -> Value {
    match graph.overview(top) {
        Ok(overview) => to_value(&overview),
        Err(e) => internal(e),
    }
}

pub fn node(graph: &GraphDb, id: &str, detail: GraphDetail) -> Value {
    match graph.node(id, detail) {
        Ok(Ok(mut result)) => {
            redact_opt(&mut result.node.source);
            to_value(&result)
        }
        Ok(Err(err)) => to_value(&err),
        Err(e) => internal(e),
    }
}

pub fn neighbors(graph: &GraphDb, params: &NeighborsParams<'_>) -> Value {
    match graph.neighbors(params) {
        Ok(Ok(mut result)) => {
            redact_opt(&mut result.root.source);
            for node in &mut result.nodes {
                redact_opt(&mut node.source);
            }
            to_value(&result)
        }
        Ok(Err(err)) => to_value(&err),
        Err(e) => internal(e),
    }
}

pub fn source(graph: &GraphDb, ids: &[String], max_output_tokens: usize) -> Value {
    match graph.source(ids, max_output_tokens) {
        Ok(mut result) => {
            for item in &mut result.items {
                redact_opt(&mut item.source);
            }
            to_value(&result)
        }
        Err(e) => internal(e),
    }
}

/// Wrap an action result in the freshness envelope. `revision` is the snapshot
/// generation the answer was computed at; `stale` flags on-disk drift since then;
/// `reload` is the background-reindex state (`none` / `running` / `failed`).
pub fn envelope(freshness: Freshness, result: Value) -> CallToolResult {
    structured(json!({
        "revision": freshness.revision,
        "stale": freshness.stale,
        "reload": freshness.reload,
        "result": result,
    }))
}

/// Static graph schema for cold-start discovery. Wraps [`schema_json`] in a tool
/// result; the JSON is split out so a test can pin the advertised contract.
pub fn schema() -> CallToolResult {
    structured(schema_json())
}

/// A transient "still indexing" result, emitted while the background load runs.
/// Not an error — the agent should retry shortly.
pub fn loading(detail: Option<&str>) -> CallToolResult {
    let mut body = json!({ "status": "loading" });
    if let Some(detail) = detail {
        body["detail"] = json!(detail);
    }
    structured(body)
}

/// Emit `body` as the MCP `structuredContent` field with a pretty-printed JSON
/// mirror in the text `content` block. Structured-aware clients (code-mode) get a
/// typed object to filter in-sandbox; plain clients still read the JSON text. The
/// payload is byte-identical between the two surfaces, so the agent-facing contract
/// (`schema_version`) is unchanged by carrying it in the native field as well.
fn structured(body: Value) -> CallToolResult {
    let text = serde_json::to_string_pretty(&body)
        .unwrap_or_else(|e| format!("{{\"error\":\"serialize\",\"detail\":\"{e}\"}}"));
    // `CallToolResult::structured` mirrors the value compactly; overwrite the text
    // block with the pretty form so plain-text clients keep the readable layout.
    let mut result = CallToolResult::structured(body);
    result.content = vec![Content::text(text)];
    result
}

/// The agent-facing graph contract: action names, node/edge vocabularies, the
/// neighbours-result and envelope shapes, and the durable id grammar. `schema_version`
/// is the contract version — bump it whenever this response shape changes (it is
/// independent of the on-disk SQLite cache layout in [`crate::graph_db`]).
fn schema_json() -> Value {
    json!({
        "schema_version": "3",
        "actions": ["overview", "schema", "node", "source", "neighbors", "callers", "callees"],
        "node_kinds": ["method", "module", "mdo", "attribute", "form", "form_item"],
        "edge_kinds": ["call", "manager_creates", "manager_access", "query_ref", "contains"],
        "provenance": ["resolved", "inferred", "visibility_blocked", "unresolved"],
        "dispatch": ["client", "server"],
        "neighbors_result": {
            "total": "usize — distinct neighbours discovered, before the max_nodes cap",
            "dropped": "string[] — bounded sample of dropped ids; full count is total - nodes.len()"
        },
        "envelope": {
            "revision": "u64 — snapshot generation the answer was computed at",
            "stale": "bool — workspace drifted on disk since this snapshot",
            "reload": "none | running | failed — background re-index state",
            "result": "the action's payload (or an {error} object)",
            "delivery": "carried both as MCP structuredContent and a mirrored JSON text block; identical payload"
        },
        "id_format": {
            "method_common": "method/common/<Module>/<Method>",
            "method_manager": "method/manager/<MdoEnglish>/<Object>/<Method>",
            "method_object": "method/object/<MdoEnglish>/<Object>/<Method>",
            "method_record_set": "method/recordset/<MdoEnglish>/<Object>/<Method>",
            "module": "module/common/<Module>",
            "mdo": "mdo/<MdoEnglish>/<ObjectName>",
            "attribute": "attribute/<MdoEnglish>/<ObjectName>/<AttrName>",
            "form": "form/<MdoEnglish>/<Object>/<Form> or form/common/<Form>",
            "form_item": "form_item/<MdoEnglish>/<Object>/<Form>/<Item> or form_item/common/<Form>/<Item>",
            "path_fallback": "method/file/<relpath>::<Method>"
        }
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_advertises_the_current_contract_shape() {
        let schema = schema_json();
        // The contract version must be bumped in lockstep with response-shape
        // changes; `total` is part of that shape since this revision, and `form`/
        // `form_item` node kinds + the `contains` edge kind since version 3.
        assert_eq!(schema["schema_version"], "3");
        assert!(
            schema["neighbors_result"]["total"].is_string(),
            "neighbours result must document the `total` field"
        );
        let node_kinds = schema["node_kinds"].as_array().unwrap();
        assert!(node_kinds.iter().any(|k| k == "form"));
        assert!(node_kinds.iter().any(|k| k == "form_item"));
        assert!(schema["edge_kinds"].as_array().unwrap().iter().any(|k| k == "contains"));
    }

    /// The text content block must parse back to exactly the `structuredContent`
    /// field, so structured-aware and plain clients see byte-identical JSON.
    fn assert_structured_mirrors_text(result: &CallToolResult) {
        let structured =
            result.structured_content.as_ref().expect("structuredContent must be populated");
        let text = result.content[0].raw.as_text().expect("text mirror").text.as_str();
        let parsed: Value = serde_json::from_str(text).expect("text mirror must be valid JSON");
        assert_eq!(&parsed, structured, "text mirror must match structuredContent");
    }

    #[test]
    fn envelope_populates_structured_content() {
        let freshness = Freshness { revision: 7, stale: true, reload: "running" };
        let result = envelope(freshness, json!({ "kind": "method", "name": "Считать" }));
        assert_structured_mirrors_text(&result);
        let body = result.structured_content.as_ref().unwrap();
        assert_eq!(body["revision"], 7);
        assert_eq!(body["stale"], true);
        assert_eq!(body["reload"], "running");
        assert_eq!(body["result"]["name"], "Считать");
        assert_eq!(result.is_error, Some(false));
    }

    #[test]
    fn schema_and_loading_populate_structured_content() {
        assert_structured_mirrors_text(&schema());
        assert_eq!(schema().structured_content.unwrap()["schema_version"], "3");

        assert_structured_mirrors_text(&loading(Some("indexing")));
        let body = loading(Some("indexing")).structured_content.unwrap();
        assert_eq!(body["status"], "loading");
        assert_eq!(body["detail"], "indexing");

        assert_eq!(loading(None).structured_content.unwrap().get("detail"), None);
    }
}
