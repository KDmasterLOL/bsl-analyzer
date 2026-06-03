use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Body, Expr, ExprId, IdConversion, ModItem, Name, Stmt, StmtId};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[
        bsl_metadata::ModuleType::ObjectModule,
        bsl_metadata::ModuleType::RecordSetModule,
        bsl_metadata::ModuleType::ValueManagerModule,
    ],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const MONITORED_PROCEDURES: &[&str] =
    &["передзаписью", "beforewrite", "призаписи", "onwrite", "передудалением", "beforedelete"];

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::DataExchangeLoading;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    if !is_applicable_module(ctx) {
        return Vec::new();
    }

    let find_first =
        ctx.config.get_bool(DiagnosticCode::DataExchangeLoading, "findFirst").unwrap_or(false);

    let item_tree = ctx.item_tree();
    let module_bodies = ctx.module_bodies();

    let mut diagnostics = Vec::new();

    let mut local_id = 0u32;
    for item in item_tree.top_level_items().iter() {
        match item {
            ModItem::Procedure(proc_idx) => {
                let proc = item_tree.procedure(*proc_idx);

                if !is_monitored_procedure(&proc.name) {
                    local_id += 1;
                    continue;
                }

                if let Some(body) = module_bodies.body(local_id) {
                    if !has_guard_pattern(body, find_first) {
                        diagnostics.push(Diagnostic {
                            code: DiagnosticCode::DataExchangeLoading,
                            message: "Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. \
                                      Необходимо добавить проверку для предотвращения выполнения логики при обмене данными"
                                .to_string(),
                            severity: ctx.severity(code),
                            range: proc.name_range,
                            tags: ctx.tags(code),
                            fixes: vec![],
                        });
                    }
                }
                local_id += 1;
            }
            ModItem::Function(_) => {
                local_id += 1;
            }
            ModItem::Variable(_) => {}
        }
    }

    diagnostics
}

fn is_applicable_module(ctx: &DiagnosticsContext) -> bool {
    let file_path = match ctx.file_path() {
        Some(path) => path,
        None => return true,
    };

    match ide_db::metadata::get_module_type_from_uri(&file_path) {
        Some(module_type) => matches!(
            module_type,
            bsl_metadata::ModuleType::ObjectModule
                | bsl_metadata::ModuleType::RecordSetModule
                | bsl_metadata::ModuleType::ValueManagerModule
        ),
        None => true,
    }
}

fn is_monitored_procedure(name: &Name) -> bool {
    let lower_name = name.as_str().to_lowercase();
    MONITORED_PROCEDURES.contains(&lower_name.as_str())
}

fn has_guard_pattern(body: &Body, find_first: bool) -> bool {
    let stmts_to_check: Vec<StmtId> = if find_first {
        body.body_stmts()
            .filter(|&stmt_id| !matches!(body.stmt(stmt_id), Stmt::VarDecl { .. }))
            .take(1)
            .collect()
    } else {
        body.body_stmts().collect()
    };

    for &stmt_id in &stmts_to_check {
        if is_guard_if_statement(body, stmt_id) {
            return true;
        }
    }

    false
}

fn is_guard_if_statement(body: &Body, stmt_id: StmtId) -> bool {
    let stmt = body.stmt(stmt_id);

    match stmt {
        Stmt::If(if_stmt) => {
            if !condition_has_data_exchange_load(body, ExprId::from_idx(if_stmt.condition)) {
                return false;
            }

            let then_branch_ids: Vec<StmtId> =
                if_stmt.then_branch.iter().map(|&idx| StmtId::from_idx(idx)).collect();
            has_return_in_branch(body, &then_branch_ids)
        }
        _ => false,
    }
}

fn condition_has_data_exchange_load(body: &Body, expr_id: ExprId) -> bool {
    let expr = body.expr(expr_id);

    match expr {
        Expr::Field { base, field } => {
            if is_data_exchange_load_field(body, ExprId::from_idx(*base), field) {
                return true;
            }
            condition_has_data_exchange_load(body, ExprId::from_idx(*base))
        }

        Expr::BinaryOp { lhs, rhs, .. } => {
            condition_has_data_exchange_load(body, ExprId::from_idx(*lhs))
                || condition_has_data_exchange_load(body, ExprId::from_idx(*rhs))
        }

        Expr::UnaryOp { expr, .. } => {
            condition_has_data_exchange_load(body, ExprId::from_idx(*expr))
        }

        _ => false,
    }
}

fn is_data_exchange_load_field(body: &Body, base_id: ExprId, field: &Name) -> bool {
    let field_lower = field.as_str().to_lowercase();
    if field_lower != "загрузка" && field_lower != "load" {
        return false;
    }

    let base_expr = body.expr(base_id);
    match base_expr {
        Expr::Path(base_name) => {
            let base_lower = base_name.as_str().to_lowercase();
            base_lower == "обменданными" || base_lower == "dataexchange"
        }
        _ => false,
    }
}

fn has_return_in_branch(body: &Body, stmts: &[StmtId]) -> bool {
    for &stmt_id in stmts {
        if has_return_anywhere(body, stmt_id) {
            return true;
        }
    }
    false
}

fn has_return_anywhere(body: &Body, stmt_id: StmtId) -> bool {
    let stmt = body.stmt(stmt_id);
    match stmt {
        Stmt::Return { .. } => true,
        Stmt::If(if_stmt) => {
            if_stmt.then_branch.iter().any(|&s| has_return_anywhere(body, StmtId::from_idx(s)))
                || if_stmt.elsif_branches.iter().any(|(_, branch)| {
                    branch.iter().any(|&s| has_return_anywhere(body, StmtId::from_idx(s)))
                })
                || if_stmt
                    .else_branch
                    .as_ref()
                    .map(|b| b.iter().any(|&s| has_return_anywhere(body, StmtId::from_idx(s))))
                    .unwrap_or(false)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic, check_ast_diagnostic_with_config, format_diags};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;
    #[test]
    fn test_basic_missing_guard() {
        let code = r#"
Процедура ПередЗаписью(Отказ)
    ВыполнитьЧтоТо();
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DataExchangeLoading @ 2:11..2:23
              message: Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. Необходимо добавить проверку для предотвращения выполнения логики при обмене данными
              severity: Critical"#]].assert_eq(&format_diags(code, &diagnostics));
        assert_eq!(diagnostics[0].code, DiagnosticCode::DataExchangeLoading);
        assert_eq!(diagnostics[0].severity, crate::Severity::Critical);
    }

    #[test]
    fn test_valid_guard_russian() {
        let code = r#"
Процедура ПередЗаписью(Отказ)
    Если ОбменДанными.Загрузка Тогда
        Возврат;
    КонецЕсли;
    ВыполнитьЧтоТо();
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_valid_guard_english() {
        let code = r#"
Procedure BeforeWrite(Cancel)
    If DataExchange.Load Then
        Return;
    EndIf;
    DoSomething();
EndProcedure
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_guard_without_return() {
        let code = r#"
Процедура ПередЗаписью(Отказ)
    Если ОбменДанными.Загрузка Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DataExchangeLoading @ 2:11..2:23
              message: Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. Необходимо добавить проверку для предотвращения выполнения логики при обмене данными
              severity: Critical"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_non_monitored_procedure() {
        let code = r#"
Процедура ОбычнаяПроцедура()
    ВыполнитьЧтоТо();
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
ПРОЦЕДУРА ПЕРЕДЗАПИСЬЮ(Отказ)
    ЕСЛИ ОБМЕНДАННЫМИ.ЗАГРУЗКА ТОГДА
        ВОЗВРАТ;
    КОНЕЦЕСЛИ;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_complex_condition() {
        let code = r#"
Процедура ПередЗаписью(Отказ)
    Если ОбменДанными.Загрузка Или ПропуститьПроверки Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_missing_guard_wrong_condition() {
        let code = r#"Процедура ПередЗаписью(Отказ)
    Если Отказ Тогда
        Сообщить("Это отказ");
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DataExchangeLoading @ 1:11..1:23
              message: Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. Необходимо добавить проверку для предотвращения выполнения логики при обмене данными
              severity: Critical"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_missing_guard_wrong_field() {
        let code = r#"Procedure OnWrite(Cancel)
    Var Value;
    If DataExchange.Recipients Then
        Return;
    EndIf;
EndProcedure"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DataExchangeLoading @ 1:11..1:18
              message: Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. Необходимо добавить проверку для предотвращения выполнения логики при обмене данными
              severity: Critical"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_guard_without_return_in_body() {
        let code = r#"Процедура ПередЗаписью(Отказ, РежимЗаписи, РежимПроведения)

    Если ОбменДанными.Загрузка Тогда
    КонецЕсли;

КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DataExchangeLoading @ 1:11..1:23
              message: Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. Необходимо добавить проверку для предотвращения выполнения логики при обмене данными
              severity: Critical"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_find_first_before_delete_triggers() {
        let code = r#"Procedure BeforeDelete(Cancel)
    For Each Item in new Array Do
        Return;
    EndDo;

    If DataExchange.Load Then
        Return;
    EndIf;
EndProcedure"#;
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::DataExchangeLoading, serde_json::json!({"findFirst": true}));
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#"
            DataExchangeLoading @ 1:11..1:23
              message: Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. Необходимо добавить проверку для предотвращения выполнения логики при обмене данными
              severity: Critical"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_find_first_before_delete_ok_with_find_first_false() {
        let code = r#"Procedure BeforeDelete(Cancel)
    For Each Item in new Array Do
        Return;
    EndDo;

    If DataExchange.Load Then
        Return;
    EndIf;
EndProcedure"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_valid_guard_with_nested_logic() {
        let code = r#"Процедура ПриЗаписи(Отказ)
    Если ОбменДанными.Загрузка Тогда
        Если Не ДополнительныеСвойства.Свойство("Пропустить") Тогда
            ДатыЗапретаИзмененияСлужебный.ОбновитьВерсию(ЭтотОбъект);
        КонецЕсли;
        Очистить();
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_valid_negated_guard() {
        let code = r#"Процедура ПередЗаписью(Отказ, РежимЗаписи, РежимПроведения)

    Если НЕ ОбменДанными.Загрузка И ТребуетсяКонтрольЗаписи Тогда
        Отказ = Истина;
        Возврат;
    КонецЕсли;

КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }
}
