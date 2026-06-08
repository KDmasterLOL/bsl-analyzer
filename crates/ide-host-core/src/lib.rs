//! Shared analysis host for every BSL consumer (LSP server, MCP server, CLI).
//!
//! Owns the Salsa [`RootDatabaseImpl`] and hands out [`Analysis`] snapshots. Keeping
//! this in one library crate (rather than re-implemented per binary) means the
//! memory model, LRU/durability tuning, and load discipline are shared: optimising
//! the host once benefits LSP, MCP, and CLI alike.

mod load;

pub use load::{build_source_root, register_files_disk_backed};

use ide::{Analysis, RootDatabaseImpl};
use salsa::Database as _;

/// Owns the resident Salsa database and produces cheap [`Analysis`] snapshots over
/// cloned handles (the clone shares the Salsa storage / LRU cache).
#[derive(Default)]
pub struct AnalysisHost {
    db: RootDatabaseImpl,
}

impl AnalysisHost {
    pub fn analysis(&self) -> Analysis {
        Analysis::from_database(self.db.clone())
    }

    pub fn raw_database(&self) -> &RootDatabaseImpl {
        &self.db
    }

    pub fn raw_database_mut(&mut self) -> &mut RootDatabaseImpl {
        &mut self.db
    }

    pub fn request_cancellation(&mut self) {
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
