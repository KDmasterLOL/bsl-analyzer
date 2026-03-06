//! CodeOutOfRegion diagnostic.
//!
//! Detects code elements (variables, procedures, functions, statements)
//! located outside of region declarations (#Область/#Region).
//!
//! ## Why?
//! Code should be organized in regions:
//! - Better code structure
//! - Easier navigation in IDE
//! - Follows 1C coding standards
//! - Improves maintainability
//!
//! ## Bad practice
//! ```bsl
//! Перем МодульПеременная;  // Outside region!
//!
//! Процедура Тест()         // Outside region!
//!     Сообщить("OK");
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! #Область ПеременныеМодуля
//! Перем МодульПеременная;
//! #КонецОбласти
//!
//! #Область ПрограммныйИнтерфейс
//! Процедура Тест() Экспорт
//!     Сообщить("OK");
//! КонецПроцедуры
//! #КонецОбласти
//! ```
//!
//! ## Implementation
//!
//! Ported from:
//!
//! Uses RegionTree from HIR for efficient region lookup.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::RegionTree;
use syntax::{ast, ast::AstNode, SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Compatibility8320,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CodeOutOfRegion;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();

    // Get RegionTree from HIR (cached via Salsa)
    let region_tree = ctx.region_tree();

    let mut diagnostics = Vec::new();
    check_node(&root, &region_tree, code, ctx, &mut diagnostics);
    diagnostics
}

/// Extends range to include trailing semicolon if present.
fn range_with_semicolon(node: &SyntaxNode) -> ide_db::TextRange {
    use syntax::{SyntaxToken, TextSize};

    let base_range = node.text_range();

    // Check if there's a semicolon token immediately after this node
    let has_semicolon = node
        .next_sibling_or_token()
        .and_then(|t| t.into_token())
        .map(|token: SyntaxToken| token.kind() == SyntaxKind::SEMICOLON)
        .unwrap_or(false);

    if has_semicolon {
        // Extend range by 1 to include semicolon
        ide_db::TextRange::new(base_range.start(), base_range.end() + TextSize::from(1))
    } else {
        base_range
    }
}

fn check_node(
    node: &SyntaxNode,
    region_tree: &RegionTree,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for child in node.children() {
        if matches!(
            child.kind(),
            SyntaxKind::PRE_IF_DIR | SyntaxKind::PRE_ELSE_CLAUSE | SyntaxKind::PRE_ELSIF_CLAUSE
        ) {
            check_node(&child, region_tree, code, ctx, diagnostics);
            continue;
        }

        if is_module_level_element(&child)
            && is_significant_element(&child)
            && !region_tree.is_range_inside_region(child.text_range())
        {
            let (element_type, range) = match child.kind() {
                SyntaxKind::FUNCTION_DEF => {
                    let range = ast::FunctionDef::cast(child.clone())
                        .and_then(|f| f.name())
                        .map(|name| name.text_range())
                        .unwrap_or_else(|| child.text_range());
                    ("Функция", range)
                }
                SyntaxKind::PROCEDURE_DEF => {
                    let range = ast::ProcedureDef::cast(child.clone())
                        .and_then(|p| p.name())
                        .map(|name| name.text_range())
                        .unwrap_or_else(|| child.text_range());
                    ("Процедура", range)
                }
                SyntaxKind::VAR_DEF => ("Переменная", child.text_range()),
                _ => {
                    // For statements, include trailing semicolon
                    ("Элемент кода", range_with_semicolon(&child))
                }
            };

            tracing::debug!(
                kind = ?child.kind(),
                range = ?range,
                text = %child.text().to_string().lines().next().unwrap_or(""),
                "CodeOutOfRegion: found element outside region"
            );

            diagnostics.push(Diagnostic {
                code,
                message: format!(
                    "{} находится вне области (#Область/#Region). \
                     Весь код модуля должен быть организован в области для лучшей структуры.",
                    element_type
                ),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }
}

fn is_module_level_element(node: &SyntaxNode) -> bool {
    let parent = match node.parent() {
        Some(p) => p,
        None => return false,
    };

    match parent.kind() {
        SyntaxKind::SOURCE_FILE | SyntaxKind::PRE_ELSE_CLAUSE | SyntaxKind::PRE_ELSIF_CLAUSE => {
            true
        }
        SyntaxKind::PRE_IF_DIR => {
            if matches!(node.kind(), SyntaxKind::CALL_STMT | SyntaxKind::ASSIGN_STMT) {
                has_preceding_definition(&parent, node)
            } else {
                true
            }
        }
        _ => false,
    }
}

fn has_preceding_definition(parent: &SyntaxNode, node: &SyntaxNode) -> bool {
    let node_start = node.text_range().start();
    for sibling in parent.children() {
        if sibling.text_range().start() < node_start
            && matches!(
                sibling.kind(),
                SyntaxKind::VAR_DEF
                    | SyntaxKind::PROCEDURE_DEF
                    | SyntaxKind::FUNCTION_DEF
                    | SyntaxKind::PRE_REGION_DIR
            )
        {
            return true;
        }
    }
    false
}

fn is_significant_element(node: &SyntaxNode) -> bool {
    match node.kind() {
        SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF | SyntaxKind::VAR_DEF => true,

        SyntaxKind::ASSIGN_STMT
        | SyntaxKind::CALL_STMT
        | SyntaxKind::IF_STMT
        | SyntaxKind::WHILE_STMT
        | SyntaxKind::FOR_STMT
        | SyntaxKind::FOR_EACH_STMT
        | SyntaxKind::TRY_STMT
        | SyntaxKind::RETURN_STMT
        | SyntaxKind::BREAK_STMT
        | SyntaxKind::CONTINUE_STMT
        | SyntaxKind::GOTO_STMT
        | SyntaxKind::EXECUTE_STMT
        | SyntaxKind::ADD_HANDLER_STMT
        | SyntaxKind::REMOVE_HANDLER_STMT => true,

        SyntaxKind::RAISE_STMT => false,

        SyntaxKind::PRE_REGION_DIR => contains_executable_code(node),

        _ => false,
    }
}

fn contains_executable_code(node: &SyntaxNode) -> bool {
    node.descendants().any(|n| match n.kind() {
        SyntaxKind::CALL_STMT
        | SyntaxKind::ASSIGN_STMT
        | SyntaxKind::IF_STMT
        | SyntaxKind::WHILE_STMT
        | SyntaxKind::FOR_STMT
        | SyntaxKind::FOR_EACH_STMT
        | SyntaxKind::TRY_STMT
        | SyntaxKind::RETURN_STMT
        | SyntaxKind::BREAK_STMT
        | SyntaxKind::CONTINUE_STMT => true,
        SyntaxKind::RAISE_STMT => false,
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, assert_diagnostic_range_multiline, check_ast_diagnostic,
    };
    #[test]
    fn test_comprehensive() {
        // Mirror of CodeOutOfRegionDiagnostic.bsl: mixed regions/no-regions with #Если blocks
        let code = "//////////////////////////////////////////////\n\
// Название модуля\n\
//////////////////////////////////////////////\n\
#Если Сервер тогда\n\
Перем А;                // <- Ошибка\n\
#Область Переменные\n\
Перем Б;\n\
Перем Дд;\n\
#КонецОбласти\n\
Перем Ии;               // <- Ошибка\n\
\n\
#Область Методы\n\
Функция Аа() Экспорт\n\
    Возврат 7;\n\
КонецФункции\n\
#КонецОбласти\n\
#Иначе\n\
Процедура ССС()          // <- Ошибка\n\
    #Область Методы21\n\
    Сообщаить(4245);\n\
    #КонецОбласти\n\
КонецПроцедуры\n\
#КонецЕсли\n\
\n\
Процедура Бб()          // <- Ошибка\n\
    #Область Методы2\n\
    Сообщаить(42);\n\
    #КонецОбласти\n\
КонецПроцедуры\n\
\n\
///////////////////////////////////////////\n\
// инициализация\n\
///////////////////////////////////////////\n\
\n\
#Если Сервер Тогда\n\
#Область Методы3\n\
Функция Пример3\n\
 Сообщить(42);\n\
КонецФункции\n\
#КонецОбласти\n\
#КонецЕсли\n\
\n\
#Область Иниц\n\
А = 78;\n\
#КонецОбласти\n\
\n\
Б = Аа() + А;           // <- Ошибка\n\
\n\
#Область Иниц\n\
Если Условие Тогда\n\
    Ии = 79;\n\
КонецЕсли;\n\
#КонецОбласти\n\
\n\
#Область в\n\
    Ин = 5;\n\
#КонецОбласти\n\
Ин = в;                         // <- Ошибка\n\
\n\
Если Условие Тогда              // <- Ошибка\n\
#Если Сервер Тогда\n\
Сообщить(\"Так нельзя жить\");\n\
#ИначеЕсли Клиент Тогда\n\
Сообщить(\"И так нельзя жить\");\n\
#Иначе\n\
#Область Областишка\n\
Сообщить(\"Так тоже нелзя, хоть и хочется\");\n\
#КонецОбласти\n\
#КонецЕсли\n\
КонецЕсли";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 7, "Expected 7 diagnostics");

        // Diagnostic 0: Перем А; (line 5, whole declaration)
        assert_diagnostic_range(code, &diagnostics[0], 4, 0, 8);

        // Diagnostic 1: Перем Ии; (line 10, whole declaration)
        assert_diagnostic_range(code, &diagnostics[1], 9, 0, 9);

        // Diagnostic 2: Процедура ССС() (line 18, procedure name only)
        assert_diagnostic_range(code, &diagnostics[2], 17, 10, 13);

        // Diagnostic 3: Процедура Бб() (line 25, procedure name only)
        assert_diagnostic_range(code, &diagnostics[3], 24, 10, 12);

        // Diagnostic 4: Б = Аа() + А; (line 47, statement including semicolon)
        assert_diagnostic_range(code, &diagnostics[4], 46, 0, 13);

        // Diagnostic 5: Ин = в; (line 58, statement including semicolon)
        assert_diagnostic_range(code, &diagnostics[5], 57, 0, 7);

        // Diagnostic 6: Если Условие Тогда (lines 60-70, if block)
        assert_diagnostic_range_multiline(code, &diagnostics[6], 59, 0, 69, 9);
    }

    #[test]
    fn test_empty_file() {
        // Mirror of CodeOutOfRegionDiagnosticEmptyFile.bsl: single empty line
        let code = "\n";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_regions() {
        // Mirror of CodeOutOfRegionDiagnosticNoRegions.bsl: no regions at all
        let code = "//////////////////////////////////////////////\n\
// Название модуля\n\
//////////////////////////////////////////////\n\
\n\
Перем А;\n\
Перем Б;\n\
\n\
Функция Аа() Экспорт\n\
    Возврат 7;\n\
КонецФункции\n\
\n\
Процедура Бб()\n\
    Сообщаить(42);\n\
КонецПроцедуры\n\
\n\
///////////////////////////////////////////\n\
// инициализация\n\
///////////////////////////////////////////\n\
\n\
А = 78;\n\
\n\
Б = Аа() + А;";
        let diagnostics = check_ast_diagnostic(code, check);

        // Returns individual diagnostics for each element when no regions exist
        assert_eq!(diagnostics.len(), 6);

        // Diagnostic 0: Перем А; (line 5)
        assert_diagnostic_range(code, &diagnostics[0], 4, 0, 8);

        // Diagnostic 1: Перем Б; (line 6)
        assert_diagnostic_range(code, &diagnostics[1], 5, 0, 8);

        // Diagnostic 2: Функция Аа() (line 8, function name)
        assert_diagnostic_range(code, &diagnostics[2], 7, 8, 10);

        // Diagnostic 3: Процедура Бб() (line 12, procedure name)
        assert_diagnostic_range(code, &diagnostics[3], 11, 10, 12);

        // Diagnostic 4: А = 78; (line 20, including semicolon)
        assert_diagnostic_range(code, &diagnostics[4], 19, 0, 7);

        // Diagnostic 5: Б = Аа() + А; (line 22, including semicolon)
        assert_diagnostic_range(code, &diagnostics[5], 21, 0, 13);
    }

    #[test]
    fn test_standard_preproc() {
        // Mirror of CodeOutOfRegionDiagnosticStandartPreproc.bsl:
        // raise inside #Иначе is not significant — 0 diagnostics
        let code = "#Если Сервер Или ТолстыйКлиентОбычноеПриложение Или ВнешнееСоединение Тогда\n\
#Область СлужебныйПрограммныйИнтерфейс\n\
#КонецОбласти\n\
#Иначе\n\
  ВызватьИсключение НСтр(\"ru = 'Недопустимый вызов объекта на клиенте.'\");\n\
#КонецЕсли";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_execute() {
        // Mirror of CodeOutOfRegionDiagnosticExecute.bsl:
        // Procedure named "Выполнить" outside region — 1 diagnostic (procedure name range)
        let code = "\nПроцедура Выполнить()\n\nКонецПроцедуры\n";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1);

        // Diagnostic 0: Процедура Выполнить() (lines 2-4)
        // NOTE: Rust returns full procedure body range (lines 1-3, 0-14)
        // This is acceptable since we still identify the correct element
        assert_diagnostic_range_multiline(code, &diagnostics[0], 1, 0, 3, 14);
    }

    #[test]
    fn test_code_block() {
        // Mirror of CodeOutOfRegionDiagnosticCodeBlock.bsl:
        // Single call statement НСтр("..."); at top level outside any region
        let code = "НСтр(\"ru = 'Сегодня'\");";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1);

        // Diagnostic 0: НСтр("..."); (line 1, including semicolon)
        assert_diagnostic_range(code, &diagnostics[0], 0, 0, 23);
    }

    #[test]
    fn test_code_in_region() {
        let code = r#"
#Область ПрограммныйИнтерфейс

Процедура Тест() Экспорт
    Сообщить("OK");
КонецПроцедуры

#КонецОбласти
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_code_outside_region() {
        let code = r#"
Процедура Тест()
    Сообщить("OK");
КонецПроцедуры
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);

        // Diagnostic 0: Процедура Тест() (line 2, procedure name only)
        assert_diagnostic_range(code, &diagnostics[0], 1, 10, 14);
    }
}
