use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Body, Expr, ExprId, IdConversion, Name, Stmt, StmtId};
use stdx::case::CaseExt;

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

/// Library functions whose body is a `ОбменДанными.Загрузка`-derived check, so
/// guarding the handler with them is equivalent to the literal check. The БСП/ЗУП
/// wrapper is recognised out of the box; projects extend the list through the
/// `guardWrappers` parameter.
const DEFAULT_GUARD_WRAPPERS: &[&str] = &["ЗарплатаКадры.ОтключитьБизнесЛогикуПриЗаписи"];

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

    let guard_wrappers: Vec<(String, String)> = ctx
        .config
        .get_string_array(code, "guardWrappers")
        .unwrap_or_else(|| DEFAULT_GUARD_WRAPPERS.iter().map(|s| s.to_string()).collect())
        .iter()
        .filter_map(|entry| {
            let (module, function) = entry.split_once('.')?;
            Some((module.trim().fold_lower(), function.trim().fold_lower()))
        })
        .collect();

    let item_tree = ctx.item_tree();
    let module_bodies = ctx.module_bodies();

    let mut diagnostics = Vec::new();

    for proc in item_tree.methods().filter(|m| !m.is_function()) {
        if !is_monitored_procedure(proc.name()) {
            continue;
        }
        let Some(body) = module_bodies.body(proc.key()) else { continue };
        if !has_guard_pattern(body, find_first, &guard_wrappers) {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::DataExchangeLoading,
                message: "Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. \
                          Необходимо добавить проверку для предотвращения выполнения логики при обмене данными"
                    .to_string(),
                severity: ctx.severity(code),
                range: proc.name_range(),
                tags: ctx.tags(code),
                fixes: vec![],
            });
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
    let lower_name = name.as_str().fold_lower();
    MONITORED_PROCEDURES.contains(&lower_name.as_str())
}

fn has_guard_pattern(body: &Body, find_first: bool, wrappers: &[(String, String)]) -> bool {
    let stmts_to_check: Vec<StmtId> = if find_first {
        body.body_stmts()
            .filter(|&stmt_id| !matches!(body.stmt(stmt_id), Stmt::VarDecl { .. }))
            .take(1)
            .collect()
    } else {
        body.body_stmts().collect()
    };

    for &stmt_id in &stmts_to_check {
        if is_guard_if_statement(body, stmt_id, wrappers) {
            return true;
        }
    }

    false
}

fn is_guard_if_statement(body: &Body, stmt_id: StmtId, wrappers: &[(String, String)]) -> bool {
    let stmt = body.stmt(stmt_id);

    match stmt {
        Stmt::If(if_stmt) => {
            if !condition_has_data_exchange_load(
                body,
                ExprId::from_idx(if_stmt.condition),
                wrappers,
            ) {
                return false;
            }

            let then_branch_ids: Vec<StmtId> =
                if_stmt.then_branch.iter().map(|&idx| StmtId::from_idx(idx)).collect();
            has_return_in_branch(body, &then_branch_ids)
        }
        _ => false,
    }
}

fn condition_has_data_exchange_load(
    body: &Body,
    expr_id: ExprId,
    wrappers: &[(String, String)],
) -> bool {
    let expr = body.expr(expr_id);

    match expr {
        Expr::Field { base, field } => {
            if is_data_exchange_load_field(body, ExprId::from_idx(*base), field) {
                return true;
            }
            condition_has_data_exchange_load(body, ExprId::from_idx(*base), wrappers)
        }

        Expr::BinaryOp { lhs, rhs, .. } => {
            condition_has_data_exchange_load(body, ExprId::from_idx(*lhs), wrappers)
                || condition_has_data_exchange_load(body, ExprId::from_idx(*rhs), wrappers)
        }

        Expr::UnaryOp { expr, .. } => {
            condition_has_data_exchange_load(body, ExprId::from_idx(*expr), wrappers)
        }

        // `Модуль.Функция(...)` lowers to Call with a Field callee; requiring a
        // plain Path base keeps nested receivers (`А.Б.Функция()`) from matching
        // a `Б.Функция` wrapper entry.
        Expr::Call { callee, .. } => {
            if let Expr::Field { base, field } = body.expr(ExprId::from_idx(*callee)) {
                is_guard_wrapper_call(body, ExprId::from_idx(*base), field, wrappers)
            } else {
                false
            }
        }

        _ => false,
    }
}

fn is_guard_wrapper_call(
    body: &Body,
    receiver_id: ExprId,
    method: &Name,
    wrappers: &[(String, String)],
) -> bool {
    let Expr::Path(module_name) = body.expr(receiver_id) else {
        return false;
    };
    let module_lower = module_name.as_str().fold_lower();
    let method_lower = method.as_str().fold_lower();
    wrappers.iter().any(|(module, function)| *module == module_lower && *function == method_lower)
}

fn is_data_exchange_load_field(body: &Body, base_id: ExprId, field: &Name) -> bool {
    let field_lower = field.as_str().fold_lower();
    if field_lower != "загрузка" && field_lower != "load" {
        return false;
    }

    let base_expr = body.expr(base_id);
    match base_expr {
        Expr::Path(base_name) => {
            let base_lower = base_name.as_str().fold_lower();
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

    /// A module variable above the handler must not hide it: the bodies are
    /// keyed by every top-level item, variables included.
    #[test]
    fn top_level_variable_above_handler_does_not_hide_missing_guard() {
        let code = r#"
Перем КэшНастроек;

Процедура ПередЗаписью(Отказ)
    ВыполнитьЧтоТо();
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DataExchangeLoading @ 4:11..4:23
              message: Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. Необходимо добавить проверку для предотвращения выполнения логики при обмене данными
              severity: Critical"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    /// With a variable above, the unguarded handler must be reported and the
    /// guarded one next to it must not inherit its neighbour's body.
    #[test]
    fn top_level_variable_above_handlers_does_not_misattribute_bodies() {
        let code = r#"
Перем КэшНастроек;

Процедура ПередЗаписью(Отказ)
    ВыполнитьЧтоТо();
КонецПроцедуры

Процедура ПриЗаписи(Отказ)
    Если ОбменДанными.Загрузка Тогда
        Возврат;
    КонецЕсли;
    ВыполнитьЧтоТо();
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DataExchangeLoading @ 4:11..4:23
              message: Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. Необходимо добавить проверку для предотвращения выполнения логики при обмене данными
              severity: Critical"#]]
        .assert_eq(&format_diags(code, &diagnostics));
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
    fn test_default_wrapper_guard_ok() {
        let code = r#"Процедура ПередЗаписью(Отказ, РежимЗаписи, РежимПроведения)
    Если ЗарплатаКадры.ОтключитьБизнесЛогикуПриЗаписи(ЭтотОбъект) Тогда
        Возврат;
    КонецЕсли;
    ВыполнитьЧтоТо();
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_wrapper_without_return_still_flags() {
        let code = r#"Процедура ПередЗаписью(Отказ)
    Если ЗарплатаКадры.ОтключитьБизнесЛогикуПриЗаписи(ЭтотОбъект) Тогда
        Сообщить("Загрузка");
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DataExchangeLoading @ 1:11..1:23
              message: Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. Необходимо добавить проверку для предотвращения выполнения логики при обмене данными
              severity: Critical"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_same_function_on_other_module_still_flags() {
        let code = r#"Процедура ПередЗаписью(Отказ)
    Если МойМодуль.ОтключитьБизнесЛогикуПриЗаписи(ЭтотОбъект) Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DataExchangeLoading @ 1:11..1:23
              message: Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. Необходимо добавить проверку для предотвращения выполнения логики при обмене данными
              severity: Critical"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_custom_wrapper_from_config() {
        let code = r#"Процедура ПередЗаписью(Отказ)
    Если Обмен.ЭтоЗагрузка(ЭтотОбъект) Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::DataExchangeLoading,
            serde_json::json!({"guardWrappers": ["Обмен.ЭтоЗагрузка"]}),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_custom_wrapper_list_replaces_default() {
        let code = r#"Процедура ПередЗаписью(Отказ)
    Если ЗарплатаКадры.ОтключитьБизнесЛогикуПриЗаписи(ЭтотОбъект) Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::DataExchangeLoading,
            serde_json::json!({"guardWrappers": ["Обмен.ЭтоЗагрузка"]}),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#"
            DataExchangeLoading @ 1:11..1:23
              message: Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. Необходимо добавить проверку для предотвращения выполнения логики при обмене данными
              severity: Critical"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_wrapper_in_disjunction_ok() {
        let code = r#"Процедура ПриЗаписи(Отказ)
    Если ЗарплатаКадры.ОтключитьБизнесЛогикуПриЗаписи(ЭтотОбъект) Или ПропуститьПроверки Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_unrelated_call_in_condition_still_flags() {
        let code = r#"Процедура ПередЗаписью(Отказ)
    Если ПроверитьЧтоТо(ЭтотОбъект) Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DataExchangeLoading @ 1:11..1:23
              message: Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. Необходимо добавить проверку для предотвращения выполнения логики при обмене данными
              severity: Critical"#]]
        .assert_eq(&format_diags(code, &diagnostics));
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
