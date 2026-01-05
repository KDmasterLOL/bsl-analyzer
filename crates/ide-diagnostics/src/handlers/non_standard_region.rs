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
//! - NonStandardRegionDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - Regions.java (bsl-language-server) - standard regions mapping

use crate::utils::standard_regions;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::ModuleType;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("NonStandardRegion::check").entered();

    if ctx.config.is_disabled(DiagnosticCode::NonStandardRegion) {
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

    let regions = ctx.db.module_level_regions(ctx.file_id);

    if regions.is_empty() {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    for region in regions.iter() {
        if !standard_regions::is_standard_region(module_type, &region.name) {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::NonStandardRegion,
                message: format!("Нужно удалить нестандартный раздел \"{}\"", region.name),
                severity: Severity::Information,
                range: region.range,
                tags: vec![],
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
