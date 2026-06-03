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

    let region_tree = ctx.region_tree();

    let mut diagnostics = Vec::new();
    for idx in region_tree.module_level_regions() {
        let region = region_tree.region(idx);
        let name = region.name.as_str();
        if !is_standard_region(module_type, name) {
            diagnostics.push(Diagnostic {
                code,
                message: format!("Нужно удалить нестандартный раздел \"{}\"", name),
                severity: ctx.severity(code),
                range: region.directive_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    tracing::debug!(count = diagnostics.len(), "NonStandardRegion diagnostics found");
    diagnostics
}
