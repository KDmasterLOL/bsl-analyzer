//! Agent-facing call-graph tool actions over the on-disk [`GraphDb`].
//!
//! These run on a blocking task against a read-only SQLite handle. Domain errors
//! (not found, malformed id) are returned in-band as structured JSON so the agent
//! can react, rather than as transport errors; an infrastructure error (e.g. a
//! failed SQL read) surfaces as an `internal` error object. Each result is wrapped
//! in a freshness [`envelope`] so the agent knows the revision the answer was
//! computed at and whether the workspace has drifted on disk since.

use ide::{Direction, GraphDetail, NeighborsParams};
use rmcp::model::CallToolResult;
use serde::Serialize;
use serde_json::{json, Value};

use crate::graph::Freshness;
use crate::graph_query::GraphDb;
use crate::tools::redact::redact_secrets;
use crate::tools::response::structured;

/// Parse the `detail` enum. `None` keeps the default (`signatures`); an unknown value is
/// rejected (mirrors `diagnostics::parse_min_severity`) so a caller is never silently served
/// a different view than it asked for.
pub fn detail_from(s: Option<&str>) -> Result<GraphDetail, String> {
    match s {
        None | Some("signatures") => Ok(GraphDetail::Signatures),
        Some("names") => Ok(GraphDetail::Names),
        Some("bodies") => Ok(GraphDetail::Bodies),
        Some(other) => Err(format!("unknown detail '{other}'; expected names|signatures|bodies")),
    }
}

/// Parse the `dir` enum. `None` keeps the default (`in`); an unknown value is rejected so a
/// caller is never silently given the opposite traversal direction.
pub fn direction_from(s: Option<&str>) -> Result<Direction, String> {
    match s {
        None | Some("in") => Ok(Direction::In),
        Some("out") => Ok(Direction::Out),
        Some("both") => Ok(Direction::Both),
        Some(other) => Err(format!("unknown dir '{other}'; expected in|out|both")),
    }
}

/// The agent-facing edge-kind labels accepted by the `edge_kinds` neighbour filter.
const EDGE_KINDS: [&str; 6] =
    ["call", "manager_creates", "manager_access", "query_ref", "contains", "data_binding"];

/// Validate an `edge_kinds` filter: every entry must be a known edge-kind label, so a
/// typo fails fast rather than silently matching nothing.
pub fn validate_edge_kinds(kinds: &[String]) -> Result<(), String> {
    for k in kinds {
        if !EDGE_KINDS.contains(&k.as_str()) {
            return Err(format!(
                "unknown edge_kind '{k}'; expected one of {}",
                EDGE_KINDS.join("|")
            ));
        }
    }
    Ok(())
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

/// The agent-facing graph contract: action names, node/edge vocabularies, the
/// neighbours-result and envelope shapes, and the durable id grammar. `schema_version`
/// is the contract version — bump it whenever this response shape changes (it is
/// independent of the on-disk SQLite cache layout in [`crate::graph_db`]).
fn schema_json() -> Value {
    json!({
        "schema_version": "9",
        "actions": ["overview", "schema", "node", "source", "neighbors", "callers", "callees"],
        "node_kinds": ["method", "module", "mdo", "attribute", "tabular_section", "form", "form_item", "form_attribute"],
        "notes": "since version 7 `node(module/<scope>)` resolves for any code module and returns a `methods` array ({id, name, is_export}) of the module's members; module membership is served on demand and is not a graph edge, so `neighbors(module/…)` stays empty",
        "edge_kinds": ["call", "manager_creates", "manager_access", "query_ref", "contains", "data_binding"],
        "provenance": ["resolved", "inferred", "visibility_blocked", "unresolved"],
        "dispatch": ["client", "server"],
        "neighbors_params": {
            "provenance": "string[] — keep only edges with these provenances (empty = all)",
            "edge_kinds": "string[] — keep only edges of these kinds (call|manager_creates|manager_access|query_ref|contains|data_binding); empty = all. Combine with provenance to isolate e.g. only query_ref metadata impact"
        },
        "neighbors_result": {
            "total": "usize — distinct neighbours discovered, before the max_nodes cap",
            "returned": "usize — neighbours returned in `nodes` (after the cap)",
            "dropped_count": "usize — neighbours dropped by the cap (total - returned)",
            "dropped": "string[] — bounded sample of the dropped ids (highest-centrality first)"
        },
        "redaction": "method source/snippets emitted by `node`/`neighbors`/`source` (and search) are secret-redacted: values that look like credentials are replaced with `***`. Structural string literals (field lists, query fragments) may also be masked; treat source as sanitized, not byte-exact.",
        "revision_note": "the graph `revision` is independent of the `diagnostics` tool's `generation` — they are separate subsystems with separate freshness counters and do not correlate.",
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
            "tabular_section": "tabular_section/<MdoEnglish>/<ObjectName>/<Section>",
            "tabular_section_attribute": "ts_attr/<MdoEnglish>/<ObjectName>/<Section>/<AttrName> (node kind: attribute)",
            "form": "form/<MdoEnglish>/<Object>/<Form> or form/common/<Form>",
            "form_item": "form_item/<MdoEnglish>/<Object>/<Form>/<Item> or form_item/common/<Form>/<Item>",
            "form_attribute": "form_attr/<MdoEnglish>/<Object>/<Form>/<Attr> or form_attr/common/<Form>/<Attr>",
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
    fn detail_from_defaults_then_rejects_unknown() {
        assert_eq!(detail_from(None), Ok(GraphDetail::Signatures));
        assert_eq!(detail_from(Some("signatures")), Ok(GraphDetail::Signatures));
        assert_eq!(detail_from(Some("names")), Ok(GraphDetail::Names));
        assert_eq!(detail_from(Some("bodies")), Ok(GraphDetail::Bodies));
        // An unknown value errors rather than silently defaulting.
        let err = detail_from(Some("everything")).unwrap_err();
        assert!(err.contains("everything") && err.contains("names|signatures|bodies"), "{err}");
    }

    #[test]
    fn validate_edge_kinds_accepts_known_rejects_unknown() {
        assert!(validate_edge_kinds(&[]).is_ok());
        assert!(validate_edge_kinds(&["query_ref".to_owned(), "contains".to_owned()]).is_ok());
        let err = validate_edge_kinds(&["calls".to_owned()]).unwrap_err();
        assert!(err.contains("calls") && err.contains("query_ref"), "{err}");
    }

    #[test]
    fn direction_from_defaults_then_rejects_unknown() {
        assert_eq!(direction_from(None), Ok(Direction::In));
        assert_eq!(direction_from(Some("in")), Ok(Direction::In));
        assert_eq!(direction_from(Some("out")), Ok(Direction::Out));
        assert_eq!(direction_from(Some("both")), Ok(Direction::Both));
        // An unknown value errors rather than silently giving the default direction.
        let err = direction_from(Some("sideways")).unwrap_err();
        assert!(err.contains("sideways") && err.contains("in|out|both"), "{err}");
    }

    #[test]
    fn schema_advertises_the_current_contract_shape() {
        let schema = schema_json();
        // The contract version must be bumped in lockstep with response-shape
        // changes; `total` since this revision, `form`/`form_item` + `contains` since
        // version 3, `form_attribute` since version 4, `tabular_section` since version
        // 5, and the `data_binding` edge since version 6.
        assert_eq!(schema["schema_version"], "9");
        assert!(
            schema["neighbors_result"]["total"].is_string(),
            "neighbours result must document the `total` field"
        );
        let node_kinds = schema["node_kinds"].as_array().unwrap();
        assert!(node_kinds.iter().any(|k| k == "form"));
        assert!(node_kinds.iter().any(|k| k == "form_item"));
        assert!(node_kinds.iter().any(|k| k == "form_attribute"));
        assert!(node_kinds.iter().any(|k| k == "tabular_section"));
        let edge_kinds = schema["edge_kinds"].as_array().unwrap();
        assert!(edge_kinds.iter().any(|k| k == "contains"));
        assert!(edge_kinds.iter().any(|k| k == "data_binding"));
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
        assert_eq!(schema().structured_content.unwrap()["schema_version"], "9");

        assert_structured_mirrors_text(&loading(Some("indexing")));
        let body = loading(Some("indexing")).structured_content.unwrap();
        assert_eq!(body["status"], "loading");
        assert_eq!(body["detail"], "indexing");

        assert_eq!(loading(None).structured_content.unwrap().get("detail"), None);
    }
}
