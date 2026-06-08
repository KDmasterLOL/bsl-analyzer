use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::traits::MdObject;
use hir::{AssignmentResolution, ExistingBindingKind};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    variable_name: &str,
    range: TextRange,
    existing_binding_kind: Option<ExistingBindingKind>,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::CommonModuleAssign;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    if existing_binding_kind.is_some() {
        return None;
    }

    match ctx.assignment_target_kind(variable_name) {
        AssignmentResolution::CommonModule(_) => {}
        AssignmentResolution::Local
        | AssignmentResolution::Param
        | AssignmentResolution::ModuleVariable(_) => return None,
        AssignmentResolution::Unknown => {
            if !ctx.is_common_module_anywhere(variable_name) {
                return None;
            }
        }
    }

    let display_name = ctx
        .resolve_common_module(variable_name)
        .map(|common_module| common_module.name().to_string())
        .unwrap_or_else(|| variable_name.to_string());

    Some(Diagnostic {
        code,
        message: format!("Недопустимо присваивание значения общему модулю '{}'", display_name),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_hir_diagnostic, format_diags};
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_no_metadata() {
        let code = r#"Процедура Тест()
    СвойМодуль = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let common_module_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::CommonModuleAssign)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &common_module_diags));
    }

    #[test]
    fn test_property_access_no_diagnostic() {
        let code = r#"Процедура Тест()
    СвойМодуль.Свойство = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let common_module_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::CommonModuleAssign)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &common_module_diags));
    }

    #[test]
    fn test_index_access_no_diagnostic() {
        let code = r#"Процедура Тест()
    Массив[0] = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let common_module_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::CommonModuleAssign)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &common_module_diags));
    }

    #[test]
    fn test_simple_variable_emits_candidate() {
        let code = r#"Процедура Тест()
    А = 1;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let common_module_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::CommonModuleAssign)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &common_module_diags));
    }
}
