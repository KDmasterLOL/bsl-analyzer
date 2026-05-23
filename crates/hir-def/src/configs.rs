//! Visible configuration access.
//!
//! BSL files can see multiple configurations at once: the main project and
//! every registered CFE extension. Name resolution for cross-module calls
//! (`CommonModule.Method`, `Documents.X.Method`) must iterate *all* visible
//! configurations; the union wins, with extensions overriding the main
//! configuration on name collisions.
//!
//! [`ConfigsDatabase`] is the narrow port through which `hir-def` (Resolver)
//! and `hir-ty` (inference) query this information. The adapter lives in
//! `ide-db` and routes to Salsa-tracked metadata queries, so configuration
//! changes invalidate dependent results automatically.

use std::sync::Arc;

use bsl_metadata::Configuration;
use vfs::FileId;

use crate::DefDatabase;

// Transitional alias (Phase 2.C). The canonical definition lives in
// `bsl-config` (Layer 0.5). This re-export keeps every existing
// `use hir_def::configs::VisibleConfig` and
// `use hir_def::VisibleConfig` site working while §2.E migrates
// consumers to import directly from `bsl-config`. The alias is
// removed in §2.F once the pre-deletion audit shows zero residual
// references via the hir-def path.
pub use bsl_config::VisibleConfig;

/// Metadata visibility trait.
///
/// Implemented by `ide-db`'s root database (production) and by test
/// fixtures. All implementations must route through Salsa-tracked queries so
/// that changes to configuration XML invalidate dependent inference and
/// resolution results.
#[salsa::db]
pub trait ConfigsDatabase: DefDatabase {
    /// All configurations visible from `file_id`: the file's own config plus
    /// every registered extension.
    ///
    /// Returns an empty `Vec` if no configuration is registered (tests,
    /// greenfield files). The main configuration, when present, has
    /// `VisibleConfig::name == None`. Extensions follow in the order they
    /// were registered — callers that need "extension wins" semantics
    /// should iterate in reverse.
    fn configurations(&self, file_id: FileId) -> Vec<VisibleConfig>;

    /// Resolve the *single merged* configuration visible from `file_id`.
    ///
    /// Returns the main configuration merged with the most-specific
    /// extension whose registered root is a prefix of `file_id`'s path
    /// (longest-prefix wins). Returns `None` when no configuration is
    /// registered or when `file_id` has no resolvable path.
    ///
    /// Separate from [`Self::configurations`]: that method returns every
    /// visible config (used by name resolution, which iterates them);
    /// this one returns the single merged view that consumers need when
    /// they pass exactly one `Configuration` down to a metadata-driven
    /// lowerer (SDBL HIR lowering is the immediate caller).
    ///
    /// Implementations must perform the merge through
    /// [`Configuration::merged_with_extension`] so the result keeps the
    /// "main + overlay" semantics of the runtime.
    fn merged_visible_configuration(&self, file_id: FileId) -> Option<Arc<Configuration>>;
}
