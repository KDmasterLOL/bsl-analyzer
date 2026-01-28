//! SemicolonPresence diagnostic
//!
//! Detects statements without trailing semicolon.
//!
//! **Source (Java):** bsl-language-server/SemicolonPresenceDiagnostic.java
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! The diagnostic is emitted in `hir-def/body/lower/stmt.rs` when a statement
//! AST node has no SEMICOLON token (excluding EMPTY_STMT, LABEL_STMT, and
//! statements with parse errors).

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch.rs when `BodyDiagnostic::MissingSemicolon` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::SemicolonPresence;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Пропущена точка с запятой в конце выражения".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![Fix {
            label: "Добавить точку с запятой".to_string(),
            edits: vec![TextEdit {
                range: TextRange::new(range.end(), range.end()),
                new_text: ";".to_string(),
            }],
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_semicolon_presence() {
        let code = include_str!("../../test_data/SemicolonPresenceDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SemicolonPresence).collect();

        assert_eq!(diags.len(), 2, "Expected 2 diagnostics");

        // Java: hasRange(3, 6, 3, 7) - line 4 (0-indexed=3), cols 6-7
        // "А = 0" - last token is "0" at position 6-7
        assert_diagnostic_range(code, diags[0], 3, 6, 7);

        // Java: hasRange(4, 0, 4, 9) - line 5 (0-indexed=4), cols 0-9
        // "КонецЕсли" is 9 characters
        assert_diagnostic_range(code, diags[1], 4, 0, 9);
    }

    #[test]
    fn test_no_missing_semicolons() {
        let code = r#"
Процедура Тест()
    А = 1;
    Б = 2;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SemicolonPresence).collect();
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_label_no_semicolon_required() {
        let code = r#"
Процедура Тест()
    ~Метка:
    А = 1;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SemicolonPresence).collect();
        assert_eq!(diags.len(), 0, "Labels should not require semicolons");
    }
}
