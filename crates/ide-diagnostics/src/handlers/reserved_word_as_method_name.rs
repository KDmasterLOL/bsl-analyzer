//! ReservedWordAsMethodName diagnostic.
//!
//! Detects procedures/functions named with reserved BSL keywords.
//! The 1C platform rejects such names with a compilation error.
//!
//! ## Example
//! ```bsl
//! Процедура Выполнить(Команда)  // Error: "Выполнить" is a reserved word
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Blocker (ERROR)

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::ReservedWordAsMethodName;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Имя \"{}\" является зарезервированным словом и не может использоваться как имя процедуры/функции",
            name
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;

    #[test]
    fn test_procedure_with_reserved_word_execute() {
        let code = r#"Процедура Выполнить(Команда)
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ReservedWordAsMethodName)
            .collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 0, 10, 19);
    }

    #[test]
    fn test_function_with_reserved_word_new() {
        let code = r#"Функция Новый()
    Возврат 1;
КонецФункции"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ReservedWordAsMethodName)
            .collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 0, 8, 13);
    }

    #[test]
    fn test_procedure_with_reserved_word_if() {
        let code = r#"Процедура Если()
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ReservedWordAsMethodName)
            .collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_procedure_with_reserved_word_english() {
        let code = r#"Procedure Execute(Command)
EndProcedure"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ReservedWordAsMethodName)
            .collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_normal_procedure_name_ok() {
        let code = r#"Процедура МояПроцедура()
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        assert!(diagnostics.iter().all(|d| d.code != DiagnosticCode::ReservedWordAsMethodName));
    }

    #[test]
    fn test_normal_function_name_ok() {
        let code = r#"Функция ПолучитьЗначение()
    Возврат 1;
КонецФункции"#;
        let diagnostics = check_hir_diagnostic(code);
        assert!(diagnostics.iter().all(|d| d.code != DiagnosticCode::ReservedWordAsMethodName));
    }
}
