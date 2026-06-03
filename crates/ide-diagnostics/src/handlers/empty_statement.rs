use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::EmptyStatement;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Пустой оператор".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![Fix {
            label: "Удалить пустой оператор".to_string(),
            edits: vec![TextEdit { range, new_text: String::new() }],
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use expect_test::expect;
    #[test]
    fn test_empty_statement_after_then() {
        let code = "А = 0;\nЕсли Истина Тогда ; // Диагностика должна сработать здесь\n  А = 0;; // и здесь\n  А = 0;\nКонецЕсли;";
        let diagnostics = check_hir_diagnostic(code);
        let empty_stmt_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::EmptyStatement).collect();
        expect![[r#"
            EmptyStatement @ 2:19..2:20
              message: Пустой оператор
              severity: Hint
            EmptyStatement @ 3:9..3:10
              message: Пустой оператор
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &empty_stmt_diags));
    }

    #[test]
    fn test_parse_error_suppresses_empty_statement() {
        let code = r#"Процедура ОшибкаРазбора()
    Для Каждого ЭлементСтруктуры Из КакаятоСтруктура Цикл
        КонецЕсли;
    КонецЦикла;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let empty_stmt_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::EmptyStatement).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &empty_stmt_diags));
    }

    #[test]
    fn test_no_empty_statements() {
        let code = r#"
Процедура Тест()
    А = 1;
    Б = 2;
    Возврат А + Б;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let empty_stmt_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::EmptyStatement).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &empty_stmt_diags));
    }

    #[test]
    fn test_double_semicolon() {
        let code = r#"
Процедура Тест()
    А = 1;;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let empty_stmt_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::EmptyStatement).collect();
        expect![[r#"
            EmptyStatement @ 3:11..3:12
              message: Пустой оператор
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &empty_stmt_diags));
    }
}
