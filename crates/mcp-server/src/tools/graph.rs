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
const EDGE_KINDS: [&str; 8] = [
    "call",
    "manager_creates",
    "manager_access",
    "query_ref",
    "contains",
    "data_binding",
    "notify_ref",
    "idle_handler",
];

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

/// Default cap on the candidate ids returned by `resolve`.
pub const DEFAULT_RESOLVE_LIMIT: usize = 20;

pub fn resolve(graph: &GraphDb, query: &str, limit: usize) -> Value {
    match graph.resolve(query, limit) {
        Ok(result) => to_value(&result),
        Err(e) => internal(e),
    }
}

/// Default body-output budget (in tokens, ~4 chars each) for `node`/`neighbors` at
/// `detail=bodies`, so a `bodies` request can never return an unbounded payload the way an
/// uncapped manager-method body could. Overridable via `max_output_tokens`.
pub const DEFAULT_BODY_BUDGET_TOKENS: usize = 6000;

/// Truncate `source` to the `remaining` char budget on a char boundary, decrementing the
/// budget. Returns `true` if it had to truncate (or drop) the body. `None`/empty sources
/// (the non-`bodies` details) consume nothing.
fn clamp_to_budget(source: &mut Option<String>, remaining: &mut usize) -> bool {
    let Some(text) = source else { return false };
    if text.len() <= *remaining {
        *remaining -= text.len();
        return false;
    }
    let mut end = *remaining;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    if end == 0 {
        // No budget left for even a partial body — drop the field entirely rather than emit an
        // empty string that reads like a method with no body. The response's `budget_exhausted`
        // flag signals why the source is absent.
        *source = None;
    } else {
        text.truncate(end);
    }
    *remaining = 0;
    true
}

pub fn node(graph: &GraphDb, id: &str, detail: GraphDetail, max_output_tokens: usize) -> Value {
    match graph.node(id, detail) {
        Ok(Ok(mut result)) => {
            redact_opt(&mut result.node.source);
            let mut remaining = max_output_tokens.saturating_mul(4);
            let truncated = clamp_to_budget(&mut result.node.source, &mut remaining);
            let mut value = to_value(&result);
            if truncated {
                value["budget_exhausted"] = json!(true);
            }
            value
        }
        Ok(Err(err)) => to_value(&err),
        Err(e) => internal(e),
    }
}

pub fn neighbors(graph: &GraphDb, params: &NeighborsParams<'_>, max_output_tokens: usize) -> Value {
    match graph.neighbors(params) {
        Ok(Ok(mut result)) => {
            redact_opt(&mut result.root.source);
            for node in &mut result.nodes {
                redact_opt(&mut node.source);
            }
            // Cumulative body budget across the root then the (centrality-ordered) nodes,
            // so a `detail=bodies` traversal stays within the output budget.
            let mut remaining = max_output_tokens.saturating_mul(4);
            let mut truncated = clamp_to_budget(&mut result.root.source, &mut remaining);
            for node in &mut result.nodes {
                truncated |= clamp_to_budget(&mut node.source, &mut remaining);
            }
            let mut value = to_value(&result);
            if truncated {
                value["budget_exhausted"] = json!(true);
            }
            value
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

/// The `status` action: the graph's lifecycle snapshot (mirrors `diagnostics status`), so an
/// agent can start the lazy build and poll its progress instead of reading a flat `loading`
/// envelope from a data action.
pub fn status(report: &crate::graph::GraphStatusReport) -> CallToolResult {
    let mut body = json!({ "state": report.state });
    if let Some(files) = report.files {
        body["files"] = json!(files);
    }
    if let Some(revision) = report.revision {
        body["revision"] = json!(revision);
    }
    if let Some(stale) = report.stale {
        body["stale"] = json!(stale);
    }
    if let Some(reload) = report.reload {
        body["reload"] = json!(reload);
    }
    if let Some(error) = &report.error {
        body["error"] = json!(error);
    }
    structured(body)
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
        "schema_version": "16",
        "actions": ["overview", "schema", "status", "node", "source", "neighbors", "callers", "callees", "resolve"],
        "status": "since version 15 `status` returns the graph lifecycle ({state: disabled|loading|ready|failed, and when ready: files, revision, stale, reload}) and kicks the lazy build — poll it instead of reading a flat `loading` envelope from a data action (mirrors `diagnostics status`).",
        "node_kinds": ["method", "module", "mdo", "attribute", "tabular_section", "form", "form_item", "form_attribute"],
        "notes": "since version 7 `node(module/<scope>)` resolves for any code module and returns a `methods` array ({id, name, is_export}) of the module's members; module membership is served on demand and is not a graph edge, so `neighbors(module/…)` stays empty",
        "resolve": "since version 13 `resolve(query)` returns candidate durable ids ({id, kind, match}) for an imprecise query — wrong casing, a bare method/object name, or a partial id — so a `not_found` from `node`/`neighbors` is recoverable without guessing. `match` is exact|case_insensitive|name|substring (strongest first); the list is capped (default 20). It is symbol/id-oriented, NOT a natural-language search: a free-text phrase (e.g. several object/form/method words) returns no candidates — use `search_code` for semantic lookup, then pass the emitted `graph_id` here.",
        "edge_kinds": ["call", "manager_creates", "manager_access", "query_ref", "contains", "data_binding", "notify_ref", "idle_handler"],
        "edge_kinds_note": "since version 16 string-dispatched callbacks are edges: `notify_ref` (Новый ОписаниеОповещения) and `idle_handler` (ПодключитьОбработчикОжидания) link a method to the handler named by a string literal. They carry `string_resolved` provenance and are kept separate from `call`, so `edge_kinds=[call]` stays a pure 'who really calls whom'. Resolution is conservative: only ЭтотОбъект/ЭтаФорма handlers and explicit common-module handlers resolve (idle handlers resolve in the current module only — a handler placed in another allowed module is intentionally not modelled, not guessed); unresolved receivers produce no edge.",
        "provenance": ["resolved", "inferred", "visibility_blocked", "unresolved", "string_resolved"],
        "dispatch": ["client", "server"],
        "neighbors_params": {
            "provenance": "string[] — keep only edges with these provenances (empty = all)",
            "edge_kinds": "string[] — keep only edges of these kinds (call|manager_creates|manager_access|query_ref|contains|data_binding|notify_ref|idle_handler); empty = all. Combine with provenance to isolate e.g. only query_ref metadata impact"
        },
        "neighbors_result": {
            "total": "usize — distinct neighbours discovered, before the max_nodes cap",
            "returned": "usize — neighbours returned in `nodes` (after the cap)",
            "dropped_count": "usize — neighbours dropped by the cap (total - returned)",
            "dropped": "string[] — bounded sample of the dropped ids (highest-centrality first)",
            "by_kind": "{ kind: count } — edge-kind distribution of the full neighbourhood (before the cap), to size an edge_kinds filter",
            "by_provenance": "{ provenance: count } — same distribution by provenance",
            "connectors_dropped": "bool — true when the cap dropped a node that was an edge endpoint, so some edges are omitted (nodes may appear without their connecting edge)",
            "out_total": "usize — distinct callees discovered (present for dir=out/both); a small value under dir=both means few outbound calls even when inbound callers fill the cap — refine with dir=out",
            "in_total": "usize — distinct callers discovered (present for dir=in/both)"
        },
        "body_budget": "`node`/`neighbors` at detail=bodies cap cumulative source output at max_output_tokens (~4 chars/token; default 6000); a truncated response carries `budget_exhausted: true`. A body fully starved by the budget is omitted (its `source` field is absent), not emitted as an empty string. In `source`, an item skipped because an earlier item exhausted the budget carries `skipped_budget_exhausted: true` (distinct from a method with no body) — retry it with a larger budget or alone.",
        "redaction": "method source/snippets emitted by `node`/`neighbors`/`source` (and search) are secret-redacted: a string literal is replaced with `***` when a sensitive marker (a credential-named identifier like `Токен`, or a key like `Вставить(\"Пароль\", …)`) precedes it in the same statement. Structural literals (field lists, type names) and localized messages are preserved; treat source as sanitized, not byte-exact.",
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
    fn status_formats_ready_and_failed_shapes() {
        use crate::graph::GraphStatusReport;
        let ready = GraphStatusReport {
            state: "ready",
            files: Some(10),
            revision: Some(3),
            stale: Some(false),
            reload: Some("none"),
            error: None,
        };
        let body = status(&ready).structured_content.unwrap();
        assert_eq!(body["state"], "ready");
        assert_eq!(body["files"], 10);
        assert_eq!(body["revision"], 3);
        assert_eq!(body["reload"], "none");
        assert!(body.get("error").is_none(), "ready has no error: {body}");

        let failed = GraphStatusReport {
            state: "failed",
            files: None,
            revision: None,
            stale: None,
            reload: None,
            error: Some("boom".to_string()),
        };
        let body = status(&failed).structured_content.unwrap();
        assert_eq!(body["state"], "failed");
        assert_eq!(body["error"], "boom");
        // Unset fields are omitted, never null.
        assert!(body.get("files").is_none() && body.get("revision").is_none(), "{body}");
    }

    #[test]
    fn status_reports_disabled_for_non_workspace_graph() {
        let report = crate::graph::GraphState::disabled().status_report();
        let body = status(&report).structured_content.unwrap();
        assert_eq!(body["state"], "disabled");
        assert!(body.get("files").is_none());
    }

    #[test]
    fn schema_advertises_the_current_contract_shape() {
        let schema = schema_json();
        // The contract version must be bumped in lockstep with response-shape
        // changes; `total` since this revision, `form`/`form_item` + `contains` since
        // version 3, `form_attribute` since version 4, `tabular_section` since version
        // 5, the `data_binding` edge since version 6, the source `skipped_budget_exhausted`
        // marker + omit-empty body since version 14, the `status` action since version 15,
        // and the `notify_ref`/`idle_handler` callback edges since version 16.
        assert_eq!(schema["schema_version"], "16");
        assert!(
            schema["neighbors_result"]["total"].is_string(),
            "neighbours result must document the `total` field"
        );
        let actions = schema["actions"].as_array().unwrap();
        assert!(actions.iter().any(|a| a == "resolve"), "resolve action must be advertised");
        assert!(actions.iter().any(|a| a == "status"), "status action must be advertised");
        assert!(schema["status"].is_string(), "status must be documented");
        assert!(schema["resolve"].is_string(), "resolve must be documented");
        let node_kinds = schema["node_kinds"].as_array().unwrap();
        assert!(node_kinds.iter().any(|k| k == "form"));
        assert!(node_kinds.iter().any(|k| k == "form_item"));
        assert!(node_kinds.iter().any(|k| k == "form_attribute"));
        assert!(node_kinds.iter().any(|k| k == "tabular_section"));
        let edge_kinds = schema["edge_kinds"].as_array().unwrap();
        assert!(edge_kinds.iter().any(|k| k == "contains"));
        assert!(edge_kinds.iter().any(|k| k == "data_binding"));
        assert!(edge_kinds.iter().any(|k| k == "notify_ref"));
        assert!(edge_kinds.iter().any(|k| k == "idle_handler"));
        let provenance = schema["provenance"].as_array().unwrap();
        assert!(provenance.iter().any(|p| p == "string_resolved"));
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
        assert_eq!(schema().structured_content.unwrap()["schema_version"], "16");

        assert_structured_mirrors_text(&loading(Some("indexing")));
        let body = loading(Some("indexing")).structured_content.unwrap();
        assert_eq!(body["status"], "loading");
        assert_eq!(body["detail"], "indexing");

        assert_eq!(loading(None).structured_content.unwrap().get("detail"), None);
    }
}
