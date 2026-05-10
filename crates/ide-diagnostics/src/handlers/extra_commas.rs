//! ExtraCommas diagnostic
//!
//! Detects trailing commas in function/method call argument lists.
//!
//! **Test file:** ExtraCommasDiagnostic.bsl
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - detects trailing commas during HIR lowering.
//!
//! ## Why?
//! Trailing commas in BSL function calls are syntax errors or cause unexpected behavior.
//! They reduce code readability and can lead to confusion with optional parameters.
//!
//! ## Bad practice
//! ```bsl
//! Результат = Метод(Парам1, Парам2,);     // Trailing comma
//! Результат = Метод(Парам1, Парам2,,,);   // Multiple trailing commas
//! ```
//!
//! ## Good practice
//! ```bsl
//! Результат = Метод(Парам1, Парам2);
//! Результат = Метод(Парам1, , Парам2);    // Empty arg is OK
//! ```

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when ExtraCommas diagnostic is emitted during lowering.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::ExtraCommas,
        "Не используйте запятые для параметры по умолчанию в конце вызова метода",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_trailing_comma_single_arg() {
        // Метод1(Парам1, , Парам2,) - trailing comma after last arg
        let code = "Результат = Метод1(Парам1, , Парам2,);";
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        expect![[r#"
            ExtraCommas @ 1:36..1:37
              message: Не используйте запятые для параметры по умолчанию в конце вызова метода
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &extra_diags));
    }

    #[test]
    fn test_trailing_commas_multiple() {
        // Метод2(Парам1, Парам2,,,) - multiple trailing commas
        let code = "Результат = Метод2(Парам1, Парам2,,,);";
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        expect![[r#"
            ExtraCommas @ 1:36..1:37
              message: Не используйте запятые для параметры по умолчанию в конце вызова метода
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &extra_diags));
    }

    #[test]
    fn test_qualified_call_trailing_comma_with_space() {
        // Модуль.Метод3(Парам1, Парам2, Парам3,, ) - trailing comma then space
        let code = "Результат = Модуль.Метод3(Парам1, Парам2, Парам3,, );";
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        expect![[r#"
            ExtraCommas @ 1:50..1:51
              message: Не используйте запятые для параметры по умолчанию в конце вызова метода
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &extra_diags));
    }

    #[test]
    fn test_qualified_call_many_trailing_commas() {
        // Модуль.Метод4(Парам1, , Парам2,,,,) - many trailing commas
        let code = "Результат = Модуль.Метод4(Парам1, , Парам2,,,,);";
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        expect![[r#"
            ExtraCommas @ 1:46..1:47
              message: Не используйте запятые для параметры по умолчанию в конце вызова метода
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &extra_diags));
    }

    #[test]
    fn test_trailing_comma_in_if_condition() {
        // Если Метод5(Парам1, , Парам2,,,,) Тогда
        let code = "Если Метод5(Парам1, , Парам2,,,,) Тогда\nКонецЕсли;";
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        expect![[r#"
            ExtraCommas @ 1:32..1:33
              message: Не используйте запятые для параметры по умолчанию в конце вызова метода
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &extra_diags));
    }

    #[test]
    fn test_qualified_trailing_comma_in_if_condition() {
        // Если Модуль.Метод6(Парам1, , Парам2,,,,) Тогда
        let code = "Если Модуль.Метод6(Парам1, , Парам2,,,,) Тогда\nКонецЕсли;";
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        expect![[r#"
            ExtraCommas @ 1:39..1:40
              message: Не используйте запятые для параметры по умолчанию в конце вызова метода
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &extra_diags));
    }

    #[test]
    fn test_good_calls_no_diagnostic() {
        // All valid calls: no trailing commas
        let code = r#"
Результат = Метод(Парам1, , Парам2);
Результат = Метод(Парам1, Парам2);
Результат = Модуль.Метод(Парам1, Парам2, Парам3);
Результат = Модуль.Метод(Парам1, , Парам2);
"#;
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &extra_diags));
    }

    #[test]
    fn test_no_trailing_commas() {
        let code = r#"
Результат = Метод(Парам1, Парам2);
Результат = Метод(Парам1, , Парам2);
"#;
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &extra_diags));
    }

    #[test]
    fn test_single_trailing_comma() {
        let code = r#"
Результат = Метод(А, Б,);
"#;
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        expect![[r#"
            ExtraCommas @ 2:23..2:24
              message: Не используйте запятые для параметры по умолчанию в конце вызова метода
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &extra_diags));
    }

    #[test]
    fn test_multiple_trailing_commas() {
        let code = r#"
Результат = Метод(А, Б,,,);
"#;
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        // Сообщается только о первой лишней запятой
        expect![[r#"
            ExtraCommas @ 2:25..2:26
              message: Не используйте запятые для параметры по умолчанию в конце вызова метода
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &extra_diags));
    }

    #[test]
    fn test_empty_call() {
        let code = r#"
Результат = Метод();
"#;
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &extra_diags));
    }
}
