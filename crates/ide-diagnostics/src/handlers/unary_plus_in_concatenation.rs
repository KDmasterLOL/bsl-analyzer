//! Detects accidental unary plus inside string concatenation.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::UnaryPlusInConcatenation,
        "Унарный плюс в конкатенации строк",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_basic() {
        let code = r#"Процедура Тест()
    Плохо = "Строка1" + + "Строка2";
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnaryPlusInConcatenation,
            expect![[r#"
                UnaryPlusInConcatenation @ 2:25..2:26
                  message: Унарный плюс в конкатенации строк
                  severity: Blocker"#]],
        );
    }

    #[test]
    fn test_numeric_literal_ok() {
        let code = r#"Процедура Тест()
    Допустимо = "Хорошо" + + 5;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnaryPlusInConcatenation,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_fixture() {
        let code = r#"Процедура Тест()

// Проверка не сработает
Хорошо = "Строка1" + "Строка2";

// Проверка сработает
Плохо = "Строка1" + + "Строка2";

// Проверка сработает
Плохо = "Строка0" + ("Строка1" + + "Строка2");

// Проверка не сработает
ОченьХорошо = Хорошо + Плохо;

// Проверка не сработает
Допустимо = Хорошо + + 5;

// Проверка не сработает
ТожеДопустимо = "Хорошо" + + 5;

// Проверка не сработает
ВообщеМинус = 5 + - 5;

// Проверка сработает
ОченьПлохо = Плохо + + Допустимо;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnaryPlusInConcatenation,
            expect![[r#"
                UnaryPlusInConcatenation @ 7:21..7:22
                  message: Унарный плюс в конкатенации строк
                  severity: Blocker
                UnaryPlusInConcatenation @ 10:34..10:35
                  message: Унарный плюс в конкатенации строк
                  severity: Blocker
                UnaryPlusInConcatenation @ 25:22..25:23
                  message: Унарный плюс в конкатенации строк
                  severity: Blocker"#]],
        );
    }
}
