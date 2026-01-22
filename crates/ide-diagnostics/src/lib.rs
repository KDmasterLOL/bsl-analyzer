//! Diagnostics for bsl-analyzer.
//!
//! This crate implements all 181 diagnostics from bsl-language-server.

mod code;
mod config;
mod context;
mod hir_dispatch;
mod metadata;
mod metadata_dispatch;
mod metadata_registry;
mod query;
mod runner;
mod types;

pub mod common_module_helpers;
pub mod handlers;
pub mod metadata_diagnostic;
pub mod method_description;
pub mod rules;
pub mod sdbl_utils;
pub mod utils;

#[cfg(test)]
pub mod test_utils;

// Re-exports for public API
pub use code::DiagnosticCode;
pub use config::{DiagnosticsConfig, EffectiveMetadata, MetadataOverride};
pub use context::DiagnosticsContext;
pub use metadata::{
    DiagnosticCompatibilityMode, DiagnosticMetadata, DiagnosticScope, DiagnosticSeverityLevel,
    DiagnosticType, MetadataTag,
};
pub use metadata_registry::get_metadata;
pub use query::file_diagnostics_query;
pub use types::{Diagnostic, DiagnosticOutput, DiagnosticTag, Fix, Severity, TextEdit};

use hir_dispatch::collect_hir_diagnostics;
use metadata_dispatch::collect_metadata_diagnostics;
use runner::{
    collect_dataflow_diagnostics, collect_metadata_ast_diagnostics, collect_sdbl_hir_diagnostics,
    collect_semantic_diagnostics, collect_syntax_diagnostics, collect_text_diagnostics,
};

/// Runs all diagnostics on a file.
///
/// This is the single entry point for all diagnostics. Each diagnostic type
/// has a dedicated collector function in `runner.rs` or its own module.
///
/// ## Diagnostic Types (in execution order)
///
/// 1. **Text-based** - Line/formatting checks (single AST pass)
/// 2. **Syntax (Tier 1)** - Syntactic pattern checks
/// 3. **Semantic (Tier 2)** - Semantic analysis checks
/// 4. **Metadata AST (Tier 3)** - AST-based metadata property checks
/// 5. **SDBL HIR** - Query language diagnostics (collected during SDBL lowering)
/// 6. **HIR** - Diagnostics collected during BSL AST→HIR lowering
/// 7. **Dataflow** - CFG + liveness/reaching definitions analysis
/// 8. **Metadata HIR** - ModuleMetadata-based checks
pub fn diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut result = Vec::new();

    // 1. Text-based diagnostics (single AST pass)
    result.extend(collect_text_diagnostics(ctx));

    // 2. Tier 1: Syntax diagnostics
    result.extend(collect_syntax_diagnostics(ctx));

    // 3. Tier 2: Semantic diagnostics
    result.extend(collect_semantic_diagnostics(ctx));

    // 4. Tier 3: Metadata diagnostics (AST-based)
    result.extend(collect_metadata_ast_diagnostics(ctx));

    // 5. SDBL HIR diagnostics (collected during SDBL lowering)
    result.extend(collect_sdbl_hir_diagnostics(ctx));

    // 6. HIR-based diagnostics (collected during AST→HIR lowering)
    result.extend(collect_hir_diagnostics(ctx));

    // 7. Dataflow-based diagnostics (using CFG + liveness analysis)
    result.extend(collect_dataflow_diagnostics(ctx));

    // 8. Metadata-based diagnostics (using module_metadata from HIR)
    result.extend(collect_metadata_diagnostics(ctx));

    result
}
