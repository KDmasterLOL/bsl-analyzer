//! MissingVariablesDescription diagnostic.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir_def::item_tree::ModItem;
use syntax::{has_variable_description, SyntaxKind, TextRange};
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MissingVariablesDescription;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let tree = ctx.item_tree();
    let parse = ctx.parse();
    let root = parse.syntax_node();
    let source_text = ctx.file_text();

    for item in tree.top_level_items() {
        let ModItem::Variable(var_idx) = item else {
            continue;
        };

        let var = tree.variable(*var_idx);

        let var_node = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::VAR_DEF && n.text_range() == var.source_range);

        let Some(var_node) = var_node else {
            continue;
        };

        let var_keyword_offset: usize = find_var_keyword_offset(&var_node).unwrap_or(0);
        let first_annotation_offset = find_first_annotation_offset(&var_node);

        if !has_variable_description(
            &var_node,
            var_keyword_offset,
            &source_text,
            first_annotation_offset,
        ) {
            let diag_range =
                compute_variable_diagnostic_range(&var_node, var.name_range, var.is_export);
            diagnostics.push(create_diagnostic(diag_range, code, ctx));
        }
    }

    diagnostics
}

fn find_var_keyword_offset(node: &syntax::SyntaxNode) -> Option<usize> {
    for child in node.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::KW_VAR {
                return Some(token.text_range().start().into());
            }
        }
    }
    None
}

fn find_first_annotation_offset(node: &syntax::SyntaxNode) -> Option<usize> {
    for child in node.children() {
        if child.kind() == SyntaxKind::ANNOTATION {
            return Some(child.text_range().start().into());
        }
    }
    None
}

fn compute_variable_diagnostic_range(
    node: &syntax::SyntaxNode,
    name_range: TextRange,
    is_export: bool,
) -> TextRange {
    if !is_export {
        return name_range;
    }

    for child in node.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::KW_EXPORT {
                return TextRange::new(name_range.start(), token.text_range().end());
            }
        }
    }

    name_range
}

fn create_diagnostic(
    range: TextRange,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: "Все объявления переменных должны иметь описание".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    use crate::DiagnosticCode;
    const FIXTURE: &str = include_str!("test_data/missing_variables_description/fixture.bsl");

    #[test]
    fn test_java_fixture_compatibility() {
        let diagnostics = check_ast_diagnostic(FIXTURE, check);

        let mvd: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::MissingVariablesDescription)
            .collect();

        assert_eq!(mvd.len(), 5, "Expected 5 diagnostics from Java fixture");

        assert_diagnostic_range(FIXTURE, mvd[0], 1, 6, 27);
        assert_diagnostic_range(FIXTURE, mvd[1], 3, 6, 45);
        assert_diagnostic_range(FIXTURE, mvd[2], 17, 6, 38);
        assert_diagnostic_range(FIXTURE, mvd[3], 21, 6, 56);
        assert_diagnostic_range(FIXTURE, mvd[4], 37, 6, 49);
    }

    #[test]
    fn test_trailing_comment() {
        let code = "Перем Переменная; // описание";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_description() {
        let code = "Перем Переменная;";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingVariablesDescription);
    }

    #[test]
    fn test_leading_comment_direct() {
        let code = "// описание\nПерем Переменная;";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_leading_comment_with_empty_line() {
        let code = "// описание\n\nПерем Переменная;";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_annotated_variable_with_trailing_comment() {
        let code = "&НаКлиенте\nПерем Переменная; // описание";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_annotated_variable_with_header_comment() {
        let code = "// описание\n&НаКлиенте\nПерем Переменная;";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_annotated_variable_with_comment_between() {
        let code = "&НаКлиенте\n// описание\n&НаСервере\nПерем Переменная;";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_annotated_variable_with_comment_below() {
        let code = "&НаКлиенте\n// описание\nПерем Переменная;";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_annotated_variable_no_description() {
        let code = "&НаКлиенте\n&НаСервере\nПерем Переменная;";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_local_variable_not_checked() {
        let code = "Процедура Тест()\n    Перем Локальная;\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_multiple_variables() {
        let code = "Перем А;\nПерем Б; // описание\n// описание\nПерем В;";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingVariablesDescription);
    }
}
