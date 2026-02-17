//! SeveralCompilerDirectives diagnostic.
//!
//! Checks that a module variable or method has no more than one compiler directive.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Unpredictable, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::SeveralCompilerDirectives;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let item_tree = ctx.item_tree();

    for (_, proc) in item_tree.procedures() {
        if proc.annotations.len() > 1 {
            diagnostics.push(make_diagnostic(proc.name_range, code, ctx));
        }
    }

    for (_, func) in item_tree.functions() {
        if func.annotations.len() > 1 {
            diagnostics.push(make_diagnostic(func.name_range, code, ctx));
        }
    }

    for (_, var) in item_tree.variables() {
        if var.annotations.len() > 1 {
            diagnostics.push(make_diagnostic(var.name_range, code, ctx));
        }
    }

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

fn make_diagnostic(range: TextRange, code: DiagnosticCode, ctx: &DiagnosticsContext) -> Diagnostic {
    Diagnostic {
        code,
        message: "Указано более одной директивы компиляции".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    #[test]
    fn test_from_java_fixture() {
        let code = include_str!("../../test_data/SeveralCompilerDirectivesDiagnostic.bsl");
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
