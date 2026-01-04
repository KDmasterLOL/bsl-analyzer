//! CommandModuleExportMethods diagnostic.
//!
//! Проверяет что модули команд не содержат экспортных методов.
//!
//! ## Почему?
//! Экспортные модификаторы в модулях команд не имеют эффекта и бессмысленны.
//!
//! ## Плохая практика
//! ```bsl
//! Процедура ВыполнитьКоманду() Экспорт  // ← Экспорт не работает!
//! КонецПроцедуры
//! ```
//!
//! ## Хорошая практика
//! ```bsl
//! Процедура ВыполнитьКоманду()  // ← Без ключевого слова Экспорт
//! КонецПроцедуры
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::ast::{AstNode, FunctionDef, ProcedureDef};
use syntax::SyntaxNode;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CommandModuleExportMethods) {
        return Vec::new();
    }

    if !is_command_module(ctx) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    find_exported_methods(&root)
}

fn is_command_module(ctx: &DiagnosticsContext) -> bool {
    let file_path = match ctx.file_path() {
        Some(path) => path,
        None => return false,
    };

    matches!(
        ide_db::metadata::get_module_type_from_uri(&file_path),
        Some(bsl_metadata::ModuleType::CommandModule)
    )
}

fn find_exported_methods(root: &SyntaxNode) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if let Some(procedure) = ProcedureDef::cast(node.clone()) {
            if procedure.export_keyword().is_some() {
                if let Some(name_token) = procedure.name() {
                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::CommandModuleExportMethods,
                        message: "Экспортные методы в модулях команд не имеют смысла".to_string(),
                        severity: Severity::Information,
                        range: name_token.text_range(),
                        tags: vec![],
                        fixes: vec![],
                    });
                }
            }
        }

        if let Some(function) = FunctionDef::cast(node.clone()) {
            if function.export_keyword().is_some() {
                if let Some(name_token) = function.name() {
                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::CommandModuleExportMethods,
                        message: "Экспортные методы в модулях команд не имеют смысла".to_string(),
                        severity: Severity::Information,
                        range: name_token.text_range(),
                        tags: vec![],
                        fixes: vec![],
                    });
                }
            }
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range_multiline;

    fn check_as_command_module(code: &str) -> Vec<Diagnostic> {
        let parse = parser::parse(code);
        let root = parse.syntax_node();
        find_exported_methods(&root)
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CommandModuleExportMethodsDiagnostic.bsl");
        let diagnostics = check_as_command_module(code);

        assert_eq!(diagnostics.len(), 2, "Expected 2 diagnostics");

        // Строка 0, колонки 10-15: имя "Тест1"
        assert_diagnostic_range_multiline(code, &diagnostics[0], 0, 10, 0, 15);

        // Строка 6, колонки 8-13: имя "Тест3"
        assert_diagnostic_range_multiline(code, &diagnostics[1], 6, 8, 6, 13);
    }

    #[test]
    fn test_non_exported_ignored() {
        let code = r#"
Процедура Тест2()
КонецПроцедуры

Функция Тест4()
    Возврат 0;
КонецФункции
"#;
        let diagnostics = check_as_command_module(code);
        assert_eq!(diagnostics.len(), 0, "Non-exported methods should be ignored");
    }

    #[test]
    fn test_exported_procedure() {
        let code = "Процедура Тест1() Экспорт\nКонецПроцедуры";
        let diagnostics = check_as_command_module(code);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for exported procedure");
    }

    #[test]
    fn test_exported_function() {
        let code = "Функция Тест3() Экспорт\n    Возврат 0;\nКонецФункции";
        let diagnostics = check_as_command_module(code);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for exported function");
    }
}
