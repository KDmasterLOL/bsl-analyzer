//! NestedStatements diagnostic.
//!
//! Reports control-flow statements nested deeper than the configured limit.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

const DEFAULT_MAX_ALLOWED_LEVEL: i64 = 4;

/// Creates a diagnostic from HIR when nesting depth exceeds the configured
/// limit.
pub fn from_hir(
    _method_name: &str,
    depth: u32,
    _is_function: bool,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::NestedStatements;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let max_allowed_level =
        ctx.config_int(code, "maxAllowedLevel", DEFAULT_MAX_ALLOWED_LEVEL) as usize;
    if (depth as usize) <= max_allowed_level {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Управляющие конструкции не должны быть вложены слишком глубоко".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        assert_diagnostic_range, check_hir_diagnostic, check_hir_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_no_nesting() {
        let code = r#"Процедура Тест()
    Если А Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_max_nesting_no_violation() {
        let code = r#"Процедура Тест()
Если а Тогда
    Если б Тогда
        Если в Тогда
            Если г Тогда
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();
        assert_eq!(diagnostics.len(), 0, "4 levels is the maximum allowed");
    }

    #[test]
    fn test_exceed_max_nesting() {
        let code = r#"Процедура Тест()
Если а Тогда
    Если б Тогда
        Если в Тогда
            Если г Тогда
                Если д Тогда
                КонецЕсли;
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();
        assert_eq!(diagnostics.len(), 1, "5 levels exceeds limit of 4");
    }

    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура А()
 Если а Тогда     //1
   Если б Тогда   //2
    Если в Тогда  //3
     Если г Тогда //4 Максимуим но не сработало
     КонецЕсли;
    КонецЕсли;
  КонецЕсли;
 КонецЕсли;
КонецПроцедуры

Если аа Тогда    //1
   Пока вв Цикл  //2
    Попытка      //3 Мимо
    Исключение
    КонецПопытки;
  КонецЦикла;
КонецЕсли;

Если ааа Тогда  //Мимо
 Если ббб Тогда
 КонецЕсли;
 Если ввв Тогда
 КонецЕсли;
 Если ггг Тогда
 КонецЕсли;
 Если ддд Тогда
 КонецЕсли;
КонецЕсли;

Пока аааа Цикл             //1
 Если бббб Тогда           //2
 Иначе
  Попытка                  //3
   Для А = 1 По гггг Цикл  //4 Максимуим
        Если дддд Тогда    //5 Сработало
        КонецЕсли;
   КонецЦикла;
  Исключение
  КонецПопытки;
 КонецЕсли;
КонецЦикла;

Пока аааа Цикл             //1
 Если бббб Тогда           //2
 Иначе
  Попытка                  //3
   Для А = 1 По гггг Цикл  //4 Максимуим
    Если дддд Тогда        //5
     Если ееее Тогда       //6
      Если жжжж Тогда      //7 Сработало

      КонецЕсли;
     КонецЕсли;
    КонецЕсли;
   КонецЦикла;
  Исключение
  КонецПопытки;
 КонецЕсли;
КонецЦикла;"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();

        assert_eq!(diagnostics.len(), 2, "Should find 2 diagnostics");

        assert_diagnostic_range(code, diagnostics[0], 35, 8, 12);
        assert_diagnostic_range(code, diagnostics[1], 50, 6, 10);
    }

    #[test]
    fn test_custom_max_level() {
        let code = r#"Процедура А()
 Если а Тогда     //1
   Если б Тогда   //2
    Если в Тогда  //3
     Если г Тогда //4 Максимуим но не сработало
     КонецЕсли;
    КонецЕсли;
  КонецЕсли;
 КонецЕсли;
КонецПроцедуры

Если аа Тогда    //1
   Пока вв Цикл  //2
    Попытка      //3 Мимо
    Исключение
    КонецПопытки;
  КонецЦикла;
КонецЕсли;

Если ааа Тогда  //Мимо
 Если ббб Тогда
 КонецЕсли;
 Если ввв Тогда
 КонецЕсли;
 Если ггг Тогда
 КонецЕсли;
 Если ддд Тогда
 КонецЕсли;
КонецЕсли;

Пока аааа Цикл             //1
 Если бббб Тогда           //2
 Иначе
  Попытка                  //3
   Для А = 1 По гггг Цикл  //4 Максимуим
        Если дддд Тогда    //5 Сработало
        КонецЕсли;
   КонецЦикла;
  Исключение
  КонецПопытки;
 КонецЕсли;
КонецЦикла;

Пока аааа Цикл             //1
 Если бббб Тогда           //2
 Иначе
  Попытка                  //3
   Для А = 1 По гггг Цикл  //4 Максимуим
    Если дддд Тогда        //5
     Если ееее Тогда       //6
      Если жжжж Тогда      //7 Сработало

      КонецЕсли;
     КонецЕсли;
    КонецЕсли;
   КонецЦикла;
  Исключение
  КонецПопытки;
 КонецЕсли;
КонецЦикла;"#;
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::NestedStatements, serde_json::json!({ "maxAllowedLevel": 6 }));

        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();

        assert_eq!(diagnostics.len(), 1, "With maxAllowedLevel=6, only 7-level nesting triggers");
        assert_diagnostic_range(code, diagnostics[0], 50, 6, 10);
    }

    #[test]
    fn test_hir_detection() {
        let code = r#"
Процедура Тест()
    Если а Тогда
        Если б Тогда
            Если в Тогда
                Если г Тогда
                    Если д Тогда
                    КонецЕсли;
                КонецЕсли;
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let nested: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();

        assert_eq!(nested.len(), 1, "HIR should detect 1 NestedStatements (depth 5)");
    }
}
