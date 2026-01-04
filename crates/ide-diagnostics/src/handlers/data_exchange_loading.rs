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

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

const MONITORED_PROCEDURES: &[&str] =
    &["передзаписью", "beforewrite", "призаписи", "onwrite", "передудалением", "beforedelete"];

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::DataExchangeLoading) {
        return Vec::new();
    }

    if !is_applicable_module(ctx) {
        return Vec::new();
    }

    let find_first =
        ctx.config.get_bool(DiagnosticCode::DataExchangeLoading, "findFirst").unwrap_or(false);

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    check_procedures_in_tree(&root, find_first)
}

fn is_applicable_module(ctx: &DiagnosticsContext) -> bool {
    let file_path = match ctx.file_path() {
        Some(path) => path,
        None => return false, // No source root - skip check (not in configuration)
    };

    match ide_db::metadata::get_module_type_from_uri(&file_path) {
        Some(module_type) => matches!(
            module_type,
            bsl_metadata::ModuleType::ObjectModule
                | bsl_metadata::ModuleType::RecordSetModule
                | bsl_metadata::ModuleType::ValueManagerModule
        ),
        None => false, // Module type unknown - skip check (not in supported module types)
    }
}

fn check_procedure(node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>, find_first: bool) {
    let name_token = match get_procedure_name(node) {
        Some(token) => token,
        None => return,
    };

    if !is_monitored_procedure(name_token.text()) {
        return;
    }

    let body = match node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST) {
        Some(b) => b,
        None => return,
    };

    if !has_data_exchange_guard(&body, find_first) {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::DataExchangeLoading,
            message: "Отсутствует проверка условия ОбменДанными.Загрузка в обработчике события. \
                      Необходимо добавить проверку для предотвращения выполнения логики при обмене данными"
                .to_string(),
            severity: Severity::Critical,
            range: name_token.text_range(),
            tags: vec![],
            fixes: vec![],
        });
    }
}

fn check_procedures_in_tree(root: &SyntaxNode, find_first: bool) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() == SyntaxKind::PROCEDURE_DEF {
            check_procedure(&node, &mut diagnostics, find_first);
        }
    }

    diagnostics
}

fn get_procedure_name(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .take_while(|el| el.as_node().map(|n| n.kind() != SyntaxKind::PARAM_LIST).unwrap_or(true))
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)
}

fn is_monitored_procedure(name: &str) -> bool {
    let lower_name = name.to_lowercase();
    MONITORED_PROCEDURES.contains(&lower_name.as_str())
}

fn has_data_exchange_guard(body: &SyntaxNode, find_first: bool) -> bool {
    let statements: Vec<_> = body
        .children()
        .filter(|n| {
            matches!(
                n.kind(),
                SyntaxKind::ASSIGN_STMT
                    | SyntaxKind::CALL_STMT
                    | SyntaxKind::RETURN_STMT
                    | SyntaxKind::IF_STMT
                    | SyntaxKind::WHILE_STMT
                    | SyntaxKind::FOR_STMT
                    | SyntaxKind::FOR_EACH_STMT
                    | SyntaxKind::TRY_STMT
                    | SyntaxKind::RAISE_STMT
                    | SyntaxKind::EXECUTE_STMT
                    | SyntaxKind::BREAK_STMT
                    | SyntaxKind::CONTINUE_STMT
                    | SyntaxKind::GOTO_STMT
                    | SyntaxKind::LABEL_STMT
                    | SyntaxKind::ADD_HANDLER_STMT
                    | SyntaxKind::REMOVE_HANDLER_STMT
                    | SyntaxKind::EMPTY_STMT
            )
        })
        .collect();

    let limit = if find_first { 1 } else { statements.len() };

    for stmt in statements.into_iter().take(limit) {
        if stmt.kind() == SyntaxKind::IF_STMT && is_if_with_guard(&stmt) {
            return true;
        }
    }

    false
}

fn is_if_with_guard(if_stmt: &SyntaxNode) -> bool {
    let condition = if_stmt.children().find(|n| {
        matches!(n.kind(), SyntaxKind::EXPR | SyntaxKind::BINARY_EXPR | SyntaxKind::CALL_EXPR)
    });

    let condition = match condition {
        Some(c) => c,
        None => return false,
    };

    if !condition_contains_data_exchange_load(&condition) {
        return false;
    }

    let if_body = if_stmt.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);

    match if_body {
        Some(body) => has_return_statement(&body),
        None => false,
    }
}

fn condition_contains_data_exchange_load(condition: &SyntaxNode) -> bool {
    let text = condition.text().to_string().to_lowercase();
    let normalized = text.chars().filter(|c| !c.is_whitespace()).collect::<String>();

    normalized.contains("обменданными.загрузка") || normalized.contains("dataexchange.load")
}

fn has_return_statement(if_body: &SyntaxNode) -> bool {
    if_body.descendants().any(|n| n.kind() == SyntaxKind::RETURN_STMT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    fn check_diagnostic(code: &str, find_first: bool) -> (Vec<Diagnostic>, String) {
        let parse = parser::parse(code);
        let root = parse.syntax_node();
        let diagnostics = check_procedures_in_tree(&root, find_first);
        (diagnostics, code.to_string())
    }

    #[test]
    fn test_basic_missing_guard() {
        let code = r#"
Процедура ПередЗаписью(Отказ)
    ВыполнитьЧтоТо();
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code, false);
        assert_eq!(diagnostics.len(), 1, "Should detect missing guard in event handler");
        assert_eq!(diagnostics[0].code, DiagnosticCode::DataExchangeLoading);
        assert_eq!(diagnostics[0].severity, Severity::Critical);
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
        let (diagnostics, _) = check_diagnostic(code, false);
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
        let (diagnostics, _) = check_diagnostic(code, false);
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
        let (diagnostics, _) = check_diagnostic(code, false);
        assert_eq!(diagnostics.len(), 1, "Guard without return should report");
    }

    #[test]
    fn test_non_monitored_procedure() {
        let code = r#"
Процедура ОбычнаяПроцедура()
    ВыполнитьЧтоТо();
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code, false);
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
        let (diagnostics, _) = check_diagnostic(code, false);
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
        let (diagnostics, _) = check_diagnostic(code, false);
        assert_eq!(
            diagnostics.len(),
            0,
            "Complex condition with DataExchange.Load should be valid"
        );
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/DataExchangeLoadingDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code, false);

        assert_eq!(
            diagnostics.len(),
            3,
            "Should match Java implementation (3 diagnostics with findFirst=false)"
        );

        assert_diagnostic_range(&file_content, &diagnostics[0], 7, 10, 22);
        assert_diagnostic_range(&file_content, &diagnostics[1], 19, 10, 17);
        assert_diagnostic_range(&file_content, &diagnostics[2], 70, 10, 22);
    }

    #[test]
    fn test_find_first_parameter() {
        let code = include_str!("../../test_data/DataExchangeLoadingDiagnostic.bsl");
        let (diagnostics, _) = check_diagnostic(code, true);

        assert_eq!(diagnostics.len(), 4, "Should find 4 diagnostics with findFirst=true");
    }
}
