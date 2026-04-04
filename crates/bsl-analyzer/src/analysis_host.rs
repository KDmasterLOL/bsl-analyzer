//! AnalysisHost — mutable wrapper around the Salsa database.
//!
//! Provides controlled access to `RootDatabaseImpl`, allowing
//! immutable snapshots for concurrent queries via `analysis()`.

use ide::{Analysis, RootDatabaseImpl};

/// Wrapper around the mutable Salsa database.
///
/// AnalysisHost provides controlled access to the database,
/// allowing snapshots for concurrent queries.
#[derive(Default)]
pub struct AnalysisHost {
    db: RootDatabaseImpl,
}

impl AnalysisHost {
    /// Creates an Analysis snapshot for queries.
    ///
    /// This is a cheap operation that creates an immutable view of the database.
    /// Note: Salsa 0.25+ uses the database directly without explicit snapshots.
    pub fn analysis(&self) -> Analysis {
        Analysis::from_database(self.db.clone())
    }

    /// Gets immutable access to the database.
    ///
    /// Used for Salsa interned types (DiagnosticsConfigId) and queries.
    pub fn raw_database(&self) -> &RootDatabaseImpl {
        &self.db
    }

    /// Gets mutable access to the database.
    ///
    /// Use this to apply changes (file updates, config changes, etc.).
    pub fn raw_database_mut(&mut self) -> &mut RootDatabaseImpl {
        &mut self.db
    }
}
