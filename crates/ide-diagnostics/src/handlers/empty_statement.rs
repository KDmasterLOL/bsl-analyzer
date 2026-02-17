//! EmptyStatement diagnostic
//!
//! Detects empty statements (standalone semicolons) in code.
//!
//! **Source (Java):** bsl-language-server/EmptyStatementDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/rules/empty_statement.rs
//!
//! Empty statements are usually typos or leftover from refactoring.
//! They make code less readable and can be confusing.
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! The diagnostic is emitted in `hir-def/body/lower/stmt.rs` when EMPTY_STMT
//! AST node is encountered during statement lowering (if no parser errors nearby).

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::EmptyStatement` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::EmptyStatement;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Empty statement".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![Fix {
            label: "Удалить пустой оператор".to_string(),
            edits: vec![TextEdit { range, new_text: String::new() }],
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    #[test]
    fn test_empty_statement() {
        let code = include_str!("../../test_data/EmptyStatementDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let empty_stmt_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyStatement).collect();

        // Java expects 2 diagnostics
        assert_eq!(empty_stmt_diags.len(), 2, "Expected 2 diagnostics");

        // Line 1 (0-indexed), cols 18-19: semicolon after "Тогда"
        assert_diagnostic_range(code, empty_stmt_diags[0], 1, 18, 19);

        // Line 2 (0-indexed), cols 8-9: second semicolon in ";;"
        assert_diagnostic_range(code, empty_stmt_diags[1], 2, 8, 9);
    }

    #[test]
    fn test_no_empty_statements() {
        let code = r#"
Процедура Тест()
    А = 1;
    Б = 2;
    Возврат А + Б;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let empty_stmt_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyStatement).collect();
        assert_eq!(empty_stmt_diags.len(), 0);
    }

    #[test]
    fn test_double_semicolon() {
        let code = r#"
Процедура Тест()
    А = 1;;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let empty_stmt_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyStatement).collect();
        assert_eq!(empty_stmt_diags.len(), 1, "Expected 1 empty statement");
    }
}
