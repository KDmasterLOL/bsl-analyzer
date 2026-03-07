//! NonStandardRegion diagnostic.
//!
//! Validates that all region names in a module conform to standard region names for that specific module type.
//!
//! ## Why?
//! Standard region names improve code organization and maintainability. Each module type
//! (FormModule, ObjectModule, CommonModule, etc.) has specific allowed region names defined by 1C standards.
//!
//! ## Bad practice
//! ```bsl
//! #Область Переменные  // Non-standard for most modules, should be "ОписаниеПеременных"
//! Перем А;
//! #КонецОбласти
//! ```
//!
//! ## Good practice
//! ```bsl
//! #Область ОписаниеПеременных  // Standard region name
//! Перем А;
//! #КонецОбласти
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (INFO)
//! - **Tags:** STANDARD
//! - **Minutes to fix:** 1
//! - **No parameters:** Standard regions are fixed for each module type
//!
//! ## Implementation
//! Uses Salsa-cached `module_level_regions()` query for performance.
//! Ported from:

use crate::define_metadata;
use crate::metadata::*;
use crate::utils::standard_regions;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::ModuleType;

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
        if !standard_regions::is_standard_region(module_type, &region.name) {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_standard_regions_utility() {
        assert!(standard_regions::is_standard_region(
            ModuleType::CommonModule,
            "ПрограммныйИнтерфейс"
        ));
        assert!(standard_regions::is_standard_region(ModuleType::CommonModule, "Public"));
        assert!(!standard_regions::is_standard_region(ModuleType::CommonModule, "CustomRegion"));
    }

    #[test]
    fn test_case_insensitive_matching() {
        assert!(standard_regions::is_standard_region(ModuleType::CommonModule, "public"));
        assert!(standard_regions::is_standard_region(ModuleType::CommonModule, "PUBLIC"));
    }

    #[test]
    fn test_module_specific_regions() {
        assert!(standard_regions::is_standard_region(ModuleType::FormModule, "FormEventHandlers"));
        assert!(!standard_regions::is_standard_region(
            ModuleType::CommonModule,
            "FormEventHandlers"
        ));

        assert!(standard_regions::is_standard_region(ModuleType::FormModule, "Variables"));
        assert!(!standard_regions::is_standard_region(ModuleType::CommonModule, "Variables"));
    }

    #[test]
    fn test_private_region_all_types() {
        assert!(standard_regions::is_standard_region(ModuleType::CommonModule, "Private"));
        assert!(standard_regions::is_standard_region(
            ModuleType::FormModule,
            "СлужебныеПроцедурыИФункции"
        ));
        assert!(standard_regions::is_standard_region(ModuleType::ObjectModule, "Private"));
    }

    #[test]
    fn test_form_table_items_with_suffix() {
        assert!(standard_regions::is_standard_region(
            ModuleType::FormModule,
            "FormTableItemsEventHandlersProducts"
        ));
        assert!(standard_regions::is_standard_region(
            ModuleType::FormModule,
            "ОбработчикиСобытийЭлементовТаблицыФормыТовары"
        ));
        assert!(standard_regions::is_standard_region(
            ModuleType::FormModule,
            "FormTableItemsEventHandlers"
        ));
    }

    #[test]
    fn test_unknown_module_type() {
        assert!(!standard_regions::is_standard_region(ModuleType::Unknown, "Public"));
        assert!(!standard_regions::is_standard_region(ModuleType::Unknown, "Private"));
    }
}
