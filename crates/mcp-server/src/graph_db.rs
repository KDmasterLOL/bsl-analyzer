//! On-disk SQLite store for the workspace call graph.
//!
//! The whole-config in-memory graph does not fit in RAM on large configurations
//! (a 25k-file ERP needs tens of GB). The graph is therefore built in bounded
//! batches and streamed into a SQLite file, which is then queried to serve `graph`
//! tool calls without ever materialising the full node/edge set in memory.
//!
//! The database is a **derived cache**: every row is reconstructable from the
//! sources, so the writer favours bulk-insert throughput (in-memory journal, no
//! per-row fsync) over crash durability. A truncated or corrupt file is detected
//! on open and rebuilt rather than trusted.
//!
//! Durable node ids ([`NodeRow::id`]) are produced by the build-time encoder in
//! [`hir::graph_index`], byte-identical to the ids the in-memory serving path
//! emits, so ids an agent holds survive the in-memory → SQLite switch.

use std::path::Path;

use anyhow::Context;
use base_db::SourceRootId;
use ide::graph_index::{EdgeRow, NodeRow};
use ide::{GraphBuildSummary, RootDatabaseImpl};
use rusqlite::{params, Connection};

/// Bumped whenever the table layout changes so a stale on-disk cache from an
/// older binary is rejected (via the `meta` row) and rebuilt.
pub(crate) const SCHEMA_VERSION: u32 = 1;

/// Build-level metadata recorded in the `meta` table, used on reopen to decide
/// whether a cached database still matches the current sources and binary. Node
/// and edge counts are derived from the bulk data at finalize time, not supplied.
pub struct GraphMeta {
    /// The [`GraphState`](crate::graph) generation this build reflects.
    pub revision: u64,
    /// On-disk fingerprint of the source tree at build time.
    pub fingerprint: u64,
    /// Number of `.bsl` files indexed.
    pub files: usize,
    /// RFC 3339 build timestamp.
    pub built_at: String,
}

/// Streams graph rows into a fresh SQLite file. Created once per build; nodes and
/// edges are appended in batches, then [`finalize`](Self::finalize) builds the
/// secondary indexes and the in-degree table in one pass over the bulk data.
pub(crate) struct GraphDbWriter {
    conn: Connection,
}

impl GraphDbWriter {
    /// Open `path` as a fresh database, discarding any prior file at that path so
    /// a stale schema can never leak into the new build. Sets bulk-load pragmas.
    pub(crate) fn create(path: &Path) -> anyhow::Result<Self> {
        for suffix in ["", "-wal", "-shm"] {
            let sibling = path.with_file_name(format!(
                "{}{suffix}",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("bsl-graph.db")
            ));
            let _ = std::fs::remove_file(&sibling);
        }

        let conn = Connection::open(path)
            .with_context(|| format!("opening graph database at {}", path.display()))?;
        // A rebuildable cache: trade durability for bulk-insert throughput.
        conn.execute_batch(
            "
            PRAGMA journal_mode = MEMORY;
            PRAGMA synchronous = OFF;
            PRAGMA temp_store = MEMORY;
            PRAGMA cache_size = -65536;

            CREATE TABLE nodes (
                id          TEXT PRIMARY KEY,
                kind        TEXT NOT NULL,
                name        TEXT NOT NULL,
                qualified   TEXT NOT NULL,
                module      TEXT,
                file        TEXT,
                name_offset INTEGER,
                src_start   INTEGER,
                src_end     INTEGER,
                dispatch    TEXT,
                is_export   INTEGER,
                addressable INTEGER NOT NULL
            ) WITHOUT ROWID;

            CREATE TABLE edges (
                from_id    TEXT NOT NULL,
                to_id      TEXT NOT NULL,
                kind       TEXT NOT NULL,
                provenance TEXT NOT NULL,
                crosses    INTEGER NOT NULL
            );

            CREATE TABLE in_degree (
                id     TEXT PRIMARY KEY,
                degree INTEGER NOT NULL
            ) WITHOUT ROWID;

            CREATE TABLE meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )
        .context("initialising graph schema")?;

        Ok(Self { conn })
    }

    /// Append a batch of nodes. A node id may be projected more than once across
    /// batches (the same MDO/method reached from several callers); the first
    /// spelling wins and later duplicates are ignored, matching the in-memory
    /// graph's first-seen node identity.
    pub(crate) fn write_nodes(&mut self, rows: &[NodeRow]) -> anyhow::Result<()> {
        let tx = self.conn.transaction().context("begin node batch")?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO nodes \
                 (id, kind, name, qualified, module, file, name_offset, src_start, src_end, \
                  dispatch, is_export, addressable) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            for row in rows {
                let dispatch =
                    if row.dispatch.is_empty() { None } else { Some(row.dispatch.join(",")) };
                stmt.execute(params![
                    row.id,
                    row.kind,
                    row.name,
                    row.qualified,
                    row.module,
                    row.file,
                    row.name_offset,
                    row.src_start,
                    row.src_end,
                    dispatch,
                    row.is_export.map(|b| b as i64),
                    row.addressable as i64,
                ])?;
            }
        }
        tx.commit().context("commit node batch")?;
        Ok(())
    }

    /// Append a batch of edges verbatim. Edge multiplicity is preserved as given;
    /// de-duplication, if any, is the build orchestrator's policy.
    pub(crate) fn write_edges(&mut self, rows: &[EdgeRow]) -> anyhow::Result<()> {
        let tx = self.conn.transaction().context("begin edge batch")?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO edges (from_id, to_id, kind, provenance, crosses) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for row in rows {
                stmt.execute(params![
                    row.from_id,
                    row.to_id,
                    row.kind,
                    row.provenance,
                    row.crosses as i64,
                ])?;
            }
        }
        tx.commit().context("commit edge batch")?;
        Ok(())
    }

    /// Build secondary indexes, materialise the in-degree table from the bulk
    /// edges, and record build metadata (including derived node/edge counts).
    /// Consumes the writer — no further rows may be appended.
    pub(crate) fn finalize(mut self, meta: &GraphMeta) -> anyhow::Result<()> {
        self.conn
            .execute_batch(
                "
                CREATE INDEX edges_from ON edges(from_id);
                CREATE INDEX edges_to ON edges(to_id);
                CREATE INDEX nodes_kind ON nodes(kind);

                INSERT INTO in_degree (id, degree)
                    SELECT to_id, COUNT(*) FROM edges GROUP BY to_id;
                ",
            )
            .context("finalising graph indexes")?;

        let nodes: i64 = self.conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?;
        let edges: i64 = self.conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;

        let rows: [(&str, String); 7] = [
            ("schema_version", SCHEMA_VERSION.to_string()),
            ("revision", meta.revision.to_string()),
            ("fingerprint", meta.fingerprint.to_string()),
            ("files", meta.files.to_string()),
            ("built_at", meta.built_at.clone()),
            ("nodes", nodes.to_string()),
            ("edges", edges.to_string()),
        ];
        let tx = self.conn.transaction().context("begin meta write")?;
        {
            let mut stmt = tx.prepare_cached("INSERT INTO meta (key, value) VALUES (?1, ?2)")?;
            for (key, value) in &rows {
                stmt.execute(params![key, value])?;
            }
        }
        tx.commit().context("commit meta write")?;
        self.conn.execute_batch("ANALYZE;").context("analyse graph database")?;
        Ok(())
    }
}

/// Build the whole-workspace call graph from `db` straight into a fresh SQLite
/// file at `out_path`, in RAM-bounded batches. The in-memory graph does not fit on
/// large configurations, so this is the path that makes a whole-config graph
/// available at all: the driver streams one batch of rows at a time and this
/// writer persists them, so peak memory is bounded by the batch size plus the
/// resident method index — never the full node/edge set.
///
/// Returns the build tally; node/edge counts in the database are recorded in its
/// `meta` table by [`GraphDbWriter::finalize`].
pub fn build_graph_database(
    db: &RootDatabaseImpl,
    source_root_id: SourceRootId,
    workspace_root: Option<&Path>,
    out_path: &Path,
    batch_size: usize,
    meta: &GraphMeta,
) -> anyhow::Result<GraphBuildSummary> {
    let mut writer = GraphDbWriter::create(out_path)?;

    // Scope the sink so its mutable borrow of `writer` ends before `finalize`.
    let summary = {
        let mut sink = |nodes: &[NodeRow],
                        edges: &[EdgeRow]|
         -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            writer.write_nodes(nodes)?;
            writer.write_edges(edges)?;
            Ok(())
        };
        ide::build_workspace_graph_rows(db, source_root_id, workspace_root, batch_size, &mut sink)
            .map_err(|e| anyhow::anyhow!("{e}"))?
    };

    writer.finalize(meta)?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method_node(id: &str, name: &str) -> NodeRow {
        NodeRow {
            id: id.to_string(),
            kind: "method",
            name: name.to_string(),
            qualified: format!("ОбщийМодуль.X.{name}"),
            module: Some("ОбщийМодуль.X".to_string()),
            file: Some("CommonModules/X/Ext/Module.bsl".to_string()),
            name_offset: Some(10),
            src_start: Some(0),
            src_end: Some(40),
            dispatch: vec!["server"],
            is_export: Some(true),
            addressable: true,
        }
    }

    fn edge(from: &str, to: &str) -> EdgeRow {
        EdgeRow {
            from_id: from.to_string(),
            to_id: to.to_string(),
            kind: "call",
            provenance: "resolved",
            crosses: false,
        }
    }

    fn open(path: &Path) -> Connection {
        Connection::open(path).unwrap()
    }

    #[test]
    fn writes_and_reads_back_nodes_and_edges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bsl-graph.db");

        let mut w = GraphDbWriter::create(&path).unwrap();
        w.write_nodes(&[
            method_node("method/common/X/A", "A"),
            method_node("method/common/X/B", "B"),
        ])
        .unwrap();
        w.write_edges(&[edge("method/common/X/A", "method/common/X/B")]).unwrap();
        w.finalize(&GraphMeta {
            revision: 1,
            fingerprint: 42,
            files: 1,
            built_at: "2026-06-01T00:00:00Z".to_string(),
        })
        .unwrap();

        let conn = open(&path);
        let nodes: i64 = conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0)).unwrap();
        let edges: i64 = conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0)).unwrap();
        assert_eq!((nodes, edges), (2, 1));

        // finalize derives the node/edge counts and records them in `meta`.
        let meta_nodes: String =
            conn.query_row("SELECT value FROM meta WHERE key = 'nodes'", [], |r| r.get(0)).unwrap();
        let meta_edges: String =
            conn.query_row("SELECT value FROM meta WHERE key = 'edges'", [], |r| r.get(0)).unwrap();
        assert_eq!((meta_nodes.as_str(), meta_edges.as_str()), ("2", "1"));

        let (name, dispatch, is_export, addressable): (String, String, i64, i64) = conn
            .query_row(
                "SELECT name, dispatch, is_export, addressable FROM nodes WHERE id = ?1",
                params!["method/common/X/A"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (name.as_str(), dispatch.as_str(), is_export, addressable),
            ("A", "server", 1, 1)
        );

        let schema: String = conn
            .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(schema, SCHEMA_VERSION.to_string());
    }

    #[test]
    fn first_node_spelling_wins_on_duplicate_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bsl-graph.db");

        let mut first = method_node("method/common/X/A", "A");
        first.qualified = "first".to_string();
        let mut second = method_node("method/common/X/A", "A");
        second.qualified = "second".to_string();

        let mut w = GraphDbWriter::create(&path).unwrap();
        w.write_nodes(&[first]).unwrap();
        w.write_nodes(&[second]).unwrap();
        w.finalize(&GraphMeta { revision: 1, fingerprint: 0, files: 0, built_at: "t".to_string() })
            .unwrap();

        let conn = open(&path);
        let qualified: String = conn
            .query_row(
                "SELECT qualified FROM nodes WHERE id = ?1",
                params!["method/common/X/A"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(qualified, "first", "INSERT OR IGNORE keeps the first-seen spelling");
    }

    #[test]
    fn in_degree_counts_incoming_edges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bsl-graph.db");

        let mut w = GraphDbWriter::create(&path).unwrap();
        w.write_nodes(&[method_node("a", "A"), method_node("b", "B"), method_node("hub", "Hub")])
            .unwrap();
        w.write_edges(&[edge("a", "hub"), edge("b", "hub"), edge("a", "b")]).unwrap();
        w.finalize(&GraphMeta { revision: 1, fingerprint: 0, files: 0, built_at: "t".to_string() })
            .unwrap();

        let conn = open(&path);
        let hub: i64 = conn
            .query_row("SELECT degree FROM in_degree WHERE id = 'hub'", [], |r| r.get(0))
            .unwrap();
        let b: i64 = conn
            .query_row("SELECT degree FROM in_degree WHERE id = 'b'", [], |r| r.get(0))
            .unwrap();
        assert_eq!((hub, b), (2, 1));
        // A source-only node has no in_degree row.
        let a_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM in_degree WHERE id = 'a'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(a_rows, 0);
    }

    #[test]
    fn create_truncates_a_prior_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bsl-graph.db");

        let mut w = GraphDbWriter::create(&path).unwrap();
        w.write_nodes(&[method_node("stale", "Stale")]).unwrap();
        w.finalize(&GraphMeta { revision: 1, fingerprint: 0, files: 0, built_at: "t".to_string() })
            .unwrap();

        // A second build at the same path must not see the prior row.
        let w2 = GraphDbWriter::create(&path).unwrap();
        w2.finalize(&GraphMeta {
            revision: 2,
            fingerprint: 0,
            files: 0,
            built_at: "t".to_string(),
        })
        .unwrap();

        let conn = open(&path);
        let nodes: i64 = conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0)).unwrap();
        assert_eq!(nodes, 0, "create() discards the prior file");
    }
}
