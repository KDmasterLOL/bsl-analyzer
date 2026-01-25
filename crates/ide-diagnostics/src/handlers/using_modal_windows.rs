//! UsingModalWindows diagnostic
//!
//! Detects usage of modal window methods (Вопрос, Предупреждение, etc.).
//!
//! Modal windows block execution and are not allowed when configuration
//! has modality mode disabled. Each modal method has a non-modal replacement.
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! The diagnostic is emitted in `hir-def/body/lower/expr.rs` when a global
//! modal method call is encountered.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub fn from_hir(
    method_name: &str,
    replacement: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::UsingModalWindows;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let message = format!(
        "Вместо модального метода \"{}\" необходимо использовать \"{}\"",
        method_name, replacement
    );

    Some(Diagnostic {
        code,
        message,
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
    fn test_using_modal_windows() {
        let code = include_str!("../../test_data/UsingModalWindowsDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let modal_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();

        assert_eq!(modal_diags.len(), 12, "Expected 12 modal window diagnostics");

        // Вопрос (multiline call)
        assert_diagnostic_range_multiline(code, modal_diags[0], 2, 12, 3, 57);

        // Предупреждение
        assert_diagnostic_range(code, modal_diags[1], 21, 4, 84);

        // ОткрытьЗначение
        assert_diagnostic_range(code, modal_diags[2], 29, 4, 26);

        // ВвестиДату (inside Если)
        assert_diagnostic_range(code, modal_diags[3], 43, 9, 58);

        // ВвестиЗначение (inside Если)
        assert_diagnostic_range(code, modal_diags[4], 72, 9, 67);

        // ВвестиСтроку (inside Если)
        assert_diagnostic_range(code, modal_diags[5], 103, 9, 50);

        // ВвестиЧисло (inside Если)
        assert_diagnostic_range(code, modal_diags[6], 122, 9, 61);

        // УстановитьВнешнююКомпоненту
        assert_diagnostic_range(code, modal_diags[7], 138, 4, 50);

        // ОткрытьФормуМодально
        assert_diagnostic_range(code, modal_diags[8], 148, 4, 33);

        // УстановитьРасширениеРаботыСФайлами (inside #Если)
        assert_diagnostic_range(code, modal_diags[9], 159, 20, 56);

        // УстановитьРасширениеРаботыСКриптографией (inside #Если)
        assert_diagnostic_range(code, modal_diags[10], 172, 20, 62);

        // ПоместитьФайл
        assert_diagnostic_range(code, modal_diags[11], 186, 4, 88);
    }

    #[test]
    fn test_no_modal_windows() {
        let code = r#"
Процедура Тест()
    // Non-modal methods should not trigger diagnostic
    ПоказатьВопрос(Оповещение, "Текст?", РежимДиалогаВопрос.ДаНет);
    ПоказатьПредупреждение(, "Текст");
    ПоказатьЗначение(, Значение);
    ПоказатьВводДаты(Оповещение, Дата, "Подсказка");
    ОткрытьФорму("Форма");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let modal_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingModalWindows).collect();
        assert_eq!(modal_diags.len(), 0);
    }
}
