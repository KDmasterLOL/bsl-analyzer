use crate::define_metadata;
use crate::metadata::*;
use crate::{BodyContext, Diagnostic, DiagnosticCode};
use hir::LocalRange;
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

fn autotest_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r#"(\.Свойство\("АвтоТест"\)|=\s*"АвтоТест"|\.Property\("AutoTest"\)|=\s*"AutoTest")"#,
        )
        .expect("Invalid regex pattern")
    })
}

fn has_only_return_statement(stmt_list: &SyntaxNode) -> bool {
    let statements: Vec<_> = stmt_list
        .children()
        .filter(|n| !matches!(n.kind(), SyntaxKind::WHITESPACE | SyntaxKind::COMMENT))
        .collect();

    statements.len() == 1 && statements[0].kind() == SyntaxKind::RETURN_STMT
}

fn check_if_statement_optimized(
    if_node: &SyntaxNode,
    return_stmts_by_parent: &std::collections::HashMap<syntax::TextSize, Vec<SyntaxNode>>,
) -> Option<TextRange> {
    let pattern = autotest_pattern();

    let stmt_list_candidate = if_node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);

    if stmt_list_candidate.is_none()
        || !has_only_return_statement(stmt_list_candidate.as_ref().unwrap())
    {
        let has_error = if_node.children().any(|n| n.kind() == SyntaxKind::ERROR);

        if has_error {
            let if_range = if_node.text_range();
            let return_count = return_stmts_by_parent
                .values()
                .flatten()
                .filter(|r| if_range.contains_range(r.text_range()))
                .count();

            if return_count != 1 {
                return None;
            }

            let if_text = if_node.text().to_string();
            if pattern.is_match(&if_text) {
                return Some(if_node.text_range());
            }
        }
        return None;
    }

    let if_text = if_node.text().to_string();
    if pattern.is_match(&if_text) {
        return Some(if_node.text_range());
    }

    None
}

pub fn check_body(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    let code = DiagnosticCode::ExcessiveAutoTestCheck;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    let mut diagnostics = Vec::new();

    let mut if_stmts = Vec::new();
    let mut return_stmts_by_parent = std::collections::HashMap::new();

    for node in ctx.nodes() {
        match node.kind() {
            SyntaxKind::IF_STMT => {
                if_stmts.push(node);
            }
            SyntaxKind::RETURN_STMT => {
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

    for if_node in if_stmts {
        if let Some(range) = check_if_statement_optimized(&if_node, &return_stmts_by_parent) {
            diagnostics.push(Diagnostic {
                code,
                message: "Избыточная проверка устаревшего параметра 'АвтоТест'".to_string(),
                severity: ctx.severity(code),
                range: LocalRange::of_detached_node(range),
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    acc.extend(diagnostics);
}

#[cfg(test)]
mod tests {
    use super::check_body;
    use crate::test_utils::*;
    #[test]
    fn test_russian_property_with_blank_lines() {
        let code = r#"
Процедура ПриСозданииНаСервере()

    Если Параметры.Свойство("АвтоТест") Тогда

        Возврат;

    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_russian_equality_with_comment() {
        let code = r#"
Процедура ОбработкаЗаполнения(ДанныеЗаполнения, ТестЗаполения, СтандартнаяОбработка)

    // Пропускаем обработку, чтобы гарантировать получение формы при передаче параметра "АвтоТест"
    Если ДанныеЗаполнения = "АвтоТест" Тогда
        Возврат;
    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
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
        let diagnostics = check_body_diagnostic(code, check_body);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_russian_multiple_statements_no_error() {
        let code = r#"
Процедура БезОшибок()

    Перечень.Вставить("АвтоТест", "АвтоТест");

    Если Перечень.Свойство("АвтоТест") Тогда

        ВыполняемДействиеСПеречнем(Перечень);
        Возврат;

    КонецЕсли;

КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
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
        let diagnostics = check_body_diagnostic(code, check_body);
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
        let diagnostics = check_body_diagnostic(code, check_body);
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
        let diagnostics = check_body_diagnostic(code, check_body);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_english_multiple_statements_no_error() {
        let code = r#"
Procedure NoError(List)

    If List.Property("AutoTest") Then

        List.Delete("AutoTest");
        Return;

    EndIf;

EndProcedure
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        assert_eq!(diagnostics.len(), 0, "Should NOT flag when multiple statements");
    }

    #[test]
    fn test_top_level_if_not_in_procedure() {
        let code = r#"
Если Отказ Тогда

    Возврат;

КонецЕсли;
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
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
        let diagnostics = check_body_diagnostic(code, check_body);
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
        let diagnostics = check_body_diagnostic(code, check_body);
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
        let diagnostics = check_body_diagnostic(code, check_body);
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
        let diagnostics = check_body_diagnostic(code, check_body);
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
        let diagnostics = check_body_diagnostic(code, check_body);
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
        let diagnostics = check_body_diagnostic(code, check_body);
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
        let diagnostics = check_body_diagnostic(code, check_body);
        assert_eq!(diagnostics.len(), 0, "Should NOT flag without AutoTest");
    }
}
