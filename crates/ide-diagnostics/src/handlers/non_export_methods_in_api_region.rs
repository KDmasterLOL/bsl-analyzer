//! NonExportMethodsInApiRegion diagnostic.
//!
//! Detects non-export methods inside API regions (ПрограммныйИнтерфейс/Public).
//!
//! ## Why?
//! API regions should contain only exported methods that form the module's public interface.
//! Non-export methods in API regions:
//! - Pollute the public API surface
//! - Create confusion about what's truly public
//! - Make code organization unclear
//!
//! ## Bad practice
//! ```bsl
//! #Область ПрограммныйИнтерфейс
//!
//! // ❌ Non-export method in API region
//! Процедура ВнутренняяПроцедура()
//!     // implementation
//! КонецПроцедуры
//!
//! #КонецОбласти
//! ```
//!
//! ## Good practice
//! Move non-export methods to service regions:
//! ```bsl
//! #Область ПрограммныйИнтерфейс
//!
//! // ✅ Export method in API region
//! Процедура ПубличнаяПроцедура() Экспорт
//!     ВнутренняяПроцедура();
//! КонецПроцедуры
//!
//! #КонецОбласти
//!
//! #Область СлужебныеПроцедурыИФункции
//!
//! // ✅ Non-export method in service region
//! Процедура ВнутренняяПроцедура()
//!     // implementation
//! КонецПроцедуры
//!
//! #КонецОбласти
//! ```
//!
//! ## Configuration
//! - **skipAnnotatedMethods** (boolean, default: false) - Skip methods with built-in annotations
//!   - Built-in: &НаКлиенте, &НаСервере, &Вместо, &До, &После
//!   - Custom annotations (like &MyAnnotation) are always checked
//! - **Enabled by default:** Yes
//! - **Severity:** MAJOR
//! - **Tags:** STANDARD (concept)
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//!
//! Uses HIR infrastructure:
//! - **ItemTree** - method signatures (is_export, annotations, name_range)
//! - **RegionTree** - region hierarchy and API region detection
//! - Both cached via Salsa queries for performance
//!
//! ## API Regions
//! Detected as top-level regions with names:
//! - Russian: ПрограммныйИнтерфейс, СлужебныйПрограммныйИнтерфейс
//! - English: Public, Internal
//!
//! ## Note on Annotations
//! ItemTree only stores built-in annotations during lowering. Custom annotations
//! (like &MyAnnotation) are filtered out in `item_tree::lower::lower_annotations()`.
//! This allows `has_builtin_annotations()` to work by checking if annotations array is non-empty.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Annotation, Name, RegionTree};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("NonExportMethodsInApiRegion::check").entered();

    let code = DiagnosticCode::NonExportMethodsInApiRegion;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let skip_annotated_methods = ctx
        .config
        .get_bool(DiagnosticCode::NonExportMethodsInApiRegion, "skipAnnotatedMethods")
        .unwrap_or(false);

    // Get HIR structures (both cached via Salsa)
    let region_tree = ctx.region_tree();
    let item_tree = ctx.item_tree();

    let mut diagnostics = Vec::new();

    // Check procedures
    for (_, proc) in item_tree.procedures() {
        check_method(
            proc.is_export,
            &proc.name,
            proc.name_range,
            proc.source_range,
            &proc.annotations,
            skip_annotated_methods,
            &region_tree,
            code,
            ctx,
            &mut diagnostics,
        );
    }

    // Check functions
    for (_, func) in item_tree.functions() {
        check_method(
            func.is_export,
            &func.name,
            func.name_range,
            func.source_range,
            &func.annotations,
            skip_annotated_methods,
            &region_tree,
            code,
            ctx,
            &mut diagnostics,
        );
    }

    tracing::debug!(count = diagnostics.len(), "NonExportMethodsInApiRegion diagnostics found");

    diagnostics
}

/// Check a single method (procedure or function) for non-export in API region.
#[allow(clippy::too_many_arguments)]
fn check_method(
    is_export: bool,
    name: &Name,
    name_range: TextRange,
    source_range: TextRange,
    annotations: &[Annotation],
    skip_annotated_methods: bool,
    region_tree: &RegionTree,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Skip exported methods - they're allowed in API regions
    if is_export {
        return;
    }

    // Check if method is inside an API region
    let Some(region_name) = region_tree.root_api_region_for_range(source_range) else {
        return; // Not in API region - ok
    };

    // Optionally skip methods with built-in annotations
    if skip_annotated_methods && has_builtin_annotations(annotations) {
        return;
    }

    // Report diagnostic
    diagnostics.push(Diagnostic {
        code,
        message: format!(
            "Move non export method \"{}\" from \"{}\" region",
            name.as_str(),
            region_name
        ),
        severity: ctx.severity(code),
        range: name_range,
        tags: ctx.tags(code),
        fixes: vec![],
    });
}

/// Check if method has any built-in annotations.
///
/// **Built-in annotations:**
/// - Execution context: &НаКлиенте, &НаСервере, &НаКлиентеНаСервере, &НаСервереБезКонтекста
/// - Extension methods: &До, &После, &Вместо
///
/// **Why this works:**
/// ItemTree only stores built-in annotations during lowering (see `item_tree::lower::lower_annotations()`).
/// Custom annotations (like &MyAnnotation) are filtered out and don't appear in the annotations array.
/// Therefore:
/// - `!annotations.is_empty()` → has built-in annotations
/// - `annotations.is_empty()` → no built-in annotations (might have custom ones, but we can't detect them)
///
/// **skipAnnotatedMethods behavior:**
/// - `true` → skip methods with built-in annotations (&Вместо, &НаКлиенте, etc.)
/// - `false` → check all non-export methods regardless of annotations
fn has_builtin_annotations(annotations: &[Annotation]) -> bool {
    !annotations.is_empty()
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
        range_to_line_col,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_non_export_in_api_region() {
        let code = r#"
#Область ПрограммныйИнтерфейс

// внутри региона, экспортная - не должно срабатывать
Процедура Хорошая() Экспорт
КонецПроцедуры

// внутри региона, неэкспортная - должно срабатывать
Процедура Плохая()
КонецПроцедуры

#КонецОбласти

#Region Public

// внутри региона, экспортная - не должно срабатывать
Процедура Good() Экспорт
КонецПроцедуры

// внутри региона, неэкспортная - должно срабатывать
Процедура Bad()
КонецПроцедуры

#Region SomeRegion
    // внутри региона, вложенная область неэкспортная - должно срабатывать
    Procedure ShouldSayFuck()
    EndProcedure
#EndRegion

#EndRegion

#Область Нестандартизованная

// внутри неспециального региона, экспортная - не должно срабатывать
Процедура ОченьХорошая() Экспорт
КонецПроцедуры

// внутри неспециального региона, неэкспортная - не должно срабатывать
Процедура ТожеНичего()
КонецПроцедуры

#КонецОбласти

#Область КакаяТоЛевая
#Область ПрограммныйИнтерфейс

// не должно сработать
Функция ВложеннаяЭкспортная()

КонецФункции

#КонецОбласти
#КонецОбласти

// вне региона, не должно срабатывать
Процедура ВнеОбласти() Экспорт
КонецПроцедуры

// вне региона, не должно срабатывать
Процедура ВнеОбластиНеЭкспортная()
КонецПроцедуры

#Область СлужебныйПрограммныйИнтерфейс

Процедура СлужебныйПрограммныйИнтерфейс() // сработает
КонецПроцедуры

&Кастом
Процедура АннотированныйМетод() // сработает
КонецПроцедуры

&Вместо
Процедура МетодВместоРасширения() // опционально
КонецПроцедуры

#КонецОбласти"#;
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 6, "Expected 6 diagnostics, got {}", diagnostics.len());

        assert_diagnostic_range(code, &diagnostics[0], 8, 10, 16); // Плохая()
        assert_diagnostic_range(code, &diagnostics[1], 20, 10, 13); // Bad()
        assert_diagnostic_range(code, &diagnostics[2], 25, 14, 27); // ShouldSayFuck()
        assert_diagnostic_range(code, &diagnostics[3], 64, 10, 39); // СлужебныйПрограммныйИнтерфейс()
        assert_diagnostic_range(code, &diagnostics[4], 68, 10, 29); // АннотированныйМетод()
        assert_diagnostic_range(code, &diagnostics[5], 72, 10, 31); // МетодВместоРасширения()
    }

    #[test]
    fn test_skip_annotated_methods() {
        let code = r#"
#Область ПрограммныйИнтерфейс

// внутри региона, экспортная - не должно срабатывать
Процедура Хорошая() Экспорт
КонецПроцедуры

// внутри региона, неэкспортная - должно срабатывать
Процедура Плохая()
КонецПроцедуры

#КонецОбласти

#Region Public

// внутри региона, экспортная - не должно срабатывать
Процедура Good() Экспорт
КонецПроцедуры

// внутри региона, неэкспортная - должно срабатывать
Процедура Bad()
КонецПроцедуры

#Region SomeRegion
    // внутри региона, вложенная область неэкспортная - должно срабатывать
    Procedure ShouldSayFuck()
    EndProcedure
#EndRegion

#EndRegion

#Область Нестандартизованная

// внутри неспециального региона, экспортная - не должно срабатывать
Процедура ОченьХорошая() Экспорт
КонецПроцедуры

// внутри неспециального региона, неэкспортная - не должно срабатывать
Процедура ТожеНичего()
КонецПроцедуры

#КонецОбласти

#Область КакаяТоЛевая
#Область ПрограммныйИнтерфейс

// не должно сработать
Функция ВложеннаяЭкспортная()

КонецФункции

#КонецОбласти
#КонецОбласти

// вне региона, не должно срабатывать
Процедура ВнеОбласти() Экспорт
КонецПроцедуры

// вне региона, не должно срабатывать
Процедура ВнеОбластиНеЭкспортная()
КонецПроцедуры

#Область СлужебныйПрограммныйИнтерфейс

Процедура СлужебныйПрограммныйИнтерфейс() // сработает
КонецПроцедуры

&Кастом
Процедура АннотированныйМетод() // сработает
КонецПроцедуры

&Вместо
Процедура МетодВместоРасширения() // опционально
КонецПроцедуры

#КонецОбласти"#;

        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("skipAnnotatedMethods".to_string(), serde_json::Value::Bool(true));
        config
            .parameters
            .insert(DiagnosticCode::NonExportMethodsInApiRegion, serde_json::Value::Object(params));

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should skip line 72 (МетодВместоРасширения with &Вместо built-in annotation)
        // But NOT skip line 68 (АннотированныйМетод with &Кастом custom annotation)
        assert_eq!(diagnostics.len(), 5, "Expected 5 diagnostics with skipAnnotatedMethods=true");

        // Verify the skipped diagnostic is line 72
        for diag in &diagnostics {
            let (line, _, _, _) = range_to_line_col(code, diag.range);
            assert_ne!(line, 72, "Line 72 should be skipped with skipAnnotatedMethods=true");
        }
    }

    #[test]
    fn test_exported_methods_in_api_region() {
        let code = r#"
#Область ПрограммныйИнтерфейс

Процедура ЭкспортнаяПроцедура() Экспорт
КонецПроцедуры

#КонецОбласти
        "#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_non_api_region() {
        let code = r#"
#Область СлужебныеПроцедурыИФункции

Процедура СлужебнаяПроцедура()
КонецПроцедуры

#КонецОбласти
        "#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }
}
