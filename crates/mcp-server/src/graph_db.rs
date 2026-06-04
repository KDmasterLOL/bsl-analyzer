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

use std::path::{Path, PathBuf};

use anyhow::Context;
use ide::graph_index::{EdgeRow, NodeRow};
use ide::{GraphBuildSummary, ModuleId, RootDatabaseImpl};
use rusqlite::{params, Connection, OptionalExtension};
use rustc_hash::FxHashMap;
use vfs::FileId;

use crate::graph::{config_metadata_paths, db_for_files, enumerate_bsl_files};

/// Bumped whenever the table layout OR the persisted edge/node content changes so a
/// stale on-disk cache from an older binary is rejected (via the `meta` row) and
/// rebuilt. Version 5 adds the `notify_ref`/`idle_handler` callback edges; version 6
/// adds the `event_subscription` handler edges.
pub(crate) const SCHEMA_VERSION: u32 = 6;

/// One file's persisted identity in the `files` table: its stat-only fingerprint
/// and (for `.bsl`) its resolution-signature hash. Persisting these per path lets a
/// reload classify drift granularly (which files changed) instead of only knowing
/// the whole-workspace fingerprint moved.
pub(crate) struct FileFingerprint {
    /// Canonical, `/`-normalised path — the same string `workspace_fingerprint` folds.
    pub path: String,
    /// `hash(mtime, len)` for this file.
    pub fingerprint: u64,
    /// Resolution-signature hash, `None` for `.xml` (filled in by the body-only fast
    /// path; currently always `None`).
    pub sig_hash: Option<u64>,
}

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
                sig_end     INTEGER,
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

            CREATE TABLE files (
                path        TEXT PRIMARY KEY,
                fingerprint INTEGER NOT NULL,
                sig_hash    INTEGER
            ) WITHOUT ROWID;

            CREATE TABLE unresolved_calls (
                target_scope TEXT NOT NULL,
                method_lower TEXT NOT NULL,
                caller_file  TEXT NOT NULL,
                PRIMARY KEY (target_scope, method_lower, caller_file)
            ) WITHOUT ROWID;
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
                 (id, kind, name, qualified, module, file, name_offset, sig_end, src_start, \
                  src_end, dispatch, is_export, addressable) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
                    row.sig_end,
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

    /// Persist the per-file fingerprints into the `files` table. Used on reload to
    /// classify which files drifted instead of only knowing the workspace-wide
    /// fingerprint moved. `INSERT OR REPLACE` so a re-run at the same path is
    /// idempotent.
    pub(crate) fn write_files(&mut self, rows: &[FileFingerprint]) -> anyhow::Result<()> {
        let tx = self.conn.transaction().context("begin files batch")?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO files (path, fingerprint, sig_hash) VALUES (?1, ?2, ?3)",
            )?;
            for row in rows {
                stmt.execute(params![
                    row.path,
                    row.fingerprint as i64,
                    row.sig_hash.map(|h| h as i64),
                ])?;
            }
        }
        tx.commit().context("commit files batch")?;
        Ok(())
    }

    /// Persist the set of inconsistently-cased objects into a single `meta` row
    /// (`casing_variants`, newline-joined). The incremental fast path reads it to
    /// refuse a body-only update that touches such an object. Empty for the common,
    /// consistently-cased configuration.
    pub(crate) fn write_casing_variants(&mut self, keys: &[String]) -> anyhow::Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('casing_variants', ?1)",
                params![keys.join("\n")],
            )
            .context("writing casing variants")?;
        Ok(())
    }

    /// Persist the module-located-but-unresolved qualified/manager call sites into the
    /// `unresolved_calls` reverse index. The PK dedups repeated call sites, so the
    /// content is order-independent. Used by the incremental fast path to find callers
    /// that would newly resolve when a target module gains/exports a method.
    pub(crate) fn write_unresolved_calls(
        &mut self,
        rows: &[(String, String, String)],
    ) -> anyhow::Result<()> {
        let tx = self.conn.transaction().context("begin unresolved_calls batch")?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO unresolved_calls (target_scope, method_lower, caller_file) \
                 VALUES (?1, ?2, ?3)",
            )?;
            for (target_scope, method_lower, caller_file) in rows {
                stmt.execute(params![target_scope, method_lower, caller_file])?;
            }
        }
        tx.commit().context("commit unresolved_calls batch")?;
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

/// Build the whole-workspace call graph straight into a fresh SQLite file at
/// `out_path`, in RAM-bounded batches. The in-memory graph does not fit on large
/// configurations (a 25k-module ERP blows past 8 GB in a single database), so this
/// is the path that makes a whole-config graph available at all.
///
/// Files are enumerated once for a stable id↔path map, then each batch's texts are
/// loaded into a throwaway database (dropped before the next), with cross-batch
/// call targets resolved through the resident compact method index — never another
/// batch's database. Peak memory is therefore bounded by the batch size plus that
/// index, not by the whole config.
///
/// Returns the build tally; node/edge counts in the database are recorded in its
/// `meta` table by [`GraphDbWriter::finalize`].
pub fn build_graph_database(
    workspace_root: &Path,
    out_path: &Path,
    batch_size: usize,
    meta: &GraphMeta,
) -> anyhow::Result<GraphBuildSummary> {
    let files = enumerate_bsl_files(workspace_root);
    let config_paths = config_metadata_paths(workspace_root);
    let modules: Vec<ModuleId> = files.iter().map(|(f, _)| ModuleId::new(*f)).collect();
    let paths: FxHashMap<FileId, String> =
        files.iter().map(|(f, p)| (*f, p.to_string_lossy().replace('\\', "/"))).collect();
    let file_paths: FxHashMap<FileId, PathBuf> =
        files.iter().map(|(f, p)| (*f, p.clone())).collect();

    // The whole-workspace source root, built once and shared (cheap `Arc` clone)
    // into every per-batch database, so the 25k-path file set is assembled a single
    // time for the build rather than re-cloned per batch.
    let source_root = crate::graph::build_source_root(&files);

    let mut writer = GraphDbWriter::create(out_path)?;

    // One configuration cache shared across every batch database (and their per-job
    // clones), so the whole-config metadata load runs once for this build instead of
    // once per fresh batch database. A fresh cache per build keeps it a content
    // snapshot — see `ide_db`'s `GraphConfigCache`.
    let config_cache = std::sync::Arc::new(ide::GraphConfigCache::default());

    // Scope the closures so their borrows end before `finalize`. `open_batch`
    // loads only the batch's texts (sharing the resident source root + config);
    // `sink` persists the freshly-encoded rows (the sole `&mut writer` borrow).
    let summary = {
        let mut open_batch = |batch: &[ModuleId]| -> RootDatabaseImpl {
            let batch_files: Vec<(FileId, PathBuf)> =
                batch.iter().map(|m| (m.file_id, file_paths[&m.file_id].clone())).collect();
            db_for_files(&source_root, &batch_files, &config_paths, Some(&config_cache))
        };
        let mut sink = |nodes: &[NodeRow],
                        edges: &[EdgeRow]|
         -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            writer.write_nodes(nodes)?;
            writer.write_edges(edges)?;
            Ok(())
        };
        ide::build_workspace_graph_rows(
            &modules,
            &paths,
            Some(workspace_root),
            batch_size,
            &mut open_batch,
            &mut sink,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?
    };

    // Persist a per-file fingerprint for every graph-relevant file (`.bsl` + `.xml`),
    // covering the same universe the workspace fingerprint folds, so a later reload
    // can classify drift granularly. For `.bsl` modules also persist the body-free
    // signature hash from the build, so a body-only edit (sig unchanged) is
    // distinguishable from a resolution-affecting one. `.xml` rows keep NULL sig.
    //
    // `file_paths` holds each module's canonical path verbatim; `scan_file_stats`
    // stringifies the same canonical path, so keying by that string lines the two up.
    let sig_by_path: FxHashMap<String, u64> = summary
        .module_sig_hashes
        .iter()
        .filter_map(|(m, &h)| {
            file_paths.get(&m.file_id).map(|p| (p.to_string_lossy().into_owned(), h))
        })
        .collect();
    let file_rows: Vec<FileFingerprint> = crate::graph::scan_file_stats(workspace_root)
        .iter()
        .map(|s| FileFingerprint {
            fingerprint: s.fingerprint(),
            sig_hash: sig_by_path.get(&s.path).copied(),
            path: s.path.clone(),
        })
        .collect();
    writer.write_files(&file_rows)?;
    writer.write_casing_variants(&summary.casing_variant_objects)?;
    writer.write_unresolved_calls(&summary.unresolved_calls)?;

    writer.finalize(meta)?;
    Ok(summary)
}

/// Canonicalise a freshly-projected aux (`mdo`/`attribute`) node/edge id against the
/// object spellings already in the store. The durable id embeds the source-written
/// object casing, but a full rebuild fixes it to the global first-seen owner; an
/// incremental reprojection of a subset only knows the subset's casing, so for an
/// object an unchanged module already owns we must reuse the stored spelling.
///
/// `existing_mdo` maps each stored `mdo/<Type>/<obj>` id, lowercased (Unicode-aware,
/// since SQLite's `lower()` folds ASCII only and object names are Cyrillic), to its
/// actual spelling. A `method`/`module` id, or an object the store does not yet know
/// (genuinely new — owned by the changed set in both paths), is returned unchanged.
fn canonicalize_aux_id(
    existing_mdo: &std::collections::HashMap<String, String>,
    id: &str,
) -> String {
    if id.starts_with("mdo/") {
        return existing_mdo.get(&id.to_lowercase()).cloned().unwrap_or_else(|| id.to_string());
    }
    if let Some(rest) = id.strip_prefix("attribute/") {
        // rest = <Type>/<object>/<attr>; only the object segment needs canonicalising
        // (Type is the stable english name, attr is the metadata-stable field name).
        let mut seg = rest.splitn(3, '/');
        if let (Some(etype), Some(_obj), Some(attr)) = (seg.next(), seg.next(), seg.next()) {
            let mdo_key = format!("mdo/{etype}/{_obj}").to_lowercase();
            if let Some(canon_mdo) = existing_mdo.get(&mdo_key) {
                if let Some(canon_obj) =
                    canon_mdo.strip_prefix("mdo/").and_then(|r| r.split_once('/')).map(|(_, o)| o)
                {
                    return format!("attribute/{etype}/{canon_obj}/{attr}");
                }
            }
        }
    }
    id.to_string()
}

/// Split an aux durable id into its `(EnglishType, object)` segments. `None` for a
/// `method`/`module` id. Object/type segments never contain `/` (BSL identifiers and
/// english type names exclude it), so the split is unambiguous.
fn aux_object(id: &str) -> Option<(&str, &str)> {
    if let Some(rest) = id.strip_prefix("mdo/") {
        return rest.split_once('/');
    }
    if let Some(rest) = id.strip_prefix("attribute/") {
        let mut seg = rest.splitn(3, '/');
        if let (Some(etype), Some(obj), Some(_attr)) = (seg.next(), seg.next(), seg.next()) {
            return Some((etype, obj));
        }
    }
    None
}

/// Refuse the body-only fast path for the two aux-spelling cases its DB-pinned
/// canonicalisation cannot reproduce byte-identically — both require cross-module
/// casing inconsistency, so a normal (consistent-casing) edit is unaffected:
///
/// - **(A) casing change of a referenced object** — a changed module references an
///   existing object with a different exact spelling. If that module is the object's
///   first-seen owner, a full rebuild would adopt the new spelling, but the fast path
///   pins to the stored one.
/// - **(B) ownership shift on drop** — a changed module drops its last reference to an
///   object that survives via another module; a full rebuild would re-derive the
///   canonical spelling from the surviving (possibly different-cased) owner.
///
/// In both cases we fall back to a full rebuild, which is always correct.
fn incremental_safety_check(
    conn: &Connection,
    changed_files: &[String],
    rows: &ide::ReprojectedRows,
) -> anyhow::Result<()> {
    use std::collections::{HashMap, HashSet};

    // (C) Objects the full build saw with inconsistent casing across modules. Their
    // cross-module first-seen ordering is not reconstructable from the canonicalised
    // store, so the fast path must not touch them. Recorded as lowercased
    // `englishtype/object` keys in the `casing_variants` meta row.
    let variant_keys: HashSet<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'casing_variants'", [], |r| {
            r.get::<_, String>(0)
        })
        .optional()?
        .into_iter()
        .flat_map(|v| v.lines().map(str::to_string).collect::<Vec<_>>())
        .filter(|s| !s.is_empty())
        .collect();
    let touches_variant = |id: &str| -> bool {
        aux_object(id).is_some_and(|(etype, obj)| {
            variant_keys.contains(&format!("{}/{}", etype.to_lowercase(), obj.to_lowercase()))
        })
    };

    // (A) Stored object spelling per (type, object), case-folded.
    let mut stored_obj: HashMap<(String, String), String> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT id FROM nodes WHERE kind IN ('mdo', 'attribute')")?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for id in ids.flatten() {
            if let Some((etype, obj)) = aux_object(&id) {
                stored_obj
                    .entry((etype.to_lowercase(), obj.to_lowercase()))
                    .or_insert_with(|| obj.to_string());
            }
        }
    }
    let reprojected_aux = rows
        .nodes
        .iter()
        .filter(|n| n.kind == "mdo" || n.kind == "attribute")
        .map(|n| n.id.as_str())
        .chain(rows.edges.iter().map(|e| e.to_id.as_str()));
    for id in reprojected_aux {
        if touches_variant(id) {
            anyhow::bail!("incremental update: touches casing-variant object {id}; full rebuild");
        }
        if let Some((etype, obj)) = aux_object(id) {
            if let Some(stored) = stored_obj.get(&(etype.to_lowercase(), obj.to_lowercase())) {
                if stored != obj {
                    anyhow::bail!(
                        "incremental update: aux object casing drift ({obj} vs stored {stored}); full rebuild"
                    );
                }
            }
        }
    }

    // (B) Aux objects the changed modules referenced before the edit.
    let placeholders = changed_files.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let old_sql = format!(
        "SELECT DISTINCT e.to_id FROM edges e JOIN nodes n ON e.from_id = n.id \
         WHERE n.file IN ({placeholders}) \
         AND (e.to_id LIKE 'mdo/%' OR e.to_id LIKE 'attribute/%')"
    );
    let old_aux: HashSet<String> = {
        let mut stmt = conn.prepare(&old_sql)?;
        let it = stmt.query_map(rusqlite::params_from_iter(changed_files.iter()), |r| {
            r.get::<_, String>(0)
        })?;
        it.filter_map(|r| r.ok()).collect()
    };
    for id in &old_aux {
        if touches_variant(id) {
            anyhow::bail!(
                "incremental update: drops/keeps a casing-variant object {id}; full rebuild"
            );
        }
    }
    let new_aux: HashSet<&str> = rows
        .edges
        .iter()
        .map(|e| e.to_id.as_str())
        .filter(|t| t.starts_with("mdo/") || t.starts_with("attribute/"))
        .collect();
    let survivors_sql = format!(
        "SELECT COUNT(*) FROM edges e JOIN nodes n ON e.from_id = n.id \
         WHERE e.to_id = ?1 AND n.file NOT IN ({placeholders})"
    );
    for dropped in old_aux.iter().filter(|x| !new_aux.contains(x.as_str())) {
        let mut params: Vec<&dyn rusqlite::ToSql> = vec![dropped];
        for f in changed_files {
            params.push(f);
        }
        let survivors: i64 = conn.query_row(&survivors_sql, params.as_slice(), |r| r.get(0))?;
        if survivors > 0 {
            anyhow::bail!(
                "incremental update: dropped aux ref {dropped} still referenced by an unchanged module; full rebuild"
            );
        }
    }
    Ok(())
}

/// Insert one node row, overriding only its `id` (for aux-id canonicalisation).
/// `INSERT OR IGNORE` keeps the first-seen spelling, exactly like the bulk writer.
fn insert_node_row(tx: &rusqlite::Transaction<'_>, row: &NodeRow, id: &str) -> anyhow::Result<()> {
    let dispatch = if row.dispatch.is_empty() { None } else { Some(row.dispatch.join(",")) };
    tx.prepare_cached(
        "INSERT OR IGNORE INTO nodes \
         (id, kind, name, qualified, module, file, name_offset, sig_end, src_start, \
          src_end, dispatch, is_export, addressable) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?
    .execute(params![
        id,
        row.kind,
        row.name,
        row.qualified,
        row.module,
        row.file,
        row.name_offset,
        row.sig_end,
        row.src_start,
        row.src_end,
        dispatch,
        row.is_export.map(|b| b as i64),
        row.addressable as i64,
    ])?;
    Ok(())
}

/// Apply an incremental update: reproject ONLY the modules at `changed_paths` and
/// patch a COPY of `src_path` written to `out_path`, leaving every unchanged module's
/// rows in place. `changed_paths` is the FULL reprojection set the caller proved
/// sufficient — either the edited body-only modules (signature unchanged), or, for a
/// signature change, the changed modules PLUS their resolved callers (the
/// caller-delta set). The result is byte-identical to a full rebuild of the edited
/// tree. This function does not re-validate eligibility — the caller
/// (`try_incremental_reload`) owns the sig/caller-delta-safety gates.
///
/// Concurrency: the patch lands on a copy and the caller atomically renames it into
/// place — the same model the full build uses, so a live reader keeps its open
/// snapshot until it reopens and no in-place mutation races a query.
pub fn update_graph_database_bodies(
    workspace_root: &Path,
    src_path: &Path,
    out_path: &Path,
    changed_paths: &[PathBuf],
    batch_size: usize,
    meta: &GraphMeta,
) -> anyhow::Result<GraphBuildSummary> {
    let files = enumerate_bsl_files(workspace_root);
    let config_paths = config_metadata_paths(workspace_root);
    let all_modules: Vec<ModuleId> = files.iter().map(|(f, _)| ModuleId::new(*f)).collect();
    let paths: FxHashMap<FileId, String> =
        files.iter().map(|(f, p)| (*f, p.to_string_lossy().replace('\\', "/"))).collect();
    let file_paths: FxHashMap<FileId, PathBuf> =
        files.iter().map(|(f, p)| (*f, p.clone())).collect();

    // Map changed canonical paths → ModuleIds, preserving file-id order so a new aux
    // object's first-seen spelling matches a full build's.
    let changed_set: std::collections::HashSet<&Path> =
        changed_paths.iter().map(|p| p.as_path()).collect();
    let changed_modules: Vec<ModuleId> = files
        .iter()
        .filter(|(_, p)| changed_set.contains(p.as_path()))
        .map(|(f, _)| ModuleId::new(*f))
        .collect();
    if changed_modules.len() != changed_paths.len() {
        anyhow::bail!(
            "incremental update: {} changed paths, {} matched modules (a path is not an indexed .bsl module)",
            changed_paths.len(),
            changed_modules.len()
        );
    }

    let source_root = crate::graph::build_source_root(&files);
    let config_cache = std::sync::Arc::new(ide::GraphConfigCache::default());
    let mut open_batch = |batch: &[ModuleId]| -> RootDatabaseImpl {
        let batch_files: Vec<(FileId, PathBuf)> =
            batch.iter().map(|m| (m.file_id, file_paths[&m.file_id].clone())).collect();
        db_for_files(&source_root, &batch_files, &config_paths, Some(&config_cache))
    };

    let rows = ide::reproject_changed_modules(
        &all_modules,
        &changed_modules,
        &paths,
        Some(workspace_root),
        batch_size,
        &mut open_batch,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Normalised `nodes.file` keys for the changed modules — used both to gate the
    // fast path and to scope the per-module deletes below.
    let changed_files: Vec<String> =
        changed_modules.iter().map(|m| paths[&m.file_id].clone()).collect();

    // Bail to a full rebuild for the aux-casing cases the DB-pinned canonicalisation
    // cannot reproduce (a no-op for normal, consistent-casing edits).
    {
        let src = Connection::open_with_flags(src_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening graph db {} read-only", src_path.display()))?;
        incremental_safety_check(&src, &changed_files, &rows)?;
    }

    // Patch a copy, never the published file (a reader keeps its snapshot until the
    // caller renames `out_path` into place).
    std::fs::copy(src_path, out_path).with_context(|| {
        format!("copying graph db {} → {}", src_path.display(), out_path.display())
    })?;

    let stat_fp: FxHashMap<String, u64> = crate::graph::scan_file_stats(workspace_root)
        .iter()
        .map(|s| (s.path.clone(), s.fingerprint()))
        .collect();

    let mut conn = Connection::open(out_path)?;
    {
        let tx = conn.transaction().context("begin incremental patch")?;

        // The first-seen object spellings the store already owns (Unicode-lowercased
        // key → actual id), loaded before inserting so new objects keep their casing.
        let existing_mdo: std::collections::HashMap<String, String> = {
            let mut stmt = tx.prepare("SELECT id FROM nodes WHERE kind = 'mdo'")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.filter_map(|r| r.ok()).map(|id| (id.to_lowercase(), id)).collect()
        };

        // Drop each changed module's outgoing edges, method nodes, AND module-code
        // node. The module-code node is re-emitted by the reprojection only if the
        // module still has a module-level edge — matching a full rebuild, which emits
        // it solely as an edge endpoint. Deleting it (rather than INSERT OR IGNORE)
        // is what lets a module that lost its last module-level edge shed the node.
        for nfile in &changed_files {
            // Only the module's body-derived outgoing edges (from its method/module-code
            // nodes) are reprojected, so only those are deleted. `contains` edges have
            // `mdo`/`form` from-endpoints — never method/module — and the form pass is
            // full-build-only, so the kind filter keeps reprojection from dropping form
            // structure it cannot re-emit. (A no-op when no form nodes exist: all current
            // edge sources are method/module nodes.)
            tx.execute(
                "DELETE FROM edges WHERE from_id IN \
                 (SELECT id FROM nodes WHERE file = ?1 AND kind IN ('method', 'module'))",
                params![nfile],
            )?;
            tx.execute(
                "DELETE FROM nodes WHERE file = ?1 AND kind IN ('method', 'module')",
                params![nfile],
            )?;
        }

        // Re-insert the reprojected nodes, canonicalising aux ids against the store.
        for row in &rows.nodes {
            match row.kind {
                "mdo" | "attribute" => {
                    let id = canonicalize_aux_id(&existing_mdo, &row.id);
                    insert_node_row(&tx, row, &id)?;
                }
                _ => insert_node_row(&tx, row, &row.id)?,
            }
        }
        // Re-insert the edges, canonicalising aux `to_id`s the same way.
        for edge in &rows.edges {
            let to_id = canonicalize_aux_id(&existing_mdo, &edge.to_id);
            tx.prepare_cached(
                "INSERT INTO edges (from_id, to_id, kind, provenance, crosses) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![
                edge.from_id,
                to_id,
                edge.kind,
                edge.provenance,
                edge.crosses as i64
            ])?;
        }

        // GC aux nodes that lost their last reference. Restricted to pure-sink kinds:
        // module-code nodes are `from_id`-only sources, so a `to_id`-absence sweep
        // would wrongly delete a live caller. An `mdo` may also be a `from_id` — the
        // parent of a `contains` edge to a form — so it must survive on either role:
        // a full rebuild keeps such an object (materialised as the contains-from
        // endpoint) even with no inbound call/query edge. (The `from_id` clause is a
        // no-op when no form nodes exist: `mdo`/`attribute` are never edge sources then.)
        tx.execute(
            "DELETE FROM nodes WHERE kind IN ('mdo', 'attribute') \
             AND id NOT IN (SELECT to_id FROM edges) \
             AND id NOT IN (SELECT from_id FROM edges)",
            [],
        )?;

        // Recompute the whole in-degree table — a delta that forgot the deleted edges'
        // old targets would leave stale degrees.
        tx.execute("DELETE FROM in_degree", [])?;
        tx.execute(
            "INSERT INTO in_degree (id, degree) SELECT to_id, COUNT(*) FROM edges GROUP BY to_id",
            [],
        )?;

        // Merge any casing variants the reprojection observed AMONG the changed
        // modules into the persisted set, so a future reload still refuses the fast
        // path for a newly-inconsistent object a multi-file edit introduced.
        if !rows.casing_variant_objects.is_empty() {
            let existing: String = tx
                .query_row("SELECT value FROM meta WHERE key = 'casing_variants'", [], |r| r.get(0))
                .optional()?
                .unwrap_or_default();
            let mut set: std::collections::BTreeSet<String> =
                existing.lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
            set.extend(rows.casing_variant_objects.iter().cloned());
            tx.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('casing_variants', ?1)",
                params![set.into_iter().collect::<Vec<_>>().join("\n")],
            )?;
        }

        // Refresh the changed modules' persisted fingerprint + signature hash.
        for module in &changed_modules {
            let canonical = file_paths[&module.file_id].to_string_lossy().into_owned();
            let fp = stat_fp.get(&canonical).copied().unwrap_or(0);
            let sig = rows.sig_hashes.get(module).copied();
            tx.execute(
                "INSERT OR REPLACE INTO files (path, fingerprint, sig_hash) VALUES (?1, ?2, ?3)",
                params![canonical, fp as i64, sig.map(|h| h as i64)],
            )?;
        }

        // Refresh the reverse index of unresolved calls for the reprojected modules:
        // drop their old rows, insert their fresh ones. Unchanged modules' rows stay.
        for nfile in &changed_files {
            tx.execute("DELETE FROM unresolved_calls WHERE caller_file = ?1", params![nfile])?;
        }
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO unresolved_calls (target_scope, method_lower, caller_file) \
                 VALUES (?1, ?2, ?3)",
            )?;
            for (target_scope, method_lower, caller_file) in &rows.unresolved_calls {
                stmt.execute(params![target_scope, method_lower, caller_file])?;
            }
        }

        // Refresh build metadata + derived counts; a clean incremental snapshot is
        // never force-stale.
        let node_count: i64 = tx.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?;
        let edge_count: i64 = tx.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
        let meta_rows: [(&str, String); 7] = [
            ("revision", meta.revision.to_string()),
            ("fingerprint", meta.fingerprint.to_string()),
            ("files", all_modules.len().to_string()),
            ("built_at", meta.built_at.clone()),
            ("nodes", node_count.to_string()),
            ("edges", edge_count.to_string()),
            ("force_stale", "0".to_string()),
        ];
        for (key, value) in &meta_rows {
            tx.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
                params![key, value],
            )?;
        }

        tx.commit().context("commit incremental patch")?;
    }

    let node_rows = rows.nodes.len();
    let edges: i64 = conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
    Ok(GraphBuildSummary {
        modules: all_modules.len(),
        node_rows,
        edges: edges as usize,
        module_sig_hashes: rows.sig_hashes,
        // Variants observed among the changed modules (merged into the persisted set
        // above); pre-existing variants for untouched objects remain in the copied db.
        casing_variant_objects: rows.casing_variant_objects,
        // The reprojected modules' unresolved refs were refreshed in the patch above.
        unresolved_calls: rows.unresolved_calls,
    })
}

/// A changed module's recomputed body-free profile: its signature hash plus the
/// resolvable-name surface a caller-delta eligibility check needs — the lowercased
/// names of its exported methods, and whether any two methods fold to the same name
/// (a collision makes "exported name" ≠ "resolvable name", since resolution is
/// first-wins).
pub struct ModuleProfile {
    pub sig_hash: u64,
    pub exported_lower: std::collections::BTreeSet<String>,
    pub has_collision: bool,
}

/// Recompute each module at `changed_paths`'s profile, for the incremental
/// eligibility checks (sig drift, and the caller-delta resolvable-name surface).
/// Builds a tiny resident index over only those modules — these reads are a module's
/// own item-tree + dispatch, no cross-module data — so it stays cheap. Keyed by
/// canonical path.
pub fn recompute_module_profiles(
    workspace_root: &Path,
    changed_paths: &[PathBuf],
) -> anyhow::Result<FxHashMap<String, ModuleProfile>> {
    use ide::graph_index::GraphIndex;

    let files = enumerate_bsl_files(workspace_root);
    let config_paths = config_metadata_paths(workspace_root);
    let source_root = crate::graph::build_source_root(&files);

    let changed_set: std::collections::HashSet<&Path> =
        changed_paths.iter().map(|p| p.as_path()).collect();
    let changed: Vec<(ModuleId, PathBuf)> = files
        .iter()
        .filter(|(_, p)| changed_set.contains(p.as_path()))
        .map(|(f, p)| (ModuleId::new(*f), p.clone()))
        .collect();

    let batch_files: Vec<(FileId, PathBuf)> =
        changed.iter().map(|(m, p)| (m.file_id, p.clone())).collect();
    let db = db_for_files(&source_root, &batch_files, &config_paths, None);
    let modules: Vec<ModuleId> = changed.iter().map(|(m, _)| *m).collect();
    let index = GraphIndex::build(&db, &modules);

    let mut out = FxHashMap::default();
    for (module, path) in &changed {
        let Some(sig_hash) = index.module_sig_hash(*module) else {
            continue;
        };
        let methods = index.module_methods(*module).unwrap_or_default();
        let lowers: Vec<String> = methods.iter().map(|(n, _)| n.to_lowercase()).collect();
        let has_collision =
            lowers.iter().collect::<std::collections::HashSet<_>>().len() != lowers.len();
        let exported_lower: std::collections::BTreeSet<String> =
            methods.iter().filter(|(_, exp)| *exp).map(|(n, _)| n.to_lowercase()).collect();
        out.insert(
            path.to_string_lossy().into_owned(),
            ModuleProfile { sig_hash, exported_lower, has_collision },
        );
    }
    Ok(out)
}

/// Plan the caller-delta for a set of signature-changed modules (the body-only fast
/// path is not eligible because their signature moved). Returns:
/// - `Ok(Some(caller_files))` — reprojecting the changed modules PLUS the returned
///   callers reproduces a full rebuild. Callers are the union of: modules with a
///   stored edge INTO a changed module (covers removal/unexport/dispatch/case-rename),
///   and modules whose previously-unresolved `B.<name>()` would newly resolve when B
///   gains a resolvable `name` (looked up in the `unresolved_calls` reverse index).
///   Excludes the changed modules themselves.
/// - `Ok(None)` — not eligible: a first-wins name collision (invalid-BSL shadowing,
///   old or new), or an added resolvable name on a module whose scope is not
///   name-keyed (so its callers cannot be found). The caller must do a full rebuild.
///
/// `sig_changed` pairs each changed module's normalised `nodes.file` key with its
/// freshly-recomputed [`ModuleProfile`].
pub fn caller_delta_plan(
    db_path: &Path,
    sig_changed: &[(&str, &ModuleProfile)],
) -> anyhow::Result<Option<Vec<PathBuf>>> {
    let conn = Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    let mut index_callers: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (file, profile) in sig_changed {
        if profile.has_collision {
            return Ok(None); // first-wins shadowing — exported set ≠ resolvable set
        }
        // OLD resolvable surface from the stored method nodes.
        let mut stmt =
            conn.prepare("SELECT name, is_export FROM nodes WHERE file = ?1 AND kind = 'method'")?;
        let rows = stmt.query_map([file], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0) != 0))
        })?;
        let mut old_lowers: Vec<String> = Vec::new();
        let mut old_exported: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for row in rows {
            let (name, exported) = row?;
            let lower = name.to_lowercase();
            if exported {
                old_exported.insert(lower.clone());
            }
            old_lowers.push(lower);
        }
        let old_collision =
            old_lowers.iter().collect::<std::collections::HashSet<_>>().len() != old_lowers.len();
        if old_collision {
            return Ok(None);
        }
        // Newly-resolvable names: callers that called them were previously unresolved
        // (dropped from `edges`), so find them through the reverse index by scope+name.
        let added: Vec<&String> =
            profile.exported_lower.iter().filter(|n| !old_exported.contains(*n)).collect();
        if !added.is_empty() {
            let Some(scope) = ide::scope_for_path(file) else {
                return Ok(None); // not name-keyed → its callers aren't indexable
            };
            let mut stmt = conn.prepare(
                "SELECT caller_file FROM unresolved_calls \
                 WHERE target_scope = ?1 AND method_lower = ?2",
            )?;
            for name in added {
                let rows = stmt.query_map(params![scope, name], |r| r.get::<_, String>(0))?;
                for row in rows {
                    index_callers.insert(row?);
                }
            }
        }
    }

    // Resolved callers: modules with a stored edge into a changed module's method node.
    let changed_files: std::collections::BTreeSet<&str> =
        sig_changed.iter().map(|(f, _)| *f).collect();
    let placeholders = changed_files.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT DISTINCT n2.file FROM edges e \
         JOIN nodes n1 ON e.to_id = n1.id \
         JOIN nodes n2 ON e.from_id = n2.id \
         WHERE n1.file IN ({placeholders}) AND n1.kind = 'method' AND n2.file IS NOT NULL"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(changed_files.iter()), |r| r.get::<_, String>(0))?;
    let mut callers: std::collections::BTreeSet<String> = index_callers;
    for row in rows {
        callers.insert(row?);
    }
    Ok(Some(
        callers
            .into_iter()
            .filter(|f| !changed_files.contains(f.as_str()))
            .map(PathBuf::from)
            .collect(),
    ))
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
            sig_end: Some(20),
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
    fn write_files_round_trips_fingerprints_and_null_sig_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bsl-graph.db");

        let mut w = GraphDbWriter::create(&path).unwrap();
        w.write_files(&[
            FileFingerprint { path: "/cfg/A.bsl".to_string(), fingerprint: 111, sig_hash: None },
            FileFingerprint { path: "/cfg/A.xml".to_string(), fingerprint: 222, sig_hash: None },
        ])
        .unwrap();
        w.finalize(&GraphMeta { revision: 1, fingerprint: 0, files: 0, built_at: "t".to_string() })
            .unwrap();

        let conn = open(&path);
        let (fp, sig): (i64, Option<i64>) = conn
            .query_row(
                "SELECT fingerprint, sig_hash FROM files WHERE path = '/cfg/A.bsl'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(fp as u64, 111);
        assert_eq!(sig, None, "sig_hash is NULL until the body-only fast path fills it");

        let xml_fp: i64 = conn
            .query_row("SELECT fingerprint FROM files WHERE path = '/cfg/A.xml'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(xml_fp as u64, 222);
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
