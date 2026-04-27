//! AnalysisHost — mutable wrapper around the Salsa database.
//!
//! Provides controlled access to `RootDatabaseImpl`, allowing
//! immutable snapshots for concurrent queries via `analysis()`.

use ide::{Analysis, RootDatabaseImpl};
use salsa::Database as _;

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

    /// Forces outstanding database handles held by background workers to be
    /// dropped before `process_changes` invokes Salsa setters.
    ///
    /// Internally calls `zalsa_mut()`, which blocks until every live
    /// `RootDatabaseImpl` clone outside the main loop has been released.
    /// This is one of two defensive layers against the DashMap/Salsa ABBA
    /// described in `base_db::Files` — the other is the short-lock pattern
    /// in the setters themselves.
    ///
    /// NB: if BSL analysis ever introduces Salsa fixpoint cycles, switch to
    /// `synthetic_write(Durability::LOW)` per rust-analyzer's
    /// `AnalysisHost::trigger_cancellation`, which bumps a revision as a
    /// reset marker to clear fixpoint poisoning.
    pub fn request_cancellation(&mut self) {
        // Anything above WARN_MS is the suspected Windows stall: zalsa_mut()
        // spinning on a live snapshot held by a background worker.
        const NOTABLE_MS: u64 = 50;
        const WARN_MS: u64 = 200;

        let start = std::time::Instant::now();
        self.db.trigger_cancellation();
        let elapsed_ms = start.elapsed().as_millis() as u64;
        if elapsed_ms >= WARN_MS {
            tracing::warn!(elapsed_ms, "trigger_cancellation slow — live Salsa snapshots");
        } else if elapsed_ms >= NOTABLE_MS {
            tracing::info!(elapsed_ms, "trigger_cancellation notable");
        } else {
            tracing::debug!(elapsed_ms, "trigger_cancellation");
        }
    }
}
