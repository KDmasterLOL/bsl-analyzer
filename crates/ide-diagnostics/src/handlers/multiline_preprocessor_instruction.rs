use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use syntax::{SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

const MESSAGE: &str = "Инструкция препроцессора разорвана переводом строки";

/// Проверка живёт здесь, а не в грамматике, потому что разрыв не мешает
/// разобрать конструкцию: платформа её отвергает, но синтаксически она
/// однозначна. Грамматика о переводе строки внутри инструкции не ветвится
/// вовсе, и норма на это счёт объявлена в
/// `docs/architecture/adr/ADR-02-line-sensitivity.md`.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("MultilinePreprocessorInstruction::check").entered();

    let code = DiagnosticCode::MultilinePreprocessorInstruction;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let root = ctx.parse().syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if !matches!(node.kind(), SyntaxKind::PRE_IF_DIR | SyntaxKind::PRE_ELSIF_CLAUSE) {
            continue;
        }

        let Some(header) = instruction_header(&node) else {
            continue;
        };

        if !crosses_a_line(&node, header) {
            continue;
        }

        diagnostics.push(Diagnostic {
            code,
            message: MESSAGE.to_string(),
            severity: ctx.severity(code),
            range: header,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    tracing::debug!(
        count = diagnostics.len(),
        "MultilinePreprocessorInstruction diagnostics found"
    );

    diagnostics
}

/// Диапазон самой инструкции — от её слова до закрывающего `Тогда`.
///
/// Тело `#Если` лежит в том же узле, и без этой границы любой перевод строки
/// внутри тела читался бы как разрыв инструкции.
///
/// Инструкция без `Тогда` пропускается: она уже сломана, и об этом сообщает
/// разбор. Второе сообщение о том же месте пользы не несёт.
fn instruction_header(node: &SyntaxNode) -> Option<TextRange> {
    let start = node.first_token()?.text_range().start();
    let then = node
        .children_with_tokens()
        .filter_map(|child| child.into_token())
        .find(|token| token.kind() == SyntaxKind::KW_THEN)?;

    Some(TextRange::new(start, then.text_range().end()))
}

fn crosses_a_line(node: &SyntaxNode, header: TextRange) -> bool {
    node.descendants_with_tokens().filter_map(|element| element.into_token()).any(|token| {
        token.kind() == SyntaxKind::NEWLINE && header.contains_range(token.text_range())
    })
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic, format_diags};
    use expect_test::expect;

    /// Контроль: то же самое в одну строку молчит. Без него утверждения ниже
    /// зелены и у проверки, которая ругается на любую инструкцию.
    #[test]
    fn an_instruction_on_one_line_is_silent() {
        let code = "#Если Сервер И Клиент Тогда\n#ИначеЕсли Клиент Тогда\n#КонецЕсли\n";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn a_condition_carried_to_the_next_line_is_reported() {
        let code = "#Если\nСервер Тогда\n#КонецЕсли\n";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MultilinePreprocessorInstruction @ 1:1..2:13
              message: Инструкция препроцессора разорвана переводом строки
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn an_operand_carried_to_the_next_line_is_reported() {
        let code = "#Если Сервер\nИ Клиент Тогда\n#КонецЕсли\n";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MultilinePreprocessorInstruction @ 1:1..2:15
              message: Инструкция препроцессора разорвана переводом строки
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn a_then_carried_to_the_next_line_is_reported() {
        let code = "#Если Сервер\nТогда\n#КонецЕсли\n";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MultilinePreprocessorInstruction @ 1:1..2:6
              message: Инструкция препроцессора разорвана переводом строки
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn an_elsif_carried_to_the_next_line_is_reported() {
        let code = "#Если Сервер Тогда\n#ИначеЕсли\nКлиент Тогда\n#КонецЕсли\n";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MultilinePreprocessorInstruction @ 2:1..3:13
              message: Инструкция препроцессора разорвана переводом строки
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    /// Тело инструкции переводами строки полно по построению, и граница
    /// заголовка — единственное, что отделяет их от разрыва самой инструкции.
    #[test]
    fn line_breaks_in_the_body_are_not_a_break_of_the_instruction() {
        let code = "#Если Сервер Тогда\n\tА = 1;\n\tБ = 2;\n#КонецЕсли\n";
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }
}
