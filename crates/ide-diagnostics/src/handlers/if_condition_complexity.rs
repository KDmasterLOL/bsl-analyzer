//! IfConditionComplexity diagnostic.
//!
//! Detects overly complex if conditions with too many boolean operations.
//!
//! ## Why?
//! Complex if conditions are hard to understand:
//! - Reduced readability
//! - Difficult to debug
//! - Error-prone
//! - Should be extracted to variables
//!
//! ## Bad practice
//! ```bsl
//! Если А И Б ИЛИ В И Г Тогда  // Too complex!
//!     ВыполнитьДействие();
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! ```bsl
//! УсловиеВыполнено = (А И Б) ИЛИ (В И Г);
//! Если УсловиеВыполнено Тогда
//!     ВыполнитьДействие();
//! КонецЕсли;
//! ```
//!
//! ## Implementation
//!
//! Migrated to HIR-based collection.
//!
//! Ported from:
//!
//! Adapted to use Rowan SyntaxNode during HIR lowering.
//!
//! ### Key algorithm:
//! - Rust: Count all BINARY_EXPR nodes with AND/OR operators + 1
//! - Default max complexity: 3
//!
//! ### Diagnostic range:
//! - Rust: Same - entire expression range

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Default maximum if condition complexity
const DEFAULT_MAX_IF_CONDITION_COMPLEXITY: usize = 3;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when IfConditionComplexity diagnostic is emitted during lowering.
pub fn from_hir(
    complexity: usize,
    max_complexity_default: usize,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::IfConditionComplexity;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    // Get maxIfConditionComplexity parameter from config (default: 3)
    let max_complexity = ctx
        .config
        .get_int(DiagnosticCode::IfConditionComplexity, "maxIfConditionComplexity")
        .map(|v| v as usize)
        .unwrap_or(DEFAULT_MAX_IF_CONDITION_COMPLEXITY);

    // Re-check against user config (lowering used default threshold)
    if complexity <= max_complexity {
        return None;
    }

    // Update max_complexity in message to reflect actual config value
    // (lowering emitted with default, we use user config)
    let _ = max_complexity_default; // Silence unused warning

    Some(Diagnostic {
        code,
        message: format!(
            "Условие имеет сложность {} (максимум {}). Упростите условие или вынесите части в переменные.",
            complexity, max_complexity
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::{DiagnosticCode, Severity};
    /// Test simple condition (should pass)
    #[test]
    fn test_simple_condition() {
        let code = r#"Процедура Тест()
    Если А И Б Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should NOT detect - complexity = 2 (1 AND + 1 = 2)
        assert_eq!(if_diags.len(), 0);
    }

    /// Test at threshold (should pass)
    #[test]
    fn test_at_threshold() {
        let code = r#"Процедура Тест()
    Если А И Б ИЛИ В Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should NOT detect - complexity = 3 (2 ops: AND + OR = 2, complexity = 2+1 = 3)
        assert_eq!(if_diags.len(), 0);
    }

    /// Test complex condition (should fail)
    #[test]
    fn test_complex_condition() {
        let code = r#"Процедура Тест()
    Если А И Б ИЛИ В И Г Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should detect - complexity = 4 (3 ops: AND, OR, AND = 3, complexity = 3+1 = 4)
        assert_eq!(if_diags.len(), 1);
        assert_eq!(if_diags[0].code, DiagnosticCode::IfConditionComplexity);
        assert_eq!(if_diags[0].severity, Severity::Information); // CodeSmell + Minor -> Information
        assert!(if_diags[0].message.contains("сложность 4"));
        assert!(if_diags[0].message.contains("максимум 3"));
    }

    /// Test elsif clause
    #[test]
    fn test_elseif_complex() {
        let code = r#"Процедура Тест()
    Если А Тогда
        Сообщить("1");
    ИначеЕсли Б И В ИЛИ Г И Д Тогда
        Сообщить("2");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should detect in elseif - complexity = 4
        assert_eq!(if_diags.len(), 1);
        assert_eq!(if_diags[0].code, DiagnosticCode::IfConditionComplexity);
    }

    /// Test English keywords
    #[test]
    fn test_english_condition() {
        let code = r#"Procedure Test()
    If A And B Or C And D Then
        Message("OK");
    EndIf;
EndProcedure"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should detect - complexity = 4
        assert_eq!(if_diags.len(), 1);
    }

    /// Large multiline condition (9 OR ops) - should warn
    #[test]
    fn test_large_multiline_condition() {
        let code = r#"Процедура Тест()
    Если ИдентификаторОбъекта = "АнализСубконто"
        ИЛИ ИдентификаторОбъекта = "АнализСчета"
        ИЛИ ИдентификаторОбъекта = "ОборотноСальдоваяВедомость"
        ИЛИ ИдентификаторОбъекта = "ОборотноСальдоваяВедомостьПоСчету"
        ИЛИ ИдентификаторОбъекта = "ОборотыМеждуСубконто"
        ИЛИ ИдентификаторОбъекта = "ОборотыСчета"
        ИЛИ ИдентификаторОбъекта = "СводныеПроводки"
        ИЛИ ИдентификаторОбъекта = "ГлавнаяКнига"
        ИЛИ ИдентификаторОбъекта = "ШахматнаяВедомость" Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        assert_eq!(if_diags.len(), 1, "Should warn on 9-OR condition");
    }

    /// Simple outer condition (2 OR ops) should pass; nested condition (3 OR ops) should warn
    #[test]
    fn test_nested_outer_pass_inner_warn() {
        let code = r#"Процедура Тест()
    Если ИдентификаторОбъекта = "АнализСубконто"
        ИЛИ ИдентификаторОбъекта = "АнализСчета" Тогда
        Если ИдентификаторОбъекта = "ОборотыМеждуСубконто"
            ИЛИ ИдентификаторОбъекта = "ОборотыСчета"
            ИЛИ ИдентификаторОбъекта = "СводныеПроводки"
            ИЛИ ИдентификаторОбъекта = "ШахматнаяВедомость" Тогда
            Возврат;
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        assert_eq!(if_diags.len(), 1, "Only inner nested condition should warn");
    }

    /// If branch (4 OR) and ElseIf branch (6 OR) both exceed threshold
    #[test]
    fn test_if_and_elseif_both_complex() {
        let code = r#"Процедура Тест()
    Если ИдентификаторОбъекта = "ИД1"
        ИЛИ ИдентификаторОбъекта = "ИД2"
        ИЛИ ИдентификаторОбъекта = "ИД3"
        ИЛИ ИдентификаторОбъекта = "ИД4" Тогда
        Возврат;
    ИначеЕсли ИдентификаторОбъекта = "ИД5"
        ИЛИ ИдентификаторОбъекта = "ИД6"
        ИЛИ ИдентификаторОбъекта = "ИД7"
        ИЛИ ИдентификаторОбъекта = "ИД8"
        ИЛИ ИдентификаторОбъекта = "ИД9"
        ИЛИ ИдентификаторОбъекта = "ИД10"
        ИЛИ ИдентификаторОбъекта = "ИД10" Тогда
        Возврат;
    Иначе
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        assert_eq!(if_diags.len(), 2, "Both If and ElseIf branches should warn");
    }
}
