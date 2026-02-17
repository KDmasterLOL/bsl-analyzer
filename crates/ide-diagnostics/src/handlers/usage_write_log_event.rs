//! UsageWriteLogEvent diagnostic.
//!
//! Validates correct usage of WriteLogEvent / ЗаписьЖурналаРегистрации method.
//!
//! ## Checks
//! 1. Method must have at least 5 parameters
//! 2. Second parameter (log level) must not be empty
//! 3. Fifth parameter (comment) must not be empty
//! 4. Inside exception blocks:
//!    - Log level must be Error (УровеньЖурналаРегистрации.Ошибка / EventLogLevel.Error)
//!    - Comment must contain DetailErrorDescription(ErrorInfo()) or have Raise statement
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** INFO
//! - **Type:** CODE_SMELL
//! - **Tags:** STANDARD, BADPRACTICE
//! - **Minutes to fix:** 1
//!
//! ## Implementation
//! **AST-based diagnostic** - requires complex context analysis.
//! Uses `bsl-platform` crate for method name resolution (bilingual, case-insensitive).
//!
//! Ported from:
//! - UsageWriteLogEventDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_platform::PlatformData;
use ide_db::TextRange;
use syntax::{SyntaxKind, SyntaxNode};
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const WRITE_LOG_EVENT_METHOD_PARAMS_COUNT: usize = 5;

/// Creates diagnostic from HIR BodyDiagnostic::UsageWriteLogEvent.
///
/// Validates WriteLogEvent calls based on collected flags:
/// 1. Must have at least 5 parameters
/// 2. Second parameter (log level) must not be empty
/// 3. Fifth parameter (comment) must not be empty
/// 4. Inside exception blocks:
///    - Log level must be Error
///    - Comment must contain DetailErrorDescription(ErrorInfo()) or have Raise statement
#[allow(clippy::too_many_arguments)]
pub fn from_hir(
    in_except_block: bool,
    arg_count: usize,
    log_level_empty: bool,
    comment_empty: bool,
    has_error_log_level: bool,
    has_detail_error_description: bool,
    except_has_raise: bool,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::UsageWriteLogEvent;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    // Check 1: Wrong param count
    if arg_count < WRITE_LOG_EVENT_METHOD_PARAMS_COUNT {
        return Some(Diagnostic {
            code,
            message: "Неверное число параметров метода".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    // Check 2: Missing log level (2nd param)
    if log_level_empty {
        return Some(Diagnostic {
            code,
            message: "Не указан 2й параметр с типом \"УровеньЖурналаРегистрации\"".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    // Check 3: Missing comment (5th param)
    if comment_empty {
        return Some(Diagnostic {
            code,
            message: "Не указан 5й параметр \"Комментарий\"".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    // Check 4: Inside except block validation
    if in_except_block {
        // Must have Error log level
        if !has_error_log_level {
            return Some(Diagnostic {
                code,
                message: "Нужно указывать уровень \"Ошибка\" при записи в журнал регистрации внутри блока Исключение-КонецПопытки".to_string(),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }

        // Must have DetailErrorDescription or Raise in block
        if !has_detail_error_description && !except_has_raise {
            return Some(Diagnostic {
                code,
                message: "В тексте комментария нет вызова \"ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())\"".to_string(),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    None
}

/// Check if a method name matches a platform global function by its English name.
///
/// Uses bsl-platform for bilingual, case-insensitive lookup.
fn is_platform_function(method_name: &str, english_name: &str) -> bool {
    let platform = PlatformData::instance();
    if let Some(func) = platform.get_global_function(method_name) {
        func.english_name.as_str() == english_name
    } else {
        false
    }
}

const COMMENTS_PARAM_INDEX: usize = 4;
const LOG_LEVEL_PARAM_INDEX: usize = 1;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UsageWriteLogEvent;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() == SyntaxKind::CALL_EXPR {
            if let Some(diag) = check_call_expr(&node, code, ctx) {
                diagnostics.push(diag);
            }
        }
    }

    diagnostics
}

fn check_call_expr(
    call_node: &SyntaxNode,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    if !is_write_log_event_call(call_node) {
        return None;
    }

    let args = collect_call_arguments(call_node);

    if args.len() < WRITE_LOG_EVENT_METHOD_PARAMS_COUNT {
        return Some(create_diagnostic(
            call_node.text_range(),
            "Неверное число параметров метода",
            code,
            ctx,
        ));
    }

    if args[LOG_LEVEL_PARAM_INDEX].is_none() {
        return Some(create_diagnostic(
            call_node.text_range(),
            "Не указан 2й параметр с типом \"УровеньЖурналаРегистрации\"",
            code,
            ctx,
        ));
    }

    if args[COMMENTS_PARAM_INDEX].is_none() {
        return Some(create_diagnostic(
            call_node.text_range(),
            "Не указан 5й параметр \"Комментарий\"",
            code,
            ctx,
        ));
    }

    if is_inside_except_block(call_node) {
        let log_level_arg = args[LOG_LEVEL_PARAM_INDEX].as_ref().unwrap();
        if !has_error_log_level(log_level_arg) {
            return Some(create_diagnostic(
                call_node.text_range(),
                "Нужно указывать уровень \"Ошибка\" при записи в журнал регистрации внутри блока Исключение-КонецПопытки",
                code,
                ctx,
            ));
        }

        let comment_arg = args[COMMENTS_PARAM_INDEX].as_ref().unwrap();
        let code_block = find_parent_code_block(call_node);
        if !is_comment_correct(comment_arg, code_block.as_ref()) {
            return Some(create_diagnostic(
                call_node.text_range(),
                "В тексте комментария нет вызова \"ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())\"",
                code,
                ctx,
            ));
        }
    }

    None
}

fn is_write_log_event_call(call_node: &SyntaxNode) -> bool {
    let mut children = call_node.children();
    let first_child = match children.next() {
        Some(c) => c,
        None => return false,
    };

    if first_child.kind() != SyntaxKind::IDENT {
        return false;
    }

    let name = first_child.text().to_string();
    is_platform_function(&name, "WriteLogEvent")
}

fn collect_call_arguments(call_node: &SyntaxNode) -> Vec<Option<SyntaxNode>> {
    let Some(arg_list) = call_node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST) else {
        return Vec::new();
    };

    let mut args = Vec::new();
    let mut current_arg: Option<SyntaxNode> = None;
    let mut has_content = false;

    for child in arg_list.children_with_tokens() {
        match child.kind() {
            SyntaxKind::COMMA => {
                args.push(current_arg.take());
                has_content = true;
            }
            SyntaxKind::L_PAREN | SyntaxKind::R_PAREN => {}
            kind if kind.is_trivia() => {}
            _ => {
                if current_arg.is_none() {
                    if let Some(node) = child.as_node() {
                        current_arg = Some(node.clone());
                        has_content = true;
                    }
                }
            }
        }
    }

    if current_arg.is_some() || has_content {
        args.push(current_arg);
    }

    args
}

fn is_inside_except_block(node: &SyntaxNode) -> bool {
    let mut parent = node.parent();
    while let Some(p) = parent {
        if p.kind() == SyntaxKind::EXCEPT_CLAUSE {
            return true;
        }
        parent = p.parent();
    }
    false
}

fn find_parent_code_block(node: &SyntaxNode) -> Option<SyntaxNode> {
    let mut parent = node.parent();
    while let Some(p) = parent {
        if p.kind() == SyntaxKind::STMT_LIST {
            return Some(p);
        }
        parent = p.parent();
    }
    None
}

fn has_error_log_level(arg: &SyntaxNode) -> bool {
    let text = arg.text().to_string();
    let lower = text.to_lowercase();

    if lower.contains("уровеньжурналарегистрации") || lower.contains("eventloglevel")
    {
        if lower.contains("ошибка") || lower.contains("error") {
            return true;
        }
        return false;
    }

    true
}

fn is_comment_correct(comment_arg: &SyntaxNode, code_block: Option<&SyntaxNode>) -> bool {
    if let Some(block) = code_block {
        if has_raise_statement(block) {
            return true;
        }
    }

    if has_detail_error_description(comment_arg) {
        return true;
    }

    if has_brief_error_description(comment_arg) || has_simple_error_description(comment_arg) {
        return false;
    }

    if has_string_literal(comment_arg) {
        return false;
    }

    if let Some(block) = code_block {
        if let Some(var_name) = get_variable_name(comment_arg) {
            if let Some(assignment_expr) = find_variable_assignment(block, &var_name) {
                if has_brief_error_description(&assignment_expr)
                    || has_simple_error_description(&assignment_expr)
                {
                    return false;
                }
                if has_detail_error_description(&assignment_expr) {
                    return true;
                }
                return false;
            }
            return true;
        }
    }

    false
}

fn has_string_literal(node: &SyntaxNode) -> bool {
    for item in node.descendants_with_tokens() {
        if let Some(token) = item.as_token() {
            let kind = token.kind();
            if kind == SyntaxKind::STRING
                || kind == SyntaxKind::STRING_START
                || kind == SyntaxKind::STRING_TAIL
                || kind == SyntaxKind::STRING_PART
            {
                return true;
            }
        }
    }
    false
}

fn has_raise_statement(code_block: &SyntaxNode) -> bool {
    for descendant in code_block.descendants() {
        if descendant.kind() == SyntaxKind::RAISE_STMT {
            return true;
        }
    }
    false
}

fn has_detail_error_description(node: &SyntaxNode) -> bool {
    for descendant in node.descendants() {
        if descendant.kind() == SyntaxKind::CALL_EXPR {
            if let Some(name) = get_call_method_name(&descendant) {
                if is_platform_function(&name, "DetailErrorDescription")
                    && has_error_info_call(&descendant)
                {
                    return true;
                }
            }

            if is_error_processing_detail_call(&descendant) {
                return true;
            }
        }
    }
    false
}

fn is_error_processing_detail_call(call_node: &SyntaxNode) -> bool {
    let text = call_node.text().to_string();
    let lower = text.to_lowercase();

    if (lower.contains("обработкаошибок") || lower.contains("errorprocessing"))
        && (lower.contains("подробноепредставлениеошибки")
            || lower.contains("detailerrordescription"))
        && (lower.contains("информацияобошибке") || lower.contains("errorinfo"))
    {
        return true;
    }

    false
}

fn has_brief_error_description(node: &SyntaxNode) -> bool {
    for descendant in node.descendants() {
        if descendant.kind() == SyntaxKind::CALL_EXPR {
            if let Some(name) = get_call_method_name(&descendant) {
                if is_platform_function(&name, "BriefErrorDescription") {
                    return true;
                }
            }

            // Check for ОбработкаОшибок.КраткоеПредставлениеОшибки pattern
            let text = descendant.text().to_string().to_lowercase();
            if (text.contains("обработкаошибок") || text.contains("errorprocessing"))
                && (text.contains("краткоепредставлениеошибки")
                    || text.contains("brieferrordescription"))
            {
                return true;
            }
        }
    }
    false
}

fn has_simple_error_description(node: &SyntaxNode) -> bool {
    for descendant in node.descendants() {
        if descendant.kind() == SyntaxKind::CALL_EXPR {
            if let Some(name) = get_call_method_name(&descendant) {
                if is_platform_function(&name, "ErrorDescription") {
                    return true;
                }
            }
        }
    }
    false
}

fn has_error_info_call(call_node: &SyntaxNode) -> bool {
    for descendant in call_node.descendants() {
        if descendant.kind() == SyntaxKind::CALL_EXPR {
            if let Some(name) = get_call_method_name(&descendant) {
                if is_platform_function(&name, "ErrorInfo") {
                    return true;
                }
            }
        }
    }
    false
}

fn get_call_method_name(call_node: &SyntaxNode) -> Option<String> {
    let mut children = call_node.children();
    let first_child = children.next()?;

    if first_child.kind() == SyntaxKind::IDENT {
        return Some(first_child.text().to_string());
    }

    None
}

fn get_variable_name(arg: &SyntaxNode) -> Option<String> {
    for item in arg.descendants_with_tokens() {
        if let Some(token) = item.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                let parent = token.parent()?;
                if parent.kind() != SyntaxKind::CALL_EXPR {
                    return Some(token.text().to_string());
                }
            }
        }
    }
    None
}

fn find_variable_assignment(code_block: &SyntaxNode, var_name: &str) -> Option<SyntaxNode> {
    let var_lower = var_name.to_lowercase();

    for descendant in code_block.descendants() {
        if descendant.kind() == SyntaxKind::ASSIGN_STMT {
            if let Some(lhs) = get_assignment_target(&descendant) {
                if lhs.to_lowercase() == var_lower {
                    if let Some(rhs) = get_assignment_value(&descendant) {
                        return Some(rhs);
                    }
                }
            }
        }
    }

    None
}

fn get_assignment_target(assign_stmt: &SyntaxNode) -> Option<String> {
    for child in assign_stmt.children() {
        if child.kind() == SyntaxKind::IDENT {
            return Some(child.text().to_string());
        }
    }

    for token in assign_stmt.children_with_tokens() {
        if let Some(t) = token.as_token() {
            if t.kind() == SyntaxKind::IDENT {
                return Some(t.text().to_string());
            }
        }
    }

    None
}

fn get_assignment_value(assign_stmt: &SyntaxNode) -> Option<SyntaxNode> {
    let mut found_eq = false;

    for item in assign_stmt.children_with_tokens() {
        if let Some(token) = item.as_token() {
            if token.kind() == SyntaxKind::EQ {
                found_eq = true;
            }
        }
        if found_eq {
            if let Some(node) = item.as_node() {
                return Some(node.clone());
            }
        }
    }

    None
}

fn create_diagnostic(
    range: TextRange,
    message: &str,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: message.to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_ast_diagnostic;
    use crate::DiagnosticCode;

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/UsageWriteLogEventDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 18, "Expected 18 diagnostics, got {}", diags.len());
    }

    #[test]
    fn test_wrong_number_params() {
        let code = r#"
&НаСервере
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие");
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("параметров"));
    }

    #[test]
    fn test_no_second_parameter() {
        let code = r#"
&НаСервере
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие",
      ,
      , , ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("2й параметр"));
    }

    #[test]
    fn test_no_comment() {
        let code = r#"
&НаСервере
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , , );
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("5й параметр"));
    }

    #[test]
    fn test_wrong_log_level_in_except() {
        let code = r#"
&НаСервере
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Предупреждение, , ,
            "Текст");
    КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Ошибка"));
    }

    #[test]
    fn test_missing_detail_error_in_except() {
        let code = r#"
&НаСервере
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            ОписаниеОшибки());
    КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("ПодробноеПредставлениеОшибки"));
    }

    #[test]
    fn test_correct_usage_outside_except() {
        let code = r#"
&НаСервере
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие",
        УровеньЖурналаРегистрации.Ошибка, , ,
        ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_correct_usage_in_except_with_raise() {
        let code = r#"
&НаСервере
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие",
            УровеньЖурналаРегистрации.Ошибка, , ,
            ОписаниеОшибки());
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_correct_usage_in_except_with_detail() {
        let code = r#"
&НаСервере
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
    КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_variable_with_detail_error() {
        let code = r#"
&НаСервере
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ТекстОшибки = ПодробноеПредставлениеОшибки(ИнформацияОбОшибке());
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            ТекстОшибки);
    КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    WriteLogEvent("Event");
EndProcedure
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    ЗАПИСЬЖУРНАЛАРЕГИСТРАЦИИ("Событие");
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_error_processing_module() {
        let code = r#"
&НаСервере
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖР,
            , , ОбработкаОшибок.ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
    КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_hir_detection_wrong_params() {
        use crate::test_utils::check_hir_diagnostic;

        let code = r#"
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 1, "HIR should detect wrong param count");
        assert!(diags[0].message.contains("параметров"));
    }

    #[test]
    fn test_hir_detection_correct_usage() {
        use crate::test_utils::check_hir_diagnostic;
        let code = r#"
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Информация, , , "Комментарий");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect();

        assert_eq!(diags.len(), 0, "HIR should not detect for correct usage");
    }
}
