use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use line_index::LineIndex;

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

const DEFAULT_ALLOWED_EMPTY_LINES: usize = 1;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("ConsecutiveEmptyLines::check").entered();

    let code = DiagnosticCode::ConsecutiveEmptyLines;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let allowed_empty_lines: usize = ctx
        .config
        .get_int(DiagnosticCode::ConsecutiveEmptyLines, "allowedEmptyLinesCount")
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(DEFAULT_ALLOWED_EMPTY_LINES);

    let file_text = ctx.file_text();

    if file_text.is_empty() {
        return Vec::new();
    }

    let line_index = ctx.line_index();

    scan_consecutive_empty_lines(&file_text, &line_index, allowed_empty_lines, code, ctx)
}

fn scan_consecutive_empty_lines(
    text: &str,
    line_index: &LineIndex,
    allowed: usize,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let num_lines = line_index.len_lines();

    let mut consecutive_empty = 0usize;
    let mut empty_start_line: Option<u32> = None;

    for line in 0..num_lines {
        let line_start = line_index.line_start(line);
        let line_range = line_index.line_range(line);

        let is_empty = if let Some(range) = line_range {
            let line_text = &text[usize::from(range.start())..usize::from(range.end())];
            line_text.trim().is_empty()
        } else {
            let start: usize = line_start.into();
            if start < text.len() {
                text[start..].trim().is_empty()
            } else {
                true
            }
        };

        if is_empty {
            if consecutive_empty == 0 {
                empty_start_line = Some(line);
            }
            consecutive_empty += 1;
        } else {
            if consecutive_empty > allowed {
                if let Some(start_line) = empty_start_line {
                    diagnostics.push(create_diagnostic(
                        line_index,
                        start_line,
                        line - 1,
                        consecutive_empty,
                        allowed,
                        code,
                        ctx,
                    ));
                }
            }
            consecutive_empty = 0;
            empty_start_line = None;
        }
    }

    if consecutive_empty > allowed {
        if let Some(start_line) = empty_start_line {
            diagnostics.push(create_diagnostic(
                line_index,
                start_line,
                num_lines - 1,
                consecutive_empty,
                allowed,
                code,
                ctx,
            ));
        }
    }

    tracing::debug!(count = diagnostics.len(), "ConsecutiveEmptyLines diagnostics found");

    diagnostics
}

fn create_diagnostic(
    line_index: &LineIndex,
    start_line: u32,
    end_line: u32,
    count: usize,
    allowed: usize,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    let start_byte = line_index.line_start(start_line);
    let end_byte = line_index.line_start(end_line);

    let message = format!("Найдено {} подряд идущих пустых строк (максимум: {})", count, allowed);

    Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range: TextRange::new(start_byte, end_byte),
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic, format_diags};
    use expect_test::expect;
    #[test]
    fn test_empty_file() {
        let code = "";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_single_empty_line() {
        let code = "Процедура А()\n\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_two_empty_lines() {
        let code = "Процедура А()\n\n\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            ConsecutiveEmptyLines @ 2:1..3:1
              message: Найдено 2 подряд идущих пустых строк (максимум: 1)
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_spaces_only_line() {
        let code = "Процедура А()\n  \n  \nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            ConsecutiveEmptyLines @ 2:1..3:1
              message: Найдено 2 подряд идущих пустых строк (максимум: 1)
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_single_newline_file() {
        let text = "\n";
        let diagnostics = check_ast_diagnostic(text, check);
        expect![[r#""#]].assert_eq(&format_diags(text, &diagnostics));
    }

    #[test]
    fn test_three_consecutive_empty_lines() {
        let code = "Процедура А()\n\n\n\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            ConsecutiveEmptyLines @ 2:1..4:1
              message: Найдено 3 подряд идущих пустых строк (максимум: 1)
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_multiple_groups_of_consecutive_empty_lines() {
        let code = "А = 1;\n\n\nБ = 2;\n\n\nВ = 3;";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            ConsecutiveEmptyLines @ 2:1..3:1
              message: Найдено 2 подряд идущих пустых строк (максимум: 1)
              severity: Hint
            ConsecutiveEmptyLines @ 5:1..6:1
              message: Найдено 2 подряд идущих пустых строк (максимум: 1)
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_for_exactly_one_empty_line_between_methods() {
        let code = "Процедура А()\nКонецПроцедуры\n\nПроцедура Б()\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }
}
