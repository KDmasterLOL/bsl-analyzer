//! IfElseIfEndsWithElse diagnostic
//!
//! Detects if-elseif chains that don't end with else clause.
//!
//! ## Why?
//! If-elseif chains without else can lead to unhandled cases:
//! - All possible branches should be covered
//! - Else clause makes code intentions explicit
//! - Prevents silent bugs from unhandled conditions
//! - Better code readability
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест(Значение)
//!     Если Значение = 1 Тогда
//!         // ...
//!     ИначеЕсли Значение = 2 Тогда
//!         // ...
//!     КонецЕсли; // Missing else!
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Тест(Значение)
//!     Если Значение = 1 Тогда
//!         // ...
//!     ИначеЕсли Значение = 2 Тогда
//!         // ...
//!     Иначе
//!         // Handle other cases
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! ## Implementation
//!
//! Migrated to HIR-based collection (rust-analyzer pattern).
//!
//! Source: bsl-language-server/src/main/java/.../diagnostics/IfElseIfEndsWithElseDiagnostic.java
//! Source: bsl-language-server-rust/crates/bsl-diagnostics/src/rules/if_else_if_ends_with_else.rs

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when IfElseIfEndsWithElse diagnostic is emitted during lowering.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::IfElseIfEndsWithElse,
        "Конструкция Если-ИначеЕсли должна заканчиваться блоком Иначе",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;
    #[test]
    fn test_if_elsif_without_else() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    ИначеЕсли Значение = 2 Тогда
        Сообщить("Два");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let endif_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::IfElseIfEndsWithElse).collect();

        // Should detect missing else
        assert_eq!(endif_diags.len(), 1);
        assert_eq!(endif_diags[0].code, DiagnosticCode::IfElseIfEndsWithElse);
    }

    #[test]
    fn test_if_elsif_with_else() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    ИначеЕсли Значение = 2 Тогда
        Сообщить("Два");
    Иначе
        Сообщить("Другое");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let endif_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::IfElseIfEndsWithElse).collect();

        // Should not detect - has else
        assert_eq!(endif_diags.len(), 0);
    }

    #[test]
    fn test_simple_if_without_elsif() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let endif_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::IfElseIfEndsWithElse).collect();

        // Should not detect - no elsif
        assert_eq!(endif_diags.len(), 0);
    }

    #[test]
    fn test_if_else_without_elsif() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    Иначе
        Сообщить("Другое");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let endif_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::IfElseIfEndsWithElse).collect();

        // Should not detect - no elsif
        assert_eq!(endif_diags.len(), 0);
    }

    #[test]
    fn test_multiple_elsif_without_else() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    ИначеЕсли Значение = 2 Тогда
        Сообщить("Два");
    ИначеЕсли Значение = 3 Тогда
        Сообщить("Три");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let endif_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::IfElseIfEndsWithElse).collect();

        // Should detect missing else
        assert_eq!(endif_diags.len(), 1);
    }

    #[test]
    fn test_multiple_if_statements() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    ИначеЕсли Значение = 2 Тогда
        Сообщить("Два");
    КонецЕсли;

    Если Значение = 3 Тогда
        Сообщить("Три");
    ИначеЕсли Значение = 4 Тогда
        Сообщить("Четыре");
    Иначе
        Сообщить("Другое");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let endif_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::IfElseIfEndsWithElse).collect();

        // Should detect only first if (missing else)
        assert_eq!(endif_diags.len(), 1);
    }

    #[test]
    fn test_nested_if_elsif() {
        let code = r#"Процедура Тест(Значение1, Значение2)
    Если Значение1 = 1 Тогда
        Если Значение2 = 1 Тогда
            Сообщить("1-1");
        ИначеЕсли Значение2 = 2 Тогда
            Сообщить("1-2");
        КонецЕсли;
    ИначеЕсли Значение1 = 2 Тогда
        Сообщить("2");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let endif_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::IfElseIfEndsWithElse).collect();

        // Should detect both (nested and outer)
        assert_eq!(endif_diags.len(), 2);
    }

    /// Test with actual fixture file from bsl-language-server
    /// Expected: 1 diagnostic at line 20, columns 0-9 (КонецЕсли)
    #[test]
    fn test_if_else_if_ends_with_else() {
        let code = include_str!("../../test_data/IfElseIfEndsWithElseDiagnostic.bsl");

        let diagnostics = check_hir_diagnostic(code);
        let endif_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::IfElseIfEndsWithElse).collect();

        // Java test expects: assertThat(diagnostics).hasSize(1);
        assert_eq!(endif_diags.len(), 1, "Expected 1 diagnostic");

        // Verify the diagnostic range matches Java implementation
        // Java: assertThat(diagnostics, true).hasRange(20, 0, 20, 9);
        assert_diagnostic_range(code, endif_diags[0], 20, 0, 9);
    }
}
