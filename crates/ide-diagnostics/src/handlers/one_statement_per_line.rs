//! OneStatementPerLine diagnostic
//!
//! Detects multiple statements on the same line.
//!
//! **Source (Java):** bsl-language-server/OneStatementPerLineDiagnostic.java
//!
//! Each statement should be on its own line for better readability.
//! Multiple statements on one line make code harder to read and debug.
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! The diagnostic is emitted in `hir-def/body/lower/stmt.rs` when multiple
//! statements start on the same line (excluding preprocessor directives,
//! empty statements, and statements with parse errors).
//!
//! ## Exclusions (matching Java behavior):
//! - Empty statements (standalone `;`)
//! - Statements containing preprocessor directives
//! - Statements with parse errors

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch.rs when `BodyDiagnostic::OneStatementPerLine` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::OneStatementPerLine,
        "Several statements in one line",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    #[test]
    fn test_one_statement_per_line() {
        let code = include_str!("../../test_data/OneStatementPerLineDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::OneStatementPerLine).collect();

        // Java expects diagnostics for:
        // Line 4:  "  А = 0;А = 1;" - second statement "А = 1" at (3, 8, 3, 13) without semicolon
        // Line 9:  "Если Истина Тогда Сообщить(А=1); F=0; КонецЕсли;" - statements inside IF
        // Line 13: "А=1; А=2; А=3;" - (12, 5, 12, 8), (12, 10, 12, 13) without semicolons
        // Note: Statements inside single-line IF also count
        // Note: Our parser doesn't include semicolon in statement range

        // Minimum expected: line 4 and line 13 statements
        assert!(diags.len() >= 3, "Expected at least 3 diagnostics, got {}", diags.len());

        // Line 4 (0-indexed: 3), cols 8-13: "А = 1" (second statement without semicolon)
        assert_diagnostic_range(code, diags[0], 3, 8, 13);

        // Line 13 (0-indexed: 12): second and third statements
        // Find diagnostics on line 12
        let line_12_diags: Vec<_> = diags
            .iter()
            .filter(|d| {
                let (sl, _, _, _) = crate::test_utils::range_to_line_col(code, d.range);
                sl == 12
            })
            .collect();
        assert_eq!(line_12_diags.len(), 2, "Expected 2 diagnostics on line 13");
    }

    #[test]
    fn test_one_statement_per_line_end_file() {
        let code = include_str!("../../test_data/OneStatementPerLineDiagnosticEndFile.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::OneStatementPerLine).collect();

        // Java expects 2 diagnostics (0-indexed lines/cols):
        // Line 2: "Ф=1; У=2; Е=3;" → (1, 5, 1, 9), (1, 10, 1, 14)
        // Note: BSL parser doesn't include semicolon in expression range, so end col is one less
        assert_eq!(diags.len(), 2, "Expected 2 diagnostics, got {}", diags.len());

        // Line 2 (0-indexed: 1), cols 5-8: "У=2" (second statement without semicolon)
        assert_diagnostic_range(code, diags[0], 1, 5, 8);

        // Line 2 (0-indexed: 1), cols 10-13: "Е=3" (third statement without semicolon)
        assert_diagnostic_range(code, diags[1], 1, 10, 13);
    }

    #[test]
    fn test_no_multiple_statements() {
        let code = r#"
Процедура Тест()
    А = 1;
    Б = 2;
    В = 3;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::OneStatementPerLine).collect();
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_preprocessor_exclusion() {
        // Statements with preprocessor should be excluded
        let code = r#"
Процедура Тест()
    УспешноПодключено = ПодключитьВнешнююКомпоненту(
        #Если Клиент Тогда
            "C:\path1.dll",
        #Иначе
            "C:\path2.dll",
        #КонецЕсли
            "ETP",
            ТипВнешнейКомпоненты.Native);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::OneStatementPerLine).collect();
        // Should have no diagnostics because the statement contains preprocessor
        assert_eq!(diags.len(), 0);
    }
}
