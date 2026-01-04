//! Diagnostic: SelfAssign
//!
//! Detects self-assignment patterns like `a = a` or `Obj.X = Obj.X`.
//!
//! ## Severity
//! Major
//!
//! ## Example
//! ```bsl
//! // Bad - self-assignment
//! А = А;
//! СтруктураДанных.Поле = СтруктураДанных.Поле;
//!
//! // Good
//! А = Б;
//! СтруктураДанных.Поле = НовоеЗначение;
//! ```
//!
//! ## Source
//! bsl-language-server/SelfAssignDiagnostic.java

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::SelfAssign` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::SelfAssign) {
        return None;
    }
    Some(Diagnostic {
        code: DiagnosticCode::SelfAssign,
        message: "Присваивание переменной самой себе".to_string(),
        severity: Severity::Major,
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
    fn test_self_assign() {
        let code = r#"Процедура Тест()
    А = А;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let self_assign_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SelfAssign).collect();
        assert_eq!(self_assign_diags.len(), 1, "Expected 1 SelfAssign diagnostic");
    }

    #[test]
    fn test_no_self_assign() {
        let code = r#"Процедура Тест()
    А = Б;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::SelfAssign),
            "Normal assignment should not trigger SelfAssign"
        );
    }
}
