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
use crate::tools::location::{
    Completeness, Freshness, FreshnessSource, Location, LocationUnavailable, PositionRange,
    ReasonCode,
};
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
    db: &ide::RootDatabaseImpl,
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
            // `root_id` qualifies `path` and nothing else, so any root beside a symbol is a
            // request that cannot be honoured: a symbol is resolved by NAME, across the whole
            // resident, and the card comes from whichever root owns that name. The empty id is
            // no exception — it asserts the configuration, and a module that exists only in an
            // extension would answer it anyway. Silence there would be the very substitution
            // this node exists to stop, and the same value would be a hard error in one shape
            // and a no-op in the other.
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
    Ok(ide::symbol_info(db, &req))
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
    stamp: &ResidentStamp<'_>,
) -> CallToolResult {
    let (mut body, budget_exhausted) = card_to_value(card, max_output_tokens);
    // Three states, not one flag: the resident lost the symbol's semantics, the graph is
    // still indexing, or the graph answered and does not know this symbol. They call for
    // different reactions — retry, retry later, don't retry — so they get different codes.
    let mut degraded = card.semantics_unavailable;
    let mut indexing = false;

    if !card.semantics_unavailable {
        match (card.graph_id.as_deref(), graph) {
            (Some(graph_id), Some(graph)) => match usages(graph, graph_id, top_modules) {
                Some(u) => {
                    body["usages"] = u;
                }
                None => {
                    body["usages_unavailable"] = json!("symbol not in call graph");
                    degraded = true;
                }
            },
            // Cards without a graph id (metadata objects/attributes, platform members) carry no
            // usages summary; only note the graph being unavailable for graph-addressable symbols.
            (Some(_), None) => {
                body["usages_unavailable"] = json!("graph indexing");
                indexing = true;
            }
            (None, _) => {}
        }
    }

    // The definition under the location contract, beside the legacy `definition` key. A list
    // because an extension may override a method (`&Вместо`, `&До`, `&После`) and the shape
    // must not have to change when the second target starts being reported.
    body["definitions"] = json!(definitions_value(card, stamp));

    let completeness = Completeness::complete()
        .when(budget_exhausted, ReasonCode::OutputBudget, "card trimmed to fit max_output_tokens")
        .when(
            degraded,
            ReasonCode::ModalityDegraded,
            "the call graph could not answer for this symbol",
        )
        // The same condition the miss shape reports, under the same code: the graph is
        // building, and a consumer driving retries off the code must not get two answers
        // for one state depending on whether the symbol happened to resolve.
        .when(
            indexing,
            ReasonCode::IndexBuilding,
            "the call graph is still indexing, so usages are not counted yet",
        )
        // Deliberately NOT stamped here: the workspace-wide unread counter. This answer is
        // one resolved symbol, and a hole in an unrelated module does not make it less
        // whole. The miss below is the shape where a hole CAN hide the answer, and that is
        // where the counter is read.
        ;
    body["freshness"] = stamp.freshness(completeness).to_value();

    structured(body)
}

/// What the caller knows about the resident that answered: its root table (for locations)
/// and its freshness. Carried as one value so a new envelope field cannot be forgotten at
/// one of the two response shapes.
pub(crate) struct ResidentStamp<'a> {
    pub roots: Option<&'a bsl_search::WorkspaceRoots>,
    pub revision: u64,
    pub topology: u64,
    pub stale: bool,
    pub unread_files: usize,
}

impl ResidentStamp<'_> {
    fn freshness(&self, completeness: Completeness) -> Freshness {
        Freshness::new(FreshnessSource::Resident, completeness)
            .with_revision(self.revision)
            .with_topology(self.topology)
            .with_stale(self.stale)
    }
}

/// The definition sites under the location contract. Empty when the card has no source
/// (platform members, metadata objects): an empty list says "no definition site", where an
/// invented location would say something false.
fn definitions_value(card: &SymbolInfoCard, stamp: &ResidentStamp<'_>) -> Vec<Value> {
    let Some(def) = &card.definition else {
        return Vec::new();
    };
    let mut entry = Map::new();

    match (&def.path, stamp.roots) {
        (Some(path), Some(roots)) => match Location::from_path(roots, Path::new(path)) {
            Ok(location) => {
                let location = location
                    .with_range(def.name_range.map(PositionRange::from))
                    .with_enclosing_range(def.enclosing_range.map(PositionRange::from))
                    .with_module(module_ref(card));
                entry.insert("location".into(), location.to_value());
            }
            Err(reason) => {
                entry.insert("location_unavailable".into(), json!(reason.code()));
            }
        },
        // The two remaining cases are different facts and must not share a code: no table
        // at all, versus a table that was there and a path that could not be named.
        (Some(_), None) => {
            entry.insert(
                "location_unavailable".into(),
                json!(LocationUnavailable::RootsUnavailable.code()),
            );
        }
        (None, _) => {
            entry.insert(
                "location_unavailable".into(),
                json!(LocationUnavailable::SourcePathUnavailable.code()),
            );
        }
    }

    // No snippet here on purpose: the card already carries it in `definition`, and a second
    // copy would double the very payload the budget just trimmed. The location says WHERE;
    // the text stays where it was.
    vec![Value::Object(entry)]
}

/// The owning module, when the card already holds both halves — never parsed back out of a
/// display string.
fn module_ref(card: &SymbolInfoCard) -> Option<crate::tools::location::ModuleRef> {
    let container = card.container.as_ref()?;
    // Only a common module's container IS the module. For a method of an object or manager
    // module the container describes the OWNING OBJECT (`Документ`, `Справочник`, …), and
    // publishing that under a key named `module` would make one key mean two things —
    // exactly the ambiguity this contract removes.
    if container.kind != "ОбщийМодуль" {
        return None;
    }
    Some(crate::tools::location::ModuleRef {
        kind: container.kind.clone(),
        name: container.name.clone(),
    })
}

/// The resident-miss response: the name dictionary's candidates.
///
/// It used to be a projection of the call graph, which made the miss useless in
/// the two cases it mattered most — a platform member, which the graph does not
/// hold at all, and a workspace with no graph yet, where the answer was an empty
/// list dressed as an answer. The dictionary asks the platform and the
/// resident's own tables too, and names whichever source could not be asked.
pub(crate) fn render_not_found(
    symbol: &str,
    answer: crate::tools::name_answer::NameAnswer,
    stamp: &ResidentStamp<'_>,
) -> CallToolResult {
    let mut body = serde_json::Map::new();
    body.insert("resolved".into(), json!(false));
    body.insert("symbol".into(), json!(symbol));
    let completeness = answer.insert_into(&mut body);
    // Here the counter matters: the symbol was not found, and an unread module is
    // a place it could have been.
    let completeness = completeness.when(
        stamp.unread_files > 0,
        ReasonCode::UnreadableFiles,
        "some workspace files could not be read, so the search was not exhaustive",
    );
    body.insert(
        "hint".into(),
        json!(
            "no exact resident match; feed a candidate's `address.symbol` back to \
             `symbol_info`, open its `address.graph_id` in `graph`, or read its \
             `address.syntax_help` in `syntax_help`"
        ),
    );
    body.insert("freshness".into(), stamp.freshness(completeness).to_value());
    structured(Value::Object(body))
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

/// The card body plus whether the output budget cut anything: the fact is known here and
/// travels up with the body rather than being recovered from the rendered JSON.
fn card_to_value(card: &SymbolInfoCard, max_output_tokens: usize) -> (Value, bool) {
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

    (Value::Object(body), truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide::{SymbolContainer, SymbolDefinition, SymbolMember};
    use line_index::LineColRange;

    /// A workspace whose configuration root owns the module the card below points at.
    fn stand_roots() -> bsl_search::WorkspaceRoots {
        let (roots, _rejected) =
            bsl_search::WorkspaceRoots::build(Path::new("/ws"), Path::new("/ws/src/cf"), &[]);
        roots
    }

    fn stamp(roots: &bsl_search::WorkspaceRoots) -> ResidentStamp<'_> {
        ResidentStamp {
            roots: Some(roots),
            revision: 7,
            topology: 0x0a1b_2c3d_4e5f_6071,
            stale: false,
            unread_files: 0,
        }
    }

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
                path: Some("/ws/src/cf/CommonModules/МойМодуль/Ext/Module.bsl".to_string()),
                line: 3,
                snippet: Some("Функция Сложить(А, Б) Экспорт".to_string()),
                name_range: Some(LineColRange {
                    start_line: 2,
                    start_character: 8,
                    end_line: 2,
                    end_character: 15,
                }),
                enclosing_range: Some(LineColRange {
                    start_line: 2,
                    start_character: 0,
                    end_line: 6,
                    end_character: 15,
                }),
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
                resident.db(),
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
        let roots = stand_roots();
        let result = render_card(&card, None, DEFAULT_TOP_MODULES, 6000, &stamp(&roots));
        let body = result.structured_content.unwrap();
        assert_eq!(body["kind"], "function");
        assert_eq!(body["container"]["context"], "Сервер");
        assert_eq!(body["usages_unavailable"], "graph indexing");
        assert!(body.get("usages").is_none());
        // A graph that has not finished indexing is `index_building` — the SAME code the
        // miss shape reports for the same condition, so a consumer driving retries off the
        // code is not told two different things about one state.
        assert_eq!(body["freshness"]["completeness"]["status"], "partial");
        assert_eq!(body["freshness"]["completeness"]["reasons"][0]["code"], "index_building");
    }

    /// The card keeps its legacy 1-based `definition.line` AND gains the contract location:
    /// both are asserted together, because dropping either is the failure this step exists
    /// to prevent.
    #[test]
    fn a_card_carries_both_the_old_line_and_the_new_location() {
        let card = method_card();
        let roots = stand_roots();
        let body = render_card(&card, None, DEFAULT_TOP_MODULES, 6000, &stamp(&roots))
            .structured_content
            .unwrap();

        assert_eq!(body["definition"]["line"], 3);
        assert_eq!(body["definition"]["path"], "/ws/src/cf/CommonModules/МойМодуль/Ext/Module.bsl");

        // The slab's own version lives INSIDE the location and nowhere else: hoisting it to
        // the top level would tie this tool's response version to the slab's, which is the
        // one thing the three-axis versioning exists to prevent.
        assert!(body.get("schema_version").is_none(), "{body}");
        let location = &body["definitions"][0]["location"];
        assert_eq!(location["root_id"], "");
        assert_eq!(location["path"], "CommonModules/МойМодуль/Ext/Module.bsl");
        assert_eq!(location["position_encoding"], "utf-16");
        // 0-based against the 1-based `definition.line` above: the same declaration.
        assert_eq!(location["range"]["start_line"], 2);
        assert_eq!(location["enclosing_range"]["end_line"], 6);
        assert_eq!(location["module"]["name"], "МойМодуль");
        assert_eq!(body["freshness"]["source"], "resident");
        assert_eq!(body["freshness"]["revision"], 7);
        assert_eq!(body["freshness"]["topology_fingerprint"], "0a1b2c3d4e5f6071");
    }

    /// Without the root table there is no pair to publish — and the answer says which,
    /// instead of omitting the location and letting it read as "no definition".
    #[test]
    fn a_card_without_roots_names_the_reason() {
        let card = method_card();
        let stamp =
            ResidentStamp { roots: None, revision: 7, topology: 1, stale: false, unread_files: 0 };
        let body =
            render_card(&card, None, DEFAULT_TOP_MODULES, 6000, &stamp).structured_content.unwrap();

        assert_eq!(body["definitions"][0]["location_unavailable"], "roots_unavailable");
        assert!(body["definitions"][0].get("location").is_none());
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
        let (body, budget_exhausted) = card_to_value(&card, 5);
        assert!(budget_exhausted, "the fact travels up with the body, not via the rendered JSON");
        assert_eq!(body["truncated"], true);
        let members = body["members"].as_array().unwrap();
        assert!(members.len() < 200 && !members.is_empty());
    }

    /// A miss carries the envelope, and an empty list says which source could
    /// not be consulted.
    ///
    /// The test this replaces asked for a name nothing could ever hold, so it
    /// stayed green whether the miss consulted five sources or none — the shape
    /// it checked was the shape of "no candidates", not of "no candidates and
    /// here is why".
    #[test]
    fn a_miss_names_the_source_it_could_not_consult() {
        let roots = stand_roots();
        let analysis = ide::Analysis::new();
        let found = ide::NameLookupResult {
            candidates: Vec::new(),
            total: 0,
            total_exact: true,
            truncated: false,
            providers: vec![
                ide::ProviderReport {
                    provider: ide::ProviderId::Platform,
                    state: ide::ProviderState::Answered,
                },
                ide::ProviderReport {
                    provider: ide::ProviderId::Graph,
                    state: ide::ProviderState::NotReady,
                },
            ],
        };
        let answer = crate::tools::name_answer::NameAnswer::render(
            analysis.database(),
            Some(&roots),
            &found,
        );

        let body =
            render_not_found("НетТакого.Метод", answer, &stamp(&roots)).structured_content.unwrap();

        assert_eq!(body["resolved"], false);
        assert_eq!(body["symbol"], "НетТакого.Метод");
        assert!(body["candidates"].as_array().unwrap().is_empty());
        assert_eq!(body["providers"][1]["provider"], "graph");
        assert_eq!(body["providers"][1]["state"], "not_ready");
        // The miss shape is assembled by its own function, so it is the one that would
        // silently ship without an envelope.
        assert_eq!(body["freshness"]["source"], "resident");
        assert_eq!(body["freshness"]["completeness"]["reasons"][0]["code"], "index_building");
    }
}
