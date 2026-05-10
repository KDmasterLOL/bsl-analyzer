//! NestedTernaryOperator diagnostic.
//!
//! Reports nested or condition-embedded ternary operators that reduce
//! readability.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::{SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Check a single syntax node for nested ternary operators.
pub fn check_node(node: &SyntaxNode, acc: &mut Vec<Diagnostic>, ctx: &DiagnosticsContext) {
    let code = DiagnosticCode::NestedTernaryOperator;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    match node.kind() {
        // Case 1: Ternary in IF condition
        SyntaxKind::IF_STMT => {
            if let Some(condition) = find_if_condition(node) {
                find_and_report_ternaries(&condition, code, ctx, acc);
            }
        }
        // Case 2: Ternary in ELSIF condition
        SyntaxKind::ELSIF_CLAUSE => {
            if let Some(condition) = find_elsif_condition(node) {
                find_and_report_ternaries(&condition, code, ctx, acc);
            }
        }
        // Case 3: Nested ternary within another ternary
        SyntaxKind::TERNARY_EXPR => {
            for nested in node.descendants().skip(1) {
                if nested.kind() == SyntaxKind::TERNARY_EXPR {
                    acc.push(make_diagnostic(&nested, code, ctx));
                }
            }
        }
        _ => {}
    }
}

/// Main entry point for the `NestedTernaryOperator` diagnostic.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("NestedTernaryOperator::check").entered();

    let code = DiagnosticCode::NestedTernaryOperator;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        check_node(&node, &mut diagnostics, ctx);
    }

    tracing::debug!(count = diagnostics.len(), "NestedTernaryOperator diagnostics found");

    diagnostics
}

/// Find the condition expression of an IF statement.
///
/// Structure: `IF_STMT` → `EXPR` (condition) → `THEN` → ...
fn find_if_condition(if_stmt: &SyntaxNode) -> Option<SyntaxNode> {
    if_stmt.children().find(|n| n.kind() == SyntaxKind::EXPR)
}

/// Find the condition expression of an ELSIF clause.
///
/// Structure: `ELSIF_CLAUSE` → `EXPR` (condition) → ...
fn find_elsif_condition(elsif_clause: &SyntaxNode) -> Option<SyntaxNode> {
    elsif_clause.children().find(|n| n.kind() == SyntaxKind::EXPR)
}

/// Find and report all ternary operators within an expression tree.
///
/// Used to detect ternary operators in IF/ELSIF conditions.
fn find_and_report_ternaries(
    condition: &SyntaxNode,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for node in condition.descendants() {
        if node.kind() == SyntaxKind::TERNARY_EXPR {
            diagnostics.push(make_diagnostic(&node, code, ctx));
        }
    }
}

/// Create a diagnostic for a nested ternary operator.
fn make_diagnostic(
    node: &SyntaxNode,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: "Не рекомендуется использовать вложенный тернарный оператор".to_string(),
        severity: ctx.severity(code),
        range: node.text_range(),
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic, check_ast_diagnostic_with_config, format_diags};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;
    #[test]
    fn test_comprehensive() {
        let code = r#"ПериодПо = ?(Шапка.ЭтоУвольнение
           , Шапка.Дата
           , ?(Шапка.ЭтоАванс
             , Дата( Год(Шапка.ПериодРегистрации)
                   , Месяц(Шапка.ПериодРегистрации)
                   , 15
                   )
             , КонецМесяца(Шапка.ПериодРегистрации)
             )
            );

Статус = ?(ПолучитьСкидку() > 10, "Особый клиент", "Обычный клиент");

Если ?(Стр.Emp_emptype = Null, 0, Стр.Emp_emptype) = 0 ИЛИ Условие() ИЛИ ?(Стр.Тест = Null, 1, Стр.Тест) = 2 Тогда

      Статус = ?(ПолучитьСкидку() > 10, "Особый клиент", "Обычный клиент");

ИначеЕсли Стр.Emp_emptype > 3 Тогда

      Статус = ?(
            ПолучитьСкидку() = 0,
            "---",
            ?(ПолучитьСкидку() > 30, "Особый клиент", "Обычный клиент")
      );

КонецЕсли;"#;
        let diagnostics = check_ast_diagnostic(code, check);

        expect![[r#"
            NestedTernaryOperator @ 3:14..9:15
              message: Не рекомендуется использовать вложенный тернарный оператор
              severity: Warning
            NestedTernaryOperator @ 14:6..14:51
              message: Не рекомендуется использовать вложенный тернарный оператор
              severity: Warning
            NestedTernaryOperator @ 14:74..14:105
              message: Не рекомендуется использовать вложенный тернарный оператор
              severity: Warning
            NestedTernaryOperator @ 23:13..23:72
              message: Не рекомендуется использовать вложенный тернарный оператор
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_for_simple_ternary() {
        let code = r#"
Результат = ?(Условие, Истина, Ложь);
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_nested_ternary_in_assignment() {
        let code = r#"
Результат = ?(Условие1, ?(Условие2, 1, 2), 3);
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            NestedTernaryOperator @ 2:25..2:42
              message: Не рекомендуется использовать вложенный тернарный оператор
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_ternary_in_if_condition() {
        let code = r#"
Если ?(А, Б, В) = 1 Тогда
    Х = 1;
КонецЕсли;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            NestedTernaryOperator @ 2:6..2:16
              message: Не рекомендуется использовать вложенный тернарный оператор
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_ternary_in_elsif_condition() {
        let code = r#"
Если Условие Тогда
    Х = 1;
ИначеЕсли ?(А, Б, В) = 1 Тогда
    Х = 2;
КонецЕсли;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            NestedTernaryOperator @ 4:11..4:21
              message: Не рекомендуется использовать вложенный тернарный оператор
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_disabled() {
        let code = r#"
Результат = ?(Условие1, ?(Условие2, 1, 2), 3);
"#;
        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::NestedTernaryOperator);

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }
}
