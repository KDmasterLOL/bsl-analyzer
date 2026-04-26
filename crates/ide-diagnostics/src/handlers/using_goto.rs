//! Reports usage of the `Goto` / `Перейти` statement.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::UsingGoto,
        "Оператор \"Перейти\" не должен использоваться",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    #[test]
    fn test_using_goto() {
        let code = r#"Процедура БезПерейти()

    // Тут код

КонецПроцедуры

Процедура СПерейти()

    Перейти ~а;
    // Тут код
    ~а: // Вот такой маневр

КонецПроцедуры

Процедура РеализацияЦиклаСПерейти()

    Сч = 0;
    ~Петля: Сообщить(СтрШаблон("Сч = %1", Сч));
    Сч = Сч + 1;

    Если Сч < 10 Тогда

        Перейти ~Петля;

    КонецЕсли;

КонецПроцедуры

Процедура ПравильныйЦикл()

    Для Сч = 0 По 10 Цикл

        Сообщить(СтрШаблон("Сч = %1", Сч))

    КонецЦикла;

КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);

        let goto_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingGoto).collect();

        assert_eq!(goto_diags.len(), 2, "Expected 2 diagnostics");

        // Line 8 (0-indexed), cols 4-14: `Перейти ~а;`
        assert_diagnostic_range(code, goto_diags[0], 8, 4, 14);

        // Line 22 (0-indexed), cols 8-22: `Перейти ~Петля;`
        assert_diagnostic_range(code, goto_diags[1], 22, 8, 22);
    }

    #[test]
    fn test_no_goto() {
        let code = r#"
Процедура Тест()
    Для Сч = 0 По 10 Цикл
        Сообщить(Сч);
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let goto_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingGoto).collect();
        assert_eq!(goto_diags.len(), 0);
    }
}
