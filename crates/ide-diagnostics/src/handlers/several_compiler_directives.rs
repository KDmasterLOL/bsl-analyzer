//! SeveralCompilerDirectives diagnostic.
//!
//! Checks that a module variable or method has no more than one compiler directive.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::SeveralCompilerDirectives) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let item_tree = ctx.item_tree();

    for (_, proc) in item_tree.procedures() {
        if proc.annotations.len() > 1 {
            diagnostics.push(make_diagnostic(proc.name_range));
        }
    }

    for (_, func) in item_tree.functions() {
        if func.annotations.len() > 1 {
            diagnostics.push(make_diagnostic(func.name_range));
        }
    }

    for (_, var) in item_tree.variables() {
        if var.annotations.len() > 1 {
            diagnostics.push(make_diagnostic(var.name_range));
        }
    }

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

fn make_diagnostic(range: TextRange) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::SeveralCompilerDirectives,
        message: "Указано более одной директивы компиляции".to_string(),
        severity: Severity::Error,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;

    #[test]
    fn test_from_java_fixture() {
        let code = include_str!("../fixtures/SeveralCompilerDirectivesDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, super::check);

        assert_eq!(diagnostics.len(), 5);

        // Variables (lines 16, 20, 26 - 1-indexed)
        assert_diagnostic_range(code, &diagnostics[0], 15, 6, 30);
        assert_diagnostic_range(code, &diagnostics[1], 19, 6, 30);
        assert_diagnostic_range(code, &diagnostics[2], 25, 6, 30);

        // Methods (lines 41, 50 - 1-indexed)
        assert_diagnostic_range(code, &diagnostics[3], 40, 10, 34);
        assert_diagnostic_range(code, &diagnostics[4], 49, 10, 34);
    }

    #[test]
    fn test_single_directive_ok() {
        let code = "&НаКлиенте\nПерем ОК;";
        let diagnostics = check_ast_diagnostic(code, super::check);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_no_directive_ok() {
        let code = "Перем ОК;\n\nПроцедура Тест()\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, super::check);
        assert!(diagnostics.is_empty());
    }
}
