use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let mut diagnostic = crate::simple_hir_diagnostic(
        DiagnosticCode::SelfAssign,
        "Присваивание переменной самой себе",
        range,
        ctx,
    )?;

    // A self-assignment is a no-op, so the fix removes its whole line — but only when the
    // line holds nothing else (no trailing comment or statement to preserve). Deletion is
    // opt-in, never an unattended `source.fixAll` edit.
    let text = ctx.file_text();
    if let Some(line) = self_assign_line_to_delete(&text, range) {
        diagnostic.fixes = vec![Fix::manual(
            "Удалить самоприсваивание",
            vec![TextEdit { range: line, new_text: String::new() }],
        )];
    }

    Some(diagnostic)
}

/// The full line to delete for a self-assignment: from its indentation through the
/// terminating newline. Returns `None` if anything other than the statement and its
/// optional `;` shares the line, so no comment or sibling statement is lost.
fn self_assign_line_to_delete(text: &str, range: TextRange) -> Option<TextRange> {
    let stmt_start: usize = range.start().into();
    let stmt_end: usize = range.end().into();
    let line_start = text[..stmt_start].rfind('\n').map_or(0, |nl| nl + 1);

    // A statement may precede the self-assign on the same line; deleting the whole line
    // would drop it, so only proceed when the prefix is indentation.
    if !text[line_start..stmt_start].trim().is_empty() {
        return None;
    }

    // Skip whitespace, then an optional `;`, to find where the statement really ends.
    let rest = &text[stmt_end..];
    let mut cursor = stmt_end + rest.len() - rest.trim_start().len();
    if text[cursor..].starts_with(';') {
        cursor += 1;
    }

    // The remainder of the line must be blank; otherwise deleting it would drop content.
    let line_end = text[cursor..].find('\n').map_or(text.len(), |nl| cursor + nl);
    if !text[cursor..line_end].trim().is_empty() {
        return None;
    }
    let delete_end = if line_end < text.len() { line_end + 1 } else { line_end };

    Some(TextRange::new((line_start as u32).into(), (delete_end as u32).into()))
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_diagnostics_snapshot_for, check_fix_snapshot_for};
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_fix_deletes_line_only_when_alone() {
        // The plain self-assign line is deleted whole; the ones sharing a line with a
        // trailing comment or a preceding statement offer no fix (nothing must be lost).
        let code =
            "Процедура Тест()\n    А = А;\n    Б = Б; // важно\n    В = 1; Г = Г;\nКонецПроцедуры";
        check_fix_snapshot_for(
            code,
            DiagnosticCode::SelfAssign,
            expect![[r#"
            SelfAssign @ 2:5..2:10 — Удалить самоприсваивание [fix_all=false]
            Процедура Тест()
                Б = Б; // важно
                В = 1; Г = Г;
            КонецПроцедуры"#]],
        );
    }

    #[test]
    fn test_self_assign() {
        let code = r#"Процедура Тест()
    А = А;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SelfAssign,
            expect![[r#"
            SelfAssign @ 2:5..2:10
              message: Присваивание переменной самой себе
              severity: Major"#]],
        );
    }

    #[test]
    fn test_self_assign_case_insensitive() {
        let code = r#"Процедура Тест()
    А = а;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SelfAssign,
            expect![[r#"
            SelfAssign @ 2:5..2:10
              message: Присваивание переменной самой себе
              severity: Major"#]],
        );
    }

    #[test]
    fn test_no_self_assign() {
        let code = r#"Процедура Тест()
    А = Б;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::SelfAssign, expect![[r#""#]]);
    }

    #[test]
    fn test_fixture_self_assign() {
        let code = r#"Процедура Тест()
    Если А = 1 Тогда
    КонецЕсли;

    A = 1;
    А = а; //Раз

    Структура.Чтото = Структура.ЧтотоДругое;
    Структура.Чтото = СтруКтура.ЧТото; // Два

    НовыйУникальныйИдентификатор = Новый УникальныйИдентификатор;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SelfAssign,
            expect![[r#"
            SelfAssign @ 6:5..6:10
              message: Присваивание переменной самой себе
              severity: Major
            SelfAssign @ 9:5..9:38
              message: Присваивание переменной самой себе
              severity: Major"#]],
        );
    }
}
