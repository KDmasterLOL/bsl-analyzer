//! TryNumber diagnostic.
//!
//! Detects use of Number()/Число() method inside Try blocks.
//!
//! ## Why?
//! Using exceptions for type casting is incorrect. Use TypeDescription capabilities instead.
//!
//! ## Bad practice
//! ```bsl
//! Попытка
//!     КоличествоДнейРазрешения = Число(Значение);
//! Исключение
//!     КоличествоДнейРазрешения = 0;
//! КонецПопытки;
//! ```
//!
//! ## Good practice
//! ```bsl
//! ОписаниеТипа = Новый ОписаниеТипов("Число");
//! КоличествоДнейРазрешения = ОписаниеТипа.ПривестиЗначение(Значение);
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** MAJOR
//! - **Type:** CODE_SMELL
//! - **Tags:** STANDARD
//! - **Minutes to fix:** 2
//!
//! ## Implementation
//! Uses HIR-based detection during lowering. Число()/Number() calls inside try blocks
//! are detected and emitted as BodyDiagnostic::TryNumber.
//!
//! Ported from:
//! - TryNumberDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use syntax::{SyntaxKind, SyntaxNode};
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::TryNumber` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::TryNumber,
        "Don't use try-catch to number cast",
        range,
        ctx,
    )
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::TryNumber;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() == SyntaxKind::TRY_STMT {
            check_try_block(&node, code, ctx, &mut diagnostics);
        }
    }

    diagnostics
}

fn check_try_block(
    try_node: &SyntaxNode,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let try_stmt_lists = collect_try_stmt_lists(try_node);

    for stmt_list in try_stmt_lists {
        check_stmt_list_for_number_calls(&stmt_list, code, ctx, diagnostics);
    }
}

fn check_stmt_list_for_number_calls(
    node: &SyntaxNode,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(range) = is_number_call(node) {
        diagnostics.push(Diagnostic {
            code,
            message: "Don't use try-catch to number cast".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    for child in node.children() {
        if child.kind() == SyntaxKind::TRY_STMT {
            continue;
        }
        check_stmt_list_for_number_calls(&child, code, ctx, diagnostics);
    }
}

fn collect_try_stmt_lists(try_node: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut result = Vec::new();
    let mut in_except = false;

    for child in try_node.children() {
        if child.kind() == SyntaxKind::EXCEPT_CLAUSE {
            in_except = true;
        } else if child.kind() == SyntaxKind::STMT_LIST && !in_except {
            result.push(child);
        }
    }

    result
}

fn is_number_call(node: &SyntaxNode) -> Option<TextRange> {
    if node.kind() != SyntaxKind::CALL_EXPR {
        return None;
    }

    let mut children = node.children();
    let first_child = children.next()?;

    if first_child.kind() == SyntaxKind::IDENT {
        let name = first_child.text().to_string();
        let name_lower = name.to_lowercase();

        if name_lower == "число" || name_lower == "number" {
            return Some(node.text_range());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic, check_hir_diagnostic};
    use crate::DiagnosticCode;
    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/TryNumberDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 3, "Expected 3 diagnostics");

        assert_diagnostic_range(code, &diagnostics[0], 8, 4, 12);
        assert_diagnostic_range(code, &diagnostics[1], 9, 4, 13);
        assert_diagnostic_range(code, &diagnostics[2], 12, 8, 17);
    }

    #[test]
    fn test_hir_detection() {
        // Test that HIR-based detection works
        let code = r#"
Процедура Тест()
    Попытка
        А = Число(Б);
    Исключение
    КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let try_number: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TryNumber).collect();

        assert_eq!(try_number.len(), 1, "HIR should detect TryNumber");
    }

    #[test]
    fn test_number_in_except_not_detected() {
        let code = r#"
Попытка
Исключение
    А = Число(Б);
КонецПопытки
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Number in except block should not be detected");
    }

    #[test]
    fn test_number_outside_try_not_detected() {
        let code = r#"
F = Number();
А = Число(Б);
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Number outside try block should not be detected");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Попытка
    А = ЧИСЛО(Б);
    Б = Number(4);
КонецПопытки
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 2, "Should be case-insensitive");
    }

    #[test]
    fn test_nested_try() {
        let code = r#"
Попытка
    Попытка
        В = Number(4);
    КонецПопытки
КонецПопытки
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect in nested try blocks");
    }
}
