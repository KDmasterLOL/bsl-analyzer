//! DuplicateRegion diagnostic.
//!
//! Detects duplicate region names at module level.
//!
//! ## Why?
//! Duplicate region names at module level cause confusion and unclear code organization.
//! Standard region names (Russian/English variants) are treated as duplicates.
//!
//! ## Bad practice
//! ```bsl
//! #Область ПрограммныйИнтерфейс
//! // код
//! #КонецОбласти
//!
//! #Region Public  // Duplicate! Same as ПрограммныйИнтерфейс
//! // код
//! #EndRegion
//! ```
//!
//! ## Good practice
//! ```bsl
//! #Область ПрограммныйИнтерфейс
//! // код
//! #КонецОбласти
//!
//! #Область СлужебныйПрограммныйИнтерфейс  // Different region
//! // код
//! #КонецОбласти
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (INFO)
//! - **Tags:** STANDARD
//! - **Minutes to fix:** 1
//!
//! ## Standard Region Pairs
//! The diagnostic recognizes 9 standard BSL region pairs where Russian and English
//! variants are considered duplicates:
//! - Public / ПрограммныйИнтерфейс
//! - Internal / СлужебныйПрограммныйИнтерфейс
//! - Private / СлужебныеПроцедурыИФункции
//! - EventHandlers / ОбработчикиСобытий
//! - FormEventHandlers / ОбработчикиСобытийФормы
//! - FormHeaderItemsEventHandlers / ОбработчикиСобытийЭлементовШапкиФормы
//! - FormCommandsEventHandlers / ОбработчикиКомандФормы
//! - Variables / ОписаниеПеременных
//! - Initialize / Инициализация
//!
//! ## Implementation
//!
//! **Uses AST-based checking (not HIR-based) because:**
//! 1. Requires range of first line (`#Область Name`), not full block or name token only
//! 2. HIR RegionTree provides: full block range or name token range, but not first line range
//! 3. Calculating first line from RegionTree would still require parse + manual text processing
//! 4. No code simplification from HIR migration - same complexity remains
//! 5. AST approach is optimal: single parse query, minimal dependencies, direct access to needed data
//!
use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::base_db::RegionInfo;
use ide_db::TextRange;
use std::collections::HashMap;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
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
    let _span = tracing::debug_span!("DuplicateRegion::check").entered();

    let code = DiagnosticCode::DuplicateRegion;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // Use Salsa-cached query (LRU=256) - shared with non_standard_region
    let regions = ctx.module_level_regions();

    let diagnostics = report_duplicates(&regions, code, ctx);

    tracing::debug!(count = diagnostics.len(), "DuplicateRegion diagnostics found");
    diagnostics
}

fn get_canonical_name(name: &str) -> String {
    let lower = name.to_lowercase();
    match lower.as_str() {
        // Public / ПрограммныйИнтерфейс
        "public" | "программныйинтерфейс" => "Public".to_string(),

        // Internal / СлужебныйПрограммныйИнтерфейс
        "internal" | "служебныйпрограммныйинтерфейс" => {
            "Internal".to_string()
        }

        // Private / СлужебныеПроцедурыИФункции
        "private" | "служебныепроцедурыифункции" => "Private".to_string(),

        // EventHandlers / ОбработчикиСобытий
        "eventhandlers" | "обработчикисобытий" => "EventHandlers".to_string(),

        // FormEventHandlers / ОбработчикиСобытийФормы
        "formeventhandlers" | "обработчикисобытийформы" => {
            "FormEventHandlers".to_string()
        }

        // FormHeaderItemsEventHandlers / ОбработчикиСобытийЭлементовШапкиФормы
        "formheaderitemseventhandlers" | "обработчикисобытийэлементовшапкиформы" => {
            "FormHeaderItemsEventHandlers".to_string()
        }

        // FormCommandsEventHandlers / ОбработчикиКомандФормы
        "formcommandseventhandlers" | "обработчикикомандформы" => {
            "FormCommandsEventHandlers".to_string()
        }

        // Variables / ОписаниеПеременных
        "variables" | "описаниепеременных" => "Variables".to_string(),

        // Initialize / Инициализация
        "initialize" | "инициализация" => "Initialize".to_string(),

        // Non-standard regions: keep original name (case-sensitive)
        _ => name.to_string(),
    }
}

fn report_duplicates(
    regions: &[RegionInfo],
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let mut groups: HashMap<String, Vec<(String, TextRange)>> = HashMap::new();

    // Group regions by canonical name
    for region in regions {
        let canonical = get_canonical_name(&region.name);
        groups.entry(canonical).or_default().push((region.name.clone(), region.range));
    }

    let mut diagnostics = Vec::new();

    // Report first occurrence for each duplicate group
    for (_canonical, group) in groups {
        if group.len() > 1 {
            let (first_name, first_range) = &group[0];

            diagnostics.push(Diagnostic {
                code,
                message: format!("Нужно удалить дубли раздела \"{}\"", first_name),
                severity: ctx.severity(code),
                range: *first_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    // Sort by position for deterministic ordering
    diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range_multiline, check_ast_diagnostic};

    #[test]
    fn test_duplicate_internal_regions() {
        // СлужебныйПрограммныйИнтерфейс and Internal map to same canonical "Internal"
        let code = r#"#Область СлужебныйПрограммныйИнтерфейс
// код
#КонецОбласти

#Region Internal
// код
#EndRegion"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range_multiline(code, &diagnostics[0], 0, 0, 0, 38);
        assert!(diagnostics[0].message.contains("СлужебныйПрограммныйИнтерфейс"));
    }

    #[test]
    fn test_duplicate_private_regions() {
        // СлужебныеПроцедурыИФункции and Private map to same canonical "Private"
        let code = r#"#Область СлужебныеПроцедурыИФункции
// код
#КонецОбласти

#Region Private
// код
#EndRegion"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range_multiline(code, &diagnostics[0], 0, 0, 0, 35);
        assert!(diagnostics[0].message.contains("СлужебныеПроцедурыИФункции"));
    }

    #[test]
    fn test_duplicate_event_handlers_regions() {
        // EventHandlers and ОбработчикиСобытий map to same canonical "EventHandlers"
        let code = r#"#Region EventHandlers
// код
#EndRegion

#Область ОбработчикиСобытий
// код
#КонецОбласти"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range_multiline(code, &diagnostics[0], 0, 0, 0, 21);
        assert!(diagnostics[0].message.contains("EventHandlers"));
    }

    #[test]
    fn test_no_duplicates() {
        let code = r#"
#Область ПрограммныйИнтерфейс
#КонецОбласти

#Область СлужебныйПрограммныйИнтерфейс
#КонецОбласти

#Область СлужебныеПроцедурыИФункции
#КонецОбласти
        "#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "No duplicates - should be 0 diagnostics");
    }

    #[test]
    fn test_nested_regions_ignored() {
        let code = r#"
#Область ПрограммныйИнтерфейс
    Процедура Тест()
        #Область ВложеннаяОбласть
        #КонецОбласти
    КонецПроцедуры
#КонецОбласти

#Область ВложеннаяОбласть
// This is module-level, not nested - different from the one inside procedure
#КонецОбласти
        "#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Nested regions inside procedures don't conflict with module-level"
        );
    }

    #[test]
    fn test_standard_ru_en_duplicate() {
        let code = r#"
#Область СлужебныйПрограммныйИнтерфейс
#КонецОбласти

#Region Internal
// This is a duplicate - Russian and English variants map to same canonical name
#EndRegion
        "#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "Russian and English standard region names are duplicates"
        );

        // Should report first occurrence (line 2, 0-indexed: 1)
        assert_diagnostic_range_multiline(code, &diagnostics[0], 1, 0, 1, 38);
        assert!(diagnostics[0].message.contains("СлужебныйПрограммныйИнтерфейс"));
    }

    #[test]
    fn test_case_insensitive_canonical() {
        let code = r#"
#Region Internal
#EndRegion

#Region INTERNAL
// This is a duplicate - case-insensitive canonical matching
#EndRegion
        "#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "Case-insensitive canonical matching for standard regions"
        );
    }

    #[test]
    fn test_non_standard_exact_match() {
        let code = r#"
#Область КастомнаяОбласть
#КонецОбласти

#Область кастомнаяобласть
// Different case - non-standard regions are case-sensitive
#КонецОбласти
        "#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Non-standard regions with different case are not duplicates"
        );
    }

    #[test]
    fn test_non_standard_exact_duplicate() {
        let code = r#"
#Область КастомнаяОбласть
#КонецОбласти

#Область КастомнаяОбласть
// Exact match - this is a duplicate
#КонецОбласти
        "#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Non-standard regions with exact match are duplicates");
    }
}
