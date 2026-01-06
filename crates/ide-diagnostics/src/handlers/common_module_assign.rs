//! CommonModuleAssign diagnostic
//!
//! Cannot assign value to CommonModule (will cause runtime error).
//!
//! Ported from: CommonModuleAssignDiagnostic.java

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::traits::MdObject;
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::CommonModuleAssign` is encountered.
///
/// This function validates the assignment target against metadata:
/// 1. Loads Configuration metadata
/// 2. Checks if variable_name matches a CommonModule name (case-insensitive)
/// 3. Returns diagnostic if it's an attempt to assign to a CommonModule
pub fn from_hir(
    variable_name: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CommonModuleAssign) {
        return None;
    }

    // Load metadata via ctx.load_configuration() for Salsa caching
    let configuration = ctx.load_configuration()?;

    // Check if variable_name matches a CommonModule name (case-insensitive)
    let common_module = configuration.find_common_module(variable_name)?;

    // Found matching CommonModule - this is an error
    Some(Diagnostic {
        code: DiagnosticCode::CommonModuleAssign,
        message: format!(
            "Недопустимо присваивание значения общему модулю '{}'",
            common_module.name()
        ),
        severity: Severity::Error,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic;
    use crate::DiagnosticCode;

    #[test]
    fn test_no_metadata() {
        // Without metadata, no CommonModuleAssign diagnostics should be emitted
        let code = r#"Процедура Тест()
    СвойМодуль = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let common_module_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CommonModuleAssign).collect();

        // No metadata available, so no diagnostics
        assert_eq!(common_module_diags.len(), 0);
    }

    #[test]
    fn test_property_access_no_diagnostic() {
        // Property access (field expression) should NOT trigger diagnostic
        let code = r#"Процедура Тест()
    СвойМодуль.Свойство = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let common_module_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CommonModuleAssign).collect();

        // Field access is not a simple identifier assignment
        assert_eq!(common_module_diags.len(), 0);
    }

    #[test]
    fn test_index_access_no_diagnostic() {
        // Index access should NOT trigger diagnostic
        let code = r#"Процедура Тест()
    Массив[0] = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let common_module_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CommonModuleAssign).collect();

        // Index access is not a simple identifier assignment
        assert_eq!(common_module_diags.len(), 0);
    }

    #[test]
    fn test_simple_variable_emits_candidate() {
        // Simple variable assignment should emit a candidate (filtered by metadata later)
        let code = r#"Процедура Тест()
    А = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        // Without metadata, candidates are filtered out
        let common_module_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CommonModuleAssign).collect();
        assert_eq!(common_module_diags.len(), 0);
    }
}
