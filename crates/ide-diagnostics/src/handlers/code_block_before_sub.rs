//! CodeBlockBeforeSub diagnostic.
//!
//! Detects executable code before procedure/function declarations.
//!
//! ## Why?
//! Code before subroutine declarations violates BSL organizational conventions:
//! - Makes code structure unclear
//! - Can lead to initialization order issues
//! - Harder to navigate and maintain
//!
//! Best practice: Variables → Procedures/Functions → Executable Code
//!
//! ## Bad practice
//! ```bsl
//! Перем МояПеременная;
//!
//! Инициализация();
//! МояПеременная = 10;
//!
//! Процедура Инициализация()
//!     // ...
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Перем МояПеременная;
//!
//! Процедура Инициализация()
//!     // ...
//! КонецПроцедуры
//!
//! Инициализация();
//! МояПеременная = 10;
//! ```
//!
//! ## Implementation
//! Adapted to use Rowan SyntaxNode traversal.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::{SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Main entry point for CodeBlockBeforeSub diagnostic.
///
/// Detects executable code blocks before the first procedure/function declaration.
/// Reports ONE diagnostic covering all code blocks (from first to last).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CodeBlockBeforeSub;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut code_blocks_before_sub: Vec<SyntaxNode> = Vec::new();

    for child in root.children() {
        if is_subroutine(&child) {
            if !code_blocks_before_sub.is_empty() {
                return vec![create_diagnostic(&code_blocks_before_sub, code, ctx)];
            }
            break;
        }

        if is_code_block(&child) {
            code_blocks_before_sub.push(child.clone());
        }
    }

    Vec::new()
}

/// Check if node is a procedure or function definition.
fn is_subroutine(node: &SyntaxNode) -> bool {
    matches!(node.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF)
}

/// Check if node is an executable code block.
///
/// Returns true for:
/// - Direct executable statements (CALL_STMT, ASSIGN_STMT, IF_STMT, etc.)
/// - Preprocessor regions containing executable code
/// - Preprocessor conditionals containing executable code
///
/// Returns false for:
/// - Variable declarations (VAR_DEF)
/// - Trivia (WHITESPACE, NEWLINE, COMMENT)
/// - Annotations (COMPILER_DIRECTIVE, ANNOTATION)
fn is_code_block(node: &SyntaxNode) -> bool {
    match node.kind() {
        SyntaxKind::CALL_STMT
        | SyntaxKind::ASSIGN_STMT
        | SyntaxKind::IF_STMT
        | SyntaxKind::WHILE_STMT
        | SyntaxKind::FOR_STMT
        | SyntaxKind::FOR_EACH_STMT
        | SyntaxKind::TRY_STMT
        | SyntaxKind::RETURN_STMT
        | SyntaxKind::RAISE_STMT
        | SyntaxKind::BREAK_STMT
        | SyntaxKind::CONTINUE_STMT => true,

        SyntaxKind::PRE_REGION_DIR | SyntaxKind::PRE_IF_DIR => contains_executable_code(node),

        SyntaxKind::VAR_DEF
        | SyntaxKind::WHITESPACE
        | SyntaxKind::NEWLINE
        | SyntaxKind::COMMENT
        | SyntaxKind::COMPILER_DIRECTIVE
        | SyntaxKind::ANNOTATION => false,

        _ => {
            tracing::debug!(
                kind = ?node.kind(),
                "Unexpected node kind in SourceFile children"
            );
            false
        }
    }
}

/// Check if node or its descendants contain executable code outside of subroutines.
///
/// Used for preprocessor regions and conditionals to determine if they should be flagged.
/// Skips procedure/function bodies — code inside them is not "free" executable code.
fn contains_executable_code(node: &SyntaxNode) -> bool {
    for child in node.children() {
        if is_subroutine(&child) {
            continue;
        }
        if matches!(
            child.kind(),
            SyntaxKind::CALL_STMT
                | SyntaxKind::ASSIGN_STMT
                | SyntaxKind::IF_STMT
                | SyntaxKind::WHILE_STMT
                | SyntaxKind::FOR_STMT
                | SyntaxKind::FOR_EACH_STMT
                | SyntaxKind::TRY_STMT
                | SyntaxKind::RETURN_STMT
                | SyntaxKind::RAISE_STMT
                | SyntaxKind::BREAK_STMT
                | SyntaxKind::CONTINUE_STMT
        ) {
            return true;
        }
        if contains_executable_code(&child) {
            return true;
        }
    }
    false
}

/// Create diagnostic for code blocks before subroutines.
///
/// Combines ranges from first block to last block (inclusive).
/// For preprocessor regions, adjusts the range to start from the first executable code inside.
fn create_diagnostic(
    code_blocks: &[SyntaxNode],
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    use ide_db::TextRange;

    let first = code_blocks.first().unwrap();
    let last = code_blocks.last().unwrap();

    let start_offset = if first.kind() == SyntaxKind::PRE_REGION_DIR {
        first
            .descendants()
            .find(is_executable_statement)
            .map(|n| n.text_range().start())
            .unwrap_or_else(|| first.text_range().start())
    } else {
        first.text_range().start()
    };

    let end_offset = last.text_range().end();
    let range = TextRange::new(start_offset, end_offset);

    Diagnostic {
        code,
        message: "Обнаружен блок кода перед объявлением процедур и функций".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

/// Check if node is an executable statement (helper for create_diagnostic).
fn is_executable_statement(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::CALL_STMT
            | SyntaxKind::ASSIGN_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::RAISE_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
    )
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range_multiline, check_ast_diagnostic, check_diagnostics_snapshot_for,
    };
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_code_inside_region_before_sub() {
        // Executable code inside a top region before procedures should trigger.
        let code = r#"Перем П;

#Область КодДоМетодов
Метод();
Сообщить("4");
#КонецОбласти

Процедура Метод()
// Метод
КонецПроцедуры

#Область Инициализация
П = 12;
#КонецОбласти"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should find 1 diagnostic for code in region before sub");
        // The diagnostic spans the entire region (starts at first executable code inside)
        assert_diagnostic_range_multiline(code, &diagnostics[0], 3, 0, 5, 13);
    }

    #[test]
    fn test_valid_order() {
        let code = r#"Перем МояПеременная;

Процедура Инициализация()
    МояПеременная = 10;
КонецПроцедуры

Инициализация();
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Code after procedures should be valid");
    }

    #[test]
    fn test_code_before_procedure() {
        let code = r#"Перем МояПеременная;

МояПеременная = 10;

Процедура Тест()
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Code before procedure should be flagged");
    }

    #[test]
    fn test_only_variables_before_procedure() {
        let code = r#"Перем Переменная1;
Перем Переменная2;

Процедура Тест()
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Only variables before procedure is valid");
    }

    #[test]
    fn test_no_procedures() {
        let code = r#"Перем МояПеременная;

МояПеременная = 10;
Сообщить(МояПеременная);
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "No procedures means no error (nothing to be 'before')");
    }

    #[test]
    fn test_multiple_code_blocks() {
        let code = r#"Перем Счетчик;

Счетчик = 0;
Инициализация();
Сообщить("Начало");

Процедура Инициализация()
    Счетчик = 1;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Multiple code blocks reported as ONE diagnostic");
    }

    #[test]
    fn test_region_with_only_procedures_before_top_level_procedure() {
        // Region contains only procedures (no free code) before a top-level procedure
        // Should NOT trigger — code inside procedure bodies is not "free" executable code
        let code = r#"#Область ОбработчикиСобытий

&НаКлиенте
Процедура ПриОткрытии(Отказ)
    Сообщить("Привет");
КонецПроцедуры

&НаСервере
Функция ПолучитьДанные()
    Возврат 42;
КонецФункции

#КонецОбласти

&НаСервере
Процедура ВнеОбласти()
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Region with only procedures should not be flagged as code block"
        );
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"Var MyVariable;

MyVariable = 10;

Procedure Initialize()
EndProcedure
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "English keywords should work");
    }

    #[test]
    fn test_non_region_code_before_region_wrapped_method_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Перем Состояние;

Состояние = Истина;

#Область СлужебныеПроцедурыИФункции
Процедура Подготовить()
КонецПроцедуры
#КонецОбласти

Процедура Выполнить()
КонецПроцедуры"#,
            DiagnosticCode::CodeBlockBeforeSub,
            expect![[r#"
                CodeBlockBeforeSub @ 3:1..3:19
                  message: Обнаружен блок кода перед объявлением процедур и функций
                  severity: Blocker"#]],
        );
    }

    #[test]
    fn test_region_wrapped_method_after_non_region_code_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Инициализировать();

#Область ОбработчикиСобытий
&НаКлиенте
Процедура ПриОткрытии(Отказ)
КонецПроцедуры
#КонецОбласти

Функция ПолучитьЗначение()
    Возврат 1;
КонецФункции"#,
            DiagnosticCode::CodeBlockBeforeSub,
            expect![[r#"
                CodeBlockBeforeSub @ 1:1..1:19
                  message: Обнаружен блок кода перед объявлением процедур и функций
                  severity: Blocker"#]],
        );
    }

    #[test]
    fn test_nested_region_method_after_region_code_snapshot() {
        check_diagnostics_snapshot_for(
            r#"#Область Инициализация
Настройки = Новый Структура;

#Область ВнутренниеМетоды
Процедура ЗаполнитьНастройки()
КонецПроцедуры
#КонецОбласти
#КонецОбласти

Процедура Выполнить()
КонецПроцедуры"#,
            DiagnosticCode::CodeBlockBeforeSub,
            expect![[r#"
                CodeBlockBeforeSub @ 2:1..8:14
                  message: Обнаружен блок кода перед объявлением процедур и функций
                  severity: Blocker"#]],
        );
    }
}
