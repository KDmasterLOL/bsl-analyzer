//! NonStandardRegion diagnostic.
//!
//! Reports regions whose names do not match the standard region set for the
//! current module type.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::ModuleType;
use hir::module_structure::standard::is_standard_region;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Compatibility8320,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("NonStandardRegion::check").entered();

    let code = DiagnosticCode::NonStandardRegion;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let module_type = match ctx.file_path() {
        Some(path) => match ide_db::metadata::get_module_type_from_uri(&path) {
            Some(mt) => mt,
            None => return Vec::new(),
        },
        None => return Vec::new(),
    };

    if module_type == ModuleType::Unknown {
        return Vec::new();
    }

    let regions = ctx.module_level_regions();

    if regions.is_empty() {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    for region in regions.iter() {
        if !is_standard_region(module_type, &region.name) {
            diagnostics.push(Diagnostic {
                code,
                message: format!("Нужно удалить нестандартный раздел \"{}\"", region.name),
                severity: ctx.severity(code),
                range: region.range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    tracing::debug!(count = diagnostics.len(), "NonStandardRegion diagnostics found");
    diagnostics
}

// Per-module-type membership behaviour is owned and tested by
// `hir_def::module_structure::standard`. The handler is a thin
// projection — additional fixture coverage lives at the workspace
// integration level.
