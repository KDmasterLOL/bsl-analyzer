mod code;
mod config;
mod context;
pub mod docs;
mod hir_dispatch;
mod hir_inference_dispatch;
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

pub fn all_diagnostic_codes() -> impl Iterator<Item = DiagnosticCode> {
    use strum::IntoEnumIterator;
    DiagnosticCode::iter()
}

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
use hir_inference_dispatch::{collect_arg_diagnostics, collect_inference_diagnostics};
use metadata_dispatch::collect_metadata_diagnostics;
use runner::{
    collect_configuration_diagnostics, collect_dataflow_diagnostics, collect_item_tree_diagnostics,
    collect_line_diagnostics, collect_module_bodies_diagnostics, collect_sdbl_hir_diagnostics,
    collect_syntax_diagnostics,
};

pub fn diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut result = Vec::new();

    result.extend(safe_collect("line", || collect_line_diagnostics(ctx)));

    result.extend(safe_collect("syntax", || collect_syntax_diagnostics(ctx)));

    result.extend(safe_collect("item_tree", || collect_item_tree_diagnostics(ctx)));
    result.extend(safe_collect("module_bodies", || collect_module_bodies_diagnostics(ctx)));

    result.extend(safe_collect("configuration", || collect_configuration_diagnostics(ctx)));

    result.extend(safe_collect("sdbl_hir", || collect_sdbl_hir_diagnostics(ctx)));

    result.extend(safe_collect("hir", || collect_hir_diagnostics(ctx)));

    result.extend(safe_collect("hir_inference", || collect_inference_diagnostics(ctx)));

    result.extend(safe_collect("hir_arg_inference", || collect_arg_diagnostics(ctx)));

    result.extend(safe_collect("dataflow", || collect_dataflow_diagnostics(ctx)));

    result.extend(safe_collect("metadata", || collect_metadata_diagnostics(ctx)));

    deduplicate_diagnostics(&mut result);

    result
}

fn safe_collect(name: &str, f: impl FnOnce() -> Vec<Diagnostic>) -> Vec<Diagnostic> {
    let start = std::time::Instant::now();
    tracing::debug!(collector = name, "collector started");
    let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(diags) => diags,
        Err(e) => {
            if e.is::<salsa::Cancelled>() {
                std::panic::resume_unwind(e);
            }
            let msg = if let Some(s) = e.downcast_ref::<&'static str>() {
                (*s).to_owned()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                format!("<non-string panic payload: {e:?}>")
            };
            tracing::warn!(collector = name, panic = %msg, "collector panicked");
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

fn deduplicate_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    let dedupe_codes = [DiagnosticCode::UnreachableCode];

    let (mut to_dedupe, mut keep): (Vec<_>, Vec<_>) =
        diagnostics.drain(..).partition(|d| dedupe_codes.contains(&d.code));

    if !to_dedupe.is_empty() {
        to_dedupe.sort_by(|a, b| {
            a.range.start().cmp(&b.range.start()).then_with(|| b.range.len().cmp(&a.range.len()))
        });

        let mut deduped: Vec<Diagnostic> = Vec::with_capacity(to_dedupe.len());
        for diag in to_dedupe {
            let dominated = deduped.iter().any(|existing| {
                existing.range.contains_range(diag.range)
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

fn ranges_overlap(a: ide_db::TextRange, b: ide_db::TextRange) -> bool {
    a.start() < b.end() && b.start() < a.end()
}
