//! Streaming analysis provider for batch/analyze mode.
//!
//! This module provides `StreamingProvider` - an implementation of `AnalysisProvider`
//! optimized for batch analysis with minimal memory usage.
//!
//! # Architecture
//!
//! Unlike `SalsaProvider` which caches everything in the Salsa database,
//! `StreamingProvider` computes per-file data on-the-fly and releases it after use.
//!
//! ## Components
//!
//! - [`GlobalContext`] - Shared data across all files (configuration, symbol trees, etc.)
//! - [`FileReader`] - File content provider (disk or in-memory)
//! - [`StreamingProvider`] - Main provider implementation

mod dependency_resolver;
mod file_processor;
mod file_reader;
mod global_context;
mod orchestrator;
mod provider;
mod shared_state;
mod worker_thread;

pub use dependency_resolver::get_or_process_symbol_tree;
pub use file_processor::{FileProcessor, FileResult};
pub use file_reader::FileReader;
pub use global_context::GlobalContext;
pub use orchestrator::{AnalysisOrchestrator, AnalysisResults, OrchestratorError};
pub use provider::StreamingProvider;
pub use shared_state::{ClaimResult, FileStatus, ProcessError, SharedState};
pub use worker_thread::worker_main;
