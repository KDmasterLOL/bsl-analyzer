use crate::hir::{BinaryOp, Expr, ExprIdx, IfStmt, Stmt, StmtIdx};
use crate::{Body, ExprId, IdConversion, StmtId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConditionMetrics {
    pub condition: ExprId,
    pub logical_op_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HirMethodMetrics {
    pub cognitive: u32,
    pub max_nesting: u32,
    pub if_conditions: Vec<ConditionMetrics>,
    pub if_condition_max: u32,
    pub nesting_leaves: Vec<NestingLeafMetrics>,
    pub params_count: u32,
    pub optional_params_count: u32,
    pub size_lines: u32,
    pub boolean_ops_count: u32,
    pub ternary_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestingLeafMetrics {
    pub stmt: StmtId,
    pub depth: u32,
}

pub fn compute_hir_metrics(body: &Body) -> HirMethodMetrics {
    let mut visitor = MetricsVisitor::default();
    for &stmt_id in body.body_stmts.iter() {
        visitor.visit_stmt(body, stmt_id, 0);
    }
    let mut metrics = visitor.finish();
    metrics.params_count = body.params.len() as u32;
    metrics.optional_params_count =
        body.params.iter().filter(|&&p| body.bindings[p].default_value.is_some()).count() as u32;
    metrics
}

#[derive(Debug, Default)]
struct MetricsVisitor {
    cognitive: u32,
    max_nesting: u32,
    if_conditions: Vec<ConditionMetrics>,
    nesting_leaves: Vec<NestingLeafMetrics>,
    boolean_ops_count: u32,
    ternary_count: u32,
    nesting_stmt_depth: u32,
    had_nested_child: bool,
}

impl MetricsVisitor {
    fn finish(self) -> HirMethodMetrics {
        let if_condition_max =
            self.if_conditions.iter().map(|c| c.logical_op_count).max().unwrap_or(0);
        HirMethodMetrics {
            cognitive: self.cognitive,
            max_nesting: self.max_nesting,
            if_conditions: self.if_conditions,
            if_condition_max,
            nesting_leaves: self.nesting_leaves,
            params_count: 0,
            optional_params_count: 0,
            size_lines: 0,
            boolean_ops_count: self.boolean_ops_count,
            ternary_count: self.ternary_count,
        }
    }

    fn note_depth(&mut self, depth: u32) {
        if depth > self.max_nesting {
            self.max_nesting = depth;
        }
    }

    fn with_nesting_stmt(&mut self, stmt_id: StmtIdx, f: impl FnOnce(&mut Self)) {
        self.nesting_stmt_depth += 1;
        self.had_nested_child = false;
        f(self);
        if !self.had_nested_child {
            self.nesting_leaves.push(NestingLeafMetrics {
                stmt: StmtId::from_idx(stmt_id),
                depth: self.nesting_stmt_depth,
            });
        }
        self.had_nested_child = true;
        self.nesting_stmt_depth -= 1;
    }

    fn visit_stmt(&mut self, body: &Body, stmt_id: StmtIdx, nesting: u32) {
        self.note_depth(nesting);
        match body.stmt_idx(stmt_id) {
            Stmt::If(if_stmt) => {
                let if_stmt = if_stmt.clone();
                self.with_nesting_stmt(stmt_id, |this| this.visit_if(body, &if_stmt, nesting));
            }

            Stmt::While { condition, body: loop_body } => {
                let condition = *condition;
                let loop_body = loop_body.clone();
                self.with_nesting_stmt(stmt_id, |this| {
                    this.cognitive += 1 + nesting;
                    this.visit_expr(body, condition);
                    for &child in loop_body.iter() {
                        this.visit_stmt(body, child, nesting + 1);
                    }
                });
            }

            Stmt::For { from, to, body: loop_body, .. } => {
                let from = *from;
                let to = *to;
                let loop_body = loop_body.clone();
                self.with_nesting_stmt(stmt_id, |this| {
                    this.cognitive += 1 + nesting;
                    this.count_extras_only(body, from);
                    this.count_extras_only(body, to);
                    for &child in loop_body.iter() {
                        this.visit_stmt(body, child, nesting + 1);
                    }
                });
            }

            Stmt::ForEach { collection, body: loop_body, .. } => {
                let collection = *collection;
                let loop_body = loop_body.clone();
                self.with_nesting_stmt(stmt_id, |this| {
                    this.cognitive += 1 + nesting;
                    this.count_extras_only(body, collection);
                    for &child in loop_body.iter() {
                        this.visit_stmt(body, child, nesting + 1);
                    }
                });
            }

            Stmt::Try { body: try_body, except } => {
                let try_body = try_body.clone();
                let except = except.clone();
                self.with_nesting_stmt(stmt_id, |this| {
                    for &child in try_body.iter() {
                        this.visit_stmt(body, child, nesting);
                    }
                    this.cognitive += 1 + nesting;
                    for &child in except.iter() {
                        this.visit_stmt(body, child, nesting + 1);
                    }
                });
            }

            Stmt::Goto(_) => {
                self.cognitive += 1;
            }

            Stmt::Expr(expr) => self.visit_expr(body, *expr),
            Stmt::Assign { target, value } => {
                self.count_extras_only(body, *target);
                self.visit_expr(body, *value);
            }
            Stmt::Return { value: Some(v) } | Stmt::Raise { value: Some(v) } => {
                self.visit_expr(body, *v);
            }
            Stmt::Execute { expr } => self.visit_expr(body, *expr),
            Stmt::AddHandler { event, handler } | Stmt::RemoveHandler { event, handler } => {
                self.visit_expr(body, *event);
                self.visit_expr(body, *handler);
            }

            Stmt::PreprocIf(preproc) => {
                self.cognitive += 1 + nesting;
                for &child in preproc.then_branch.iter() {
                    self.visit_stmt(body, child, nesting + 1);
                }
                for (_cond_range, _directive_range, elsif_body) in preproc.elsif_branches.iter() {
                    self.cognitive += 1;
                    for &child in elsif_body.iter() {
                        self.visit_stmt(body, child, nesting + 2);
                    }
                }
                if let Some(ref else_body) = preproc.else_branch {
                    self.cognitive += 1;
                    for &child in else_body.iter() {
                        self.visit_stmt(body, child, nesting + 2);
                    }
                }
            }

            Stmt::Return { value: None }
            | Stmt::Raise { value: None }
            | Stmt::VarDecl { .. }
            | Stmt::Break
            | Stmt::Continue
            | Stmt::Label(_) => {}
        }
    }

    fn visit_if(&mut self, body: &Body, if_stmt: &IfStmt, nesting: u32) {
        self.cognitive += 1 + nesting;
        self.note_condition(body, if_stmt.condition);
        self.visit_expr(body, if_stmt.condition);

        for &child in if_stmt.then_branch.iter() {
            self.visit_stmt(body, child, nesting + 1);
        }

        for (elsif_cond, elsif_body) in if_stmt.elsif_branches.iter() {
            self.cognitive += 1;
            self.note_condition(body, *elsif_cond);
            self.visit_expr(body, *elsif_cond);
            for &child in elsif_body.iter() {
                self.visit_stmt(body, child, nesting + 2);
            }
        }

        if let Some(ref else_body) = if_stmt.else_branch {
            self.cognitive += 1;
            for &child in else_body.iter() {
                self.visit_stmt(body, child, nesting + 2);
            }
        }
    }

    fn visit_expr(&mut self, body: &Body, expr_id: ExprIdx) {
        let is_recovered = body.is_recovered(ExprId::from_idx(expr_id));
        match body.expr_idx(expr_id) {
            Expr::Ternary { condition, then_expr, else_expr } => {
                self.cognitive += 1;
                if !is_recovered {
                    self.ternary_count += 1;
                }
                self.visit_expr(body, *condition);
                self.visit_expr(body, *then_expr);
                self.visit_expr(body, *else_expr);
            }
            Expr::BinaryOp { lhs, rhs, op } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    self.cognitive += 1;
                    if !is_recovered {
                        self.boolean_ops_count += 1;
                    }
                }
                self.visit_expr(body, *lhs);
                self.visit_expr(body, *rhs);
            }
            Expr::UnaryOp { expr, .. } => self.visit_expr(body, *expr),
            Expr::Call { callee, args } => {
                self.visit_expr(body, *callee);
                for &arg in args.iter() {
                    self.visit_expr(body, arg);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.visit_expr(body, *receiver);
                for &arg in args.iter() {
                    self.visit_expr(body, arg);
                }
            }
            Expr::Index { base, index } => {
                self.visit_expr(body, *base);
                self.visit_expr(body, *index);
            }
            Expr::Field { base, .. } => self.visit_expr(body, *base),
            Expr::New { args, .. } => {
                for &arg in args.iter() {
                    self.visit_expr(body, arg);
                }
            }
            Expr::Array(items) => {
                for &item in items.iter() {
                    self.visit_expr(body, item);
                }
            }
            Expr::Await { expr } => self.visit_expr(body, *expr),
            Expr::Missing | Expr::Literal(_) | Expr::Path(_) => {}
        }
    }

    fn count_extras_only(&mut self, body: &Body, expr_id: ExprIdx) {
        let is_recovered = body.is_recovered(ExprId::from_idx(expr_id));
        match body.expr_idx(expr_id) {
            Expr::Ternary { condition, then_expr, else_expr } => {
                if !is_recovered {
                    self.ternary_count += 1;
                }
                self.count_extras_only(body, *condition);
                self.count_extras_only(body, *then_expr);
                self.count_extras_only(body, *else_expr);
            }
            Expr::BinaryOp { lhs, rhs, op } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) && !is_recovered {
                    self.boolean_ops_count += 1;
                }
                self.count_extras_only(body, *lhs);
                self.count_extras_only(body, *rhs);
            }
            Expr::UnaryOp { expr, .. } => self.count_extras_only(body, *expr),
            Expr::Call { callee, args } => {
                self.count_extras_only(body, *callee);
                for &arg in args.iter() {
                    self.count_extras_only(body, arg);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.count_extras_only(body, *receiver);
                for &arg in args.iter() {
                    self.count_extras_only(body, arg);
                }
            }
            Expr::Index { base, index } => {
                self.count_extras_only(body, *base);
                self.count_extras_only(body, *index);
            }
            Expr::Field { base, .. } => self.count_extras_only(body, *base),
            Expr::New { args, .. } => {
                for &arg in args.iter() {
                    self.count_extras_only(body, arg);
                }
            }
            Expr::Array(items) => {
                for &item in items.iter() {
                    self.count_extras_only(body, item);
                }
            }
            Expr::Await { expr } => self.count_extras_only(body, *expr),
            Expr::Missing | Expr::Literal(_) | Expr::Path(_) => {}
        }
    }

    fn note_condition(&mut self, body: &Body, condition: ExprIdx) {
        let count = count_logical_ops(body, condition);
        self.if_conditions.push(ConditionMetrics {
            condition: ExprId::from_idx(condition),
            logical_op_count: count,
        });
    }
}

fn count_logical_ops(body: &Body, expr: ExprIdx) -> u32 {
    let mut total = 0;
    fn walk(body: &Body, expr: ExprIdx, total: &mut u32) {
        match body.expr_idx(expr) {
            Expr::BinaryOp { lhs, rhs, op } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    *total += 1;
                }
                walk(body, *lhs, total);
                walk(body, *rhs, total);
            }
            Expr::UnaryOp { expr, .. } => walk(body, *expr, total),
            Expr::Ternary { condition, then_expr, else_expr } => {
                walk(body, *condition, total);
                walk(body, *then_expr, total);
                walk(body, *else_expr, total);
            }
            Expr::Call { callee, args } => {
                walk(body, *callee, total);
                for &arg in args.iter() {
                    walk(body, arg, total);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                walk(body, *receiver, total);
                for &arg in args.iter() {
                    walk(body, arg, total);
                }
            }
            Expr::Index { base, index } => {
                walk(body, *base, total);
                walk(body, *index, total);
            }
            Expr::Field { base, .. } => walk(body, *base, total),
            Expr::New { args, .. } => {
                for &arg in args.iter() {
                    walk(body, arg, total);
                }
            }
            Expr::Array(items) => {
                for &item in items.iter() {
                    walk(body, item, total);
                }
            }
            Expr::Await { expr } => walk(body, *expr, total),
            Expr::Missing | Expr::Literal(_) | Expr::Path(_) => {}
        }
    }
    walk(body, expr, &mut total);
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::lower_method;
    use syntax::SyntaxKind;

    fn parse_and_lower(code: &str) -> Body {
        let parse = parser::parse(code);
        let root = parse.syntax_node();
        let method_node = root
            .descendants()
            .find(|n| matches!(n.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF))
            .expect("Should have a method");
        let is_function = method_node.kind() == SyntaxKind::FUNCTION_DEF;
        let result = lower_method(&method_node, is_function);
        result.body
    }

    #[test]
    fn empty_method_returns_zero_metrics() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
КонецПроцедуры
"#,
        );
        assert_eq!(compute_hir_metrics(&body), HirMethodMetrics::default());
    }

    #[test]
    fn straight_line_keeps_zero_metrics() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Сообщить("Привет");
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.cognitive, 0);
        assert_eq!(m.max_nesting, 0);
        assert_eq!(m.if_condition_max, 0);
    }

    #[test]
    fn single_if_metrics() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если Истина Тогда
        Сообщить("");
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.cognitive, 1);
        assert_eq!(m.max_nesting, 1);
        assert_eq!(m.if_condition_max, 0);
    }

    #[test]
    fn nested_if_while_with_and_op() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если А Тогда
        Пока Б И В Цикл
            Сообщить("");
        КонецЦикла;
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.cognitive, 4);
        assert_eq!(m.max_nesting, 2);
        assert_eq!(m.if_conditions.len(), 1);
        assert_eq!(m.if_condition_max, 0);
    }

    #[test]
    fn while_and_ternary_conditions_are_not_recorded() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Пока А И Б Цикл
        Х = ?(В ИЛИ Г, 1, 2);
    КонецЦикла;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert!(
            m.if_conditions.is_empty(),
            "While/ternary conditions must not appear in if_conditions, got {:?}",
            m.if_conditions
        );
        assert_eq!(m.if_condition_max, 0);
    }

    #[test]
    fn if_condition_max_picks_widest_condition() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если А И Б ИЛИ В Тогда
        Сообщить("");
    КонецЕсли;
    Если Г Тогда
        Сообщить("");
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.cognitive, 1 + 2 + 1);
        assert_eq!(m.if_condition_max, 2);
    }

    #[test]
    fn if_conditions_lists_each_condition_separately() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если А И Б ИЛИ В Тогда
        Сообщить("");
    КонецЕсли;
    Если Г Тогда
        Сообщить("");
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.if_conditions.len(), 2);
        assert_eq!(m.if_conditions[0].logical_op_count, 2);
        assert_eq!(m.if_conditions[1].logical_op_count, 0);
        assert_eq!(m.if_condition_max, 2);
    }

    #[test]
    fn nesting_leaves_record_innermost_only() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если А Тогда
        Если Б Тогда
            Если В Тогда
                Сообщить("");
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.nesting_leaves.len(), 1, "only innermost If is a leaf");
        assert_eq!(m.nesting_leaves[0].depth, 3, "leaf at depth=3 matches legacy");
    }

    #[test]
    fn nesting_leaves_emit_per_leaf_branch() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если А Тогда
        Если Б Тогда
            Сообщить("");
        КонецЕсли;
        Если В Тогда
            Сообщить("");
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.nesting_leaves.len(), 2, "two sibling Ifs are both leaves");
        assert!(
            m.nesting_leaves.iter().all(|l| l.depth == 2),
            "both leaves at depth=2 (under outer If)"
        );
    }

    #[test]
    fn nesting_leaves_count_all_kinds() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если А Тогда
        Попытка
            Для Каждого Э Из К Цикл
                Сообщить("");
            КонецЦикла;
        Исключение
        КонецПопытки;
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.nesting_leaves.len(), 1, "innermost For is the only leaf");
        assert_eq!(m.nesting_leaves[0].depth, 3, "If→Try→For = depth 3");
    }

    #[test]
    fn for_bounds_and_collection_extras_do_not_leak_into_cognitive() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Для Каждого Э Из ?(Условие, Колл1, Колл2) Цикл
        Сообщить("");
    КонецЦикла;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(
            m.cognitive, 1,
            "cognitive must stay at the ForEach decision point — ternary in \
             collection must not leak in, got {}",
            m.cognitive
        );
        assert_eq!(
            m.ternary_count, 1,
            "ternary in ForEach collection must contribute to cyclomatic extras, got {}",
            m.ternary_count
        );
    }

    #[test]
    fn assign_target_extras_do_not_leak_into_cognitive() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Массив[?(Условие, 0, 1)] = 5;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.cognitive, 0, "ternary in assign target must not bump cognitive");
        assert_eq!(m.ternary_count, 1, "ternary in assign target must contribute to extras");
    }

    #[test]
    fn recovered_exprs_do_not_inflate_cyclomatic_extras() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Х = ?(а., 1, 2);
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert!(
            m.boolean_ops_count == 0,
            "boolean_ops must stay 0 for fixture without real AND/OR, got {}",
            m.boolean_ops_count
        );
        assert!(
            m.ternary_count <= 1,
            "ternary_count must not be inflated by recovered subtrees, got {}",
            m.ternary_count
        );
    }

    #[test]
    fn default_matches_empty_metrics() {
        let body = parse_and_lower(
            r#"
Процедура Пусто()
КонецПроцедуры
"#,
        );
        assert_eq!(compute_hir_metrics(&body), HirMethodMetrics::default());
    }
}
