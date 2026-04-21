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

/// A configuration visible from a given file: main or extension.
///
/// Carries only the metadata — never a filesystem URI — because name
/// resolution and type inference only consume the declarative
/// `Configuration` description.
#[derive(Clone)]
pub struct VisibleConfig {
    /// Extension name; `None` for the main configuration.
    pub name: Option<String>,
    /// Loaded configuration metadata.
    pub configuration: Arc<Configuration>,
}

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
}
