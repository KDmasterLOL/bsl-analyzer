use crate::define_metadata;
use crate::metadata::*;
use crate::BodyContext;
use crate::{Diagnostic, DiagnosticCode, Fix, TextEdit};
use hir::LocalRange;
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

pub fn check_node(node: &SyntaxNode, acc: &mut Vec<Diagnostic<LocalRange>>, ctx: &BodyContext) {
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

fn check_ternary(node: &SyntaxNode, ctx: &BodyContext) -> Option<Diagnostic<LocalRange>> {
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
            range: LocalRange::of_detached_node(node.text_range()),
            tags: ctx.tags(code),
            fixes: canonical_fix(node, condition, condition_bool, true_bool, false_bool, ctx),
        });
    }

    None
}

/// Offer a fix only for the unambiguous canonical form `?(условие, Истина, Ложь)`,
/// where the condition is a real expression (not a boolean literal) and the branches
/// are exactly `Истина`/`Ложь`. The whole ternary is replaced with the verbatim source
/// of the condition. The inverted form (`Ложь`/`Истина`) and boolean-literal conditions
/// need negation or branch selection and are left as report-only.
fn canonical_fix(
    node: &SyntaxNode,
    condition: &SyntaxNode,
    condition_bool: Option<BooleanValue>,
    true_bool: Option<BooleanValue>,
    false_bool: Option<BooleanValue>,
    ctx: &BodyContext,
) -> Vec<Fix<LocalRange>> {
    if condition_bool.is_some()
        || true_bool != Some(BooleanValue::True)
        || false_bool != Some(BooleanValue::False)
    {
        return vec![];
    }

    let cond_src = ctx.text_of(LocalRange::of_detached_node(condition.text_range()));

    // A compound condition (`Б = 1`, `НЕ Х`) substituted verbatim would change
    // precedence in an operand slot (`Х + ?(Б=1,…)` → `Х + Б = 1` reparses as
    // `(Х + Б) = 1`), so parenthesise it. The parentheses are also clearer for the
    // common `А = (Б = 1)` case where the second `=` is a comparison, and are always
    // valid, so they are added unconditionally for compound conditions.
    let new_text =
        if is_compound_condition(condition) { format!("({})", cond_src) } else { cond_src };

    vec![Fix::safe(
        "Заменить на условие",
        vec![TextEdit { range: LocalRange::of_detached_node(node.text_range()), new_text }],
    )]
}

/// Whether the condition is a binary/unary expression (whose bare substitution could
/// bind differently against surrounding operators). The grammar wraps operands in nested
/// `EXPR` nodes, so peel them first to reach the real operator node.
fn is_compound_condition(condition: &SyntaxNode) -> bool {
    matches!(peel_expr(condition).kind(), SyntaxKind::BINARY_EXPR | SyntaxKind::UNARY_EXPR)
}

/// Descend through the nested `EXPR` wrapper nodes the grammar emits to the first
/// meaningful expression node.
fn peel_expr(node: &SyntaxNode) -> SyntaxNode {
    let mut current = node.clone();
    while current.kind() == SyntaxKind::EXPR {
        match current.children().next() {
            Some(child) => current = child,
            None => break,
        }
    }
    current
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
    use crate::test_utils::{check_diagnostics_snapshot_for, check_fix_snapshot_for};
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_fix_canonical_only() {
        // Canonical `?(cond, Истина, Ложь)` gets a fix; the inverted form and a
        // boolean-literal condition are report-only (no fix offered).
        let code = r#"А = ?(Б = 1, Истина, Ложь);
В = ?(Б = 0, False, True);
Г = ?(истина, 1, 0);"#;
        check_fix_snapshot_for(
            code,
            DiagnosticCode::UselessTernaryOperator,
            expect![[r#"
                UselessTernaryOperator @ 1:5..1:27 — Заменить на условие [fix_all=true]
                А = (Б = 1);
                В = ?(Б = 0, False, True);
                Г = ?(истина, 1, 0);"#]],
        );
    }

    #[test]
    fn test_fix_wraps_compound_condition() {
        // A compound condition is parenthesised so it binds correctly in any operand
        // slot (`Х + (Б = 1)`) and reads unambiguously as an assignment RHS.
        let code = r#"А = ?(Б = 1, Истина, Ложь);
В = Х + ?(Б = 1, Истина, Ложь);
Г = ?(НЕ Флаг, Истина, Ложь);"#;
        check_fix_snapshot_for(
            code,
            DiagnosticCode::UselessTernaryOperator,
            expect![[r#"
                UselessTernaryOperator @ 1:5..1:27 — Заменить на условие [fix_all=true]
                А = (Б = 1);
                В = Х + ?(Б = 1, Истина, Ложь);
                Г = ?(НЕ Флаг, Истина, Ложь);

                UselessTernaryOperator @ 2:9..2:31 — Заменить на условие [fix_all=true]
                А = ?(Б = 1, Истина, Ложь);
                В = Х + (Б = 1);
                Г = ?(НЕ Флаг, Истина, Ложь);

                UselessTernaryOperator @ 3:5..3:29 — Заменить на условие [fix_all=true]
                А = ?(Б = 1, Истина, Ложь);
                В = Х + ?(Б = 1, Истина, Ложь);
                Г = (НЕ Флаг);"#]],
        );
    }

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
