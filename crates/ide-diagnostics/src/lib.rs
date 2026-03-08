//! Diagnostics for bsl-analyzer.
//!
//! This crate implements all 181 diagnostics.

mod code;
mod config;
mod context;
pub mod docs;
mod hir_dispatch;
mod metadata;
mod metadata_dispatch;
mod query;
mod runner;
mod single_pass;
mod types;

pub mod common_module_helpers;
pub mod handlers;
pub mod sdbl_utils;
pub mod utils;

#[cfg(test)]
pub mod test_utils;

// Re-exports for public API
pub use code::DiagnosticCode;
pub use config::{DiagnosticsConfig, EffectiveMetadata, MetadataOverride};
pub use context::DiagnosticsContext;
pub use handlers::get_metadata;
pub use metadata::{
    CleanCodeAttribute, DiagnosticCompatibilityMode, DiagnosticMetadata, DiagnosticScope,
    DiagnosticSeverityLevel, DiagnosticType, Impact, ImpactSeverity, MetadataTag, SoftwareQuality,
};
pub use query::file_diagnostics_query;
pub use types::{Diagnostic, DiagnosticOutput, DiagnosticTag, Fix, Severity, TextEdit};

/// Returns an iterator over all diagnostic codes.
pub fn all_diagnostic_codes() -> impl Iterator<Item = DiagnosticCode> {
    use strum::IntoEnumIterator;
    DiagnosticCode::iter()
}

/// Creates a simple HIR diagnostic with standard disabled check, severity, and tags.
///
/// Use this for `from_hir()` handlers that only need a code, message, and range
/// (no custom fixes or extra logic).
pub fn simple_hir_diagnostic(
    code: DiagnosticCode,
    message: impl Into<String>,
    range: ide_db::TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    if ctx.is_disabled_with_metadata(code) {
        return None;
    }
    Some(Diagnostic {
        code,
        message: message.into(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

use hir_dispatch::collect_hir_diagnostics;
use metadata_dispatch::collect_metadata_diagnostics;
use runner::{
    collect_configuration_diagnostics, collect_dataflow_diagnostics, collect_item_tree_diagnostics,
    collect_line_diagnostics, collect_module_bodies_diagnostics, collect_sdbl_hir_diagnostics,
    collect_syntax_diagnostics,
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
/// 4. **Configuration** - Configuration XML-based checks (SessionModule only)
/// 5. **SDBL HIR** - Query language diagnostics (collected during SDBL lowering)
/// 6. **HIR** - Diagnostics collected during BSL AST→HIR lowering
/// 7. **Dataflow** - CFG + liveness/reaching definitions analysis
/// 8. **Metadata HIR** - ModuleMetadata-based checks
pub fn diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut result = Vec::new();

    // 1. Line-based diagnostics (text/formatting checks)
    result.extend(safe_collect("line", || collect_line_diagnostics(ctx)));

    // 2. Tier 1: Syntax diagnostics
    result.extend(safe_collect("syntax", || collect_syntax_diagnostics(ctx)));

    // 3. Tier 2: Semantic diagnostics (ItemTree + ModuleBodies)
    result.extend(safe_collect("item_tree", || collect_item_tree_diagnostics(ctx)));
    result.extend(safe_collect("module_bodies", || collect_module_bodies_diagnostics(ctx)));

    // 4. Configuration-based diagnostics (SessionModule only)
    result.extend(safe_collect("configuration", || collect_configuration_diagnostics(ctx)));

    // 5. SDBL HIR diagnostics (collected during SDBL lowering)
    result.extend(safe_collect("sdbl_hir", || collect_sdbl_hir_diagnostics(ctx)));

    // 6. HIR-based diagnostics (collected during AST→HIR lowering)
    result.extend(safe_collect("hir", || collect_hir_diagnostics(ctx)));

    // 7. Dataflow-based diagnostics (using CFG + liveness analysis)
    result.extend(safe_collect("dataflow", || collect_dataflow_diagnostics(ctx)));

    // 8. Metadata-based diagnostics (using module_metadata from HIR)
    result.extend(safe_collect("metadata", || collect_metadata_diagnostics(ctx)));

    // 9. Deduplicate diagnostics with overlapping ranges for the same code
    deduplicate_diagnostics(&mut result);

    result
}

/// Run a diagnostic collector with panic protection.
///
/// If a collector panics (e.g. Salsa cancellation), logs a warning and returns
/// empty vec. Other collectors continue to produce partial results.
/// This is the single source of truth for panic recovery in diagnostic collection.
fn safe_collect(name: &str, f: impl FnOnce() -> Vec<Diagnostic>) -> Vec<Diagnostic> {
    let start = std::time::Instant::now();
    let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(diags) => diags,
        Err(e) => {
            tracing::warn!("Panic in collector '{name}': {e:?}");
            Vec::new()
        }
    };
    let elapsed = start.elapsed();
    if elapsed.as_millis() > 100 {
        tracing::info!(
            collector = name,
            elapsed_ms = elapsed.as_millis() as u64,
            diags = result.len(),
            "Slow collector"
        );
    }
    result
}

/// Remove duplicate diagnostics with the same code and overlapping/contained ranges.
///
/// Some diagnostics like UnreachableCode can be detected by multiple sources
/// (HIR lowering and CFG analysis). This function merges overlapping diagnostics
/// by keeping the diagnostic with the larger range when ranges overlap.
///
/// Only applies to specific diagnostic codes that are known to have duplicates.
fn deduplicate_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    // Codes that need deduplication due to multiple detection sources
    let dedupe_codes = [DiagnosticCode::UnreachableCode];

    // Separate diagnostics into those that need deduplication and those that don't
    let (mut to_dedupe, mut keep): (Vec<_>, Vec<_>) =
        diagnostics.drain(..).partition(|d| dedupe_codes.contains(&d.code));

    // Deduplicate the relevant diagnostics
    if !to_dedupe.is_empty() {
        // Sort by range start, then by range length (descending) to prefer larger ranges
        to_dedupe.sort_by(|a, b| {
            a.range.start().cmp(&b.range.start()).then_with(|| b.range.len().cmp(&a.range.len()))
        });

        // Remove diagnostics whose range is contained in or overlaps with a previous one
        let mut deduped: Vec<Diagnostic> = Vec::with_capacity(to_dedupe.len());
        for diag in to_dedupe {
            let dominated = deduped.iter().any(|existing| {
                // Check if diag's range is fully contained in existing's range
                existing.range.contains_range(diag.range)
                    // Or if they significantly overlap (same start or end)
                    || (existing.range.start() == diag.range.start()
                        || existing.range.end() == diag.range.end())
                        && ranges_overlap(existing.range, diag.range)
            });
            if !dominated {
                deduped.push(diag);
            }
        }
        keep.extend(deduped);
    }

    *diagnostics = keep;
}

/// Check if two ranges overlap (have common region).
fn ranges_overlap(a: ide_db::TextRange, b: ide_db::TextRange) -> bool {
    a.start() < b.end() && b.start() < a.end()
}
