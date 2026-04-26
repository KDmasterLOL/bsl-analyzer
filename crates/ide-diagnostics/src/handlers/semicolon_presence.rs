//! Reports statements that omit a trailing semicolon.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates a diagnostic from HIR semicolon-lowering data.
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
        let code = r#"А = 0;
Если Истина Тогда
  А = 0;
  А = 0           // Диагностика должна сработать здесь
КонецЕсли         // и здесь

#Область ИмяОбласти

#КонецОбласти

Асинх Процедура а()
    Существует = Ждать ФайлНаДиске.СуществуетАсинх();
КонецПроцедуры

Процедура ОшибкаРазбора()
    Для ЭлементСтруктуры Из КакаятоСтруктура Цикл // Здесь ошибки не будет, т.к. ошибка разбора

    КонецЦикла;  // Здесь ошибки не будет, т.к. ошибка разбора
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);

        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SemicolonPresence).collect();

        assert_eq!(diags.len(), 2, "Expected 2 diagnostics");

        // "А = 0" - last token is "0" at position 6-7
        assert_diagnostic_range(code, diags[0], 3, 6, 7);

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

    #[test]
    fn test_return_without_semicolon_before_endif() {
        // BSL allows omitting semicolon before КонецЕсли, but it's bad practice
        // SemicolonPresence should warn about missing semicolon
        let code = r#"Процедура Тест()
    Если Истина Тогда
        Возврат
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SemicolonPresence).collect();
        assert_eq!(diags.len(), 1, "Should detect missing semicolon after Возврат");
        // Возврат is on line 2 (0-indexed), columns 8-15 (Возврат = 7 chars)
        assert_diagnostic_range(code, diags[0], 2, 8, 15);
    }
}
