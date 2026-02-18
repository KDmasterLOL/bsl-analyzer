//! UsageWriteLogEvent diagnostic.
//!
//! Validates correct usage of WriteLogEvent / ЗаписьЖурналаРегистрации method.
//!
//! ## Checks
//! 1. Method must have at least 5 parameters
//! 2. Second parameter (log level) must not be empty
//! 3. Fifth parameter (comment) must not be empty
//! 4. Inside exception blocks:
//!    - Log level must be Error (УровеньЖурналаРегистрации.Ошибка / EventLogLevel.Error)
//!    - Comment must contain DetailErrorDescription(ErrorInfo()) or have Raise statement
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** INFO
//! - **Type:** CODE_SMELL
//! - **Tags:** STANDARD, BADPRACTICE
//! - **Minutes to fix:** 1
//!
//! ## Implementation
//! **AST-based diagnostic** - requires complex context analysis.
//! Uses `bsl-platform` crate for method name resolution (bilingual, case-insensitive).
//!
//! Ported from:
//! - UsageWriteLogEventDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const WRITE_LOG_EVENT_METHOD_PARAMS_COUNT: usize = 5;

/// Creates diagnostic from HIR BodyDiagnostic::UsageWriteLogEvent.
///
/// Validates WriteLogEvent calls based on collected flags:
/// 1. Must have at least 5 parameters
/// 2. Second parameter (log level) must not be empty
/// 3. Fifth parameter (comment) must not be empty
/// 4. Inside exception blocks:
///    - Log level must be Error
///    - Comment must contain DetailErrorDescription(ErrorInfo()) or have Raise statement
#[allow(clippy::too_many_arguments)]
pub fn from_hir(
    in_except_block: bool,
    arg_count: usize,
    log_level_empty: bool,
    comment_empty: bool,
    has_error_log_level: bool,
    has_detail_error_description: bool,
    except_has_raise: bool,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::UsageWriteLogEvent;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    // Check 1: Wrong param count
    if arg_count < WRITE_LOG_EVENT_METHOD_PARAMS_COUNT {
        return Some(Diagnostic {
            code,
            message: "Неверное число параметров метода".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    // Check 2: Missing log level (2nd param)
    if log_level_empty {
        return Some(Diagnostic {
            code,
            message: "Не указан 2й параметр с типом \"УровеньЖурналаРегистрации\"".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    // Check 3: Missing comment (5th param)
    if comment_empty {
        return Some(Diagnostic {
            code,
            message: "Не указан 5й параметр \"Комментарий\"".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    // Check 4: Inside except block validation
    if in_except_block {
        // Must have Error log level
        if !has_error_log_level {
            return Some(Diagnostic {
                code,
                message: "Нужно указывать уровень \"Ошибка\" при записи в журнал регистрации внутри блока Исключение-КонецПопытки".to_string(),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }

        // Must have DetailErrorDescription or Raise in block
        if !has_detail_error_description && !except_has_raise {
            return Some(Diagnostic {
                code,
                message: "В тексте комментария нет вызова \"ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())\"".to_string(),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic;
    use crate::DiagnosticCode;

    fn filter(diagnostics: &[crate::Diagnostic]) -> Vec<&crate::Diagnostic> {
        diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsageWriteLogEvent).collect()
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/UsageWriteLogEventDiagnostic.bsl");
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        // Matches Java: 18 diagnostics (variable tracing + relaxed log level heuristic)
        assert_eq!(diags.len(), 18, "Expected 18 diagnostics, got {}", diags.len());
    }

    #[test]
    fn test_wrong_number_params() {
        let code = r#"
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие");
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("параметров"));
    }

    #[test]
    fn test_no_second_parameter() {
        let code = r#"
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие",
      ,
      , , ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("2й параметр"));
    }

    #[test]
    fn test_no_comment() {
        let code = r#"
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , , );
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("5й параметр"));
    }

    #[test]
    fn test_wrong_log_level_in_except() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Предупреждение, , ,
            "Текст");
    КонецПопытки;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Ошибка"));
    }

    #[test]
    fn test_missing_detail_error_in_except() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            ОписаниеОшибки());
    КонецПопытки;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("ПодробноеПредставлениеОшибки"));
    }

    #[test]
    fn test_correct_usage_outside_except() {
        let code = r#"
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие",
        УровеньЖурналаРегистрации.Ошибка, , ,
        ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_correct_usage_in_except_with_raise() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие",
            УровеньЖурналаРегистрации.Ошибка, , ,
            ОписаниеОшибки());
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_correct_usage_in_except_with_detail() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
    КонецПопытки;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_variable_with_detail_error() {
        // Variable tracing: ТекстОшибки = ПодробноеПредставлениеОшибки(...) in except block
        // resolves to true → no diagnostic (matches Java)
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ТекстОшибки = ПодробноеПредставлениеОшибки(ИнформацияОбОшибке());
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            ТекстОшибки);
    КонецПопытки;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    WriteLogEvent("Event");
EndProcedure
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    ЗАПИСЬЖУРНАЛАРЕГИСТРАЦИИ("Событие");
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_error_processing_module() {
        // ОбработкаОшибок.ПодробноеПредставлениеОшибки contains the substring → detected.
        // УровеньЖР is not EventLogLevel enum → assume OK (matches Java).
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖР,
            , , ОбработкаОшибок.ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
    КонецПопытки;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 0);
    }
}
