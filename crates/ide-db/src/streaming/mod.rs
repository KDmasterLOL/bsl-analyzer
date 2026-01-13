//! Streaming analysis provider for batch/analyze mode.
//!
//! This module provides infrastructure components for streaming analysis:
//! - `StreamingProvider` - Implementation of `AnalysisProvider` for low-memory batch analysis
//! - `SharedState` - Lock-free coordination for parallel workers
//! - `GlobalContext` - Read-only shared data across all files
//! - `FileReader` - File content provider (disk or in-memory)
//!
//! # Architecture
//!
//! This module contains **infrastructure layer** components (low-level primitives).
//! For **feature layer** components (FileProcessor, WorkerThread, AnalysisOrchestrator),
//! see the `ide::streaming` module which can depend on `ide_diagnostics`.
//!
//! ## Why the split?
//!
//! - Infrastructure (this module): NO dependencies on feature crates
//! - Features (`ide::streaming`): CAN depend on `ide_diagnostics`, `ide_assists`, etc.
//!
//! This separation prevents circular dependencies between `ide-db` and `ide-diagnostics`.

mod dependency_resolver;
mod file_reader;
mod global_context;
mod provider;
mod shared_state;

pub use dependency_resolver::get_or_process_symbol_tree;
pub use file_reader::FileReader;
pub use global_context::GlobalContext;
pub use provider::StreamingProvider;
pub use shared_state::{ClaimResult, FileStatus, ProcessError, SharedState};
