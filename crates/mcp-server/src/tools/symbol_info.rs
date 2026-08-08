//! Agent-facing `symbol_info` tool: a consolidated semantic card for ONE symbol.
//!
//! Thin projection over [`ide::symbol_info`]. The resident host owns all BSL semantics
//! (kind, signature, type, doc, definition); this adapter bridges the resolved symbol to the
//! call graph for the `usages` summary and, on a resident miss, offers graph-derived
//! candidates. The core card is served whenever the resident is `Ready`, independent of the
//! graph's readiness.

use std::path::Path;

use ide::{Locale, SymbolInfoCard, SymbolInfoRequest, SymbolInfoSections, SymbolPosition};
use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;
use serde_json::{json, Map, Value};

use crate::diagnostics_state::DiagnosticsResident;
use crate::graph_query::GraphDb;
use crate::tools::response::{structured, trim_items_to_budget, truncate_text_to_budget};

/// Default number of top calling modules included in the `usages` summary.
pub(crate) const DEFAULT_TOP_MODULES: usize = 5;

/// Default cap on graph candidates offered on a resident miss.
pub(crate) const DEFAULT_CANDIDATE_LIMIT: usize = 20;

/// Interpret the `include` section filter. An empty filter means "all sections"; unknown
/// entries are ignored (a superset request never narrows below the named sections).
pub(crate) fn sections_from(include: &[String]) -> SymbolInfoSections {
    if include.is_empty() {
        return SymbolInfoSections::all();
    }
    let has = |name: &str| include.iter().any(|s| s.eq_ignore_ascii_case(name));
    SymbolInfoSections { definition: has("definition"), type_: has("type"), doc: has("doc") }
}

/// Parse the optional locale (`ru`/`en`), defaulting to the project default.
pub(crate) fn locale_from(raw: Option<&str>) -> Result<Locale, McpError> {
    match raw {
        Some(s) => {
            Locale::from_config_str(s).map_err(|e| McpError::invalid_params(e.to_string(), None))
        }
        None => Ok(Locale::default()),
    }
}

/// Resolve the semantic card on the resident host. Runs inside the resident read lock.
/// `Ok(None)` means the resident could not resolve the request — the caller then offers graph
/// candidates rather than a hard error. A malformed positional request is a param error.
#[allow(
    clippy::too_many_arguments,
    reason = "one argument per declared tool parameter; a grouping struct here would have to be \
              built at the single call site and taken apart again in the first statement"
)]
pub(crate) fn resolve_card(
    resident: &DiagnosticsResident,
    symbol: Option<&str>,
    root_id: Option<&str>,
    path: Option<&str>,
    line: Option<u32>,
    column: Option<u32>,
    sections: SymbolInfoSections,
    locale: Locale,
) -> Result<Option<SymbolInfoCard>, McpError> {
    let position = match path {
        Some(path) => {
            // Resolved ONCE and bound to the same name, so the unreadable branch below reads
            // the same file this resolves to rather than the same spelling the caller sent.
            let path = resident
                .resolve_rooted_path(root_id, Path::new(path))
                .map_err(|error| McpError::invalid_params(error.to_string(), None))?;
            let path = path.as_path();
            let file_id = resident.file_id_for(path).ok_or_else(|| {
                // Same split as `diagnostics file`: an existing but unreadable file
                // gets its own answer rather than being called a non-workspace path.
                if resident.is_unread(path) {
                    McpError::invalid_params(
                        format!(
                            "'{}' is a workspace .bsl file whose bytes could not be read; \
                             it is held out of service and re-read every drift window",
                            path.display()
                        ),
                        None,
                    )
                } else {
                    McpError::invalid_params(
                        format!("'{}' is not a resident workspace .bsl file", path.display()),
                        None,
                    )
                }
            })?;
            let line = line
                .ok_or_else(|| McpError::invalid_params("'line' is required with 'path'", None))?;
            let column = column.unwrap_or(0);
            Some(SymbolPosition { file_id, line, column })
        }
        None => {
            // `root_id` qualifies `path` and nothing else. Accepting it silently for a
            // symbol-only request would make the same value a hard error in one shape and a
            // no-op in the other — and an agent forwarding a hit's root alongside its symbol
            // would get a card resolved by name, from whichever root owns that name.
            if root_id.is_some() {
                return Err(McpError::invalid_params(
                    "'root_id' spells out which root 'path' is relative to, so it needs a \
                     'path'; a 'symbol' is resolved by name and belongs to no root"
                        .to_owned(),
                    None,
                ));
            }
            None
        }
    };

    if symbol.is_none() && position.is_none() {
        return Err(McpError::invalid_params("one of 'symbol' or 'path'+'line' is required", None));
    }

    let req = SymbolInfoRequest {
        symbol: symbol.map(str::to_string),
        position,
        locale,
        sections,
        // The graph was built against the resident's workspace root; a form handler's path-fallback
        // graph id must be encoded relative to it (NOT the config root) to resolve its usages.
        workspace_root: Some(resident.workspace_root().to_path_buf()),
    };
    Ok(ide::symbol_info(resident.db(), &req))
}

/// Serialize a resolved card, enriching it with a graph-derived `usages` summary when the
/// graph is available. `graph` is `None` when the call graph is not `Ready` — the core card is
/// still returned, stamped with `usages_unavailable`. The symbol is bridged to the graph by the
/// durable id the resident resolution already produced (`card.graph_id`), so no fuzzy
/// re-resolution is needed and a dotted `Module.Method` name resolves its fan-in reliably.
pub(crate) fn render_card(
    card: &SymbolInfoCard,
    graph: Option<&GraphDb>,
    top_modules: usize,
    max_output_tokens: usize,
) -> CallToolResult {
    let mut body = card_to_value(card, max_output_tokens);

    if !card.semantics_unavailable {
        match (card.graph_id.as_deref(), graph) {
            (Some(graph_id), Some(graph)) => match usages(graph, graph_id, top_modules) {
                Some(u) => {
                    body["usages"] = u;
                }
                None => {
                    body["usages_unavailable"] = json!("symbol not in call graph");
                }
            },
            // Cards without a graph id (metadata objects/attributes, platform members) carry no
            // usages summary; only note the graph being unavailable for graph-addressable symbols.
            (Some(_), None) => {
                body["usages_unavailable"] = json!("graph indexing");
            }
            (None, _) => {}
        }
    }

    structured(body)
}

/// The resident-miss response: graph-derived candidates (imprecise-name path). When the graph
/// is not ready, returns a hint to retry rather than an empty list.
pub(crate) fn render_not_found(
    symbol: &str,
    graph: Option<&GraphDb>,
    limit: usize,
) -> CallToolResult {
    let Some(graph) = graph else {
        return structured(json!({
            "resolved": false,
            "symbol": symbol,
            "candidates": [],
            "hint": "call graph is still indexing; retry shortly for candidate matches",
        }));
    };
    match graph.resolve(symbol, limit) {
        Ok(result) => {
            // Candidates carry DURABLE GRAPH IDS (`method/common/…`), not `symbol_info` qualified
            // names — they are not re-feedable as `symbol` (resident parsing expects a dotted BSL
            // name). Expose them as `id` and point the agent at `graph`, or at refining the name.
            let candidates: Vec<Value> = result
                .candidates
                .iter()
                .map(|c| json!({ "id": c.id, "kind": c.kind, "match": c.match_kind }))
                .collect();
            structured(json!({
                "resolved": false,
                "symbol": symbol,
                "candidates": candidates,
                "total": result.total,
                "truncated": result.truncated,
                "hint": "no exact resident match; open a candidate id in `graph`, or refine to a \
                         qualified BSL name for `symbol_info`",
            }))
        }
        Err(e) => structured(json!({
            "resolved": false,
            "symbol": symbol,
            "candidates": [],
            "error": "internal",
            "detail": e.to_string(),
        })),
    }
}

fn usages(graph: &GraphDb, graph_id: &str, top_modules: usize) -> Option<Value> {
    let summary = graph.usages(graph_id, top_modules).ok()??;
    let top: Vec<Value> = summary
        .top_modules
        .iter()
        .map(|(module, count)| json!({ "module": module, "count": count }))
        .collect();
    let mut value = json!({
        "count": summary.count,
        "top_modules": top,
        "graph_id": graph_id,
    });
    // `count` is the full fan-in; `top_modules` is aggregated from a capped caller sample. Flag
    // the discrepancy so an agent knows the per-module breakdown is partial and can walk the
    // full caller list via `graph` with the returned `graph_id`.
    if summary.top_modules_sampled {
        value["top_modules_sampled"] = json!(true);
    }
    Some(value)
}

fn card_to_value(card: &SymbolInfoCard, max_output_tokens: usize) -> Value {
    let mut body = Map::new();
    let mut truncated = false;
    body.insert("symbol".into(), json!(card.symbol));
    body.insert("kind".into(), json!(card.kind));

    if card.semantics_unavailable {
        body.insert("semantics_unavailable".into(), json!(true));
    }
    if let Some(container) = &card.container {
        let mut c = json!({ "kind": container.kind, "name": container.name });
        if let Some(context) = &container.context {
            c["context"] = json!(context);
        }
        body.insert("container".into(), c);
    }
    if let Some(signature) = &card.signature {
        body.insert("signature".into(), json!(signature));
    }
    if let Some(doc) = &card.doc {
        // Doc comments can be arbitrarily large; bound them against the budget so a single field
        // cannot blow the response (the other free-text/list tools budget likewise).
        let mut doc = doc.clone();
        truncated |= truncate_text_to_budget(&mut doc, max_output_tokens, "\n… (truncated)");
        body.insert("doc".into(), json!(doc));
    }
    if let Some(return_type) = &card.return_type {
        body.insert("return_type".into(), json!(return_type));
    }
    if let Some(def) = &card.definition {
        let mut d = json!({ "line": def.line });
        if let Some(path) = &def.path {
            d["path"] = json!(path);
        }
        if let Some(snippet) = &def.snippet {
            let mut snippet = snippet.clone();
            truncated |= truncate_text_to_budget(&mut snippet, max_output_tokens, " …");
            d["snippet"] = json!(snippet);
        }
        body.insert("definition".into(), d);
    }

    if !card.members.is_empty() {
        let mut members: Vec<Value> = card
            .members
            .iter()
            .map(|m| {
                let mut v = json!({ "name": m.name, "kind": m.kind });
                if let Some(ty) = &m.ty {
                    v["type"] = json!(ty);
                }
                v
            })
            .collect();
        truncated |= trim_items_to_budget(&mut members, max_output_tokens);
        body.insert("members".into(), json!(members));
    }
    if truncated {
        body.insert("truncated".into(), json!(true));
        body.insert(
            "budget_hint".into(),
            json!("output trimmed to fit max_output_tokens; raise it to see the rest"),
        );
    }

    Value::Object(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide::{SymbolContainer, SymbolDefinition, SymbolMember};

    fn method_card() -> SymbolInfoCard {
        SymbolInfoCard {
            symbol: "МойМодуль.Сложить".to_string(),
            kind: "function",
            container: Some(SymbolContainer {
                kind: "ОбщийМодуль".to_string(),
                name: "МойМодуль".to_string(),
                context: Some("Сервер".to_string()),
            }),
            signature: Some("Функция Сложить(А, Б) Экспорт".to_string()),
            doc: Some("Складывает".to_string()),
            return_type: Some("Число".to_string()),
            definition: Some(SymbolDefinition {
                path: Some("/CommonModules/МойМодуль/Ext/Module.bsl".to_string()),
                line: 3,
                snippet: Some("Функция Сложить(А, Б) Экспорт".to_string()),
            }),
            members: Vec::new(),
            graph_id: Some("method/common/МойМодуль/Сложить".to_string()),
            semantics_unavailable: false,
        }
    }

    /// The positional request is the one place `symbol_info` takes a path, and a path from a
    /// `search` hit is spelled against the root that owns it. Answered without the root, this
    /// lands on the configuration's module of the same name and describes a symbol the caller
    /// never asked about — which is why the two modules declare differently named functions.
    #[test]
    fn a_rooted_position_describes_the_symbol_of_that_root() {
        use crate::diagnostics_state::test_support::{
            extension_root_id, wait_ready, workspace_with_an_outside_extension,
            CONFIGURATION_SYMBOL, EXTENSION_SYMBOL, SHARED_MODULE_REL,
        };
        use crate::diagnostics_state::DiagnosticsState;

        let (_dir, workspace, extension) = workspace_with_an_outside_extension();
        let root_id = extension_root_id(&workspace, &extension);
        let state = DiagnosticsState::for_workspace(workspace.clone());
        state.ensure_loading();
        wait_ready(&state);

        let card = match state.read(|resident, _| {
            // Line 1, past `Функция `, is the function's own name in both modules.
            resolve_card(
                resident,
                None,
                Some(root_id.as_str()),
                Some(SHARED_MODULE_REL),
                Some(1),
                Some(8),
                sections_from(&[]),
                Locale::default(),
            )
        }) {
            crate::diagnostics_state::ResidentOutcome::Ready(card, _) => {
                card.expect("the request resolves").expect("the position names a symbol")
            }
            _ => panic!("the resident is ready in this stand"),
        };

        assert!(
            card.symbol.contains(EXTENSION_SYMBOL),
            "the card describes the extension's function: {}",
            card.symbol,
        );
        assert!(
            !card.symbol.contains(CONFIGURATION_SYMBOL),
            "and not its configuration namesake: {}",
            card.symbol,
        );
    }

    #[test]
    fn empty_include_means_all_sections() {
        let s = sections_from(&[]);
        assert!(s.definition && s.type_ && s.doc);
    }

    #[test]
    fn include_narrows_to_named_sections() {
        let s = sections_from(&["doc".to_string(), "type".to_string()]);
        assert!(s.doc && s.type_);
        assert!(!s.definition);
    }

    #[test]
    fn render_card_without_graph_stamps_usages_unavailable() {
        let card = method_card();
        let result = render_card(&card, None, DEFAULT_TOP_MODULES, 6000);
        let body = result.structured_content.unwrap();
        assert_eq!(body["kind"], "function");
        assert_eq!(body["container"]["context"], "Сервер");
        assert_eq!(body["usages_unavailable"], "graph indexing");
        assert!(body.get("usages").is_none());
    }

    #[test]
    fn card_members_truncate_under_budget() {
        let mut card = method_card();
        card.kind = "metadata object";
        card.members = (0..200)
            .map(|i| SymbolMember {
                name: format!("Реквизит{i}"),
                kind: "Реквизит".to_string(),
                ty: Some("Строка".to_string()),
            })
            .collect();
        // A tiny budget forces the member list to be trimmed.
        let body = card_to_value(&card, 5);
        assert_eq!(body["truncated"], true);
        let members = body["members"].as_array().unwrap();
        assert!(members.len() < 200 && !members.is_empty());
    }

    #[test]
    fn not_found_without_graph_returns_retry_hint() {
        let result = render_not_found("НетТакого.Метод", None, DEFAULT_CANDIDATE_LIMIT);
        let body = result.structured_content.unwrap();
        assert_eq!(body["resolved"], false);
        assert_eq!(body["symbol"], "НетТакого.Метод");
        assert_eq!(body["candidates"].as_array().unwrap().len(), 0);
        assert!(body["hint"].as_str().unwrap().contains("indexing"));
    }
}
