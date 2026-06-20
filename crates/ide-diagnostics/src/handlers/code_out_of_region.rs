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

    let region_tree = ctx.region_tree();

    let mut diagnostics = Vec::new();
    check_node(&root, &region_tree, code, ctx, &mut diagnostics);

    // A module with no module-level regions is a single structural problem:
    // report it once, on the first out-of-region element, instead of nagging
    // on every module-level statement, variable and method.
    if region_tree.module_level_regions().next().is_none() {
        diagnostics.truncate(1);
    }

    diagnostics
}

fn range_with_semicolon(node: &SyntaxNode) -> ide_db::TextRange {
    use syntax::{SyntaxToken, TextSize};

    let base_range = node.text_range();

    let has_semicolon = node
        .next_sibling_or_token()
        .and_then(|t| t.into_token())
        .map(|token: SyntaxToken| token.kind() == SyntaxKind::SEMICOLON)
        .unwrap_or(false);

    if has_semicolon {
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
                _ => ("Элемент кода", range_with_semicolon(&child)),
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
    use crate::test_utils::check_diagnostics_snapshot_for;
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#"
                CodeOutOfRegion @ 5:1..5:11
                  message: Переменная находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint
                CodeOutOfRegion @ 10:1..10:16
                  message: Переменная находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint
                CodeOutOfRegion @ 18:11..18:20
                  message: Процедура находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint
                CodeOutOfRegion @ 25:11..25:22
                  message: Процедура находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint
                CodeOutOfRegion @ 47:1..47:33
                  message: Элемент кода находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint
                CodeOutOfRegion @ 58:1..58:21
                  message: Элемент кода находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint
                CodeOutOfRegion @ 60:1..70:10
                  message: Элемент кода находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint"#]],
        );
    }

    #[test]
    fn test_empty_file() {
        let code = "\n";
        check_diagnostics_snapshot_for(code, DiagnosticCode::CodeOutOfRegion, expect![[r#""#]]);
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#"
                CodeOutOfRegion @ 5:1..5:11
                  message: Переменная находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint"#]],
        );
    }

    #[test]
    fn test_region_less_module_collapses_to_single_finding() {
        let code = "Процедура Первая()\n\
КонецПроцедуры\n\
\n\
Процедура Вторая()\n\
КонецПроцедуры\n\
\n\
Функция Третья()\n\
    Возврат 1;\n\
КонецФункции";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#"
                CodeOutOfRegion @ 1:11..1:17
                  message: Процедура находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint"#]],
        );
    }

    #[test]
    fn test_standard_preproc() {
        let code = "#Если Сервер Или ТолстыйКлиентОбычноеПриложение Или ВнешнееСоединение Тогда\n\
#Область СлужебныйПрограммныйИнтерфейс\n\
#КонецОбласти\n\
#Иначе\n\
  ВызватьИсключение НСтр(\"ru = 'Недопустимый вызов на клиенте.'\");\n\
#КонецЕсли";
        check_diagnostics_snapshot_for(code, DiagnosticCode::CodeOutOfRegion, expect![[r#""#]]);
    }

    #[test]
    fn test_execute() {
        let code = "\nПроцедура Запустить()\n\nКонецПроцедуры\n";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#"
                CodeOutOfRegion @ 2:11..2:20
                  message: Процедура находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint"#]],
        );
    }

    #[test]
    fn test_code_block() {
        let code = "Сообщить(\"Сегодня\");";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#"
                CodeOutOfRegion @ 1:1..1:21
                  message: Элемент кода находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint"#]],
        );
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

        check_diagnostics_snapshot_for(code, DiagnosticCode::CodeOutOfRegion, expect![[r#""#]]);
    }

    #[test]
    fn test_code_outside_region() {
        let code = r#"
Процедура Тест()
    Сообщить("OK");
КонецПроцедуры
"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CodeOutOfRegion,
            expect![[r#"
                CodeOutOfRegion @ 2:11..2:15
                  message: Процедура находится вне области (#Область/#Region). Весь код модуля должен быть организован в области для лучшей структуры.
                  severity: Hint"#]],
        );
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
