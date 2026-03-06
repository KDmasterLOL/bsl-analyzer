//! GlobalContextMethodCollision8312 diagnostic.
//!
//! Checks for method name collisions with platform 8.3.12 global context methods.
//!
//! ## Why?
//! Starting from platform version 8.3.12, new bitwise operation methods were added
//! to the global context. User-defined methods with these names will conflict with
//! platform methods, leading to unexpected behavior.
//!
//! ## Conflicting methods (Russian/English)
//! - ПроверитьБит / CheckBit
//! - ПроверитьПоБитовойМаске / CheckByBitMask
//! - УстановитьБит / SetBit
//! - ПобитовоеИ / BitwiseAnd
//! - ПобитовоеИли / BitwiseOr
//! - ПобитовоеНе / BitwiseNot
//! - ПобитовоеИНе / BitwiseAndNot
//! - ПобитовоеИсключительноеИли / BitwiseXor
//! - ПобитовыйСдвигВлево / BitwiseShiftLeft
//! - ПобитовыйСдвигВправо / BitwiseShiftRight
//!
//! ## Bad practice
//! ```bsl
//! Функция ПроверитьБит(Число, Позиция)
//!     // Custom implementation conflicts with platform method
//!     Возврат (Число % 2) = 1;
//! КонецФункции
//! ```
//!
//! ## Good practice
//! ```bsl
//! Функция ПроверитьБитПользовательский(Число, Позиция)
//!     Возврат (Число % 2) = 1;
//! КонецФункции
//! ```
//!
//! ## Implementation
//!
//! Ported from:
//!
//! ### Algorithm:
//! - O(n) complexity: single pass through function definitions
//! - Case-insensitive matching (BSL is case-insensitive)
//! - Checks both Russian and English method names
//!
//! ### Diagnostic range:
//! - Rust: First IDENT token before PARAM_LIST (function name)
//!
//! ## References
//! - Source: https://its.1c.ru/db/metod8dev#content:5293:hdoc:pereimenovaniya_metodov_i_svojstv

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_12,
    tags: &[MetadataTag::Error, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when GlobalContextMethodCollision8312 diagnostic is emitted during lowering.
pub fn from_hir(
    method_name: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::GlobalContextMethodCollision8312;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Имя метода \"{}\" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12",
            method_name
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
    /// Integration test matching reference test structure.
    ///
    /// Based on reference test
    /// Uses the same test file: GlobalContextMethodCollision8312Diagnostic.bsl
    ///
    /// Expected: 20 diagnostics (all conflicting method names)
    /// Lines 0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 30, 33, 36, 39, 42, 45, 48, 51, 54, 57
    #[test]
    fn test_8312() {
        let code = r#"Функция ПроверитьБит()
КонецФункции

Функция ПроверитьПоБитовойМаске()
КонецФункции

Функция УстановитьБит()
КонецФункции

Функция ПобитовоеИ()
КонецФункции

Функция ПобитовоеИли()
КонецФункции

Функция ПобитовоеНе()
КонецФункции

Функция ПобитовоеИНе()
КонецФункции

Функция ПобитовоеИсключительноеИли()
КонецФункции

Функция ПобитовыйСдвигВлево()
КонецФункции

Функция ПобитовыйСдвигВправо()
КонецФункции

Функция CheckBit()
КонецФункции

Функция CheckByBitMask()
КонецФункции

Функция SetBit()
КонецФункции

Функция BitwiseAnd()
КонецФункции

Функция BitwiseOr()
КонецФункции

Функция BitwiseNot()
КонецФункции

Функция BitwiseAndNot()
КонецФункции

Функция BitwiseXor()
КонецФункции

Функция BitwiseShiftLeft()
КонецФункции

Функция BitwiseShiftRight()
КонецФункции

Функция _ПроверитьБит()
КонецФункции

Функция ПроверитьПоБитовойМаске_()
КонецФункции

Функция БИТУстановитьБит()
КонецФункции
"#;

        let diagnostics = check_hir_diagnostic(code);
        let collision_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::GlobalContextMethodCollision8312)
            .collect();

        // Expected 20 diagnostics
        assert_eq!(collision_diags.len(), 20, "Expected 20 diagnostics");

        // Verify all diagnostic ranges
        // Format: line, start_col, end_col
        let expected_ranges = [
            (0, 8, 20),  // ПроверитьБит
            (3, 8, 31),  // ПроверитьПоБитовойМаске
            (6, 8, 21),  // УстановитьБит
            (9, 8, 18),  // ПобитовоеИ
            (12, 8, 20), // ПобитовоеИли
            (15, 8, 19), // ПобитовоеНе
            (18, 8, 20), // ПобитовоеИНе
            (21, 8, 34), // ПобитовоеИсключительноеИли
            (24, 8, 27), // ПобитовыйСдвигВлево
            (27, 8, 28), // ПобитовыйСдвигВправо
            (30, 8, 16), // CheckBit
            (33, 8, 22), // CheckByBitMask
            (36, 8, 14), // SetBit
            (39, 8, 18), // BitwiseAnd
            (42, 8, 17), // BitwiseOr
            (45, 8, 18), // BitwiseNot
            (48, 8, 21), // BitwiseAndNot
            (51, 8, 18), // BitwiseXor
            (54, 8, 24), // BitwiseShiftLeft
            (57, 8, 25), // BitwiseShiftRight
        ];

        for (i, (line, start_col, end_col)) in expected_ranges.iter().enumerate() {
            assert_eq!(
                collision_diags[i].severity,
                Severity::Blocker,
                "Diagnostic {} should have Blocker severity",
                i
            );
            assert_diagnostic_range(code, collision_diags[i], *line, *start_col, *end_col);
        }
    }

    /// Test that methods with prefixes/suffixes don't trigger
    #[test]
    fn test_no_collision_with_prefix_suffix() {
        let code = r#"Функция _ПроверитьБит()
КонецФункции

Функция ПроверитьПоБитовойМаске_()
КонецФункции

Функция БИТУстановитьБит()
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let collision_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::GlobalContextMethodCollision8312)
            .collect();

        // These methods have prefixes/suffixes so they don't conflict
        assert_eq!(collision_diags.len(), 0, "Methods with prefix/suffix should not trigger");
    }

    /// Test case-insensitive matching (Russian uppercase)
    #[test]
    fn test_case_insensitive_russian() {
        let code = r#"Функция ПРОВЕРИТЬБИТ()
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let collision_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::GlobalContextMethodCollision8312)
            .collect();

        assert_eq!(collision_diags.len(), 1);
        assert_diagnostic_range(code, collision_diags[0], 0, 8, 20);
    }

    /// Test case-insensitive matching (English mixed case)
    #[test]
    fn test_case_insensitive_english() {
        let code = r#"Function CheckBit()
EndFunction"#;

        let diagnostics = check_hir_diagnostic(code);
        let collision_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::GlobalContextMethodCollision8312)
            .collect();

        assert_eq!(collision_diags.len(), 1);
        assert_diagnostic_range(code, collision_diags[0], 0, 9, 17);
    }

    /// Test multiple conflicting functions
    #[test]
    fn test_multiple_collisions() {
        let code = r#"Функция ПроверитьБит()
КонецФункции

Функция CheckBit()
КонецФункции

Функция ПобитовоеИ()
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let collision_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::GlobalContextMethodCollision8312)
            .collect();

        assert_eq!(collision_diags.len(), 3, "Should detect all 3 collisions");
    }

    /// Test non-conflicting function names
    #[test]
    fn test_no_collision() {
        let code = r#"Функция МояФункция()
КонецФункции

Функция ВычислитьСумму()
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let collision_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::GlobalContextMethodCollision8312)
            .collect();

        assert_eq!(collision_diags.len(), 0, "Non-conflicting names should not trigger");
    }
}
