//! ExcessiveAutoTestCheck diagnostic.
//!
//! Detects excessive checks for deprecated "АвтоТест" parameter.
//!
//! Standard 772 "Interaction with automated testing tools" has been deprecated.
//! If-statements that check for "АвтоТест" and immediately return are no longer needed.
//!
//! ## Bad practice
//! ```bsl
//! Процедура ПриСозданииНаСервере()
//!     Если Параметры.Свойство("АвтоТест") Тогда
//!         Возврат;
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! ## Why deprecated
//! The 1C standard 772 requiring "АвтоТест" checks has been revoked.
//! This pattern should be removed from code.
//!

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use regex::Regex;
use std::sync::OnceLock;
use syntax::{SyntaxKind, SyntaxNode, TextRange};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[
        bsl_metadata::ModuleType::FormModule,
        bsl_metadata::ModuleType::ObjectModule,
        bsl_metadata::ModuleType::RecordSetModule,
        bsl_metadata::ModuleType::CommonModule,
    ],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Pattern to match AutoTest checks in condition expressions.
///
/// Matches 4 variants:
/// 1. `.Свойство("АвтоТест")` (Russian property call)
/// 2. `= "АвтоТест"` (Russian equality, with optional whitespace)
/// 3. `.Property("AutoTest")` (English property call)
/// 4. `= "AutoTest"` (English equality, with optional whitespace)
fn autotest_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r#"(\.Свойство\("АвтоТест"\)|=\s*"АвтоТест"|\.Property\("AutoTest"\)|=\s*"AutoTest")"#,
        )
        .expect("Invalid regex pattern")
    })
}

/// Check if statement list contains only a return statement.
///
/// Returns true if the statement list has exactly one child that is a RETURN_STMT.
/// Ignores whitespace and comments.
fn has_only_return_statement(stmt_list: &SyntaxNode) -> bool {
    let statements: Vec<_> = stmt_list
        .children()
        .filter(|n| !matches!(n.kind(), SyntaxKind::WHITESPACE | SyntaxKind::COMMENT))
        .collect();

    statements.len() == 1 && statements[0].kind() == SyntaxKind::RETURN_STMT
}

/// Check if if-statement should be flagged (optimized version).
///
/// Returns Some(TextRange) if the if-statement:
/// 1. Condition matches AutoTest pattern
/// 2. Body contains only a return statement
fn check_if_statement_optimized(
    if_node: &SyntaxNode,
    return_stmts_by_parent: &std::collections::HashMap<syntax::TextSize, Vec<SyntaxNode>>,
) -> Option<TextRange> {
    let pattern = autotest_pattern();

    // First, try to find STMT_LIST among direct children (normal case)
    let stmt_list_candidate = if_node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);

    // If no STMT_LIST found among children, or if it doesn't have only return,
    // try to find RETURN_STMT using pre-collected map (workaround for parser bug with `=`)
    if stmt_list_candidate.is_none()
        || !has_only_return_statement(stmt_list_candidate.as_ref().unwrap())
    {
        // Check if there's an ERROR node (indicates parser issue)
        let has_error = if_node.children().any(|n| n.kind() == SyntaxKind::ERROR);

        if has_error {
            // Workaround: Count RETURN_STMT nodes that are descendants of this IF_STMT
            // by checking if any pre-collected returns are within this if_node's range
            let if_range = if_node.text_range();
            let return_count = return_stmts_by_parent
                .values()
                .flatten()
                .filter(|r| if_range.contains_range(r.text_range()))
                .count();

            // Should have exactly one RETURN_STMT for this diagnostic
            if return_count != 1 {
                return None;
            }

            // Check pattern in full if-statement text
            let if_text = if_node.text().to_string();
            if pattern.is_match(&if_text) {
                return Some(if_node.text_range());
            }
        }
        return None;
    }

    // Normal case: STMT_LIST found and has only return
    let if_text = if_node.text().to_string();
    if pattern.is_match(&if_text) {
        return Some(if_node.text_range());
    }

    None
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ExcessiveAutoTestCheck;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Optimized: Collect IF_STMT nodes and RETURN_STMT nodes in one pass
    let mut if_stmts = Vec::new();
    let mut return_stmts_by_parent = std::collections::HashMap::new();

    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::IF_STMT => {
                if_stmts.push(node);
            }
            SyntaxKind::RETURN_STMT => {
                // Track return statements for ERROR case workaround
                if let Some(parent) = node.parent() {
                    return_stmts_by_parent
                        .entry(parent.text_range().start())
                        .or_insert_with(Vec::new)
                        .push(node);
                }
            }
            _ => {}
        }
    }

    // Check each IF_STMT
    for if_node in if_stmts {
        if let Some(range) = check_if_statement_optimized(&if_node, &return_stmts_by_parent) {
            diagnostics.push(Diagnostic {
                code,
                message: "Excessive check for deprecated 'АвтоТест' parameter".to_string(),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::*;
    #[test]
    fn test_russian_property_with_blank_lines() {
        // Multi-line if-block with blank lines around return
        let code = r#"
Процедура ПриСозданииНаСервере()

    Если Параметры.Свойство("АвтоТест") Тогда

        Возврат;

    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_russian_equality_with_comment() {
        // Equality check with explanatory comment above
        let code = r#"
Процедура ОбработкаЗаполнения(ДанныеЗаполнения, ТестЗаполения, СтандартнаяОбработка)

    // Пропускаем обработку, чтобы гарантировать получение формы при передаче параметра "АвтоТест"
    Если ДанныеЗаполнения = "АвтоТест" Тогда
        Возврат;
    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_russian_property_on_local_variable() {
        let code = r#"
Процедура ПроверитьВыполение(Перечень)

    Если Перечень.Свойство("АвтоТест") Тогда

        Возврат;

    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_russian_multiple_statements_no_error() {
        // Multiple statements in if body — should NOT flag
        let code = r#"
Процедура БезОшибок()

    Перечень.Вставить("АвтоТест", "АвтоТест");

    Если Перечень.Свойство("АвтоТест") Тогда

        ВыполняемДействиеСПеречнем(Перечень);
        Возврат;

    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should NOT flag when multiple statements in body");
    }

    #[test]
    fn test_english_property_with_annotation() {
        let code = r#"
&AtServer
Procedure OnCreateAtServer()

    If Parameters.Property("AutoTest") Then
        Return;
    EndIf;

EndProcedure
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_english_equality_check() {
        let code = r#"
Procedure Filling()

    If VariableName = "AutoTest" Then
        Return;
    EndIf;

EndProcedure
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_english_property_with_blank_lines() {
        let code = r#"
Procedure Check(List)

    If List.Property("AutoTest") Then

        Return;

    EndIf;

EndProcedure
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_english_multiple_statements_no_error() {
        // Multiple statements in if body — should NOT flag
        let code = r#"
Procedure NoError(List)

    If List.Property("AutoTest") Then

        List.Delete("AutoTest");
        Return;

    EndIf;

EndProcedure
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should NOT flag when multiple statements");
    }

    #[test]
    fn test_top_level_if_not_in_procedure() {
        // Top-level if outside any procedure — no diagnostic expected
        let code = r#"
Если Отказ Тогда

    Возврат;

КонецЕсли;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Top-level if without AutoTest should not flag");
    }

    #[test]
    fn test_russian_property() {
        let code = r#"
Процедура Тест()
    Если Параметры.Свойство("АвтоТест") Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_english_property() {
        let code = r#"
Procedure Test()
    If Parameters.Property("AutoTest") Then
        Return;
    EndIf;
EndProcedure
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_russian_equality() {
        let code = r#"
Процедура Тест()
    Если Переменная = "АвтоТест" Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_english_equality() {
        let code = r#"
Procedure Test()
    If Variable = "AutoTest" Then
        Return;
    EndIf;
EndProcedure
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_multiple_statements_no_error() {
        let code = r#"
Процедура Тест()
    Если Параметры.Свойство("АвтоТест") Тогда
        Действие();
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should NOT flag when multiple statements");
    }

    #[test]
    fn test_no_return_no_error() {
        let code = r#"
Процедура Тест()
    Если Параметры.Свойство("АвтоТест") Тогда
        Действие();
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should NOT flag when no return");
    }

    #[test]
    fn test_no_autotest_check() {
        let code = r#"
Процедура Тест()
    Если Параметры.Свойство("ДругойПараметр") Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should NOT flag without AutoTest");
    }
}
