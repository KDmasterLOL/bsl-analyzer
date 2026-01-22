//! DataExchangeLoading diagnostic.
//!
//! Detects missing data exchange guards in event handlers.
//!
//! ## Why?
//! Event handlers (BeforeWrite, OnWrite, BeforeDelete) in object modules must check
//! `ОбменДанными.Загрузка` (DataExchange.Load) property to prevent business logic
//! execution during data exchange synchronization. Without this guard, data exchange
//! can fail or produce incorrect results.
//!
//! ## Bad practice
//! ```bsl
//! Процедура ПередЗаписью(Отказ)
//!     // Business logic without guard - ERROR!
//!     ВыполнитьПроверку();
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура ПередЗаписью(Отказ)
//!     Если ОбменДанными.Загрузка Тогда
//!         Возврат;
//!     КонецЕсли;
//!     ВыполнитьПроверку();
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **findFirst** (boolean, default: false) - Only check first statement if true
//! - **Enabled by default:** Yes
//! - **Severity:** CRITICAL
//! - **Scope:** ObjectModule, RecordSetModule, ValueManagerModule
//! - **Tags:** STANDARD, BADPRACTICE, UNPREDICTABLE
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! Ported from:
//! - DataExchangeLoadingDiagnostic.java (bsl-language-server) - Java reference
//! - data_exchange_loading.rs (bsl-language-server-rust) - Rust reference

use cfg_types::IdConversion;

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::hir_def::{item_tree::ModItem, Body, Expr, ExprId, Name, Stmt, StmtId};

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

    // Use HIR queries instead of AST parsing
    let item_tree = ctx.item_tree();
    let module_bodies = ctx.module_bodies();

    let mut diagnostics = Vec::new();

    // Iterate over all procedures in the module
    // local_id counts only Procedures and Functions (not Variables)
    let mut local_id = 0u32;
    for item in item_tree.top_level_items().iter() {
        match item {
            ModItem::Procedure(proc_idx) => {
                let proc = item_tree.procedure(*proc_idx);

                // Check if this is a monitored procedure
                if !is_monitored_procedure(&proc.name) {
                    local_id += 1; // Increment even for non-monitored procedures
                    continue;
                }

                // Get HIR body and check for guard pattern
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
                local_id += 1; // Increment after processing procedure
            }
            ModItem::Function(_) => {
                local_id += 1; // Functions also count toward local_id
            }
            ModItem::Variable(_) => {
                // Variables don't count toward local_id
            }
        }
    }

    diagnostics
}

fn is_applicable_module(ctx: &DiagnosticsContext) -> bool {
    let file_path = match ctx.file_path() {
        Some(path) => path,
        // No source root - assume test environment, allow check
        None => return true,
    };

    match ide_db::metadata::get_module_type_from_uri(&file_path) {
        Some(module_type) => matches!(
            module_type,
            bsl_metadata::ModuleType::ObjectModule
                | bsl_metadata::ModuleType::RecordSetModule
                | bsl_metadata::ModuleType::ValueManagerModule
        ),
        // Module type unknown - allow check (could be test or standalone file)
        None => true,
    }
}

fn is_monitored_procedure(name: &Name) -> bool {
    let lower_name = name.as_str().to_lowercase();
    MONITORED_PROCEDURES.contains(&lower_name.as_str())
}

/// Check if HIR body has the DataExchange.Load guard pattern.
fn has_guard_pattern(body: &Body, find_first: bool) -> bool {
    // Determine how many statements to check
    // With findFirst=true, we need to skip Var declarations and check first executable statement
    let stmts_to_check: Vec<StmtId> = if find_first {
        // Skip Var declarations and take first non-var statement
        body.body_stmts()
            .filter(|&stmt_id| !matches!(body.stmt(stmt_id), Stmt::VarDecl { .. }))
            .take(1)
            .collect()
    } else {
        body.body_stmts().collect()
    };

    // Check statements for guard pattern
    for &stmt_id in &stmts_to_check {
        if is_guard_if_statement(body, stmt_id) {
            return true;
        }
    }

    false
}

/// Check if HIR statement is an IF with DataExchange.Load guard pattern.
fn is_guard_if_statement(body: &Body, stmt_id: StmtId) -> bool {
    let stmt = body.stmt(stmt_id);

    match stmt {
        Stmt::If(if_stmt) => {
            // Check condition contains DataExchange.Load
            if !condition_has_data_exchange_load(body, ExprId::from_idx(if_stmt.condition)) {
                return false;
            }

            // Check then_branch has Return
            // Convert &[StmtIdx] to Vec<StmtId>
            let then_branch_ids: Vec<StmtId> =
                if_stmt.then_branch.iter().map(|&idx| StmtId::from_idx(idx)).collect();
            has_return_in_branch(body, &then_branch_ids)
        }
        _ => false,
    }
}

/// Recursively check if HIR expression contains DataExchange.Load pattern.
/// NOTE: Java implementation checks for any mention of DataExchange.Load in condition,
/// even if negated. The guard is valid as long as there's a Return in then_branch.
fn condition_has_data_exchange_load(body: &Body, expr_id: ExprId) -> bool {
    let expr = body.expr(expr_id);

    match expr {
        // Direct field access: ОбменДанными.Загрузка
        Expr::Field { base, field } => {
            if is_data_exchange_load_field(body, ExprId::from_idx(*base), field) {
                return true;
            }
            // Also check nested fields
            condition_has_data_exchange_load(body, ExprId::from_idx(*base))
        }

        // Binary operators (И/OR) - check both sides
        Expr::BinaryOp { lhs, rhs, .. } => {
            condition_has_data_exchange_load(body, ExprId::from_idx(*lhs))
                || condition_has_data_exchange_load(body, ExprId::from_idx(*rhs))
        }

        // Unary operators (НЕ/NOT) - check inner expression
        Expr::UnaryOp { expr, .. } => {
            condition_has_data_exchange_load(body, ExprId::from_idx(*expr))
        }

        _ => false,
    }
}

/// Check if base.field matches DataExchange.Load pattern.
fn is_data_exchange_load_field(body: &Body, base_id: ExprId, field: &Name) -> bool {
    // Check field name (case-insensitive)
    let field_lower = field.as_str().to_lowercase();
    if field_lower != "загрузка" && field_lower != "load" {
        return false;
    }

    // Check base is "ОбменДанными" or "DataExchange"
    let base_expr = body.expr(base_id);
    match base_expr {
        Expr::Path(base_name) => {
            let base_lower = base_name.as_str().to_lowercase();
            base_lower == "обменданными" || base_lower == "dataexchange"
        }
        _ => false,
    }
}

/// Check if branch contains Return statement.
/// For DataExchangeLoading guard pattern, match Java behavior:
/// The guard pattern should be simple: just a Return statement, possibly with other
/// simple statements but Return should be present.
/// However, Java seems to accept any Return in the branch.
fn has_return_in_branch(body: &Body, stmts: &[StmtId]) -> bool {
    // Java implementation uses descendants().any(Return), which finds Return anywhere
    // But based on test failures, it seems Java may have stricter requirements
    // Let's check if Return exists anywhere in the statements
    for &stmt_id in stmts {
        if has_return_anywhere(body, stmt_id) {
            return true;
        }
    }
    false
}

/// Recursively check if statement or its children contain Return.
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
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};

    #[test]
    fn test_basic_missing_guard() {
        let code = r#"
Процедура ПередЗаписью(Отказ)
    ВыполнитьЧтоТо();
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect missing guard in event handler");
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
        assert_eq!(diagnostics.len(), 0, "Valid guard should not report");
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
        assert_eq!(diagnostics.len(), 0, "Valid English guard should not report");
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
        assert_eq!(diagnostics.len(), 1, "Guard without return should report");
    }

    #[test]
    fn test_non_monitored_procedure() {
        let code = r#"
Процедура ОбычнаяПроцедура()
    ВыполнитьЧтоТо();
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should ignore non-monitored procedures");
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
        assert_eq!(diagnostics.len(), 0, "Should handle case-insensitive keywords");
    }

    #[test]
    fn test_complex_condition() {
        let code = r#"
Процедура ПередЗаписью(Отказ)
    Если ОбменДанными.Загрузка Или ДополнительныеСвойства.Свойство("НеПроверятьУникальность") Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Complex condition with DataExchange.Load should be valid"
        );
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/DataExchangeLoadingDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            3,
            "Should match Java implementation (3 diagnostics with findFirst=false)"
        );

        assert_diagnostic_range(code, &diagnostics[0], 7, 10, 22);
        assert_diagnostic_range(code, &diagnostics[1], 19, 10, 17);
        assert_diagnostic_range(code, &diagnostics[2], 70, 10, 22);
    }

    #[test]
    fn test_find_first_parameter() {
        let code = include_str!("../../test_data/DataExchangeLoadingDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::DataExchangeLoading, serde_json::json!({"findFirst": true}));

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert_eq!(diagnostics.len(), 4, "Should find 4 diagnostics with findFirst=true");
    }
}
