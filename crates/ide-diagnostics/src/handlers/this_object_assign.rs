//! ThisObjectAssign diagnostic
//!
//! Detects assignment to ЭтотОбъект/ThisObject property which is read-only.
//! Applies only to CommonModule and FormModule.
//!

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule, bsl_metadata::ModuleType::FormModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_3,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::ThisObjectAssign,
        "Свойство ЭтотОбъект доступно только для чтения",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;
    #[test]
    fn test_this_object_assign_simple() {
        let code = r#"Процедура ПриСозданииНаСервере()
    ЭтотОбъект = РеквизитФормыВЗначение("Объект");
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let this_object_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ThisObjectAssign).collect();

        assert_eq!(this_object_diags.len(), 1, "Expected 1 ThisObjectAssign diagnostic");
        assert_diagnostic_range(code, this_object_diags[0], 1, 4, 14);
    }

    #[test]
    fn test_this_object_assign_english() {
        let code = r#"Procedure OnCreate()
    ThisObject = FormAttributeToValue("Object");
EndProcedure"#;

        let diagnostics = check_hir_diagnostic(code);
        let this_object_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ThisObjectAssign).collect();

        assert_eq!(this_object_diags.len(), 1, "Expected 1 ThisObjectAssign diagnostic");
        assert_diagnostic_range(code, this_object_diags[0], 1, 4, 14);
    }

    #[test]
    fn test_this_object_assign_case_insensitive() {
        let code = r#"Процедура Тест()
    этотОБЪЕКТ = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let this_object_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ThisObjectAssign).collect();

        assert_eq!(this_object_diags.len(), 1, "Case-insensitive match should detect");
    }

    #[test]
    fn test_this_object_property_access_no_diagnostic() {
        let code = r#"Процедура Тест()
    ЭтотОбъект.Реквизит1 = А;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let this_object_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ThisObjectAssign).collect();

        assert_eq!(this_object_diags.len(), 0, "Property access should not trigger diagnostic");
    }

    #[test]
    fn test_fixture() {
        let code = r#"Процедура ПриСозданииНаСервере()
    ЭтотОбъект = РеквизитФормыВЗначение("Объект");
КонецПроцедуры

ЭтотОбъект.Реквизит1 = А;
"#;

        let diagnostics = check_hir_diagnostic(code);
        let this_object_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ThisObjectAssign).collect();

        // Line 1: ЭтотОбъект = ... inside procedure - should be flagged
        // Line 4: ЭтотОбъект.Реквизит1 = ... - property access, should NOT be flagged
        assert_eq!(this_object_diags.len(), 1, "Only direct assignment should be flagged");
        assert_diagnostic_range(code, this_object_diags[0], 1, 4, 14);
    }
}
