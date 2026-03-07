//! IfElseDuplicatedCodeBlock diagnostic
//!
//! Detects identical code blocks in if/elseif/else branches.
//!
//! ## Why?
//! When if/else branches contain identical code, the condition is meaningless
//! and the code should be simplified.
//!
//! ## Bad practice
//! ```bsl
//! Если Условие Тогда
//!     ПоказатьПредупреждение("Ошибка");
//!     Возврат;
//! Иначе
//!     ПоказатьПредупреждение("Ошибка");
//!     Возврат;
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Remove the condition, keep the common code
//! ПоказатьПредупреждение("Ошибка");
//! Возврат;
//! ```
//!

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::IfElseDuplicatedCodeBlock` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::IfElseDuplicatedCodeBlock,
        "Ветки Если и Иначе содержат идентичный код",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic;
    use crate::DiagnosticCode;
    #[test]
    fn test_simple_if_else_duplicate() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    Иначе
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 1, "Expected 1 diagnostic for duplicate if/else blocks");
    }

    #[test]
    fn test_different_blocks() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
        ПоказатьПредупреждение("Ошибка 1");
    Иначе
        ПоказатьПредупреждение("Ошибка 2");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 0, "Should not report different blocks");
    }

    #[test]
    fn test_elsif_duplicate() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    ИначеЕсли x = 2 Тогда
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 1, "Expected 1 diagnostic for duplicate if/elsif blocks");
    }

    #[test]
    fn test_empty_blocks_ignored() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
    ИначеЕсли x = 2 Тогда
    Иначе
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 0, "Empty blocks should be ignored");
    }

    /// Empty blocks across all branches should not trigger duplicate detection
    #[test]
    fn test_empty_blocks_all_branches() {
        let code = r#"Процедура Тест()
    Если ПараметрКоманды.Количество() = 0 Тогда
    ИначеЕсли ПараметрКоманды.Количество() = 1 Тогда
    Иначе
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 0, "Empty blocks should not trigger duplicate detection");
    }

    /// If and Else with identical two-statement blocks should warn
    #[test]
    fn test_if_else_two_statement_duplicate() {
        let code = r#"Процедура Тест()
    Если ПараметрКоманды.Количество() = 0 Тогда
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        Возврат;
    Иначе
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 1, "Identical if/else two-statement blocks should warn");
    }

    /// If block differs from Else block (different statement count) - should not warn
    #[test]
    fn test_if_else_different_statement_count() {
        let code = r#"Процедура Тест()
    Если ПараметрКоманды.Количество() = 0 Тогда
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
    Иначе
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 0, "Different statement counts should not warn");
    }

    /// If/ElseIf with identical blocks should warn
    #[test]
    fn test_if_elseif_duplicate_with_else() {
        let code = r#"Процедура Тест()
    Если ПараметрКоманды.Количество() = 0 Тогда
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        Возврат;
    ИначеЕсли ПараметрКоманды.Количество() = 1 Тогда
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        Возврат;
    Иначе
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 1, "If/ElseIf with identical blocks should warn");
    }

    /// Nested if with duplicates inside outer if and else branches (3 diagnostics total)
    #[test]
    fn test_nested_duplicates_in_outer_branches() {
        let code = r#"Процедура Тест()
    Если ТипЗнч(ПараметрКоманды) = Тип("Массив") Тогда
        Если ПараметрКоманды.Количество() = 0 Тогда
            ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
            Возврат;
        ИначеЕсли ПараметрКоманды.Количество() = 1 Тогда
            ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
            Возврат;
        Иначе
            ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        КонецЕсли;
    Иначе
        Если ПараметрКоманды.Количество() = 0 Тогда
            ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
            Возврат;
        ИначеЕсли ПараметрКоманды.Количество() = 1 Тогда
            ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
            Возврат;
        Иначе
            ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 3, "Outer if/else and both inner if/elseif chains should warn");
    }
}
