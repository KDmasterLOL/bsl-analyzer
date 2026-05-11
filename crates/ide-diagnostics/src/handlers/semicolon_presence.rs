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
    use expect_test::expect;
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SemicolonPresence,
            expect![[r#"
            SemicolonPresence @ 4:7..4:8
              message: Пропущена точка с запятой в конце выражения
              severity: Information
            SemicolonPresence @ 5:1..5:10
              message: Пропущена точка с запятой в конце выражения
              severity: Information"#]],
        );
    }

    #[test]
    fn test_no_missing_semicolons() {
        let code = r#"
Процедура Тест()
    А = 1;
    Б = 2;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::SemicolonPresence, expect![[r#""#]]);
    }

    #[test]
    fn test_label_no_semicolon_required() {
        let code = r#"
Процедура Тест()
    ~Метка:
    А = 1;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::SemicolonPresence, expect![[r#""#]]);
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SemicolonPresence,
            expect![[r#"
            SemicolonPresence @ 3:9..3:16
              message: Пропущена точка с запятой в конце выражения
              severity: Information"#]],
        );
    }
}
