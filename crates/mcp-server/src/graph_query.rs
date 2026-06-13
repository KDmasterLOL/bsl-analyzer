//! Read-only serving of the workspace call graph from the SQLite store.
//!
//! Mirrors the agent-facing shapes of `ide::Analysis::graph_*` (the same response
//! structs, hence the same JSON), but every fact comes from the `.build` database
//! built by [`crate::graph_db`] rather than from a resident Salsa database — so a
//! 25k-module config can be served without holding the whole graph in RAM.
//!
//! Source text is read on demand from the file + byte ranges stored per node, so
//! method bodies stay out of the database.

use std::path::Path;

use anyhow::Context;
use ide::{
    classify_graph_id, Direction, EdgeRef, GraphContext, GraphDetail, GraphError, GraphIdKind,
    GraphOverview, NeighborsParams, NeighborsResult, NodeRef, NodeResult, SourceItem, SourceResult,
    MAX_DROPPED_SAMPLE,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use crate::graph_db::SCHEMA_VERSION;

/// A node as stored, before projection to a [`NodeRef`].
struct StoredNode {
    id: String,
    kind: String,
    name: String,
    qualified: String,
    module: Option<String>,
    file: Option<String>,
    name_offset: Option<u32>,
    sig_end: Option<u32>,
    src_start: Option<u32>,
    src_end: Option<u32>,
    dispatch: Option<String>,
    is_export: Option<bool>,
    addressable: bool,
}

const NODE_COLUMNS: &str =
    "id, kind, name, qualified, module, file, name_offset, sig_end, src_start, src_end, dispatch, is_export, addressable";

fn row_to_stored(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredNode> {
    Ok(StoredNode {
        id: row.get(0)?,
        kind: row.get(1)?,
        name: row.get(2)?,
        qualified: row.get(3)?,
        module: row.get(4)?,
        file: row.get(5)?,
        name_offset: row.get::<_, Option<i64>>(6)?.map(|v| v as u32),
        sig_end: row.get::<_, Option<i64>>(7)?.map(|v| v as u32),
        src_start: row.get::<_, Option<i64>>(8)?.map(|v| v as u32),
        src_end: row.get::<_, Option<i64>>(9)?.map(|v| v as u32),
        dispatch: row.get(10)?,
        is_export: row.get::<_, Option<i64>>(11)?.map(|v| v != 0),
        addressable: row.get::<_, i64>(12)? != 0,
    })
}

/// The `[lo, hi)` id range that selects a module's member methods. A `module/<scope>`
/// id's methods are `method/<scope>/<name>`; a `module/file/<rel>` id's methods are
/// `method/file/<rel>::<name>` (the `::` member separator). The half-open upper bound is
/// the prefix with its last (ASCII separator) byte incremented, so the scan rides the
/// `id` primary-key index and never matches a sibling scope. `None` for a non-module id.
fn method_id_range(module_id: &str) -> Option<(String, String)> {
    let scope = module_id.strip_prefix("module/")?;
    if scope.is_empty() {
        return None;
    }
    let sep = if scope.starts_with("file/") { "::" } else { "/" };
    let prefix = format!("method/{scope}{sep}");
    let mut upper = prefix.clone();
    let last = upper.pop()?; // ASCII '/' or ':'
    upper.push(((last as u8) + 1) as char);
    Some((prefix, upper))
}

/// Map a stored node kind to the agent-facing static label `NodeRef` expects.
fn node_kind(kind: &str) -> &'static str {
    match kind {
        "module" => "module",
        "mdo" => "mdo",
        "attribute" => "attribute",
        "form" => "form",
        "form_item" => "form_item",
        "form_attribute" => "form_attribute",
        "tabular_section" => "tabular_section",
        _ => "method",
    }
}

fn dispatch_labels(stored: &Option<String>) -> Vec<&'static str> {
    let mut labels = Vec::new();
    if let Some(d) = stored {
        if d.split(',').any(|t| t == "client") {
            labels.push("client");
        }
        if d.split(',').any(|t| t == "server") {
            labels.push("server");
        }
    }
    labels
}

fn edge_kind(kind: &str) -> &'static str {
    match kind {
        "manager_creates" => "manager_creates",
        "manager_access" => "manager_access",
        "query_ref" => "query_ref",
        "contains" => "contains",
        "data_binding" => "data_binding",
        "notify_ref" => "notify_ref",
        "idle_handler" => "idle_handler",
        "event_subscription" => "event_subscription",
        "register_movement" => "register_movement",
        "subsystem_membership" => "subsystem_membership",
        "role_reference" => "role_reference",
        "register_records" => "register_records",
        _ => "call",
    }
}

fn provenance(p: &str) -> &'static str {
    match p {
        "inferred" => "inferred",
        "visibility_blocked" => "visibility_blocked",
        "unresolved" => "unresolved",
        "string_resolved" => "string_resolved",
        _ => "resolved",
    }
}

/// A read-only handle to a built graph database.
pub struct GraphDb {
    conn: Connection,
}

impl GraphDb {
    /// Open `path` read-only and validate it is a complete build of the current
    /// schema. A truncated build (e.g. a crash mid-write, which leaves no `meta`
    /// rows because they are written last) or a stale schema version is rejected so
    /// the caller rebuilds rather than serving a partial graph.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening graph database at {}", path.display()))?;
        let db = Self { conn };
        db.validate_meta()?;
        Ok(db)
    }

    fn meta(&self, key: &str) -> anyhow::Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
            .with_context(|| format!("reading meta key {key}"))
    }

    fn validate_meta(&self) -> anyhow::Result<()> {
        let version = self
            .meta("schema_version")?
            .context("graph database has no schema_version (incomplete build)")?;
        anyhow::ensure!(
            version == SCHEMA_VERSION.to_string(),
            "graph database schema_version {version} != expected {SCHEMA_VERSION}"
        );
        // `nodes`/`edges` are the last meta rows finalize writes; their presence
        // means the build ran to completion.
        anyhow::ensure!(
            self.meta("nodes")?.is_some() && self.meta("edges")?.is_some(),
            "graph database is missing node/edge counts (incomplete build)"
        );
        Ok(())
    }

    /// The build's freshness token — `(revision, fingerprint, force_stale)` — read
    /// from the file's own `meta`, so a served response's revision/staleness always
    /// describe the exact build being served (never a torn mix where a concurrent
    /// reload renamed a newer file in after the generation was captured elsewhere).
    /// `force_stale` defaults to false when absent.
    pub fn freshness_token(&self) -> anyhow::Result<(u64, u64, bool)> {
        let revision = self
            .meta("revision")?
            .and_then(|v| v.parse().ok())
            .context("graph database meta.revision missing or unparsable")?;
        let fingerprint = self
            .meta("fingerprint")?
            .and_then(|v| v.parse().ok())
            .context("graph database meta.fingerprint missing or unparsable")?;
        let force_stale = self.meta("force_stale")?.map(|v| v == "1").unwrap_or(false);
        Ok((revision, fingerprint, force_stale))
    }

    /// The indexed `.bsl` file count recorded at build time, for status display.
    /// Defaults to 0 when absent (an older build without the row).
    pub fn files(&self) -> anyhow::Result<usize> {
        Ok(self.meta("files")?.and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    fn count(&self, sql: &str) -> anyhow::Result<usize> {
        let n: i64 = self.conn.query_row(sql, [], |r| r.get(0)).context("counting graph rows")?;
        Ok(n as usize)
    }

    fn fetch_node(&self, id: &str) -> anyhow::Result<Option<StoredNode>> {
        self.conn
            .query_row(
                &format!("SELECT {NODE_COLUMNS} FROM nodes WHERE id = ?1"),
                params![id],
                row_to_stored,
            )
            .optional()
            .with_context(|| format!("fetching node {id}"))
    }

    /// Resolve a durable id to a stored node, mirroring the in-memory resolver:
    /// a malformed id is [`GraphError::BadId`]; metadata (`mdo`/`attribute`) ids
    /// match case-insensitively on the object/attribute name (BSL is
    /// case-insensitive), while path/scope ids match exactly (the canonical form
    /// agents receive from the graph). A well-formed but absent id is `NotFound`.
    fn resolve_stored(&self, id: &str) -> anyhow::Result<Result<StoredNode, GraphError>> {
        let kind = match classify_graph_id(id) {
            Ok(k) => k,
            Err(bad) => return Ok(Err(bad)),
        };
        if let Some(node) = self.fetch_node(id)? {
            return Ok(Ok(node));
        }
        // A `module/<scope>` id has no stored row unless the module happened to be a
        // module-level edge endpoint. Synthesize it from its member methods (addressed by
        // the `method/<scope>/…` id prefix) so `node(module/…)` resolves and lists members
        // — without polluting the graph with module nodes/edges.
        if matches!(&kind, GraphIdKind::Module { .. } | GraphIdKind::ModuleFile { .. }) {
            return Ok(match self.synthesize_module_node(id)? {
                Some(node) => Ok(node),
                None => Err(GraphError::NotFound { id: id.to_string() }),
            });
        }
        // Case-insensitive fallback for metadata ids only. Both the prefix and the
        // comparison target are rebuilt from the PARSED type's English name (not the
        // raw id segment), so a localized type spelling (`Справочник` → `Catalog`)
        // still matches the stored canonical id. The object/attribute name may be
        // Cyrillic, which SQL cannot fold, so the final compare is done in Rust.
        let (sql_kind, prefix, target) = match &kind {
            GraphIdKind::Mdo { mdo_type, object } => {
                let eng = mdo_type.english_name();
                ("mdo", format!("mdo/{eng}/"), format!("mdo/{eng}/{object}").to_lowercase())
            }
            GraphIdKind::Attribute { mdo_type, object, attr } => {
                let eng = mdo_type.english_name();
                (
                    "attribute",
                    format!("attribute/{eng}/"),
                    format!("attribute/{eng}/{object}/{attr}").to_lowercase(),
                )
            }
            GraphIdKind::Form { owner, form_name } => match owner {
                Some((mdo_type, object)) => {
                    let eng = mdo_type.english_name();
                    (
                        "form",
                        format!("form/{eng}/"),
                        format!("form/{eng}/{object}/{form_name}").to_lowercase(),
                    )
                }
                None => (
                    "form",
                    "form/common/".to_string(),
                    format!("form/common/{form_name}").to_lowercase(),
                ),
            },
            GraphIdKind::FormItem { owner, form_name, item_name } => match owner {
                Some((mdo_type, object)) => {
                    let eng = mdo_type.english_name();
                    (
                        "form_item",
                        format!("form_item/{eng}/"),
                        format!("form_item/{eng}/{object}/{form_name}/{item_name}").to_lowercase(),
                    )
                }
                None => (
                    "form_item",
                    "form_item/common/".to_string(),
                    format!("form_item/common/{form_name}/{item_name}").to_lowercase(),
                ),
            },
            GraphIdKind::FormAttribute { owner, form_name, attr_name } => match owner {
                Some((mdo_type, object)) => {
                    let eng = mdo_type.english_name();
                    (
                        "form_attribute",
                        format!("form_attr/{eng}/"),
                        format!("form_attr/{eng}/{object}/{form_name}/{attr_name}").to_lowercase(),
                    )
                }
                None => (
                    "form_attribute",
                    "form_attr/common/".to_string(),
                    format!("form_attr/common/{form_name}/{attr_name}").to_lowercase(),
                ),
            },
            GraphIdKind::TabularSection { mdo_type, object, section } => {
                let eng = mdo_type.english_name();
                (
                    "tabular_section",
                    format!("tabular_section/{eng}/"),
                    format!("tabular_section/{eng}/{object}/{section}").to_lowercase(),
                )
            }
            GraphIdKind::TabularSectionAttribute { mdo_type, object, section, attr } => {
                let eng = mdo_type.english_name();
                // Stored as an `attribute`-kind node with a `ts_attr/` id.
                (
                    "attribute",
                    format!("ts_attr/{eng}/"),
                    format!("ts_attr/{eng}/{object}/{section}/{attr}").to_lowercase(),
                )
            }
            _ => return Ok(Err(GraphError::NotFound { id: id.to_string() })),
        };
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {NODE_COLUMNS} FROM nodes WHERE kind = ?1 AND id LIKE ?2"))?;
        let rows = stmt.query_map(params![sql_kind, format!("{prefix}%")], row_to_stored)?;
        for row in rows {
            let node = row?;
            if node.id.to_lowercase() == target {
                return Ok(Ok(node));
            }
        }
        Ok(Err(GraphError::NotFound { id: id.to_string() }))
    }

    /// Synthesize a `module` node from its member methods. A module has a stored row only
    /// when it was an edge endpoint, but its methods are always present as
    /// `method/<scope>/…` rows; the first member supplies the module's file and display
    /// name. `None` when the module has no methods (then `node` reports `not_found`).
    fn synthesize_module_node(&self, id: &str) -> anyhow::Result<Option<StoredNode>> {
        let Some((lo, hi)) = method_id_range(id) else { return Ok(None) };
        let first: Option<(Option<String>, Option<String>)> = self
            .conn
            .query_row(
                "SELECT file, module FROM nodes WHERE kind = 'method' AND id >= ?1 AND id < ?2 \
                 ORDER BY id LIMIT 1",
                params![lo, hi],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .context("probing module members")?;
        let Some((file, module_display)) = first else { return Ok(None) };
        let name = module_display.clone().unwrap_or_else(|| id.to_string());
        Ok(Some(StoredNode {
            id: id.to_string(),
            kind: "module".to_string(),
            name: name.clone(),
            qualified: name,
            module: module_display,
            file,
            name_offset: None,
            sig_end: None,
            src_start: None,
            src_end: None,
            dispatch: None,
            is_export: None,
            addressable: true,
        }))
    }

    /// The member methods of a `module/<scope>` node, addressed by the `method/<scope>/…`
    /// id prefix (the durable scope, NOT the `module` display column).
    fn module_members(&self, module_id: &str) -> anyhow::Result<Vec<ide::ModuleMethod>> {
        let Some((lo, hi)) = method_id_range(module_id) else { return Ok(Vec::new()) };
        let mut stmt = self.conn.prepare(
            "SELECT id, name, is_export FROM nodes WHERE kind = 'method' AND id >= ?1 AND id < ?2 \
             ORDER BY name",
        )?;
        let rows = stmt.query_map(params![lo, hi], |r| {
            Ok(ide::ModuleMethod {
                id: r.get(0)?,
                name: r.get(1)?,
                is_export: r.get::<_, Option<i64>>(2)?.map(|v| v != 0),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().context("listing module members")
    }

    /// The true distinct-module count: every module that owns a method (derived from the
    /// `method/<scope>/…` id prefix via [`ide::module_id_of_method`]) unioned with any
    /// `module`-kind row (a module body persisted only because it was an edge endpoint).
    /// Counting `kind='module'` rows alone undercounts, since module nodes are synthesized
    /// on demand and not generally stored — the symptom the agent saw as `modules=13`.
    fn count_modules(&self) -> anyhow::Result<usize> {
        let mut modules: std::collections::HashSet<String> = std::collections::HashSet::new();
        {
            let mut stmt = self.conn.prepare("SELECT id FROM nodes WHERE kind='module'")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for id in rows {
                modules.insert(id?);
            }
        }
        {
            let mut stmt = self.conn.prepare("SELECT id FROM nodes WHERE kind='method'")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for id in rows {
                if let Some(module) = ide::module_id_of_method(&id?) {
                    modules.insert(module);
                }
            }
        }
        Ok(modules.len())
    }

    /// Near-miss id lookup: rank every node's durable id against an imprecise `query`
    /// (wrong casing, bare method/object name, or partial id), capped at `limit`, through the
    /// shared [`ide::rank_resolve_candidates`] ranker.
    ///
    /// Stored nodes alone are not enough: module nodes are synthesized on demand and generally
    /// absent from the table (only persisted when they happen to be an edge endpoint), so a
    /// wrong-cased `module/common/<name>` query would find no candidate even though
    /// `graph(node)` recovers it. So we ALSO derive each owning-module id from its method rows
    /// (the same union [`Self::count_modules`] uses), deduped against the stored set — matching
    /// the in-memory `Analysis::graph_resolve`, which sees module nodes directly.
    pub fn resolve(&self, query: &str, limit: usize) -> anyhow::Result<ide::ResolveResult> {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut candidates: Vec<(String, &'static str)> = Vec::new();
        {
            let mut stmt = self.conn.prepare("SELECT id, kind FROM nodes")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .context("scanning nodes for resolve")?;
            for (id, kind) in rows {
                if seen.insert(id.clone()) {
                    candidates.push((id, node_kind(&kind)));
                }
            }
        }
        {
            let mut stmt = self.conn.prepare("SELECT id FROM nodes WHERE kind='method'")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for id in rows {
                if let Some(module) = ide::module_id_of_method(&id?) {
                    if seen.insert(module.clone()) {
                        candidates.push((module, node_kind("module")));
                    }
                }
            }
        }
        let (candidates, total) =
            ide::rank_resolve_candidates(candidates.into_iter(), query, limit);
        Ok(ide::ResolveResult::new(query, candidates, total))
    }

    fn in_degree(&self, id: &str) -> anyhow::Result<usize> {
        let d: Option<i64> = self
            .conn
            .query_row("SELECT degree FROM in_degree WHERE id = ?1", params![id], |r| r.get(0))
            .optional()
            .context("reading in_degree")?;
        Ok(d.unwrap_or(0) as usize)
    }

    /// The full declaration signature, from the keyword line containing `name_offset`
    /// through the header end `sig_end` (the closing `)` or export keyword). Internal
    /// runs of whitespace — including the newlines of a wrapped parameter list — are
    /// collapsed to single spaces so a multi-line declaration reads as one line.
    fn signature_at(&self, file: &str, name_offset: u32, sig_end: u32) -> Option<String> {
        let text = std::fs::read_to_string(file).ok()?;
        let name = (name_offset as usize).min(text.len());
        let end = (sig_end as usize).min(text.len());
        if name > end || !text.is_char_boundary(name) || !text.is_char_boundary(end) {
            return None;
        }
        let start = text[..name].rfind('\n').map_or(0, |i| i + 1);
        let slice = text.get(start..end)?;
        Some(slice.split_whitespace().collect::<Vec<_>>().join(" "))
    }

    fn slice(&self, file: &str, start: u32, end: u32) -> Option<String> {
        let text = std::fs::read_to_string(file).ok()?;
        // Line endings are normalized to LF before redaction and budget clamping:
        // consumers read the source, they do not need byte-exact CRLF, and the CR
        // would only inflate the JSON escaping.
        text.get(start as usize..end as usize).map(|s| s.replace("\r\n", "\n"))
    }

    /// Project a stored node to its agent-facing [`NodeRef`] at `detail`.
    fn node_ref(&self, n: &StoredNode, detail: GraphDetail) -> NodeRef {
        let kind = node_kind(&n.kind);
        let mut node = NodeRef {
            id: n.id.clone(),
            kind,
            name: n.name.clone(),
            // For code nodes the stored qualified merely restates module + name, so it
            // is not served; metadata nodes keep their russified display path.
            qualified: (!matches!(kind, "method" | "module")).then(|| n.qualified.clone()),
            module: n.module.clone(),
            signature: None,
            source: None,
            truncated: false,
            dispatch: dispatch_labels(&n.dispatch),
            is_export: n.is_export,
            // Populated by `node()` for a `module` node (the member list); a separate
            // query, so it is not done in this projection helper.
            methods: None,
            addressable: n.addressable,
        };
        if n.kind == "method" && matches!(detail, GraphDetail::Signatures | GraphDetail::Bodies) {
            if let (Some(file), Some(off), Some(end)) = (&n.file, n.name_offset, n.sig_end) {
                node.signature = self.signature_at(file, off, end);
            }
            if detail == GraphDetail::Bodies {
                if let (Some(file), Some(s), Some(e)) = (&n.file, n.src_start, n.src_end) {
                    node.source = self.slice(file, s, e);
                }
            }
        }
        node
    }

    /// Cold-start overview: node/edge tallies, the most-called nodes, and the
    /// provenance/dispatch profile.
    pub fn overview(&self, top_n: usize) -> anyhow::Result<GraphOverview> {
        let nodes = self.count("SELECT COUNT(*) FROM nodes")?;
        let methods = self.count("SELECT COUNT(*) FROM nodes WHERE kind='method'")?;
        let modules = self.count_modules()?;
        let mdos = self.count("SELECT COUNT(*) FROM nodes WHERE kind='mdo'")?;
        let attributes = self.count("SELECT COUNT(*) FROM nodes WHERE kind='attribute'")?;
        let tabular_sections =
            self.count("SELECT COUNT(*) FROM nodes WHERE kind='tabular_section'")?;
        let forms = self.count("SELECT COUNT(*) FROM nodes WHERE kind='form'")?;
        let form_items = self.count("SELECT COUNT(*) FROM nodes WHERE kind='form_item'")?;
        let form_attributes =
            self.count("SELECT COUNT(*) FROM nodes WHERE kind='form_attribute'")?;
        let edges = self.count("SELECT COUNT(*) FROM edges")?;
        let client_to_server_edges = self.count("SELECT COUNT(*) FROM edges WHERE crosses=1")?;

        let mut edge_provenance = std::collections::BTreeMap::new();
        {
            let mut stmt =
                self.conn.prepare("SELECT provenance, COUNT(*) FROM edges GROUP BY provenance")?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))?;
            for row in rows {
                let (p, c) = row?;
                edge_provenance.insert(provenance(&p), c);
            }
        }

        let top_ids: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM in_degree ORDER BY degree DESC, id ASC LIMIT ?1")?;
            let rows = stmt.query_map(params![top_n as i64], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let mut top_by_centrality = Vec::with_capacity(top_ids.len());
        for id in top_ids {
            if let Some(n) = self.fetch_node(&id)? {
                top_by_centrality.push(self.node_ref(&n, GraphDetail::Signatures));
            }
        }

        Ok(GraphOverview {
            modules,
            methods,
            mdos,
            attributes,
            tabular_sections,
            forms,
            form_items,
            form_attributes,
            nodes,
            edges,
            top_by_centrality,
            edge_provenance,
            client_to_server_edges,
        })
    }

    /// Resolve a durable id to a single node at `detail`. The id must match a
    /// stored node exactly (the ids agents receive from the graph are canonical).
    pub fn node(
        &self,
        id: &str,
        detail: GraphDetail,
    ) -> anyhow::Result<Result<NodeResult, GraphError>> {
        let stored = match self.resolve_stored(id)? {
            Ok(n) => n,
            Err(e) => return Ok(Err(e)),
        };
        let mut node = self.node_ref(&stored, detail);
        // A `module` node lists its members so an agent discovers them from `node(module/…)`
        // directly, without a traversal.
        if stored.kind == "module" {
            node.methods = Some(self.module_members(&stored.id)?);
        }
        Ok(Ok(NodeResult { node }))
    }

    fn directed_edges(
        &self,
        node_id: &str,
        dir: Direction,
        provenance_filter: &[String],
        kind_filter: &[String],
    ) -> anyhow::Result<Vec<StoredEdge>> {
        let mut edges = Vec::new();
        if matches!(dir, Direction::Out | Direction::Both) {
            self.collect_edges("from_id", node_id, provenance_filter, kind_filter, &mut edges)?;
        }
        if matches!(dir, Direction::In | Direction::Both) {
            self.collect_edges("to_id", node_id, provenance_filter, kind_filter, &mut edges)?;
        }
        Ok(edges)
    }

    fn collect_edges(
        &self,
        column: &str,
        node_id: &str,
        provenance_filter: &[String],
        kind_filter: &[String],
        out: &mut Vec<StoredEdge>,
    ) -> anyhow::Result<()> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT from_id, to_id, kind, provenance, crosses FROM edges WHERE {column} = ?1",
        ))?;
        let rows = stmt.query_map(params![node_id], |r| {
            Ok(StoredEdge {
                from: r.get(0)?,
                to: r.get(1)?,
                kind: r.get(2)?,
                provenance: r.get(3)?,
                crosses: r.get::<_, i64>(4)? != 0,
            })
        })?;
        for row in rows {
            let edge = row?;
            let prov_ok = provenance_filter.is_empty()
                || provenance_filter.iter().any(|p| *p == provenance(&edge.provenance));
            let kind_ok =
                kind_filter.is_empty() || kind_filter.iter().any(|k| *k == edge_kind(&edge.kind));
            if prov_ok && kind_ok {
                out.push(edge);
            }
        }
        Ok(())
    }

    /// Traverse callers/callees from a node up to `depth`, bounded by `max_nodes`
    /// (the lowest-centrality discovered nodes are the ones dropped past the cap).
    pub fn neighbors(
        &self,
        params: &NeighborsParams<'_>,
    ) -> anyhow::Result<Result<NeighborsResult, GraphError>> {
        let root = match self.resolve_stored(params.id)? {
            Ok(n) => n,
            Err(err) => return Ok(Err(err)),
        };
        let depth = params.depth.max(1);

        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        seen.insert(root.id.clone());
        let mut discovered: Vec<String> = Vec::new();
        let mut out_edges: Vec<StoredEdge> = Vec::new();
        // Distinct non-root nodes reached downstream vs upstream (mirrors the in-memory
        // path) so a `Both` traversal reports each direction's fan-out.
        let mut out_reached: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut in_reached: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut frontier = vec![root.id.clone()];

        for _ in 0..depth {
            let mut next = Vec::new();
            for node_id in &frontier {
                for edge in self.directed_edges(
                    node_id,
                    params.dir,
                    &params.provenance_filter,
                    &params.edge_kind_filter,
                )? {
                    let downstream = &edge.from == node_id;
                    let other = if downstream { edge.to.clone() } else { edge.from.clone() };
                    if other != root.id {
                        if downstream {
                            out_reached.insert(other.clone());
                        } else {
                            in_reached.insert(other.clone());
                        }
                    }
                    out_edges.push(edge);
                    if seen.insert(other.clone()) {
                        next.push(other.clone());
                        discovered.push(other);
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        let out_total =
            matches!(params.dir, Direction::Out | Direction::Both).then_some(out_reached.len());
        let in_total =
            matches!(params.dir, Direction::In | Direction::Both).then_some(in_reached.len());

        // Centrality-ranked tail-drop of discovered (non-root) nodes.
        let mut ranked: Vec<(usize, String)> = Vec::with_capacity(discovered.len());
        for id in discovered {
            ranked.push((self.in_degree(&id)?, id));
        }
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let total = ranked.len();
        let mut dropped = Vec::new();
        if ranked.len() > params.max_nodes {
            for (_, id) in ranked.split_off(params.max_nodes).into_iter().take(MAX_DROPPED_SAMPLE) {
                dropped.push(id);
            }
        }
        let kept: std::collections::HashSet<&String> = ranked.iter().map(|(_, id)| id).collect();

        let mut nodes = Vec::with_capacity(ranked.len());
        for (_, id) in &ranked {
            if let Some(n) = self.fetch_node(id)? {
                nodes.push(self.node_ref(&n, params.detail));
            }
        }

        // Distribution + connector-loss over the deduped full neighbourhood (every
        // discovered edge, before the node-cap edge-survival filter), mirroring the
        // in-memory serve path so the counts are byte-identical.
        let mut counted: std::collections::HashSet<(String, String, String)> =
            std::collections::HashSet::new();
        let mut by_kind: std::collections::BTreeMap<&'static str, usize> =
            std::collections::BTreeMap::new();
        let mut by_provenance: std::collections::BTreeMap<&'static str, usize> =
            std::collections::BTreeMap::new();
        let mut connectors_dropped = false;
        for e in &out_edges {
            if !counted.insert((e.from.clone(), e.to.clone(), e.kind.clone())) {
                continue;
            }
            *by_kind.entry(edge_kind(&e.kind)).or_default() += 1;
            *by_provenance.entry(provenance(&e.provenance)).or_default() += 1;
            let survives = (e.from == root.id || kept.contains(&e.from))
                && (e.to == root.id || kept.contains(&e.to));
            if !survives {
                connectors_dropped = true;
            }
        }

        // Keep only edges whose endpoints both survived; dedup by (from, to, kind)
        // so a `Both` sweep that meets an edge from each end emits it once. An
        // endpoint equal to the root is omitted (the response carries the root once).
        let mut seen_edges: std::collections::HashSet<(String, String, String)> =
            std::collections::HashSet::new();
        let edges = out_edges
            .into_iter()
            .filter(|e| {
                (e.from == root.id || kept.contains(&e.from))
                    && (e.to == root.id || kept.contains(&e.to))
            })
            .filter(|e| seen_edges.insert((e.from.clone(), e.to.clone(), e.kind.clone())))
            .map(|e| EdgeRef {
                kind: edge_kind(&e.kind),
                provenance: provenance(&e.provenance),
                crosses_client_to_server: e.crosses,
                from: (e.from != root.id).then_some(e.from),
                to: (e.to != root.id).then_some(e.to),
            })
            .collect();

        let returned = nodes.len();
        let confidence = (!by_provenance.is_empty()).then(|| ide::confidence_label(&by_provenance));
        Ok(Ok(NeighborsResult {
            root: self.node_ref(&root, params.detail),
            nodes,
            edges,
            total,
            returned,
            dropped_count: total - returned,
            dropped,
            by_kind,
            by_provenance,
            confidence,
            connectors_dropped,
            out_total,
            in_total,
        }))
    }

    /// Render a method's outbound graph context (dispatch, signature, calls, metadata
    /// reads) from the stored graph — the SQLite-backed twin of
    /// [`ide::Analysis::graph_context_for_method`]. Returns byte-identical text to the
    /// in-memory renderer (guarded by a parity test), so a chunk enriched from either
    /// source keys the same embedding. `None` for a non-method id or one absent from
    /// the graph.
    pub fn graph_context(&self, id: &str) -> anyhow::Result<Option<String>> {
        let node = match self.fetch_node(id)? {
            Some(n) if n.kind == "method" => n,
            _ => return Ok(None),
        };
        let nref = self.node_ref(&node, GraphDetail::Signatures);

        // Mirror the in-memory renderer's facts exactly by EDGE kind, not just target
        // kind: calls come only from `call` edges, reads only from a method's
        // metadata-touch edges (`manager_*` / `query_ref`). A method never has
        // `contains`/`data_binding` outbound edges (those originate at mdo/form nodes),
        // but gating on the kind keeps this equivalent even if that changes.
        let is_read_edge =
            |kind: &str| matches!(kind, "manager_creates" | "manager_access" | "query_ref");
        let mut calls = Vec::new();
        let mut reads = Vec::new();
        for edge in self.directed_edges(id, Direction::Out, &[], &[])? {
            match classify_graph_id(&edge.to) {
                Ok(GraphIdKind::Method { name, .. }) | Ok(GraphIdKind::MethodFile { name, .. })
                    if edge.kind == "call" =>
                {
                    calls.push(name);
                }
                Ok(GraphIdKind::Mdo { mdo_type, object }) if is_read_edge(&edge.kind) => {
                    reads.push(format!("{}.{}", mdo_type.russian_name(), object));
                }
                Ok(GraphIdKind::Attribute { mdo_type, object, attr })
                    if is_read_edge(&edge.kind) =>
                {
                    reads.push(format!("{}.{}.{}", mdo_type.russian_name(), object, attr));
                }
                _ => {}
            }
        }
        calls.sort();
        calls.dedup();
        reads.sort();
        reads.dedup();

        let ctx = GraphContext { dispatch: nref.dispatch, signature: nref.signature, calls, reads };
        Ok(Some(ctx.render()))
    }

    /// Fetch method source for a set of ids, stopping once the rough output budget
    /// (`max_output_tokens`, ~4 chars/token) is reached.
    pub fn source(&self, ids: &[String], max_output_tokens: usize) -> anyhow::Result<SourceResult> {
        let budget_chars = max_output_tokens.saturating_mul(4).max(1);
        let mut used = 0usize;
        let mut budget_exhausted = false;
        let mut items = Vec::with_capacity(ids.len());

        for id in ids {
            let item = match self.resolve_stored(id)? {
                Err(err) => SourceItem {
                    id: id.clone(),
                    source: None,
                    error: Some(err),
                    truncated: false,
                    skipped_budget_exhausted: false,
                },
                Ok(n) if n.kind != "method" => {
                    let reason = if n.kind == "module" {
                        "module-body source is not served; request a method"
                    } else {
                        "a metadata node has no source; request a method"
                    };
                    SourceItem {
                        id: id.clone(),
                        source: None,
                        error: Some(GraphError::Unsupported {
                            id: id.clone(),
                            reason: reason.into(),
                        }),
                        truncated: false,
                        skipped_budget_exhausted: false,
                    }
                }
                Ok(n) => match (n.file.as_deref(), n.src_start, n.src_end) {
                    (Some(file), Some(s), Some(e)) => match self.slice(file, s, e) {
                        Some(_) if used >= budget_chars => {
                            budget_exhausted = true;
                            SourceItem {
                                id: id.clone(),
                                source: None,
                                error: None,
                                truncated: true,
                                skipped_budget_exhausted: true,
                            }
                        }
                        Some(src) => {
                            let remaining = budget_chars - used;
                            let (text, truncated) = clamp_source(src, remaining);
                            used += text.len();
                            budget_exhausted |= truncated;
                            SourceItem {
                                id: id.clone(),
                                source: Some(text),
                                error: None,
                                truncated,
                                skipped_budget_exhausted: false,
                            }
                        }
                        None => SourceItem {
                            id: id.clone(),
                            source: None,
                            error: Some(GraphError::NotFound { id: id.clone() }),
                            truncated: false,
                            skipped_budget_exhausted: false,
                        },
                    },
                    _ => SourceItem {
                        id: id.clone(),
                        source: None,
                        error: Some(GraphError::NotFound { id: id.clone() }),
                        truncated: false,
                        skipped_budget_exhausted: false,
                    },
                },
            };
            items.push(item);
        }

        Ok(SourceResult { items, budget_exhausted })
    }
}

/// A [`bsl_search::GraphContextProvider`] backed by the on-disk graph
/// ([`GraphDb`]). This is the production source for bulk index enrichment: reading a
/// method's outbound facts from the prebuilt `.build/bsl-graph.db` is RAM-bounded and
/// shares the graph's freshness, unlike rendering from a whole-workspace `Analysis`.
///
/// `GraphDb` holds a non-`Sync` rusqlite connection; the [`Mutex`] makes the provider
/// `Sync` for the trait. Calls are sequential at the chunk-text stage, so contention
/// is nil.
pub struct GraphDbContextProvider {
    db: std::sync::Mutex<GraphDb>,
}

impl GraphDbContextProvider {
    pub fn new(db: GraphDb) -> Self {
        Self { db: std::sync::Mutex::new(db) }
    }
}

impl bsl_search::GraphContextProvider for GraphDbContextProvider {
    fn graph_context(&self, rel_path: &str, symbol_name: &str, _kind: &str) -> Option<String> {
        // Methods in metadata-keyed modules resolve to a durable id; form/command
        // modules (path-fallback ids) are not enriched here.
        let id = ide::method_id_for_path(rel_path, symbol_name)?;
        let db = self.db.lock().ok()?;
        db.graph_context(&id).ok().flatten()
    }
}

struct StoredEdge {
    from: String,
    to: String,
    kind: String,
    provenance: String,
    crosses: bool,
}

/// Truncate `src` to at most `max_chars` bytes on a char boundary.
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
    use super::{edge_kind, method_id_range, provenance, GraphDb};
    use rusqlite::{params, Connection};

    #[test]
    fn stored_callback_edge_kinds_round_trip_not_collapsed_to_call() {
        // Regression: the normalizers fell through to "call"/"resolved" for unknown
        // stored strings, so persisted callback edges served as plain calls.
        assert_eq!(edge_kind("notify_ref"), "notify_ref");
        assert_eq!(edge_kind("idle_handler"), "idle_handler");
        assert_eq!(edge_kind("event_subscription"), "event_subscription");
        assert_eq!(provenance("string_resolved"), "string_resolved");
        // The catch-all still normalizes a genuinely unknown string.
        assert_eq!(edge_kind("call"), "call");
        assert_eq!(provenance("resolved"), "resolved");
    }

    /// A minimal in-memory graph holding only the `nodes` columns `resolve` reads.
    fn graph_db_with_nodes(rows: &[(&str, &str)]) -> GraphDb {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE nodes (id TEXT NOT NULL, kind TEXT NOT NULL);").unwrap();
        for (id, kind) in rows {
            conn.execute("INSERT INTO nodes (id, kind) VALUES (?1, ?2)", params![id, kind])
                .unwrap();
        }
        GraphDb { conn }
    }

    #[test]
    fn resolve_recovers_wrong_cased_module_id_from_member_methods() {
        // Module nodes are synthesized on demand and not stored; only the methods are. A
        // wrong-cased `module/...` query must still resolve via the owning-module id derived
        // from a member method (the bug: it previously found nothing).
        let db = graph_db_with_nodes(&[(
            "method/common/СтроковыеФункцииКлиентСервер/ПодставитьПараметрыВСтроку",
            "method",
        )]);
        let res = db.resolve("module/common/строковыефункцииклиентсервер", 10).unwrap();
        let module = res
            .candidates
            .iter()
            .find(|c| c.kind == "module")
            .expect("a module candidate is derived from the member method");
        assert_eq!(module.id, "module/common/СтроковыеФункцииКлиентСервер");
        assert_eq!(module.match_kind, "case_insensitive");
    }

    #[test]
    fn resolve_does_not_duplicate_a_stored_module() {
        // A module that is BOTH stored (an edge endpoint) and derivable from its methods must
        // appear once, not twice — the derived id is deduped against the stored set.
        let db = graph_db_with_nodes(&[
            ("module/common/Сервер", "module"),
            ("method/common/Сервер/Считать", "method"),
        ]);
        let res = db.resolve("module/common/Сервер", 10).unwrap();
        let modules: Vec<_> =
            res.candidates.iter().filter(|c| c.id == "module/common/Сервер").collect();
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].match_kind, "exact");
    }

    #[test]
    fn method_id_range_covers_each_module_form() {
        // Common/manager/object modules use the `/` member separator.
        let (lo, hi) = method_id_range("module/common/Сервер").unwrap();
        assert_eq!(lo, "method/common/Сервер/");
        assert_eq!(hi, "method/common/Сервер0"); // '/' (0x2F) bumped to '0' (0x30)
        assert!("method/common/Сервер/Считать" >= lo.as_str());
        assert!("method/common/Сервер/Считать" < hi.as_str());
        // A sibling scope (longer name sharing the prefix) is NOT in range.
        assert!("method/common/СерверДва/М" >= hi.as_str());

        let (lo, _) = method_id_range("module/manager/Catalog/Товары").unwrap();
        assert_eq!(lo, "method/manager/Catalog/Товары/");

        // File modules use the `::` member separator.
        let (lo, hi) = method_id_range("module/file/src/cf/Forms/A/Module.bsl").unwrap();
        assert_eq!(lo, "method/file/src/cf/Forms/A/Module.bsl::");
        assert_eq!(hi, "method/file/src/cf/Forms/A/Module.bsl:;"); // ':' bumped to ';'
        assert!("method/file/src/cf/Forms/A/Module.bsl::ПриОткрытии" >= lo.as_str());
        assert!("method/file/src/cf/Forms/A/Module.bsl::ПриОткрытии" < hi.as_str());

        // Not a module id (no `module/` prefix), and an empty scope.
        assert!(method_id_range("method/common/X/Y").is_none());
        assert!(method_id_range("module/").is_none());
    }
}
