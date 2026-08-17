use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use ide_db::TextRange;
use syntax::SyntaxKind;

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

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let mut diagnostic = crate::simple_hir_diagnostic(
        DiagnosticCode::ExtraCommas,
        "Не используйте запятые для параметры по умолчанию в конце вызова метода",
        range,
        ctx,
    )?;

    // The diagnostic flags only the last trailing comma, but a batch fix must remove the
    // whole run at once or `,,,)` would still be reported after one pass.
    if let Some(run) = trailing_comma_run(ctx, range) {
        diagnostic.fixes = vec![Fix::safe(
            "Убрать лишние запятые",
            vec![TextEdit { range: run, new_text: String::new() }],
        )];
    }

    Some(diagnostic)
}

/// Extend the flagged (last) trailing comma backwards over every consecutive comma and
/// whitespace up to the last real argument, yielding the full run to delete.
fn trailing_comma_run(ctx: &DiagnosticsContext, range: TextRange) -> Option<TextRange> {
    let parse = ctx.parse();
    let comma = parse.syntax_node().token_at_offset(range.start()).right_biased()?;
    if comma.kind() != SyntaxKind::COMMA {
        return None;
    }

    let mut run_start = comma.text_range().start();
    let mut prev = syntax::prev_token_past_empty(&comma);
    while let Some(token) = prev {
        match token.kind() {
            SyntaxKind::COMMA => run_start = token.text_range().start(),
            SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {}
            _ => break,
        }
        prev = syntax::prev_token_past_empty(&token);
    }

    Some(TextRange::new(run_start, comma.text_range().end()))
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_fix_removes_whole_trailing_run() {
        // A single fix must strip every trailing comma so one pass converges.
        let code = "Результат = Метод(А, Б,,,);";
        check_fix_snapshot_for(
            code,
            DiagnosticCode::ExtraCommas,
            expect![[r#"
            ExtraCommas @ 1:25..1:26 — Убрать лишние запятые [fix_all=true]
            Результат = Метод(А, Б);"#]],
        );
    }

    #[test]
    fn test_trailing_comma_single_arg() {
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
