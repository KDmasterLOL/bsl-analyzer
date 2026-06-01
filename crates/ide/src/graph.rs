//! Agent-facing projection of the whole-config call graph.
//!
//! The MCP surface speaks in **durable string ids**, not interned handles:
//! `MethodId.local_id` is an item-tree position that shifts whenever methods are
//! added or removed above it, so an interned id is valid only within one Salsa
//! revision. The durable id is a path-derived qualified name (`method/common/
//! РаботаСКонтрагентами/ПроверитьИНН`) that survives edits and re-resolves to the
//! current revision's handle per request. BSL identifiers cannot contain `/`,
//! `:` or `.`, so the structural separators never collide with names.
//!
//! Modules that the metadata index does not key by name (forms, commands) fall
//! back to a workspace-relative path id (`method/file/<relpath>::<method>`),
//! resolvable only when a workspace root is supplied.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use bsl_metadata::MdoType;
use hir::call_graph::{EdgeKind, MethodDispatch};
use hir::{
    module_key_for_path, ConfigsDatabase, DefDatabase, GraphNode, MethodId, ModuleId, ModuleIndex,
    ModuleKey, Semantics, WorkspaceCallEdge, WorkspaceCallGraph,
};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use serde::Serialize;
use vfs::FileId;

use crate::{Analysis, RootDatabaseImpl};

/// How much detail to materialise per node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphDetail {
    /// Id + name + kind only.
    Names,
    /// Adds the declaration line and dispatch.
    #[default]
    Signatures,
    /// Adds the full method source.
    Bodies,
}

/// Traversal direction over the call graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Callers (incoming edges).
    In,
    /// Callees (outgoing edges).
    Out,
    /// Both.
    Both,
}

/// A node, projected for the agent. `dispatch` is surfaced top-level (not nested
/// under a type) because client/server is a first-order BSL concern.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NodeRef {
    pub id: String,
    pub kind: &'static str,
    pub name: String,
    pub qualified: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dispatch: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_export: Option<bool>,
    /// Whether this id round-trips back to a node on its own. `false` for
    /// path-fallback nodes seen only as an edge endpoint.
    pub addressable: bool,
}

/// A resolved call edge, projected for the agent.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EdgeRef {
    pub from: String,
    pub to: String,
    pub kind: &'static str,
    pub provenance: &'static str,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub crosses_client_to_server: bool,
}

/// Cold-start orientation for an unfamiliar project.
#[derive(Debug, Clone, Serialize)]
pub struct GraphOverview {
    pub modules: usize,
    pub methods: usize,
    pub mdos: usize,
    pub nodes: usize,
    pub edges: usize,
    pub top_by_centrality: Vec<NodeRef>,
    pub edge_provenance: BTreeMap<&'static str, usize>,
    pub client_to_server_edges: usize,
}

/// One node plus optional neighbour expansion.
#[derive(Debug, Clone, Serialize)]
pub struct NodeResult {
    pub node: NodeRef,
}

/// Neighbour traversal result.
#[derive(Debug, Clone, Serialize)]
pub struct NeighborsResult {
    pub root: NodeRef,
    pub nodes: Vec<NodeRef>,
    pub edges: Vec<EdgeRef>,
    /// Nodes dropped by the `max_nodes` early-exit, lowest-centrality first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<String>,
}

/// Source for one requested node.
#[derive(Debug, Clone, Serialize)]
pub struct SourceItem {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<GraphError>,
    /// The source was cut short to stay within the output budget.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

/// Budgeted source for a set of nodes.
#[derive(Debug, Clone, Serialize)]
pub struct SourceResult {
    pub items: Vec<SourceItem>,
    /// The token budget was reached; later items carry no source.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub budget_exhausted: bool,
}

/// Why a graph request could not be served.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum GraphError {
    /// The id is well-formed but resolves to no node in the current revision.
    NotFound { id: String },
    /// The id is malformed.
    BadId { id: String, reason: String },
    /// The operation is not supported for this id (e.g. a path id with no root).
    Unsupported { id: String, reason: String },
}

/// Parameters for [`Analysis::graph_neighbors`].
#[derive(Debug, Clone)]
pub struct NeighborsParams<'a> {
    pub id: &'a str,
    pub dir: Direction,
    pub depth: usize,
    pub max_nodes: usize,
    pub detail: GraphDetail,
    /// Keep only edges whose provenance is in this set, when non-empty.
    pub provenance_filter: Vec<String>,
}

impl Analysis {
    /// Cold-start overview: module/method/edge counts, the most-called methods,
    /// and the provenance/dispatch profile.
    pub fn graph_overview(
        &self,
        source_root_id: SourceRootId,
        workspace_root: Option<&Path>,
        top_n: usize,
    ) -> GraphOverview {
        let ctx = GraphCtx::new(self.database(), source_root_id, workspace_root);
        ctx.overview(top_n)
    }

    /// Resolve a durable id to a single node, at the requested detail.
    pub fn graph_node(
        &self,
        source_root_id: SourceRootId,
        workspace_root: Option<&Path>,
        id: &str,
        detail: GraphDetail,
    ) -> Result<NodeResult, GraphError> {
        let ctx = GraphCtx::new(self.database(), source_root_id, workspace_root);
        let node = ctx.resolve_id(id)?;
        Ok(NodeResult { node: ctx.node_ref(node, detail) })
    }

    /// Traverse callers/callees from a node up to `depth`, bounded by `max_nodes`.
    pub fn graph_neighbors(
        &self,
        source_root_id: SourceRootId,
        workspace_root: Option<&Path>,
        params: &NeighborsParams<'_>,
    ) -> Result<NeighborsResult, GraphError> {
        let ctx = GraphCtx::new(self.database(), source_root_id, workspace_root);
        ctx.neighbors(params)
    }

    /// Fetch method source for a set of durable ids, stopping once the rough
    /// output budget (`max_output_tokens`, ~4 chars/token) is reached. Returns
    /// raw source — the MCP adapter is responsible for any redaction.
    pub fn graph_source(
        &self,
        source_root_id: SourceRootId,
        workspace_root: Option<&Path>,
        ids: &[String],
        max_output_tokens: usize,
    ) -> SourceResult {
        let ctx = GraphCtx::new(self.database(), source_root_id, workspace_root);
        ctx.source(ids, max_output_tokens)
    }
}

struct GraphCtx<'a> {
    db: &'a RootDatabaseImpl,
    graph: Arc<WorkspaceCallGraph>,
    index: Arc<ModuleIndex>,
    source_root: SourceRoot,
    workspace_root: Option<&'a Path>,
}

impl<'a> GraphCtx<'a> {
    fn new(
        db: &'a RootDatabaseImpl,
        source_root_id: SourceRootId,
        workspace_root: Option<&'a Path>,
    ) -> Self {
        let graph = db.workspace_call_graph(source_root_id);
        let index = db.module_index(source_root_id);
        let source_root = db.source_root_input(source_root_id).root(db);
        Self { db, graph, index, source_root, workspace_root }
    }

    fn path_for(&self, file_id: FileId) -> Option<String> {
        let vfs_path = self.source_root.file_set().path_for_file(&file_id)?;
        Some(vfs_path.as_path().to_str()?.replace('\\', "/"))
    }

    fn rel_path(&self, abs: &str) -> Option<String> {
        let root = self.workspace_root?;
        let root_str = root.to_str()?.replace('\\', "/");
        let stripped = abs.strip_prefix(&root_str)?;
        Some(stripped.trim_start_matches('/').to_string())
    }

    // ---- id encoding --------------------------------------------------------

    fn encode_node(&self, node: &GraphNode) -> (String, bool) {
        match node {
            GraphNode::Method(method) => self.encode_method(*method),
            GraphNode::ModuleCode(module) => self.encode_module(*module),
            GraphNode::Mdo { mdo_type, object_name } => {
                (format!("mdo/{}/{}", mdo_type.english_name(), object_name.as_str()), true)
            }
        }
    }

    fn encode_method(&self, method: MethodId) -> (String, bool) {
        let method_name = self.method_name(method);
        let path = self.path_for(method.module.file_id);
        if let Some(key) = path.as_deref().and_then(module_key_for_path) {
            let scope = encode_scope(&key);
            (format!("method/{scope}/{method_name}"), true)
        } else if let Some(rel) = path.as_deref().and_then(|p| self.rel_path(p)) {
            (format!("method/file/{rel}::{method_name}"), true)
        } else {
            let basename = path.as_deref().and_then(basename).unwrap_or("?");
            (format!("method/file/{basename}::{method_name}"), false)
        }
    }

    fn encode_module(&self, module: ModuleId) -> (String, bool) {
        let path = self.path_for(module.file_id);
        if let Some(key) = path.as_deref().and_then(module_key_for_path) {
            (format!("module/{}", encode_scope(&key)), true)
        } else if let Some(rel) = path.as_deref().and_then(|p| self.rel_path(p)) {
            (format!("module/file/{rel}"), true)
        } else {
            let basename = path.as_deref().and_then(basename).unwrap_or("?");
            (format!("module/file/{basename}"), false)
        }
    }

    // ---- id resolution ------------------------------------------------------

    fn resolve_id(&self, id: &str) -> Result<GraphNode, GraphError> {
        if let Some(rest) = id.strip_prefix("method/file/") {
            // Split off the method from the right: a method name cannot contain
            // ':' but a relative path conceivably could.
            let (rel, method) = rest.rsplit_once("::").ok_or_else(|| GraphError::BadId {
                id: id.to_string(),
                reason: "path method id must contain '::<method>'".to_string(),
            })?;
            let file_id = self
                .resolve_rel_path(rel)
                .ok_or_else(|| GraphError::NotFound { id: id.to_string() })?;
            return self.resolve_method_in(file_id, method, id);
        }
        if let Some(rel) = id.strip_prefix("module/file/") {
            let file_id = self
                .resolve_rel_path(rel)
                .ok_or_else(|| GraphError::NotFound { id: id.to_string() })?;
            return Ok(GraphNode::ModuleCode(ModuleId::new(file_id)));
        }
        if let Some(rest) = id.strip_prefix("mdo/") {
            return self.resolve_mdo_id(rest, id);
        }

        let parts: Vec<&str> = id.split('/').collect();
        let (is_method, rest) = match parts.first().copied() {
            Some("method") => (true, &parts[1..]),
            Some("module") => (false, &parts[1..]),
            _ => {
                return Err(GraphError::BadId {
                    id: id.to_string(),
                    reason: "id must start with 'method/' or 'module/'".to_string(),
                })
            }
        };
        let (key, method) = decode_scope(rest, is_method).ok_or_else(|| GraphError::BadId {
            id: id.to_string(),
            reason: "malformed scope".to_string(),
        })?;
        let file_id = self
            .index
            .resolve_module_key(&key)
            .ok_or_else(|| GraphError::NotFound { id: id.to_string() })?;
        match method {
            Some(method) => self.resolve_method_in(file_id, &method, id),
            None => Ok(GraphNode::ModuleCode(ModuleId::new(file_id))),
        }
    }

    fn resolve_method_in(
        &self,
        file_id: FileId,
        method_name: &str,
        id: &str,
    ) -> Result<GraphNode, GraphError> {
        let sema = Semantics::new(self.db);
        let method = sema
            .find_method(file_id, method_name)
            .ok_or_else(|| GraphError::NotFound { id: id.to_string() })?;
        Ok(GraphNode::Method(method.id()))
    }

    /// Resolve `<MdoEnglish>/<ObjectName>` to the metadata-object node, if the
    /// workspace graph references it. The match is case-insensitive on the object
    /// name (BSL is case-insensitive) and returns the graph's canonical spelling.
    fn resolve_mdo_id(&self, rest: &str, id: &str) -> Result<GraphNode, GraphError> {
        let (mdo_eng, object) = rest.split_once('/').ok_or_else(|| GraphError::BadId {
            id: id.to_string(),
            reason: "mdo id must be 'mdo/<MdoType>/<Object>'".to_string(),
        })?;
        let mdo_type: MdoType = mdo_eng.parse().map_err(|_| GraphError::BadId {
            id: id.to_string(),
            reason: format!("unknown metadata type '{mdo_eng}'"),
        })?;
        let object_lower = object.to_lowercase();
        self.graph
            .nodes()
            .find(|n| {
                matches!(n, GraphNode::Mdo { mdo_type: mt, object_name }
                    if *mt == mdo_type && object_name.as_str().to_lowercase() == object_lower)
            })
            .ok_or_else(|| GraphError::NotFound { id: id.to_string() })
    }

    fn resolve_rel_path(&self, rel: &str) -> Option<FileId> {
        for file_id in self.source_root.iter() {
            let abs = match self.path_for(file_id) {
                Some(p) => p,
                None => continue,
            };
            match self.rel_path(&abs) {
                Some(file_rel) if file_rel == rel => return Some(file_id),
                _ => {}
            }
        }
        None
    }

    // ---- projection ---------------------------------------------------------

    fn method_name(&self, method: MethodId) -> String {
        hir::Method::new(self.db, method).name().as_str().to_string()
    }

    fn node_ref(&self, node: GraphNode, detail: GraphDetail) -> NodeRef {
        let (id, addressable) = self.encode_node(&node);
        match node {
            GraphNode::Method(method) => self.method_node_ref(method, id, addressable, detail),
            GraphNode::ModuleCode(module) => self.module_node_ref(module, id, addressable),
            GraphNode::Mdo { mdo_type, object_name } => {
                self.mdo_node_ref(mdo_type, object_name.as_str(), id, addressable)
            }
        }
    }

    fn mdo_node_ref(
        &self,
        mdo_type: MdoType,
        object_name: &str,
        id: String,
        addressable: bool,
    ) -> NodeRef {
        let name = object_name.to_string();
        let qualified = format!("{}.{name}", mdo_type.russian_name());
        NodeRef {
            id,
            kind: "mdo",
            name,
            qualified,
            module: None,
            signature: None,
            source: None,
            dispatch: Vec::new(),
            is_export: None,
            addressable,
        }
    }

    fn method_node_ref(
        &self,
        method: MethodId,
        id: String,
        addressable: bool,
        detail: GraphDetail,
    ) -> NodeRef {
        let m = hir::Method::new(self.db, method);
        let name = m.name().as_str().to_string();
        let module_display = self.module_display(method.module);
        let qualified = match &module_display {
            Some(scope) => format!("{scope}.{name}"),
            None => name.clone(),
        };
        let dispatch = self
            .graph
            .dispatch(&GraphNode::Method(method))
            .map(dispatch_labels)
            .unwrap_or_default();

        let mut node = NodeRef {
            id,
            kind: "method",
            name,
            qualified,
            module: module_display,
            signature: None,
            source: None,
            dispatch,
            is_export: Some(m.is_export()),
            addressable,
        };

        if matches!(detail, GraphDetail::Signatures | GraphDetail::Bodies) {
            // The signature is the declaration line (skipping any `&НаСервере`-style
            // annotation lines that the method's source range includes).
            node.signature =
                m.name_range().and_then(|r| self.line_at(method.module.file_id, r.start()));
            if detail == GraphDetail::Bodies {
                node.source = m.source_range().and_then(|r| self.slice(method.module.file_id, r));
            }
        }
        node
    }

    fn module_node_ref(&self, module: ModuleId, id: String, addressable: bool) -> NodeRef {
        let display = self.module_display(module);
        let name = display.clone().unwrap_or_else(|| "<модуль>".to_string());
        NodeRef {
            id,
            kind: "module",
            name: name.clone(),
            qualified: name,
            module: display,
            signature: None,
            source: None,
            dispatch: self
                .graph
                .dispatch(&GraphNode::ModuleCode(module))
                .map(dispatch_labels)
                .unwrap_or_default(),
            is_export: None,
            addressable,
        }
    }

    fn module_display(&self, module: ModuleId) -> Option<String> {
        let path = self.path_for(module.file_id)?;
        match module_key_for_path(&path) {
            Some(key) => Some(display_scope(&key)),
            None => self.rel_path(&path).or_else(|| basename(&path).map(str::to_string)),
        }
    }

    fn slice(&self, file_id: FileId, range: syntax::TextRange) -> Option<String> {
        let text = self.db.file_text_input(file_id).text(self.db).clone();
        let start = u32::from(range.start()) as usize;
        let end = u32::from(range.end()) as usize;
        text.get(start..end).map(str::to_string)
    }

    fn method_source(&self, method: MethodId) -> Option<String> {
        let range = hir::Method::new(self.db, method).source_range()?;
        self.slice(method.module.file_id, range)
    }

    fn source(&self, ids: &[String], max_output_tokens: usize) -> SourceResult {
        let budget_chars = max_output_tokens.saturating_mul(4).max(1);
        let mut used = 0usize;
        let mut budget_exhausted = false;
        let mut items = Vec::with_capacity(ids.len());

        for id in ids {
            let item = match self.resolve_id(id) {
                Err(err) => {
                    SourceItem { id: id.clone(), source: None, error: Some(err), truncated: false }
                }
                Ok(GraphNode::Method(method)) => match self.method_source(method) {
                    Some(src) => {
                        if used >= budget_chars {
                            budget_exhausted = true;
                            SourceItem {
                                id: id.clone(),
                                source: None,
                                error: None,
                                truncated: true,
                            }
                        } else {
                            let remaining = budget_chars - used;
                            let (text, truncated) = clamp_source(src, remaining);
                            used += text.len();
                            budget_exhausted |= truncated;
                            SourceItem {
                                id: id.clone(),
                                source: Some(text),
                                error: None,
                                truncated,
                            }
                        }
                    }
                    None => SourceItem {
                        id: id.clone(),
                        source: None,
                        error: Some(GraphError::NotFound { id: id.clone() }),
                        truncated: false,
                    },
                },
                Ok(GraphNode::ModuleCode(_)) => SourceItem {
                    id: id.clone(),
                    source: None,
                    error: Some(GraphError::Unsupported {
                        id: id.clone(),
                        reason: "module-body source is not served; request a method".to_string(),
                    }),
                    truncated: false,
                },
                Ok(GraphNode::Mdo { .. }) => SourceItem {
                    id: id.clone(),
                    source: None,
                    error: Some(GraphError::Unsupported {
                        id: id.clone(),
                        reason: "a metadata object has no source; request a method".to_string(),
                    }),
                    truncated: false,
                },
            };
            items.push(item);
        }

        SourceResult { items, budget_exhausted }
    }

    /// The trimmed source line containing `offset`.
    fn line_at(&self, file_id: FileId, offset: syntax::TextSize) -> Option<String> {
        let text = self.db.file_text_input(file_id).text(self.db).clone();
        let off = (u32::from(offset) as usize).min(text.len());
        let start = text[..off].rfind('\n').map_or(0, |i| i + 1);
        let end = text[off..].find('\n').map_or(text.len(), |i| off + i);
        Some(text[start..end].trim().to_string())
    }

    // ---- queries ------------------------------------------------------------

    fn overview(&self, top_n: usize) -> GraphOverview {
        let mut methods = 0usize;
        let mut modules = 0usize;
        let mut mdos = 0usize;
        let mut node_count = 0usize;
        for node in self.graph.nodes() {
            node_count += 1;
            match node {
                GraphNode::Method(_) => methods += 1,
                GraphNode::ModuleCode(_) => modules += 1,
                GraphNode::Mdo { .. } => mdos += 1,
            }
        }

        let mut edge_provenance: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut client_to_server_edges = 0usize;
        for edge in self.graph.edges() {
            *edge_provenance.entry(provenance_label(edge)).or_default() += 1;
            if edge.crosses_client_to_server {
                client_to_server_edges += 1;
            }
        }

        let mut ranked: Vec<(usize, GraphNode)> = self
            .graph
            .nodes()
            .map(|n| (self.graph.in_degree(&n), n))
            .filter(|(d, _)| *d > 0)
            .collect();
        ranked.sort_by_key(|&(degree, _)| std::cmp::Reverse(degree));
        let top_by_centrality = ranked
            .into_iter()
            .take(top_n)
            .map(|(_, n)| self.node_ref(n, GraphDetail::Signatures))
            .collect();

        GraphOverview {
            modules,
            methods,
            mdos,
            nodes: node_count,
            edges: self.graph.edge_count(),
            top_by_centrality,
            edge_provenance,
            client_to_server_edges,
        }
    }

    fn neighbors(&self, params: &NeighborsParams<'_>) -> Result<NeighborsResult, GraphError> {
        let root = self.resolve_id(params.id)?;
        let depth = params.depth.max(1);

        let mut visited: Vec<GraphNode> = vec![root.clone()];
        let mut seen: std::collections::HashSet<GraphNode> = std::collections::HashSet::new();
        seen.insert(root.clone());
        let mut out_edges: Vec<&WorkspaceCallEdge> = Vec::new();
        let mut frontier = vec![root.clone()];

        for _ in 0..depth {
            let mut next: Vec<GraphNode> = Vec::new();
            for node in &frontier {
                for edge in self.directed_edges(node, params.dir) {
                    if !self.provenance_allowed(edge, &params.provenance_filter) {
                        continue;
                    }
                    out_edges.push(edge);
                    let other = if &edge.from == node { &edge.to } else { &edge.from };
                    if seen.insert(other.clone()) {
                        next.push(other.clone());
                        visited.push(other.clone());
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }

        // Centrality-ranked tail-drop of discovered (non-root) nodes.
        let mut discovered: Vec<GraphNode> = visited.into_iter().filter(|n| *n != root).collect();
        discovered.sort_by_key(|n| std::cmp::Reverse(self.graph.in_degree(n)));
        let mut dropped: Vec<String> = Vec::new();
        if discovered.len() > params.max_nodes {
            for node in discovered.split_off(params.max_nodes) {
                dropped.push(self.encode_node(&node).0);
            }
        }
        let kept: std::collections::HashSet<GraphNode> = discovered.iter().cloned().collect();

        let nodes = discovered.iter().map(|n| self.node_ref(n.clone(), params.detail)).collect();
        // A `Direction::Both` sweep visits an edge from each endpoint, so a
        // self-call surfaces twice; dedup by `(from, to, kind)` so the two
        // manager edge kinds between the same pair are not collapsed.
        let mut seen_edges: std::collections::HashSet<(GraphNode, GraphNode, EdgeKind)> =
            std::collections::HashSet::new();
        let edges = out_edges
            .iter()
            .filter(|e| {
                (e.from == root || kept.contains(&e.from)) && (e.to == root || kept.contains(&e.to))
            })
            .filter(|e| seen_edges.insert((e.from.clone(), e.to.clone(), e.kind)))
            .map(|e| self.edge_ref(e))
            .collect();

        Ok(NeighborsResult { root: self.node_ref(root, params.detail), nodes, edges, dropped })
    }

    fn directed_edges(&self, node: &GraphNode, dir: Direction) -> Vec<&WorkspaceCallEdge> {
        match dir {
            Direction::Out => self.graph.callees(node).iter().collect(),
            Direction::In => self.graph.callers(node).iter().collect(),
            Direction::Both => {
                self.graph.callees(node).iter().chain(self.graph.callers(node).iter()).collect()
            }
        }
    }

    fn provenance_allowed(&self, edge: &WorkspaceCallEdge, filter: &[String]) -> bool {
        filter.is_empty() || filter.iter().any(|p| p == provenance_label(edge))
    }

    fn edge_ref(&self, edge: &WorkspaceCallEdge) -> EdgeRef {
        EdgeRef {
            from: self.encode_node(&edge.from).0,
            to: self.encode_node(&edge.to).0,
            kind: edge_kind_label(edge.kind),
            provenance: provenance_label(edge),
            crosses_client_to_server: edge.crosses_client_to_server,
        }
    }
}

/// Derive the durable method id for `method_name` in the module at `path`,
/// without a database. Returns `None` when `path` is not an indexable user
/// module (forms, commands, non-module files). Best-effort: the id is not
/// verified to resolve, but it round-trips through [`Analysis::graph_node`] when
/// the method exists. Used to bridge code-search hits into the graph.
pub fn method_id_for_path(path: &str, method_name: &str) -> Option<String> {
    let key = module_key_for_path(path)?;
    Some(format!("method/{}/{method_name}", encode_scope(&key)))
}

fn encode_scope(key: &ModuleKey) -> String {
    match key {
        ModuleKey::Common { name } => format!("common/{name}"),
        ModuleKey::Manager { mdo_type, name } => {
            format!("manager/{}/{name}", mdo_type.english_name())
        }
        ModuleKey::Object { mdo_type, name } => {
            format!("object/{}/{name}", mdo_type.english_name())
        }
        ModuleKey::RecordSet { mdo_type, name } => {
            format!("recordset/{}/{name}", mdo_type.english_name())
        }
    }
}

fn decode_scope(rest: &[&str], is_method: bool) -> Option<(ModuleKey, Option<String>)> {
    // For a method id the trailing segment is the method name; module ids end at
    // the scope.
    let (scope, method) = if is_method {
        let (method, scope) = rest.split_last()?;
        (scope, Some((*method).to_string()))
    } else {
        (rest, None)
    };

    let key = match scope {
        ["common", name] => ModuleKey::Common { name: name.to_string() },
        ["manager", mdo, name] => {
            ModuleKey::Manager { mdo_type: parse_mdo(mdo)?, name: name.to_string() }
        }
        ["object", mdo, name] => {
            ModuleKey::Object { mdo_type: parse_mdo(mdo)?, name: name.to_string() }
        }
        ["recordset", mdo, name] => {
            ModuleKey::RecordSet { mdo_type: parse_mdo(mdo)?, name: name.to_string() }
        }
        _ => return None,
    };
    Some((key, method))
}

fn parse_mdo(s: &str) -> Option<MdoType> {
    // Bilingual `MdoType: FromStr` accepts the English folder names we encode.
    s.parse().ok()
}

fn display_scope(key: &ModuleKey) -> String {
    match key {
        ModuleKey::Common { name } => format!("ОбщийМодуль.{name}"),
        ModuleKey::Manager { mdo_type, name } => {
            format!("{}.{name}.МодульМенеджера", mdo_type.russian_name())
        }
        ModuleKey::Object { mdo_type, name } => {
            format!("{}.{name}.МодульОбъекта", mdo_type.russian_name())
        }
        ModuleKey::RecordSet { mdo_type, name } => {
            format!("{}.{name}.МодульНабораЗаписей", mdo_type.russian_name())
        }
    }
}

fn dispatch_labels(d: MethodDispatch) -> Vec<&'static str> {
    let mut labels = Vec::new();
    if d.can_run_on_client {
        labels.push("client");
    }
    if d.can_run_on_server {
        labels.push("server");
    }
    labels
}

fn provenance_label(edge: &WorkspaceCallEdge) -> &'static str {
    use hir::EdgeProvenance::*;
    match edge.provenance {
        Resolved => "resolved",
        Inferred => "inferred",
        VisibilityBlocked => "visibility_blocked",
        Unresolved => "unresolved",
    }
}

fn edge_kind_label(kind: EdgeKind) -> &'static str {
    // Both direct-call variants project to the same agent-facing edge kind; the
    // local-vs-qualified distinction is an internal resolution detail.
    match kind {
        EdgeKind::DirectLocal | EdgeKind::DirectQualifiedModule => "call",
        EdgeKind::ManagerCreates => "manager_creates",
        EdgeKind::ManagerAccess => "manager_access",
        EdgeKind::QueryRef => "query_ref",
    }
}

fn basename(path: &str) -> Option<&str> {
    path.rsplit('/').next()
}

/// Truncate `src` to at most `max_chars` bytes on a char boundary, returning the
/// (possibly shortened) text and whether it was cut.
fn clamp_source(src: String, max_chars: usize) -> (String, bool) {
    if src.len() <= max_chars {
        return (src, false);
    }
    let mut end = max_chars;
    while end > 0 && !src.is_char_boundary(end) {
        end -= 1;
    }
    (src[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Analysis, RootDatabaseImpl};
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use vfs::{file_set::FileSet, FileId, VfsPath};

    const ROOT: SourceRootId = SourceRootId(0);

    fn workspace(files: &[(&str, &str)]) -> Analysis {
        let mut db = RootDatabaseImpl::new();
        let mut file_set = FileSet::new();
        for (i, (path, _)) in files.iter().enumerate() {
            file_set.insert(FileId(i as u32), VfsPath::new(*path));
        }
        db.set_source_root(ROOT, SourceRoot::new_local(file_set));
        for (i, (_, text)) in files.iter().enumerate() {
            let fid = FileId(i as u32);
            db.set_file_source_root(fid, ROOT);
            db.set_file_text(fid, text);
        }
        Analysis::from_database(db)
    }

    /// Two common modules: a client-dispatched caller invoking a server-only
    /// exported function in another module.
    fn client_server_workspace() -> Analysis {
        workspace(&[
            (
                "/src/CommonModules/Клиент/Ext/Module.bsl",
                "&НаКлиенте\n\
                 Процедура Главная() Экспорт\n\
                 Сервер.Считать();\n\
                 КонецПроцедуры",
            ),
            (
                "/src/CommonModules/Сервер/Ext/Module.bsl",
                "&НаСервере\n\
                 Функция Считать() Экспорт КонецФункции",
            ),
        ])
    }

    #[test]
    fn node_id_round_trips_for_common_module_method() {
        let a = client_server_workspace();
        let result = a
            .graph_node(ROOT, None, "method/common/Сервер/Считать", GraphDetail::Signatures)
            .expect("server method resolves by durable id");
        let node = result.node;
        assert_eq!(node.id, "method/common/Сервер/Считать");
        assert_eq!(node.kind, "method");
        assert_eq!(node.name, "Считать");
        assert_eq!(node.qualified, "ОбщийМодуль.Сервер.Считать");
        assert_eq!(node.is_export, Some(true));
        assert_eq!(node.dispatch, vec!["server"]);
        assert!(node.signature.as_deref().unwrap().contains("Считать"));
        assert!(node.addressable);
    }

    #[test]
    fn unknown_id_reports_not_found() {
        let a = client_server_workspace();
        let err = a
            .graph_node(ROOT, None, "method/common/Сервер/НетТакого", GraphDetail::Names)
            .unwrap_err();
        assert_eq!(
            err,
            GraphError::NotFound {
                id: "method/common/Сервер/НетТакого".to_string()
            }
        );
    }

    #[test]
    fn malformed_id_reports_bad_id() {
        let a = client_server_workspace();
        let err = a.graph_node(ROOT, None, "Сервер.Считать", GraphDetail::Names).unwrap_err();
        assert!(matches!(err, GraphError::BadId { .. }));
    }

    #[test]
    fn neighbors_in_lists_caller_and_flags_client_server_crossing() {
        let a = client_server_workspace();
        let params = NeighborsParams {
            id: "method/common/Сервер/Считать",
            dir: Direction::In,
            depth: 1,
            max_nodes: 50,
            detail: GraphDetail::Names,
            provenance_filter: Vec::new(),
        };
        let res = a.graph_neighbors(ROOT, None, &params).expect("neighbors resolve");
        assert_eq!(res.root.id, "method/common/Сервер/Считать");
        assert!(res.nodes.iter().any(|n| n.id == "method/common/Клиент/Главная"));
        let edge = res
            .edges
            .iter()
            .find(|e| e.to == "method/common/Сервер/Считать")
            .expect("an incoming edge to the server method");
        assert_eq!(edge.from, "method/common/Клиент/Главная");
        assert_eq!(edge.kind, "call");
        assert_eq!(edge.provenance, "resolved");
        assert!(edge.crosses_client_to_server);
    }

    #[test]
    fn overview_counts_edges_and_ranks_called_method() {
        let a = client_server_workspace();
        let ov = a.graph_overview(ROOT, None, 10);
        assert!(ov.methods >= 2);
        assert_eq!(ov.edges, 1);
        assert_eq!(ov.client_to_server_edges, 1);
        assert_eq!(ov.edge_provenance.get("resolved"), Some(&1));
        assert_eq!(
            ov.top_by_centrality.first().map(|n| n.id.as_str()),
            Some("method/common/Сервер/Считать")
        );
    }

    #[test]
    fn manager_module_method_id_round_trips() {
        let a = workspace(&[
            (
                "/src/CommonModules/Вызыватель/Ext/Module.bsl",
                "Процедура Делать() Экспорт\n\
                 Справочники.Контрагенты.НайтиПоИНН();\n\
                 КонецПроцедуры",
            ),
            (
                "/src/Catalogs/Контрагенты/Ext/ManagerModule.bsl",
                "Функция НайтиПоИНН() Экспорт КонецФункции",
            ),
        ]);
        let id = "method/manager/Catalog/Контрагенты/НайтиПоИНН";
        let node =
            a.graph_node(ROOT, None, id, GraphDetail::Names).expect("manager method resolves");
        assert_eq!(node.node.id, id);
        assert_eq!(node.node.qualified, "Справочник.Контрагенты.МодульМенеджера.НайтиПоИНН");

        // And the caller reaches it via an inferred edge.
        let params = NeighborsParams {
            id,
            dir: Direction::In,
            depth: 1,
            max_nodes: 50,
            detail: GraphDetail::Names,
            provenance_filter: Vec::new(),
        };
        let res = a.graph_neighbors(ROOT, None, &params).unwrap();
        assert!(res.nodes.iter().any(|n| n.id == "method/common/Вызыватель/Делать"));
        assert!(res.edges.iter().any(|e| e.provenance == "inferred"));
    }

    #[test]
    fn platform_manager_calls_link_to_mdo_node() {
        // No manager module for Контрагенты, so СоздатьЭлемент/НайтиПоКоду are
        // platform methods that touch the metadata object rather than a user node.
        let a = workspace(&[(
            "/src/CommonModules/Вызыватель/Ext/Module.bsl",
            "Процедура Делать() Экспорт\n\
             Справочники.Контрагенты.СоздатьЭлемент();\n\
             Справочники.Контрагенты.НайтиПоКоду();\n\
             КонецПроцедуры",
        )]);

        let id = "mdo/Catalog/Контрагенты";
        let node = a.graph_node(ROOT, None, id, GraphDetail::Names).expect("mdo node resolves");
        assert_eq!(node.node.id, id);
        assert_eq!(node.node.kind, "mdo");
        assert_eq!(node.node.name, "Контрагенты");
        assert_eq!(node.node.qualified, "Справочник.Контрагенты");
        assert!(node.node.addressable);

        let ov = a.graph_overview(ROOT, None, 10);
        assert_eq!(ov.mdos, 1, "one metadata object node");

        // The Mdo node's callers carry both edge kinds (create + access), deduped
        // by kind rather than collapsed to one.
        let params = NeighborsParams {
            id,
            dir: Direction::In,
            depth: 1,
            max_nodes: 50,
            detail: GraphDetail::Names,
            provenance_filter: Vec::new(),
        };
        let res = a.graph_neighbors(ROOT, None, &params).unwrap();
        assert!(res.nodes.iter().any(|n| n.id == "method/common/Вызыватель/Делать"));
        assert!(res
            .edges
            .iter()
            .any(|e| e.kind == "manager_creates" && e.provenance == "inferred"));
        assert!(res.edges.iter().any(|e| e.kind == "manager_access"));
    }

    #[test]
    fn source_returns_method_body_and_reports_bad_ids() {
        let a = client_server_workspace();
        let ids = vec![
            "method/common/Сервер/Считать".to_string(),
            "method/common/Сервер/НетТакого".to_string(),
        ];
        let result = a.graph_source(ROOT, None, &ids, 4000);
        assert_eq!(result.items.len(), 2);
        assert!(result.items[0].source.as_deref().unwrap().contains("Функция Считать"));
        assert!(result.items[0].error.is_none());
        assert!(result.items[1].source.is_none());
        assert!(matches!(result.items[1].error, Some(GraphError::NotFound { .. })));
    }

    #[test]
    fn source_honors_token_budget() {
        let a = client_server_workspace();
        let ids = vec![
            "method/common/Сервер/Считать".to_string(),
            "method/common/Клиент/Главная".to_string(),
        ];
        // 1 token ≈ 4 chars: the bodies far exceed it, so output is capped.
        let result = a.graph_source(ROOT, None, &ids, 1);
        assert!(result.budget_exhausted);
        let emitted: usize =
            result.items.iter().filter_map(|i| i.source.as_ref()).map(String::len).sum();
        assert!(emitted <= 4, "emitted {emitted} bytes must stay within the 4-byte budget");
        assert!(result.items.iter().all(|i| i.truncated || i.source.is_none()));
    }

    #[test]
    fn provenance_filter_excludes_non_matching_edges() {
        let a = client_server_workspace();
        let params = NeighborsParams {
            id: "method/common/Сервер/Считать",
            dir: Direction::In,
            depth: 1,
            max_nodes: 50,
            detail: GraphDetail::Names,
            provenance_filter: vec!["inferred".to_string()],
        };
        let res = a.graph_neighbors(ROOT, None, &params).unwrap();
        // The only incoming edge is `resolved`, so the inferred-only filter drops it.
        assert!(res.edges.is_empty());
        assert!(res.nodes.is_empty());
    }
}
