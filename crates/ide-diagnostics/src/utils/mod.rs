pub mod literal_context;
pub mod nstr;
pub mod platform_event_handlers;
pub mod preprocessor_symbols;
pub mod standard_regions;

use crate::{Diagnostic, DiagnosticsContext};

/// Iterate over all method bodies and module-level code, collecting diagnostics.
///
/// Calls `f` for each body (method bodies first, then module-level code if present).
/// Returns the accumulated diagnostics.
pub(crate) fn for_each_body(
    ctx: &DiagnosticsContext,
    mut f: impl FnMut(&hir::Body, &hir::BodySourceMap, &mut Vec<Diagnostic>),
) -> Vec<Diagnostic> {
    let module_bodies = ctx.module_bodies();
    let mut diagnostics = Vec::new();
    for (_, body, source_map) in module_bodies.method_bodies() {
        f(body, source_map, &mut diagnostics);
    }
    if let Some(module_code) = module_bodies.module_code_result() {
        f(&module_code.body, &module_code.source_map, &mut diagnostics);
    }
    diagnostics
}
