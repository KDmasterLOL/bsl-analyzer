//! SetPrivilegedMode diagnostic.
//!
//! Finds calls to УстановитьПривилегированныйРежим/SetPrivilegedMode that enable
//! privileged mode (security hotspot).
//!
//! Calls with argument `Ложь`/`False` are NOT flagged (disabling privileged mode is safe).

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::SetPrivilegedMode;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Проверьте установку привилегированного режима".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_from_java_fixture() {
        let code = include_str!("../../fixtures/SetPrivilegedModeDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SetPrivilegedMode).collect();

        assert_eq!(diags.len(), 2);
        // Line 2 (0-indexed): УстановитьПривилегированныйРежим(Истина)
        assert_diagnostic_range(code, diags[0], 2, 4, 36);
        // Line 4 (0-indexed): УстановитьПривилегированныйРежим(Значение)
        assert_diagnostic_range(code, diags[1], 4, 4, 36);
    }
}
