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
//! Uses RegionTree from HIR for efficient region lookup.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::module_structure::significant::is_significant_for_code_out_of_region;
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
            && is_significant_for_code_out_of_region(&child)
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

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, assert_diagnostic_range_multiline, check_ast_diagnostic,
        check_diagnostics_snapshot_for,
    };
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_comprehensive() {
        let code = "//////////////////////////////////////////////\n\
// Служебный модуль\n\
//////////////////////////////////////////////\n\
#Если Сервер тогда\n\
Перем Кэш;              // <- Ошибка\n\
#Область ОписаниеПеременных\n\
Перем Настройки;\n\
Перем Параметры;\n\
#КонецОбласти\n\
Перем Контекст;         // <- Ошибка\n\
\n\
#Область ПрограммныйИнтерфейс\n\
Функция ПолучитьИмя() Экспорт\n\
    Возврат \"Имя\";\n\
КонецФункции\n\
#КонецОбласти\n\
#Иначе\n\
Процедура Временная()    // <- Ошибка\n\
    #Область ЛокальнаяЛогика\n\
    Сообщить(100);\n\
    #КонецОбласти\n\
КонецПроцедуры\n\
#КонецЕсли\n\
\n\
Процедура Подготовить()  // <- Ошибка\n\
    #Область Вложенная\n\
    Сообщить(\"Подготовка\");\n\
    #КонецОбласти\n\
КонецПроцедуры\n\
\n\
///////////////////////////////////////////\n\
// раздел инициализации\n\
///////////////////////////////////////////\n\
\n\
#Если Сервер Тогда\n\
#Область СлужебныеПроцедурыИФункции\n\
Функция ПолучитьЧисло()\n\
 Сообщить(42);\n\
КонецФункции\n\
#КонецОбласти\n\
#КонецЕсли\n\
\n\
#Область Инициализация\n\
Кэш = 78;\n\
#КонецОбласти\n\
\n\
Настройки = ПолучитьИмя() + Кэш; // <- Ошибка\n\
\n\
#Область Инициализация\n\
Если Условие Тогда\n\
    Контекст = 79;\n\
КонецЕсли;\n\
#КонецОбласти\n\
\n\
#Область ЛокальныеДанные\n\
    Значение = 5;\n\
#КонецОбласти\n\
Значение = Контекст;            // <- Ошибка\n\
\n\
Если Условие Тогда              // <- Ошибка\n\
#Если Сервер Тогда\n\
Сообщить(\"Так оформлять нельзя\");\n\
#ИначеЕсли Клиент Тогда\n\
Сообщить(\"И так тоже нельзя\");\n\
#Иначе\n\
#Область Обработчики\n\
Сообщить(\"Область внутри ветки не спасает\");\n\
#КонецОбласти\n\
#КонецЕсли\n\
КонецЕсли";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 7, "Expected 7 diagnostics");

        // Diagnostic 0: Перем Кэш; (line 5, whole declaration)
        assert_diagnostic_range(code, &diagnostics[0], 4, 0, 10);

        // Diagnostic 1: Перем Контекст; (line 10, whole declaration)
        assert_diagnostic_range(code, &diagnostics[1], 9, 0, 15);

        // Diagnostic 2: Процедура Временная() (line 18, procedure name only)
        assert_diagnostic_range(code, &diagnostics[2], 17, 10, 19);

        // Diagnostic 3: Процедура Подготовить() (line 25, procedure name only)
        assert_diagnostic_range(code, &diagnostics[3], 24, 10, 21);

        // Diagnostic 4: Настройки = ПолучитьИмя() + Кэш; (line 47, statement including semicolon)
        assert_diagnostic_range(code, &diagnostics[4], 46, 0, 32);

        // Diagnostic 5: Значение = Контекст; (line 58, statement including semicolon)
        assert_diagnostic_range(code, &diagnostics[5], 57, 0, 20);

        // Diagnostic 6: Если Условие Тогда (lines 60-70, if block)
        assert_diagnostic_range_multiline(code, &diagnostics[6], 59, 0, 69, 9);
    }

    #[test]
    fn test_empty_file() {
        let code = "\n";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_regions() {
        let code = "//////////////////////////////////////////////\n\
// Модуль без областей\n\
//////////////////////////////////////////////\n\
\n\
Перем Кэш;\n\
Перем Контекст;\n\
\n\
Функция ПолучитьИмя() Экспорт\n\
    Возврат \"Имя\";\n\
КонецФункции\n\
\n\
Процедура Подготовить()\n\
    Сообщить(\"Подготовка\");\n\
КонецПроцедуры\n\
\n\
///////////////////////////////////////////\n\
// инициализация\n\
///////////////////////////////////////////\n\
\n\
Кэш = 78;\n\
\n\
Контекст = ПолучитьИмя() + Кэш;";
        let diagnostics = check_ast_diagnostic(code, check);

        // Returns individual diagnostics for each element when no regions exist
        assert_eq!(diagnostics.len(), 6);

        // Diagnostic 0: Перем Кэш; (line 5)
        assert_diagnostic_range(code, &diagnostics[0], 4, 0, 10);

        // Diagnostic 1: Перем Контекст; (line 6)
        assert_diagnostic_range(code, &diagnostics[1], 5, 0, 15);

        // Diagnostic 2: Функция ПолучитьИмя() (line 8, function name)
        assert_diagnostic_range(code, &diagnostics[2], 7, 8, 19);

        // Diagnostic 3: Процедура Подготовить() (line 12, procedure name)
        assert_diagnostic_range(code, &diagnostics[3], 11, 10, 21);

        // Diagnostic 4: Кэш = 78; (line 20, including semicolon)
        assert_diagnostic_range(code, &diagnostics[4], 19, 0, 9);

        // Diagnostic 5: Контекст = ПолучитьИмя() + Кэш; (line 22, including semicolon)
        assert_diagnostic_range(code, &diagnostics[5], 21, 0, 31);
    }

    #[test]
    fn test_standard_preproc() {
        let code = "#Если Сервер Или ТолстыйКлиентОбычноеПриложение Или ВнешнееСоединение Тогда\n\
#Область СлужебныйПрограммныйИнтерфейс\n\
#КонецОбласти\n\
#Иначе\n\
  ВызватьИсключение НСтр(\"ru = 'Недопустимый вызов на клиенте.'\");\n\
#КонецЕсли";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_execute() {
        let code = "\nПроцедура Запустить()\n\nКонецПроцедуры\n";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1);

        // Diagnostic 0: Процедура Запустить() (line 2, procedure name only)
        assert_diagnostic_range(code, &diagnostics[0], 1, 10, 19);
    }

    #[test]
    fn test_code_block() {
        let code = "Сообщить(\"Сегодня\");";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1);

        // Diagnostic 0: Сообщить("Сегодня"); (line 1, including semicolon)
        assert_diagnostic_range(code, &diagnostics[0], 0, 0, 20);
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

    #[test]
    fn test_goto_stmt_outside_region_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Перейти ~Метка;"#,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#"
                CodeOutOfRegion @ 1:1..1:16
                  message: Элемент кода находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint"#]],
        );
    }

    #[test]
    fn test_label_stmt_outside_region_snapshot() {
        check_diagnostics_snapshot_for(
            r#"~Метка:"#,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_execute_stmt_outside_region_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Выполнить("код");"#,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#"
                CodeOutOfRegion @ 1:1..1:18
                  message: Элемент кода находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint"#]],
        );
    }

    #[test]
    fn test_add_handler_stmt_outside_region_snapshot() {
        check_diagnostics_snapshot_for(
            r#"ДобавитьОбработчик ИмяСобытия, ОбработчикСобытия;"#,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#"
                CodeOutOfRegion @ 1:1..1:50
                  message: Элемент кода находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint"#]],
        );
    }

    #[test]
    fn test_remove_handler_stmt_outside_region_snapshot() {
        check_diagnostics_snapshot_for(
            r#"УдалитьОбработчик ИмяСобытия, ОбработчикСобытия;"#,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#"
                CodeOutOfRegion @ 1:1..1:49
                  message: Элемент кода находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint"#]],
        );
    }

    #[test]
    fn test_standalone_raise_stmt_outside_region_snapshot() {
        check_diagnostics_snapshot_for(
            r#"ВызватьИсключение;"#,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_pre_region_dir_covers_inner_code_but_not_following_stmt_snapshot() {
        check_diagnostics_snapshot_for(
            r#"#Область Инициализация
Сообщить("Внутри");
#КонецОбласти

Сообщить("Снаружи");"#,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#"
                CodeOutOfRegion @ 5:1..5:21
                  message: Элемент кода находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint"#]],
        );
    }
}
