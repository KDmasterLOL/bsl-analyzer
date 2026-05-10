use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::{SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Checks a single syntax node for ternary expressions reducible to a simpler form.
/// Check a single syntax node for useless ternary operators (node-based API).
///
/// This is called from `collect_syntax_single_pass()` for each node in single AST pass.
pub fn check_node(node: &SyntaxNode, acc: &mut Vec<Diagnostic>, ctx: &DiagnosticsContext) {
    let code = DiagnosticCode::UselessTernaryOperator;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    if node.kind() == SyntaxKind::TERNARY_EXPR {
        if let Some(diag) = check_ternary(node, ctx) {
            acc.push(diag);
        }
    }
}

/// Main entry point for syntax-based useless-ternary detection.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UselessTernaryOperator;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        check_node(&node, &mut diagnostics, ctx);
    }

    diagnostics
}

fn check_ternary(node: &SyntaxNode, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::UselessTernaryOperator;

    let exprs: Vec<_> = node.children().filter(|n| n.kind() == SyntaxKind::EXPR).collect();

    if exprs.len() < 3 {
        return None;
    }

    let condition = &exprs[0];
    let true_branch = &exprs[1];
    let false_branch = &exprs[2];

    let condition_bool = get_boolean_literal(condition);
    let true_bool = get_boolean_literal(true_branch);
    let false_bool = get_boolean_literal(false_branch);

    let is_useless = condition_bool.is_some() || (true_bool.is_some() && false_bool.is_some());

    if is_useless {
        return Some(Diagnostic {
            code,
            message: "Бесполезный тернарный оператор".to_string(),
            severity: ctx.severity(code),
            range: node.text_range(),
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BooleanValue {
    True,
    False,
}

fn get_boolean_literal(expr: &SyntaxNode) -> Option<BooleanValue> {
    for child in expr.descendants_with_tokens() {
        if let Some(token) = child.as_token() {
            match token.kind() {
                SyntaxKind::KW_TRUE => return Some(BooleanValue::True),
                SyntaxKind::KW_FALSE => return Some(BooleanValue::False),
                _ => {}
            }
        }

        if let Some(node) = child.as_node() {
            let kind = node.kind();
            if kind == SyntaxKind::BINARY_EXPR
                || kind == SyntaxKind::CALL_EXPR
                || kind == SyntaxKind::TERNARY_EXPR
                || kind == SyntaxKind::UNARY_EXPR
                || kind == SyntaxKind::FIELD_EXPR
                || kind == SyntaxKind::INDEX_EXPR
            {
                return None;
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_direct_ternary() {
        let code = "А = ?(Б = 1, Истина, Ложь);";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UselessTernaryOperator,
            expect![[r#"
            UselessTernaryOperator @ 1:5..1:27
              message: Бесполезный тернарный оператор
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_inverted_ternary() {
        let code = "А = ?(Б = 0, False, True);";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UselessTernaryOperator,
            expect![[r#"
            UselessTernaryOperator @ 1:5..1:26
              message: Бесполезный тернарный оператор
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_condition_is_boolean() {
        let code = "А = ?(истина, 1, 0);";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UselessTernaryOperator,
            expect![[r#"
            UselessTernaryOperator @ 1:5..1:20
              message: Бесполезный тернарный оператор
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_valid_ternary() {
        let code = r#"ОбластьМакета.Параметры.ДебетСубСчета = ОбластьМакета.Параметры.ДебетСубСчета
					+ ?(ПустаяСтрока(ОбластьМакета.Параметры.ДебетСубСчета), "", ", ")
					+ СчетДт;"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UselessTernaryOperator,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_single_boolean_branch_is_not_useless() {
        // null-guard: ?(obj = Неопределено, Ложь, obj.Свойство)
        let code = r#"А = ?(СтрокаПредмета.Предмет = Неопределено, Ложь, СтрокаПредмета.Предмет.ПометкаУдаления);"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UselessTernaryOperator,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_mixed_boolean_nonboolean_not_useless() {
        let code = "А = ?(Б = 1, True, 1);";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UselessTernaryOperator,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_comprehensive() {
        let code = r#"// Бессмысленные тернарники
А = ?(Б = 1, Истина, Ложь);// прямой, фиксится в А = Б = 1;
А = ?(Б = 0, False, True);// обратный, фиксится в А = НЕ (Б = 0);
А = ?(Б = 1, True, Истина);
А = ?(Б = 0, Ложь, False);
А = ?(истина, 1, 0);
А = ?(false, 0, 1);

// валидные: одна ветка-литерал — не бесполезный тернарник (null-guard и т.п.)
А = ?(Б = 1, True, 1);
А = ?(Б = 0, 0, False);
СтрокаПредмета.Картинка = МультипредметностьКлиентСервер.ИндексКартинкиРолиПредмета(
            СтрокаПредмета.РольПредмета, ?(СтрокаПредмета.Предмет = Неопределено, Ложь, СтрокаПредмета.Предмет.ПометкаУдаления));

// валидный: обе ветки — не булевы литералы
ОбластьМакета.Параметры.ДебетСубСчета = ОбластьМакета.Параметры.ДебетСубСчета
						+ ?(ПустаяСтрока(ОбластьМакета.Параметры.ДебетСубСчета), "", ", ")
						+ СчетДт;
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UselessTernaryOperator,
            expect![[r#"
            UselessTernaryOperator @ 2:5..2:27
              message: Бесполезный тернарный оператор
              severity: Hint
            UselessTernaryOperator @ 3:5..3:26
              message: Бесполезный тернарный оператор
              severity: Hint
            UselessTernaryOperator @ 4:5..4:27
              message: Бесполезный тернарный оператор
              severity: Hint
            UselessTernaryOperator @ 5:5..5:26
              message: Бесполезный тернарный оператор
              severity: Hint
            UselessTernaryOperator @ 6:5..6:20
              message: Бесполезный тернарный оператор
              severity: Hint
            UselessTernaryOperator @ 7:5..7:19
              message: Бесполезный тернарный оператор
              severity: Hint"#]],
        );
    }
}
