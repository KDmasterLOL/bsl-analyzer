use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{TextRange, TextSize};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::ConsecutiveEmptyLines) {
        return Vec::new();
    }

    let allowed_empty_lines: usize = ctx
        .config
        .get_int(DiagnosticCode::ConsecutiveEmptyLines, "allowedEmptyLinesCount")
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(1);

    let file_text_input = ctx.db.file_text_input(ctx.file_id);
    let file_text = file_text_input.text(ctx.db);

    scan_consecutive_empty_lines(&file_text, allowed_empty_lines)
}

fn scan_consecutive_empty_lines(text: &str, allowed: usize) -> Vec<Diagnostic> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    let lines: Vec<&str> = text.split('\n').collect();

    let mut line_offsets: Vec<TextSize> = Vec::new();
    let mut byte_offset = TextSize::from(0);

    for (idx, line) in lines.iter().enumerate() {
        line_offsets.push(byte_offset);
        byte_offset += TextSize::from(line.len() as u32);
        if idx < lines.len() - 1 {
            byte_offset += TextSize::from(1);
        }
    }

    line_offsets.push(byte_offset);

    let mut consecutive_empty = 0;
    let mut empty_start_idx: Option<usize> = None;

    for (idx, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            if consecutive_empty == 0 {
                empty_start_idx = Some(idx);
            }
            consecutive_empty += 1;
        } else {
            if consecutive_empty > allowed {
                if let Some(start_idx) = empty_start_idx {
                    diagnostics.push(create_diagnostic(
                        &line_offsets,
                        start_idx,
                        idx - 1,
                        consecutive_empty,
                        allowed,
                    ));
                }
            }
            consecutive_empty = 0;
            empty_start_idx = None;
        }
    }

    if consecutive_empty > allowed {
        if let Some(start_idx) = empty_start_idx {
            let end_idx = lines.len() - 1;
            diagnostics.push(create_diagnostic(
                &line_offsets,
                start_idx,
                end_idx,
                consecutive_empty,
                allowed,
            ));
        }
    }

    diagnostics
}

fn create_diagnostic(
    line_offsets: &[TextSize],
    start_line_idx: usize,
    end_line_idx: usize,
    count: usize,
    allowed: usize,
) -> Diagnostic {
    let start_byte = line_offsets[start_line_idx];
    let end_byte = line_offsets[end_line_idx];

    let message = format!("Найдено {} подряд идущих пустых строк (максимум: {})", count, allowed);

    Diagnostic {
        code: DiagnosticCode::ConsecutiveEmptyLines,
        message,
        severity: Severity::Information,
        range: TextRange::new(start_byte, end_byte),
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range_multiline;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = crate::DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_empty_file() {
        let code = "";
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_single_empty_line() {
        let code = "Процедура А()\n\nКонецПроцедуры";
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_two_empty_lines() {
        let code = "Процедура А()\n\n\nКонецПроцедуры";
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range_multiline(code, &diagnostics[0], 1, 0, 2, 0);
    }

    #[test]
    fn test_spaces_only_line() {
        let code = "Процедура А()\n  \n  \nКонецПроцедуры";
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_single_newline_file() {
        let text = "\n";
        let diagnostics = scan_consecutive_empty_lines(text, 1);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for single newline");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/ConsecutiveEmptyLinesDiagnostic.bsl");
        let diagnostics = scan_consecutive_empty_lines(code, 1);

        assert_eq!(diagnostics.len(), 9, "Expected 9 diagnostics, got {}", diagnostics.len());

        assert_diagnostic_range_multiline(code, &diagnostics[0], 0, 0, 1, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[1], 5, 0, 6, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[2], 10, 0, 11, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[3], 14, 0, 15, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[4], 17, 0, 18, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[5], 22, 0, 23, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[6], 26, 0, 27, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[7], 29, 0, 31, 0);
        assert_diagnostic_range_multiline(code, &diagnostics[8], 33, 0, 34, 0);
    }
}
