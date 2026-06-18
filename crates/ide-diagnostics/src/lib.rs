mod code;
mod config;
mod context;
pub mod docs;
mod effective;
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

/// File diagnostics with `&ИзменениеИКонтроль` extension merging applied.
///
/// Runs the ordinary standalone pass, then — only for an extension module that pairs to a
/// base with a usable change-and-validate splice — adds the inference diagnostics computed
/// against the spliced effective module, remapped to the `#Вставка` ranges the author
/// wrote. Copied-base inference false positives inside change-and-validate bodies (which
/// reference base-module siblings absent from the standalone ext file) are suppressed.
///
/// For every ordinary module — and every extension file without a usable change — this is
/// byte-identical to the standalone `diagnostics` pass (`effective_target` returns `None`).
pub fn file_diagnostics(
    db: &dyn ide_db::RootDatabase,
    file_id: vfs::FileId,
    config: &DiagnosticsConfig,
) -> Vec<Diagnostic> {
    let config_path_input = ide_db::configuration_path_for_file(db, file_id);
    let provider = ide_db::SalsaProvider::new(db, config_path_input);
    let standalone = diagnostics(&DiagnosticsContext::new(config, file_id, &provider));
    apply_extension_merge(db, file_id, config, config_path_input, None, standalone)
}

/// Augment a file's already-computed standalone diagnostics with the `&ИзменениеИКонтроль`
/// effective merge, when `file_id` is an extension module that pairs to a base with a usable
/// change-and-validate splice. For every other file this returns `standalone` unchanged.
///
/// Factored out of [`file_diagnostics`] so the batch CLI (which builds its provider
/// `with_file_set` and a run-global configuration path) can reuse the exact same merge while
/// keeping its own standalone provider construction — pass the same `config_path_input` and
/// `file_set` the standalone pass used so the effective pass resolves metadata/cross-module
/// context identically.
pub fn apply_extension_merge<'db>(
    db: &'db dyn ide_db::RootDatabase,
    file_id: vfs::FileId,
    config: &DiagnosticsConfig,
    config_path_input: Option<ide_db::metadata::ConfigurationPathInput<'db>>,
    file_set: Option<&'db vfs::file_set::FileSet>,
    mut standalone: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let Some(eid) = ide_db::effective_target(db, file_id) else {
        return standalone;
    };
    let Some(effmod) = hir::effective_module_text(db, eid) else {
        return standalone;
    };

    // Effective pass: ONLY the inference collector, which reads exclusively `infer` +
    // `module_bodies` (both routed effective on this provider → one coherent source map).
    // The full runner is deliberately NOT used: its other collectors (arg/cfg/dataflow/
    // sdbl/metrics) read standalone-keyed queries that would mix source maps.
    let eff_provider = ide_db::SalsaProvider::with_file_set(db, config_path_input, file_set)
        .with_effective(eid, file_id);
    let eff_ctx = DiagnosticsContext::new(config, file_id, &eff_provider);
    let eff_inference = hir_inference_dispatch::collect_inference_diagnostics(&eff_ctx);
    let kept_effective = effective::remap_inserted(eff_inference, &effmod.segments);

    // Drop standalone inference-class diagnostics inside change-and-validate bodies: their
    // copied-base statements reference base siblings the standalone ext file cannot see.
    let cav_bodies = effective::cav_body_ranges(&db.parse(file_id).syntax_node());
    standalone.retain(|d| {
        !(hir_inference_dispatch::INFERENCE_DIAGNOSTICS.contains(&d.code)
            && effective::range_inside_any(d.range, &cav_bodies))
    });

    standalone.extend(kept_effective);
    deduplicate_diagnostics(&mut standalone);
    standalone
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
