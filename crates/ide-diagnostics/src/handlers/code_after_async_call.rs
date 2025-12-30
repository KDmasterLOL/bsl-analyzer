//! CodeAfterAsyncCall diagnostic.
//!
//! Detects code that executes immediately after asynchronous method calls in BSL.
//!
//! ## Why?
//! When using asynchronous methods in 1C:Enterprise client-side code, developers sometimes
//! make the mistake of writing code immediately after an async call. This code executes
//! synchronously without waiting for the async operation to complete, leading to logic errors.
//!
//! Asynchronous methods return immediately and execute in the background. Any code after the
//! async call will execute BEFORE the async operation completes. To properly handle async
//! results, you must use callback functions (`ОписаниеОповещения`/`NotifyDescription`) or
//! async/await patterns.
//!
//! ## Bad practice
//! ```bsl
//! &НаКлиенте
//! Процедура Команда1(Команда)
//!     ДополнительныеПараметры = Новый Структура("Результат", 10);
//!     Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества", ЭтотОбъект);
//!     ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
//!
//!     Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат); // ERROR! Always shows 10
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! Move code that depends on async results into the callback function:
//! ```bsl
//! &НаКлиенте
//! Процедура Команда1(Команда)
//!     ДополнительныеПараметры = Новый Структура("Результат", 10);
//!     Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества", ЭтотОбъект);
//!     ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
//! КонецПроцедуры
//!
//! &НаКлиенте
//! Процедура ПослеВводаКоличества(Число, ДополнительныеПараметры) Экспорт
//!     Если Число <> Неопределено Тогда
//!         ДополнительныеПараметры.Результат = Число;
//!         Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат); // Correct!
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! Or use async/await:
//! ```bsl
//! &НаКлиенте
//! Асинх Процедура Команда1(Команда)
//!     Число = Ждать ПоказатьВводЧислаАсинх(1, "Введите количество", 10, 2);
//!     Если Число <> Неопределено Тогда
//!         Сообщить("Введенное количество равно " + Число); // Correct with async/await
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** No (must be enabled via config)
//! - **Severity:** Warning
//! - **Tags:** SUSPICIOUS
//! - **Minutes to fix:** 10
//!
//! ## Implementation
//!
//! Ported from:
//! - code_after_async_call.rs (bsl-language-server-rust) - PRIMARY
//! - CodeAfterAsyncCallDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode};

/// List of asynchronous methods that trigger this diagnostic.
///
/// Contains 50 methods (25 Russian + 25 English):
/// - Dialog methods: ShowQueryBox/ПоказатьВопрос, ShowValue/ПоказатьЗначение, etc.
/// - Input methods: ShowInputNumber/ПоказатьВводЧисла, etc.
/// - File operations: BeginPutFile/НачатьПомещениеФайла, etc.
/// - Extension operations: BeginInstallAddIn/НачатьУстановкуВнешнейКомпоненты, etc.
/// - Directory operations: BeginGettingTempFilesDir/НачатьПолучениеКаталогаВременныхФайлов, etc.
/// - Other: BeginRequestingUserPermission/НачатьЗапросРазрешенияПользователя, etc.
const ASYNC_METHODS: &[&str] = &[
    // Russian names (25)
    "ПОКАЗАТЬВОПРОС",
    "ПОКАЗАТЬЗНАЧЕНИЕ",
    "ПОКАЗАТЬПРЕДУПРЕЖДЕНИЕ",
    "ПОКАЗАТЬВВОДДАТЫ",
    "ПОКАЗАТЬВВОДЗНАЧЕНИЯ",
    "ПОКАЗАТЬВВОДСТРОКИ",
    "ПОКАЗАТЬВВОДЧИСЛА",
    "НАЧАТЬУСТАНОВКУВНЕШНЕЙКОМПОНЕНТЫ",
    "НАЧАТЬУСТАНОВКУРАСШИРЕНИЯРАБОТЫСФАЙЛАМИ",
    "НАЧАТЬУСТАНОВКУРАСШИРЕНИЯРАБОТЫСКРИПТОГРАФИЕЙ",
    "НАЧАТЬПОДКЛЮЧЕНИЕРАСШИРЕНИЯРАБОТЫСКРИПТОГРАФИЕЙ",
    "НАЧАТЬПОДКЛЮЧЕНИЕРАСШИРЕНИЯРАБОТЫСФАЙЛАМИ",
    "НАЧАТЬПОМЕЩЕНИЕФАЙЛА",
    "НАЧАТЬКОПИРОВАНИЕФАЙЛА",
    "НАЧАТЬПЕРЕМЕЩЕНИЕФАЙЛА",
    "НАЧАТЬПОИСКФАЙЛОВ",
    "НАЧАТЬУДАЛЕНИЕФАЙЛОВ",
    "НАЧАТЬСОЗДАНИЕКАТАЛОГА",
    "НАЧАТЬПОЛУЧЕНИЕКАТАЛОГАВРЕМЕННЫХФАЙЛОВ",
    "НАЧАТЬПОЛУЧЕНИЕКАТАЛОГАДОКУМЕНТОВ",
    "НАЧАТЬПОЛУЧЕНИЕРАБОЧЕГОКАТАЛОГАДАННЫХПОЛЬЗОВАТЕЛЯ",
    "НАЧАТЬПОЛУЧЕНИЕФАЙЛОВ",
    "НАЧАТЬПОМЕЩЕНИЕФАЙЛОВ",
    "НАЧАТЬЗАПРОСРАЗРЕШЕНИЯПОЛЬЗОВАТЕЛЯ",
    "НАЧАТЬЗАПУСКПРИЛОЖЕНИЯ",
    // English names (25)
    "SHOWQUERYBOX",
    "SHOWVALUE",
    "SHOWMESSAGEBOX",
    "SHOWINPUTDATE",
    "SHOWINPUTVALUE",
    "SHOWINPUTSTRING",
    "SHOWINPUTNUMBER",
    "BEGININSTALLADDIN",
    "BEGININSTALLFILESYSTEMEXTENSION",
    "BEGININSTALLCRYPTOEXTENSION",
    "BEGINATTACHINGCRYPTOEXTENSION",
    "BEGINATTACHINGFILESYSTEMEXTENSION",
    "BEGINPUTFILE",
    "BEGINCOPYINGFILE",
    "BEGINMOVINGFILE",
    "BEGINFINDINGFILES",
    "BEGINDELETINGFILES",
    "BEGINCREATINGDIRECTORY",
    "BEGINGETTINGTEMPFILESDIR",
    "BEGINGETTINGDOCUMENTSDIR",
    "BEGINGETTINGUSERDATAWORKDIR",
    "BEGINGETTINGFILES",
    "BEGINPUTTINGFILES",
    "BEGINREQUESTINGUSERPERMISSION",
    "BEGINRUNNINGAPPLICATION",
];

/// Main entry point for CodeAfterAsyncCall diagnostic.
///
/// Detects when code executes immediately after asynchronous method calls.
/// This is a logic error because async methods return immediately without waiting
/// for the operation to complete.
///
/// Detection algorithm:
/// 1. Find all async method calls in the syntax tree
/// 2. For each async call, check if there's executable code after it
/// 3. Skip if the code after is a Return or Break statement (safe exits)
/// 4. Skip code inside exception handlers (Исключение blocks)
/// 5. Recursively check parent blocks for code after control structures
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CodeAfterAsyncCall) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Find all async method calls
    let async_calls = find_async_calls(&root);

    // Check each async call for code after it
    for call_stmt in async_calls {
        if has_statements_after(&call_stmt) {
            if let Some(method_name) = get_method_name(&call_stmt) {
                diagnostics.push(make_diagnostic(&call_stmt, &method_name));
            }
        }
    }

    diagnostics
}

/// Find all async method calls in the syntax tree.
///
/// Returns a list of CALL_STMT nodes that are global calls to async methods.
/// Filters out:
/// - Qualified calls (Object.Method())
/// - Non-async methods
fn find_async_calls(root: &SyntaxNode) -> Vec<SyntaxNode> {
    root.descendants()
        .filter(|node| node.kind() == SyntaxKind::CALL_STMT)
        .filter(is_global_call)
        .filter(
            |node| {
                if let Some(name) = get_method_name(node) {
                    is_async_method(&name)
                } else {
                    false
                }
            },
        )
        .collect()
}

/// Check if a method name is an async method (case-insensitive).
fn is_async_method(name: &str) -> bool {
    let name_upper = name.to_uppercase();
    ASYNC_METHODS.contains(&name_upper.as_str())
}

/// Check if a CALL_STMT is a global call (not Object.Method()).
///
/// Returns false for qualified calls that contain FIELD_EXPR nodes.
fn is_global_call(stmt: &SyntaxNode) -> bool {
    // Must be CALL_STMT
    if stmt.kind() != SyntaxKind::CALL_STMT {
        return false;
    }

    // Skip if contains FIELD_EXPR (qualified call like Object.Method())
    if stmt.descendants().any(|n| n.kind() == SyntaxKind::FIELD_EXPR) {
        return false;
    }

    true
}

/// Extract the method name from a CALL_STMT node.
///
/// Finds the first IDENT token which represents the method name.
fn get_method_name(stmt: &SyntaxNode) -> Option<String> {
    stmt.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
}

/// Check if there are executable statements after the given async call statement.
///
/// Algorithm:
/// 1. Check immediate siblings in the same block
/// 2. If first sibling is Return → false (safe exit)
/// 3. If first sibling is Break → check parent blocks (loop exit, but may have code after loop)
/// 4. Skip code inside exception handlers (between КлючевоеСловоИсключение and КонецПопытки)
/// 5. If any executable statement found → true
/// 6. Recursively check parent blocks for code after control structures
///
/// Returns true if there's code that will execute after the async call.
fn has_statements_after(stmt: &SyntaxNode) -> bool {
    let Some(parent) = stmt.parent() else {
        return false;
    };

    // Check immediate siblings after the async call
    let mut first_stmt_is_return = false;
    let mut first_stmt_is_break = false;
    let mut has_any_stmts = false;
    let mut in_exception_handler = false;

    let mut sibling = stmt.next_sibling();
    while let Some(next) = sibling {
        // Track exception handler boundaries (skip code inside EXCEPT...КонецПопытки)
        if is_except_keyword(&next) {
            in_exception_handler = true;
        }
        if is_end_try_keyword(&next) {
            in_exception_handler = false;
        }

        // Skip code inside exception handlers (it only runs on errors)
        if in_exception_handler {
            sibling = next.next_sibling();
            continue;
        }

        // Check if this is an executable statement or return/break
        if is_executable_statement(&next) || is_return_or_break(&next) {
            if !has_any_stmts {
                // First statement after async
                if next.kind() == SyntaxKind::RETURN_STMT {
                    first_stmt_is_return = true;
                } else if next.kind() == SyntaxKind::BREAK_STMT {
                    first_stmt_is_break = true;
                }
            }
            has_any_stmts = true;
        }

        sibling = next.next_sibling();
    }

    // If first statement is Return, it's a safe exit (exits function)
    if first_stmt_is_return {
        return false;
    }

    // Java logic: (!break && has_stmts) || check_parent
    // If there are statements and first is NOT break, that's an error
    // If first is break, still need to check parent (code may exist after loop)
    let immediate_error = !first_stmt_is_break && has_any_stmts;
    immediate_error || check_parent_block(&parent)
}

/// Recursively check parent blocks for code after control structures containing the async call.
///
/// Walks up the AST tree checking if there's executable code after control structures
/// (IF, WHILE, FOR, TRY) that contain the async call.
///
/// Example: `if (...) { async(); } Code();` → ERROR (code after IF block)
fn check_parent_block(node: &SyntaxNode) -> bool {
    let mut current = node.clone();

    loop {
        match current.kind() {
            // Control structures: check for code AFTER the structure
            SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT => {
                // Check siblings after this control structure
                let mut sibling = current.next_sibling();
                while let Some(next) = sibling {
                    // Skip else/elsif clauses (they're part of the IF structure, not "after")
                    if is_else_clause(&next) {
                        sibling = next.next_sibling();
                        continue;
                    }

                    // Return or Break after control structure is OK (safe exit)
                    if is_return_or_break(&next) {
                        return false;
                    }

                    // Found executable code after control structure → ERROR
                    if is_executable_statement(&next) {
                        return true;
                    }

                    sibling = next.next_sibling();
                }

                // No code after this control structure, check parent
                if let Some(parent) = current.parent() {
                    current = parent;
                } else {
                    return false;
                }
            }

            // Reached procedure/function boundary
            SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF => {
                return false;
            }

            // Other nodes: continue walking up
            _ => {
                if let Some(parent) = current.parent() {
                    current = parent;
                } else {
                    return false;
                }
            }
        }
    }
}

/// Check if a node is an executable statement.
fn is_executable_statement(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::ASSIGN_STMT
            | SyntaxKind::CALL_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::EXECUTE_STMT
            | SyntaxKind::RAISE_STMT
    )
}

/// Check if a node is a Return or Break statement (safe exits).
fn is_return_or_break(node: &SyntaxNode) -> bool {
    matches!(node.kind(), SyntaxKind::RETURN_STMT | SyntaxKind::BREAK_STMT)
}

/// Check if a node contains the КлючевоеСловоИсключение (EXCEPT) keyword.
///
/// This marks the start of an exception handler block.
fn is_except_keyword(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| t.kind() == SyntaxKind::KW_EXCEPT)
}

/// Check if a node contains the КонецПопытки (END_TRY) keyword.
///
/// This marks the end of a Try-Except block.
fn is_end_try_keyword(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| t.kind() == SyntaxKind::KW_END_TRY)
}

/// Check if a node is an Else or ElseIf clause.
///
/// These are part of the IF structure, not separate statements "after" async.
fn is_else_clause(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| matches!(t.kind(), SyntaxKind::KW_ELSIF | SyntaxKind::KW_ELSE))
}

/// Create a diagnostic for code after async call.
fn make_diagnostic(node: &SyntaxNode, method_name: &str) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::CodeAfterAsyncCall,
        message: format!(
            "После вызова асинхронного метода '{}' есть строки кода. Код выполнится немедленно, не дожидаясь завершения асинхронной операции",
            method_name
        ),
        severity: Severity::Warning,
        range: node.text_range(),
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_no_code_after_async() {
        let code = r#"Процедура Тест()
    ПоказатьВводЧисла(Оповещение, 1, "Текст", 10, 2);
    // Только комментарий
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "No code after async should be valid");
    }

    #[test]
    fn test_code_after_async() {
        let code = r#"Процедура Тест()
    ПоказатьВводЧисла(Оповещение, 1, "Текст", 10, 2);
    Сообщить("Ошибка!");
КонецПроцедуры"#;

        let (diagnostics, file_content) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Code after async should be an error");
        assert_diagnostic_range(&file_content, &diagnostics[0], 1, 4, 52);
    }

    #[test]
    fn test_return_after_async() {
        let code = r#"Процедура Тест()
    ПоказатьВводЧисла(Оповещение, 1, "Текст", 10, 2);
    Возврат;
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Return after async should be valid");
    }

    #[test]
    fn test_async_in_if_with_code_after() {
        let code = r#"Процедура Тест()
    Если Условие Тогда
        ПоказатьВводЧисла(Оповещение, 1, "Текст", 10, 2);
    КонецЕсли;
    Сообщить("Ошибка!");
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Code after IF containing async should be an error");
    }

    #[test]
    fn test_english_methods() {
        let code = r#"Procedure Test()
    ShowInputNumber(Notification, 1, "Text", 10, 2);
    Message("Error!");
EndProcedure"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "English async methods should be detected");
    }

    #[test]
    fn test_qualified_call_ignored() {
        let code = r#"Процедура Тест()
    Форма.ПоказатьВводЧисла(Оповещение, 1, "Текст", 10, 2);
    Сообщить("OK");
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Qualified calls should be ignored");
    }

    #[test]
    fn test_break_after_async_in_loop() {
        let code = r#"Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        ПоказатьВводЧисла(Оповещение, 1, "Текст", 10, 2);
        Прервать;
    КонецЦикла;
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Break after async in loop should be valid");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CodeAfterAsyncCallDiagnostic.bsl");

        let (diagnostics, file_content) = check_diagnostic(code);

        // Java expects 10 diagnostics (from CodeAfterAsyncCallDiagnosticTest.java)
        assert_eq!(diagnostics.len(), 10, "Should match Java implementation (10 diagnostics)");

        // Verify exact positions match Java test expectations (line:col ranges)
        // Java format: hasRange(line, startCol, endCol) from CodeAfterAsyncCallDiagnosticTest.java
        assert_diagnostic_range(&file_content, &diagnostics[0], 4, 4, 96);
        assert_diagnostic_range(&file_content, &diagnostics[1], 21, 8, 100);
        assert_diagnostic_range(&file_content, &diagnostics[2], 34, 8, 100);
        assert_diagnostic_range(&file_content, &diagnostics[3], 48, 12, 104);
        assert_diagnostic_range(&file_content, &diagnostics[4], 63, 12, 104);
        assert_diagnostic_range(&file_content, &diagnostics[5], 78, 12, 104);
        assert_diagnostic_range(&file_content, &diagnostics[6], 93, 12, 104);
        assert_diagnostic_range(&file_content, &diagnostics[7], 108, 12, 104);
        assert_diagnostic_range(&file_content, &diagnostics[8], 123, 12, 104);
        assert_diagnostic_range(&file_content, &diagnostics[9], 270, 12, 104);
    }
}
