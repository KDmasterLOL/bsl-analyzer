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
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_this_object_assign_simple() {
        let code = r#"Процедура ПриСозданииНаСервере()
    ЭтотОбъект = РеквизитФормыВЗначение("Объект");
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ThisObjectAssign,
            expect![[r#"
            ThisObjectAssign @ 2:5..2:15
              message: Свойство ЭтотОбъект доступно только для чтения
              severity: Blocker"#]],
        );
    }

    #[test]
    fn test_this_object_assign_english() {
        let code = r#"Procedure OnCreate()
    ThisObject = FormAttributeToValue("Object");
EndProcedure"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ThisObjectAssign,
            expect![[r#"
            ThisObjectAssign @ 2:5..2:15
              message: Свойство ЭтотОбъект доступно только для чтения
              severity: Blocker"#]],
        );
    }

    #[test]
    fn test_this_object_assign_case_insensitive() {
        let code = r#"Процедура Тест()
    этотОБЪЕКТ = 1;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ThisObjectAssign,
            expect![[r#"
            ThisObjectAssign @ 2:5..2:15
              message: Свойство ЭтотОбъект доступно только для чтения
              severity: Blocker"#]],
        );
    }

    #[test]
    fn test_this_object_property_access_no_diagnostic() {
        let code = r#"Процедура Тест()
    ЭтотОбъект.Реквизит1 = А;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::ThisObjectAssign, expect![[r#""#]]);
    }

    #[test]
    fn test_fixture() {
        let code = r#"Процедура ПриСозданииНаСервере()
    ЭтотОбъект = РеквизитФормыВЗначение("Объект");
КонецПроцедуры

ЭтотОбъект.Реквизит1 = А;
"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ThisObjectAssign,
            expect![[r#"
            ThisObjectAssign @ 2:5..2:15
              message: Свойство ЭтотОбъект доступно только для чтения
              severity: Blocker"#]],
        );
    }
}
