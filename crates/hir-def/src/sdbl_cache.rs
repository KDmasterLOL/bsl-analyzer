//! Salsa caches for SDBL queries and lowered HIR per file.
//!
//! Lives in `hir-def` (not `ide-db`) because `hir-ty`'s SDBL ↔ Ty bridge
//! needs to consume the cache from below. Reaches metadata through the
//! narrow [`ConfigsDatabase`] port — that keeps the invalidation chain
//! identical to every other typing query in this crate (a configuration
//! change re-invalidates the cache exactly like it does resolver
//! lookups and inference).
//!
//! The queries mirror the legacy `ide_db::queries::{all_sdbl_in_file_query,
//! sdbl_hir_in_file_query}` shapes so `ide-db` can re-export the type
//! aliases without touching any consumer.
//!
//! ## Configuration visibility
//!
//! SDBL lowering needs exactly one merged `Configuration` — the file's
//! main view plus the most-specific extension whose root path contains
//! the file. That decision is filesystem-bound (it inspects the file's
//! path against registered extension roots), so it lives in the
//! [`ConfigsDatabase::merged_visible_configuration`] port adapter
//! (`ide-db`) rather than here. This crate stays VFS-free; the cache
//! just consumes the resolved `Option<Arc<Configuration>>`.
//!
//! Lowering proceeds with `metadata = None` when no configuration is
//! visible (greenfield file, tests without a fixture configuration) —
//! `sdbl_hir` degrades gracefully to a name-only HIR without
//! metadata-driven type inference.

use std::sync::Arc;

use base_db::FileIdInput;

use crate::configs::ConfigsDatabase;
use crate::{DefDatabase, ModuleId, SdblExprId};

/// SDBL literals collected from a file's lowered bodies.
///
/// Each entry pairs a [`SdblExprId`] (unique across all bodies in the
/// file) with the [`syntax::SdblQueryInfo`] produced eagerly at body
/// lower (see [`crate::body::Body::sdbl_exprs`]). Sorted by source
/// position for deterministic downstream output.
pub type SdblInFile = Vec<(SdblExprId, syntax::SdblQueryInfo)>;

/// Per-file lowered SDBL HIR cache.
///
/// Maps each SDBL literal in a file (identified by its [`SdblExprId`])
/// to the lowered [`sdbl_hir::SdblPackage`]. The pair is the entry
/// point for SDBL-aware inference, completion and diagnostics.
pub type SdblHirEntries = Arc<Vec<(SdblExprId, Arc<sdbl_hir::SdblPackage>)>>;

/// Extract all SDBL literals from the file's lowered bodies.
///
/// Cheap pass over [`crate::body::Body::sdbl_exprs`] — the actual SDBL
/// parsing already happened at body lower time. Returns `(SdblExprId,
/// SdblQueryInfo)` pairs sorted by source position.
///
/// # Salsa caching
/// - LRU: 128 (lightweight extraction).
/// - Invalidation: automatic when [`crate::DefDatabase::module_bodies`]
///   changes.
#[salsa::tracked(lru = 128)]
pub fn all_sdbl_in_file_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<SdblInFile> {
    let _span = tracing::debug_span!("all_sdbl_in_file", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);

    let module_bodies = db.module_bodies(module_id);
    let mut result = Vec::new();

    for (local_id, body) in module_bodies.iter_bodies() {
        for (expr_id, query_info) in body.sdbl_exprs() {
            let sdbl_expr_id = SdblExprId::from_method(local_id, expr_id);
            result.push((sdbl_expr_id, query_info.clone()));
        }
    }

    if let Some(module_code) = module_bodies.module_code() {
        for (expr_id, query_info) in module_code.sdbl_exprs() {
            let sdbl_expr_id = SdblExprId::from_module_code(expr_id);
            result.push((sdbl_expr_id, query_info.clone()));
        }
    }

    result.sort_by_key(|(_, query_info)| query_info.bsl_literal_range.start());

    tracing::debug!(count = result.len(), "Collected SDBL from HIR");
    Arc::new(result)
}

/// Get SDBL HIR for all queries in a file.
///
/// Lowers every SDBL literal collected by [`all_sdbl_in_file_query`]
/// against the file's visible configuration (merging main + extension
/// when both are present). The result is the single source of truth for
/// any consumer that needs the `(SdblExprId → Arc<SdblPackage>)`
/// mapping.
///
/// # Salsa caching
/// - LRU: 64 (heavy SDBL HIR lowering operation).
/// - Invalidation: automatic when file content or any visible
///   configuration changes (transitive through
///   [`all_sdbl_in_file_query`] and
///   [`ConfigsDatabase::configurations`]).
///
/// # Performance
/// - First call: ~10-50ms (SDBL parsing + lowering + type inference).
/// - Cached: < 1ms.
#[salsa::tracked(lru = 64)]
pub fn sdbl_hir_for_file_query<'db>(
    db: &'db dyn ConfigsDatabase,
    file_id_input: FileIdInput<'db>,
) -> SdblHirEntries {
    let _span = tracing::debug_span!("sdbl_hir_for_file", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);

    let sdbl_queries = all_sdbl_in_file_query(db, file_id_input);
    if sdbl_queries.is_empty() {
        return Arc::new(Vec::new());
    }

    // Single merged view comes from the port — `merged_visible_configuration`
    // owns the "main + longest-prefix extension" decision (kept in `ide-db`
    // because the file-path prefix check is filesystem-bound and must not
    // leak into hir-def).
    let configuration = db.merged_visible_configuration(file_id);

    let mut result = Vec::with_capacity(sdbl_queries.len());
    for (expr_id, query_info) in sdbl_queries.iter() {
        if let Some(ref sdbl_ast) = query_info.query_ast {
            let sdbl_package = sdbl_hir::lower_sdbl_to_hir(sdbl_ast, configuration.clone());
            result.push((*expr_id, Arc::new(sdbl_package)));
        }
    }

    tracing::debug!(count = result.len(), "Lowered SDBL HIR for file");
    Arc::new(result)
}
