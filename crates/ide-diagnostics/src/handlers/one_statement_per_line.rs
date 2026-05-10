//! OneStatementPerLine diagnostic.
//!
//! Reports multiple statements placed on the same line.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates a diagnostic from HIR when more than one statement is placed on the
/// same source line.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::OneStatementPerLine,
        "Несколько операторов в одной строке",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use expect_test::expect;
    #[test]
    fn test_one_statement_per_line() {
        let code = r#"А = 0;;
Если Истина Тогда

  А = 0;А = 1; // Диагностика должна сработать здесь
КонецЕсли

#Область ИмяОбласти

Если Истина Тогда Сообщить(А=1); F=0; КонецЕсли;

#КонецОбласти

А=1; А=2; А=3;

Процедура А()
 УспешноПодключено = ПодключитьВнешнююКомпоненту(
		#Если Клиент Тогда
			"C:\Projects\ETPAddin\Bin\Debug-Win32\AddInNative\AddInNative.dll",
		#Иначе
			"C:\Projects\ETPAddin\Bin\Debug-x64\AddInNative\AddInNative.dll",
		#КонецЕсли
			"ETP",
			ТипВнешнейКомпоненты.Native);
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::OneStatementPerLine,
            expect![[r#"
                OneStatementPerLine @ 4:9..4:14
                  message: Несколько операторов в одной строке
                  severity: Information
                OneStatementPerLine @ 9:34..9:37
                  message: Несколько операторов в одной строке
                  severity: Information
                OneStatementPerLine @ 13:6..13:9
                  message: Несколько операторов в одной строке
                  severity: Information
                OneStatementPerLine @ 13:11..13:14
                  message: Несколько операторов в одной строке
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_one_statement_per_line_end_file() {
        let code = r#"А = 0;;
Ф=1; У=2; Е=3;

Асинх Процедура а()
    Существует = Ждать ФайлНаДиске.СуществуетАсинх();
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::OneStatementPerLine,
            expect![[r#"
                OneStatementPerLine @ 2:6..2:9
                  message: Несколько операторов в одной строке
                  severity: Information
                OneStatementPerLine @ 2:11..2:14
                  message: Несколько операторов в одной строке
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_no_multiple_statements() {
        let code = r#"
Процедура Тест()
    А = 1;
    Б = 2;
    В = 3;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::OneStatementPerLine, expect![[r#""#]]);
    }

    #[test]
    fn test_preprocessor_exclusion() {
        // Statements with preprocessor should be excluded
        let code = r#"
Процедура Тест()
    УспешноПодключено = ПодключитьВнешнююКомпоненту(
        #Если Клиент Тогда
            "C:\path1.dll",
        #Иначе
            "C:\path2.dll",
        #КонецЕсли
            "ETP",
            ТипВнешнейКомпоненты.Native);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::OneStatementPerLine, expect![[r#""#]]);
    }
}
