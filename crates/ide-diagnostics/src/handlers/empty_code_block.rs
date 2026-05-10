//! EmptyCodeBlock diagnostic
//!
//! Detects empty code blocks in control structures (if/while/for/etc).
//!
//!
//! BSL supports empty code blocks in control structures, but they often indicate
//! incomplete implementation or unintended code.  This diagnostic helps detect such cases.
//!
//! ## Empty blocks detected:
//! - Empty if/then blocks
//! - Empty elsif blocks
//! - Empty else blocks
//! - Empty while/for/foreach loops
//!
//! ## NOT checked (other diagnostics handle these):
//! - Empty function/procedure bodies (handled by other diagnostic)
//! - Empty try/except blocks (handled by other diagnostic)
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! The diagnostic is emitted in `hir-def/body/lower/stmt.rs` during statement lowering
//! for if/elsif/else/while/for/foreach/try/except blocks.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::EmptyCodeBlock` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(DiagnosticCode::EmptyCodeBlock, "Пустой блок кода", range, ctx)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_empty_else_block() {
        let code = r#"Процедура А()
    Якорь = 0;
    Если Истина Тогда
        А = 0;
    Иначе
        // только комментарий - пустой блок
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::EmptyCodeBlock).collect();
        expect![[r#"
            EmptyCodeBlock @ 5:5..5:10
              message: Пустой блок кода
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_empty_while_loop() {
        let code = r#"Процедура А()
    Пока Истина Цикл
        // только комментарий - пустой цикл
    КонецЦикла;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::EmptyCodeBlock).collect();
        expect![[r#"
            EmptyCodeBlock @ 2:5..2:21
              message: Пустой блок кода
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_empty_if_block() {
        let code = r#"Процедура А()
    Если Истина Тогда

    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::EmptyCodeBlock).collect();
        expect![[r#"
            EmptyCodeBlock @ 2:5..2:22
              message: Пустой блок кода
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_empty_elseif_and_else() {
        let code = r#"а = 5;
Если а = 0 Тогда
ИначеЕсли А = 1 Тогда
    Иначе
КонецЕсли;"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::EmptyCodeBlock).collect();
        // Empty if, empty elseif, empty else = 3 diagnostics
        expect![[r#"
            EmptyCodeBlock @ 2:1..2:17
              message: Пустой блок кода
              severity: Warning
            EmptyCodeBlock @ 3:1..3:22
              message: Пустой блок кода
              severity: Warning
            EmptyCodeBlock @ 4:5..4:10
              message: Пустой блок кода
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_no_diagnostic_for_empty_try_except() {
        // Empty except blocks are NOT reported by EmptyCodeBlock (separate diagnostic)
        let code = r#"Процедура А()
    Попытка
        А = 0;
    Исключение
        // комментарий
    КонецПопытки;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::EmptyCodeBlock).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_no_diagnostic_for_empty_procedure_body() {
        // Empty procedure bodies are NOT reported by EmptyCodeBlock (separate diagnostic)
        let code = r#"Функция В()
    // только комментарий
КонецФункции"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::EmptyCodeBlock).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
    }
}
