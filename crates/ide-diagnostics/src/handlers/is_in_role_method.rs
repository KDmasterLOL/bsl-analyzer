use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Expr, ExprId, IdConversion, Stmt};
use ide_db::TextRange;
use std::collections::HashSet;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::IsInRoleMethod;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut all_diagnostics = Vec::new();
    let module_bodies = ctx.module_bodies();

    for (_local_id, body, source_map) in module_bodies.method_bodies() {
        let mut checker = IsInRoleChecker {
            is_in_role_vars: HashSet::new(),
            privileged_mode_vars: HashSet::new(),
            diagnostics: Vec::new(),
            body,
            source_map,
            code,
            ctx,
        };

        checker.collect_variables();

        checker.check_statements();

        all_diagnostics.extend(checker.diagnostics);
    }

    if let Some(lower_result) = module_bodies.module_code_result() {
        let mut checker = IsInRoleChecker {
            is_in_role_vars: HashSet::new(),
            privileged_mode_vars: HashSet::new(),
            diagnostics: Vec::new(),
            body: &lower_result.body,
            source_map: &lower_result.source_map,
            code,
            ctx,
        };

        checker.collect_variables();
        checker.check_statements();

        all_diagnostics.extend(checker.diagnostics);
    }

    all_diagnostics.sort_by_key(|d| d.range.start());
    all_diagnostics
}

struct IsInRoleChecker<'a> {
    is_in_role_vars: HashSet<String>,
    privileged_mode_vars: HashSet<String>,
    diagnostics: Vec<Diagnostic>,
    body: &'a hir::Body,
    source_map: &'a hir::BodySourceMap,
    code: DiagnosticCode,
    ctx: &'a DiagnosticsContext<'a>,
}

impl<'a> IsInRoleChecker<'a> {
    fn collect_variables(&mut self) {
        for (_stmt_id, stmt) in self.body.stmts_iter() {
            if let Stmt::Assign { target, value } = stmt {
                self.handle_assignment(ExprId::from_idx(*target), ExprId::from_idx(*value));
            }
        }
    }

    fn check_statements(&mut self) {
        for (_stmt_id, stmt) in self.body.stmts_iter() {
            if let Stmt::If(if_stmt) = stmt {
                self.check_expression(ExprId::from_idx(if_stmt.condition));

                for (elsif_condition, _elsif_stmts) in if_stmt.elsif_branches.iter() {
                    self.check_expression(ExprId::from_idx(*elsif_condition));
                }
            }
        }
    }

    fn handle_assignment(&mut self, target: ExprId, value: ExprId) {
        let var_name = if let Expr::Path(name) = self.body.expr(target) {
            Some(name.as_str().to_lowercase())
        } else {
            None
        };

        if let Some(ref var) = var_name {
            self.is_in_role_vars.remove(var);
            self.privileged_mode_vars.remove(var);
        }

        if let Some(ref var) = var_name {
            if self.is_is_in_role_call(value) {
                self.is_in_role_vars.insert(var.clone());
            } else if self.is_privileged_mode_call(value) {
                self.privileged_mode_vars.insert(var.clone());
            }
        }
    }

    fn check_expression(&mut self, root_expr_id: ExprId) {
        let has_protection = self.has_privileged_mode_protection(root_expr_id);

        self.find_is_in_role_usages(root_expr_id, has_protection);
    }

    fn find_is_in_role_usages(&mut self, expr_id: ExprId, has_protection: bool) {
        if self.is_is_in_role_call(expr_id) && !has_protection {
            if let Some(range) = self.source_map.expr_range(expr_id) {
                self.diagnostics.push(create_diagnostic(range, self.code, self.ctx));
            }
        }

        if let Expr::Path(name) = self.body.expr(expr_id) {
            let var_name = name.as_str().to_lowercase();
            if self.is_in_role_vars.contains(&var_name) && !has_protection {
                if let Some(range) = self.source_map.expr_range(expr_id) {
                    self.diagnostics.push(create_diagnostic(range, self.code, self.ctx));
                }
            }
        }

        match self.body.expr(expr_id) {
            Expr::BinaryOp { lhs, rhs, .. } => {
                self.find_is_in_role_usages(ExprId::from_idx(*lhs), has_protection);
                self.find_is_in_role_usages(ExprId::from_idx(*rhs), has_protection);
            }
            Expr::UnaryOp { expr, .. } => {
                self.find_is_in_role_usages(ExprId::from_idx(*expr), has_protection);
            }
            Expr::Ternary { condition, then_expr, else_expr } => {
                self.find_is_in_role_usages(ExprId::from_idx(*condition), has_protection);
                self.find_is_in_role_usages(ExprId::from_idx(*then_expr), has_protection);
                self.find_is_in_role_usages(ExprId::from_idx(*else_expr), has_protection);
            }
            _ => {}
        }
    }

    fn is_is_in_role_call(&self, expr_id: ExprId) -> bool {
        if let Expr::Call { callee, .. } = self.body.expr(expr_id) {
            if let Expr::Path(name) = self.body.expr(ExprId::from_idx(*callee)) {
                return is_is_in_role_method(name.as_str());
            }
        }
        false
    }

    fn is_privileged_mode_call(&self, expr_id: ExprId) -> bool {
        if let Expr::Call { callee, .. } = self.body.expr(expr_id) {
            if let Expr::Path(name) = self.body.expr(ExprId::from_idx(*callee)) {
                return is_privileged_mode_method(name.as_str());
            }
        }
        false
    }

    fn has_privileged_mode_protection(&self, expr_id: ExprId) -> bool {
        self.contains_privileged_mode(expr_id)
    }

    fn contains_privileged_mode(&self, expr_id: ExprId) -> bool {
        match self.body.expr(expr_id) {
            Expr::Call { callee, .. } => {
                if let Expr::Path(name) = self.body.expr(ExprId::from_idx(*callee)) {
                    if is_privileged_mode_method(name.as_str()) {
                        return true;
                    }
                }
                false
            }

            Expr::Path(name) => {
                let var_name = name.as_str().to_lowercase();
                self.privileged_mode_vars.contains(&var_name)
            }

            Expr::BinaryOp { lhs, rhs, .. } => {
                self.contains_privileged_mode(ExprId::from_idx(*lhs))
                    || self.contains_privileged_mode(ExprId::from_idx(*rhs))
            }

            Expr::UnaryOp { expr, .. } => self.contains_privileged_mode(ExprId::from_idx(*expr)),

            Expr::Ternary { condition, then_expr, else_expr } => {
                self.contains_privileged_mode(ExprId::from_idx(*condition))
                    || self.contains_privileged_mode(ExprId::from_idx(*then_expr))
                    || self.contains_privileged_mode(ExprId::from_idx(*else_expr))
            }

            _ => false,
        }
    }
}

fn is_is_in_role_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "рольдоступна" | "isinrole")
}

fn is_privileged_mode_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "привилегированныйрежим" | "privilegedmode")
}

fn create_diagnostic(
    range: TextRange,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: "Для проверки прав доступа в коде следует использовать метод ПравоДоступа"
            .to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура Тест()
    ДоступРазрешен = РольДоступна("НужнаяРоль");
    ПР = ПривилегированныйРежим();
    Если ДоступРазрешен ИЛИ ПР Тогда // Нет срабатывания. Есть проверка на привилегированный режим
    КонецЕсли;
КонецПроцедуры

Процедура Тест()
    Если РольДоступна("НужнаяРоль") ИЛИ ПривилегированныйРежим() Тогда // Нет срабатывания. Есть проверка на привилегированный режим
    КонецЕсли;
КонецПроцедуры

Процедура Тест()
    ДоступРазрешен = РольДоступна("НужнаяРоль");
    Если ДоступРазрешен ИЛИ ПривилегированныйРежим() Тогда // Нет срабатывания. Есть проверка на привилегированный режим
    КонецЕсли;
КонецПроцедуры

Процедура Тест()
    ПР = ПривилегированныйРежим();
    Если РольДоступна("НужнаяРоль") ИЛИ ПР Тогда // Нет срабатывания. Есть проверка на привилегированный режим
    КонецЕсли;
КонецПроцедуры

Процедура Тест()
    ДоступРазрешен = РольДоступна("НужнаяРоль");
    ДоступРазрешен = ПР();
    Если ДоступРазрешен Тогда // Нет срабатывания
    КонецЕсли;
КонецПроцедуры

Процедура Тест2()
    Если РольДоступна("НужнаяРоль") Тогда // Срабатывание
    КонецЕсли;
КонецПроцедуры

Процедура Тест3()
    ДоступРазрешен = РольДоступна("НужнаяРоль");
    Если ДоступРазрешен Тогда // Срабатывание
    КонецЕсли;
КонецПроцедуры

Процедура Тест4()
	ЕстьДоступ = РольДоступна("Тест");

	Если Истина Тогда
		ЕстьДоступ = Ложь;
		Если ЕстьДоступ Тогда // Нет срабатывания. Переменная очищена
			Возврат;
		КонецЕсли;
	КонецЕсли;

КонецПроцедуры

Процедура Тест5()
    Если Ложь Тогда
    ИначеЕсли РольДоступна("НужнаяРоль") Тогда // Срабатывание
    КонецЕсли;

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IsInRoleMethod,
            expect![[r#"
            IsInRoleMethod @ 33:10..33:36
              message: Для проверки прав доступа в коде следует использовать метод ПравоДоступа
              severity: Warning
            IsInRoleMethod @ 39:10..39:24
              message: Для проверки прав доступа в коде следует использовать метод ПравоДоступа
              severity: Warning
            IsInRoleMethod @ 57:15..57:41
              message: Для проверки прав доступа в коде следует использовать метод ПравоДоступа
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_direct_call_without_protection() {
        let code = r#"
Процедура Тест()
    Если РольДоступна("Роль") Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IsInRoleMethod,
            expect![[r#"
            IsInRoleMethod @ 3:10..3:30
              message: Для проверки прав доступа в коде следует использовать метод ПравоДоступа
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_direct_call_with_protection() {
        let code = r#"
Процедура Тест()
    Если РольДоступна("Роль") ИЛИ ПривилегированныйРежим() Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::IsInRoleMethod, expect![[r#""#]]);
    }

    #[test]
    fn test_variable_without_protection() {
        let code = r#"
Процедура Тест()
    Доступ = РольДоступна("Роль");
    Если Доступ Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IsInRoleMethod,
            expect![[r#"
            IsInRoleMethod @ 4:10..4:16
              message: Для проверки прав доступа в коде следует использовать метод ПравоДоступа
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_variable_with_protection() {
        let code = r#"
Процедура Тест()
    Доступ = РольДоступна("Роль");
    Если Доступ ИЛИ ПривилегированныйРежим() Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::IsInRoleMethod, expect![[r#""#]]);
    }

    #[test]
    fn test_variable_reassignment_clears_tracking() {
        let code = r#"
Процедура Тест()
    Доступ = РольДоступна("Роль");
    Доступ = Ложь;
    Если Доступ Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::IsInRoleMethod, expect![[r#""#]]);
    }

    #[test]
    fn test_elsif_clause() {
        let code = r#"
Процедура Тест()
    Если Ложь Тогда
    ИначеЕсли РольДоступна("Роль") Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IsInRoleMethod,
            expect![[r#"
            IsInRoleMethod @ 4:15..4:35
              message: Для проверки прав доступа в коде следует использовать метод ПравоДоступа
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Если РОЛЬДОСТУПНА("Роль") Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IsInRoleMethod,
            expect![[r#"
            IsInRoleMethod @ 3:10..3:30
              message: Для проверки прав доступа в коде следует использовать метод ПравоДоступа
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    If IsInRole("Role") Then
    EndIf;
EndProcedure
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IsInRoleMethod,
            expect![[r#"
            IsInRoleMethod @ 3:8..3:24
              message: Для проверки прав доступа в коде следует использовать метод ПравоДоступа
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_privileged_mode_variable() {
        let code = r#"
Процедура Тест()
    ПР = ПривилегированныйРежим();
    Если РольДоступна("Роль") ИЛИ ПР Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::IsInRoleMethod, expect![[r#""#]]);
    }
}
