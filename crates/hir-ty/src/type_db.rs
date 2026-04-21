//! Metadata access bridge for the type system.
//!
//! [`TypeDatabase`] is the port through which `hir-ty` queries 1C metadata
//! (main configuration + CFE extensions) without depending on `ide-db`.
//! The adapter lives in `ide-db` and delegates to `AnalysisProvider`.
//!
//! ## Why a separate trait
//!
//! Keeping metadata access out of [`crate::db::HirDatabase`] lets the type
//! inference layer depend only on the narrow surface it actually needs, and
//! makes the direction of the dependency explicit:
//!
//! ```text
//! hir-ty  ─ depends on ──▶ hir-def (DefDatabase)
//!                          │
//!                          └── plus TypeDatabase (metadata access)
//!
//! ide-db  ─ implements ──▶ TypeDatabase (adapter)
//! ```
//!
//! Under 1C extension semantics, a file can see its own configuration and
//! every registered extension. Name resolution for cross-module calls
//! (`CommonModule.Method`, `Documents.X.Method`) must iterate *all* visible
//! configurations; the union of matches wins, with extensions overriding main
//! for same-named modules.

use std::sync::Arc;

use bsl_metadata::Configuration;
use hir_def::DefDatabase;
use vfs::FileId;

/// A configuration visible from a given file: main or extension.
///
/// `hir-ty` does not carry the configuration root path (unlike
/// `ide_db::provider::VisibleConfig`) because type inference never resolves
/// filesystem URIs — it only consumes metadata.
#[derive(Clone)]
pub struct VisibleConfig {
    /// Extension name; `None` for the main configuration.
    pub name: Option<String>,
    /// Loaded configuration metadata.
    pub configuration: Arc<Configuration>,
}

/// Metadata access trait for the type system.
///
/// Implemented by `ide-db`'s root database (production) and by test fixtures.
/// All implementations must route through Salsa-tracked queries so that
/// changes to configuration XML invalidate dependent inference results.
#[salsa::db]
pub trait TypeDatabase: DefDatabase {
    /// All configurations visible from `file_id`: the file's own config plus
    /// every registered extension.
    ///
    /// Returns an empty `Vec` if no configuration is registered (tests,
    /// greenfield files). The main configuration, when present, has
    /// `VisibleConfig::name == None`.
    fn configurations(&self, file_id: FileId) -> Vec<VisibleConfig>;
}
