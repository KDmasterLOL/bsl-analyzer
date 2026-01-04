//! CompilationDirectiveLost diagnostic.
//!
//! Detects functions/procedures without compilation directives (&НаСервере, &НаКлиенте, etc.)
//! in FormModule and CommandModule contexts.
//!
//! ## Why?
//! In form modules and command modules, every procedure/function should have a compilation
//! directive indicating where it executes (&AtServer, &AtClient, &AtServerNoContext, etc.).
//! Missing directives lead to unpredictable behavior.
//!
//! ## Bad practice
//! ```bsl
//! &НаСервере
//! Процедура МетодНаСервере()
//! КонецПроцедуры
//!
//! Процедура МетодБезДирективы()  // Error! Missing &НаСервере or &НаКлиенте
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! &НаСервере
//! Процедура МетодНаСервере()
//! КонецПроцедуры
//!
//! &НаКлиенте
//! Процедура МетодНаКлиенте()
//! КонецПроцедуры
//! ```
//!
//! ## Implementation
//!
//! Ported from:
//! - CompilationDirectiveLostDiagnostic.java (bsl-language-server) - PRIMARY
//! - compilation_directive_lost.rs (bsl-language-server-rust) - REFERENCE
//!
//! Only applies to FormModule and CommandModule (not CommonModule).

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::ast::{AstNode, FunctionDef, ProcedureDef};
use syntax::SyntaxNode;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CompilationDirectiveLost) {
        return Vec::new();
    }

    if !is_form_or_command_module(ctx) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    find_methods_without_directives(&root)
}

fn is_form_or_command_module(ctx: &DiagnosticsContext) -> bool {
    let file_path = match ctx.file_path() {
        Some(path) => path,
        None => return false,
    };

    matches!(
        ide_db::metadata::get_module_type_from_uri(&file_path),
        Some(bsl_metadata::ModuleType::FormModule) | Some(bsl_metadata::ModuleType::CommandModule)
    )
}

fn find_methods_without_directives(root: &SyntaxNode) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if let Some(procedure) = ProcedureDef::cast(node.clone()) {
            if procedure.annotations().next().is_none() {
                if let Some(name_token) = procedure.name() {
                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::CompilationDirectiveLost,
                        message: format!(
                            "Пропущена директива компиляции для '{}'. \
                             В модулях форм и команд требуется указывать \
                             &НаСервере, &НаКлиенте и т.д.",
                            name_token.text()
                        ),
                        severity: Severity::Warning,
                        range: name_token.text_range(),
                        tags: vec![],
                        fixes: vec![],
                    });
                }
            }
        }

        if let Some(function) = FunctionDef::cast(node.clone()) {
            if function.annotations().next().is_none() {
                if let Some(name_token) = function.name() {
                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::CompilationDirectiveLost,
                        message: format!(
                            "Пропущена директива компиляции для '{}'. \
                             В модулях форм и команд требуется указывать \
                             &НаСервере, &НаКлиенте и т.д.",
                            name_token.text()
                        ),
                        severity: Severity::Warning,
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
    use crate::test_utils::assert_diagnostic_range;

    fn check_without_module_type(code: &str) -> Vec<Diagnostic> {
        let parse = parser::parse(code);
        let root = parse.syntax_node();
        find_methods_without_directives(&root)
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CompilationDirectiveLostDiagnostic.bsl");
        let diagnostics = check_without_module_type(code);

        assert_eq!(diagnostics.len(), 1, "Should find exactly 1 diagnostic");

        // Line 10 (1-indexed) = line 9 (0-indexed)
        // "Функция СОшибкой()" - name "СОшибкой" at columns 8-16
        assert_diagnostic_range(code, &diagnostics[0], 9, 8, 16);
    }

    #[test]
    fn test_with_directive() {
        let code = "&НаСервере\nПроцедура А()\nКонецПроцедуры";
        let diagnostics = check_without_module_type(code);
        assert_eq!(diagnostics.len(), 0, "Should not report methods with directives");
    }

    #[test]
    fn test_without_directive() {
        let code = "Процедура БезДирективы()\nКонецПроцедуры";
        let diagnostics = check_without_module_type(code);
        assert_eq!(diagnostics.len(), 1, "Should report methods without directives");
    }

    #[test]
    fn test_mixed() {
        let code = r#"
&НаСервере
Процедура А()
КонецПроцедуры

&НаКлиенте
Функция Б()
КонецФункции

Функция СОшибкой()
КонецФункции
"#;
        let diagnostics = check_without_module_type(code);
        assert_eq!(diagnostics.len(), 1, "Should report only methods without directives");
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
&AtServer
Procedure ServerMethod()
EndProcedure

Function MissingDirective()
EndFunction
"#;
        let diagnostics = check_without_module_type(code);
        assert_eq!(diagnostics.len(), 1, "Should work with English keywords");
    }

    #[test]
    fn test_multiple_missing() {
        let code = r#"
Процедура Первая()
КонецПроцедуры

Функция Вторая()
КонецФункции

&НаКлиенте
Процедура Третья()
КонецПроцедуры

Процедура Четвёртая()
КонецПроцедуры
"#;
        let diagnostics = check_without_module_type(code);
        assert_eq!(diagnostics.len(), 3, "Should report all methods without directives");
    }
}
