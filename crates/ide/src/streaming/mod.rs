//! Streaming analysis with diagnostics integration.
//!
//! This module provides high-level API for batch file analysis with
//! low memory usage and parallel processing.
//!
//! Unlike the infrastructure components in `ide_db::streaming`, this module
//! contains feature-level coordination that can depend on `ide_diagnostics`,
//! `ide_assists`, and other feature crates.
//!
//! ## Architecture
//!
//! ```text
//! ide::streaming (Features Layer)
//! ├── FileProcessor      - 3-phase processing WITH diagnostics
//! ├── WorkerThread       - Worker loop with panic protection
//! └── AnalysisOrchestrator - High-level coordination
//!     │
//!     ├── ide_diagnostics (diagnostics collection)
//!     │
//!     └── ide_db::streaming (Infrastructure Layer)
//!         ├── SharedState        - Lock-free coordination
//!         ├── GlobalContext      - Shared data
//!         ├── StreamingProvider  - AnalysisProvider implementation
//!         └── FileReader         - I/O abstraction
//! ```

mod file_processor;
mod orchestrator;
mod worker_thread;

pub use file_processor::{DiagnosticOutput, FileProcessor, FileResult};
pub use orchestrator::{AnalysisOrchestrator, AnalysisResults, OrchestratorError};
pub use worker_thread::worker_main;
