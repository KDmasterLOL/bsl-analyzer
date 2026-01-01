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
//! Ported from:
//! - DuplicateRegionDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - duplicate_region.rs (bsl-language-server-rust) - Rust reference (tree-sitter)
//!
//! Adapted to use Rowan SyntaxNode and PreRegionDir AST helper.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use std::collections::HashMap;
use syntax::{ast::AstNode, ast::PreRegionDir, SyntaxKind, TextSize};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("DuplicateRegion::check").entered();

    if ctx.config.is_disabled(DiagnosticCode::DuplicateRegion) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let regions = collect_module_level_regions(&root);
    let diagnostics = report_duplicates(regions);

    tracing::debug!(count = diagnostics.len(), "DuplicateRegion diagnostics found");
    diagnostics
}

fn collect_module_level_regions(root: &syntax::SyntaxNode) -> Vec<(String, TextRange)> {
    let mut regions = Vec::new();

    // CRITICAL: Use children() not descendants() - only module level
    for child in root.children() {
        if child.kind() == SyntaxKind::PRE_REGION_DIR {
            if let Some(region) = PreRegionDir::cast(child.clone()) {
                // Only region starts (#Область/#Region), not ends (#КонецОбласти/#EndRegion)
                if region.is_start() {
                    if let Some(name) = region.name() {
                        // PRE_REGION_DIR node includes entire region from #Область to #КонецОбласти
                        // We only want the range of the first line (#Область Name)
                        let text = child.text().to_string();
                        let first_line = text.lines().next().unwrap_or(&text);
                        let first_line_len = first_line.len();

                        let start = child.text_range().start();
                        let end = start + TextSize::from(first_line_len as u32);
                        let range = TextRange::new(start, end);

                        regions.push((name, range));
                    }
                }
            }
        }
    }

    regions
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

fn report_duplicates(regions: Vec<(String, TextRange)>) -> Vec<Diagnostic> {
    let mut groups: HashMap<String, Vec<(String, TextRange)>> = HashMap::new();

    // Group regions by canonical name
    for (name, range) in regions {
        let canonical = get_canonical_name(&name);
        groups.entry(canonical).or_default().push((name, range));
    }

    let mut diagnostics = Vec::new();

    // Report first occurrence for each duplicate group
    for (_canonical, group) in groups {
        if group.len() > 1 {
            let (first_name, first_range) = &group[0];

            diagnostics.push(Diagnostic {
                code: DiagnosticCode::DuplicateRegion,
                message: format!("Нужно удалить дубли раздела \"{}\"", first_name),
                severity: Severity::Information,
                range: *first_range,
                tags: vec![],
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
    use super::*;
    use crate::{
        test_utils::assert_diagnostic_range_multiline, DiagnosticsConfig, DiagnosticsContext,
    };
    use ide_db::base_db::SourceDatabase;
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let config = Rc::new(DiagnosticsConfig::default());
        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/DuplicateRegionDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 3, "Should match Java: 3 diagnostics (lines 12, 16, 21)");

        // Line 12 (1-indexed) = line 11 (0-indexed): #Область СлужебныйПрограммныйИнтерфейс
        assert_diagnostic_range_multiline(code, &diagnostics[0], 11, 0, 11, 38);
        assert!(
            diagnostics[0].message.contains("СлужебныйПрограммныйИнтерфейс"),
            "Message should contain original region name"
        );

        // Line 16 (1-indexed) = line 15 (0-indexed): #Область СлужебныеПроцедурыИФункции
        assert_diagnostic_range_multiline(code, &diagnostics[1], 15, 0, 15, 35);
        assert!(
            diagnostics[1].message.contains("СлужебныеПроцедурыИФункции"),
            "Message should contain original region name"
        );

        // Line 21 (1-indexed) = line 20 (0-indexed): #Region EventHandlers
        assert_diagnostic_range_multiline(code, &diagnostics[2], 20, 0, 20, 21);
        assert!(
            diagnostics[2].message.contains("EventHandlers"),
            "Message should contain original region name"
        );
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

        let diagnostics = check_diagnostic(code);
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

        let diagnostics = check_diagnostic(code);
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

        let diagnostics = check_diagnostic(code);
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

        let diagnostics = check_diagnostic(code);
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

        let diagnostics = check_diagnostic(code);
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

        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Non-standard regions with exact match are duplicates");
    }
}
