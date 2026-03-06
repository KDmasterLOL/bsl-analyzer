//! FunctionNameStartsWithGet diagnostic
//!
//! Detects functions with names starting with "Получить" (Russian for "Get").
//!
//!
//! ## Why?
//! Function names starting with "Получить" are considered a code smell in 1C:Enterprise.
//! According to 1C coding standards, such names should be avoided and replaced with more
//! descriptive alternatives that don't use the "Получить" prefix.
//!
//! **Note:** This diagnostic only checks Russian "Получить" prefix, not English "Get".
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - detects function names during HIR lowering.
//!
//! ## Bad practice
//! ```bsl
//! Функция ПолучитьИмяПоКоду()  // Bad!
//!     Возврат "Имя";
//! КонецФункции
//! ```
//!
//! ## Good practice
//! ```bsl
//! Функция ИмяПоКоду()  // Good!
//!     Возврат "Имя";
//! КонецФункции
//! ```

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when FunctionNameStartsWithGet diagnostic is emitted during lowering.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::FunctionNameStartsWithGet;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!("Имя функции '{}' не должно начинаться с 'Получить'", name),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;

    #[test]
    fn test_function_name_starts_with_get() {
        let code = r#"// Source comment
Функция ПолучитьИмяПоКоду()

КонецФункции

Функция НеПолучитьИмяПоКоду()

КонецФункции

Функция ИмяПоКоду()

КонецФункции

Процедура ПолучитьИмяПоКоду()

КонецПроцедуры

Function GetNameByCode()

EndFunction

Function NotGetNameByCode()

EndFunction

Function NameByCode()

EndFunction

Procedure GetNameByCode()

EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionNameStartsWithGet)
            .collect();

        // Only the first function "ПолучитьИмяПоКоду" should trigger
        assert_eq!(func_diags.len(), 1, "Expected 1 diagnostic");

        // Line 1 (0-indexed), cols 8-25
        assert_diagnostic_range(code, func_diags[0], 1, 8, 25);
        assert!(func_diags[0].message.contains("ПолучитьИмяПоКоду"));
    }

    #[test]
    fn test_no_get_prefix() {
        let code = r#"
Функция ИмяПоКоду()
    Возврат "Имя";
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionNameStartsWithGet)
            .collect();
        assert_eq!(func_diags.len(), 0, "Should not detect functions without 'Получить' prefix");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Функция ПОЛУЧИТЬДАННЫЕ()
    Возврат "Данные";
КонецФункции

Функция получитьзначение()
    Возврат 42;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionNameStartsWithGet)
            .collect();
        assert_eq!(func_diags.len(), 2, "Should detect case-insensitive 'Получить' variations");
    }

    #[test]
    fn test_procedure_not_detected() {
        let code = r#"
Процедура ПолучитьИмяПоКоду()
    // Процедура не должна срабатывать
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionNameStartsWithGet)
            .collect();
        assert_eq!(func_diags.len(), 0, "Should NOT detect procedures");
    }

    #[test]
    fn test_english_get_not_detected() {
        let code = r#"
Function GetNameByCode()
    Return "Name";
EndFunction
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionNameStartsWithGet)
            .collect();
        assert_eq!(
            func_diags.len(),
            0,
            "Should NOT detect English 'Get' prefix (only Russian 'Получить')"
        );
    }

    #[test]
    fn test_partial_match_not_detected() {
        let code = r#"
Функция НеПолучитьИмяПоКоду()
    Возврат "Имя";
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionNameStartsWithGet)
            .collect();
        assert_eq!(func_diags.len(), 0, "Should NOT detect names that don't START with 'Получить'");
    }
}
