//! IDE database for bsl-analyzer.
//!
//! This crate provides the database for IDE functionality with full DefDatabase implementation.
//!
//! ## Module structure
//!
//! - **`root_db`** — `RootDatabase` trait (application-facing port)
//! - **`database`** — `RootDatabaseImpl` struct + all Salsa trait impls (adapter)
//! - **`types`** — Shared types (`SymbolKind`, `SdblHirEntries`)
//! - **`provider`** — `AnalysisProvider` trait (data access port)
//! - **`salsa_provider`** — `SalsaProvider` (Salsa-backed provider)
//! - **`queries`** — Salsa tracked query functions
//! - **`metadata`** — `MetadataDb` trait + configuration loading
//! - **`streaming`** — Runtime infrastructure for parallel batch analysis

// Re-export commonly used types
pub use base_db;
pub use hir::is_bsl_source;
pub use syntax::TextRange;
pub use vfs;

// ========================================================================
// Modules
// ========================================================================

pub mod database;
pub mod features;
pub mod metadata;
pub mod provider;
pub mod queries;
pub mod root_db;
pub mod salsa_provider;
pub mod streaming;
pub mod types;
pub(crate) mod vfs_helpers;

// ========================================================================
// Re-exports: public API
// ========================================================================

// Core types
pub use types::{SdblHirEntries, SymbolKind};

// Port: RootDatabase trait
pub use root_db::RootDatabase;

// Adapter: RootDatabaseImpl
pub use database::RootDatabaseImpl;

// Provider types
pub use provider::AnalysisProvider;
pub use salsa_provider::SalsaProvider;

// Streaming types
pub use streaming::{
    ClaimResult, FileReader, FileStatus, GlobalContext, ProcessError, SharedState,
    StreamingProvider,
};

// Salsa query functions
pub use queries::{
    all_sdbl_in_file_query, configuration_path_for_file, line_index_query, liveness_analysis_query,
    method_cfg_query, module_metadata_query, reaching_definitions_query, sdbl_hir_in_file_query,
};

// Metadata helpers
pub use metadata::build_module_metadata;
