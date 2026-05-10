//! MultilineStringInQuery diagnostic.
//!
//! Reports suspicious multi-line string literals inside SDBL query text.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Suspicious, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Single-pass dispatch for MultilineStringInQuery.
///
/// HIR lowering flags ALL SDBL_MULTI_STRING nodes with >2 tokens as potential
/// multiline strings. This includes valid strings like `"Ж"` (3 tokens: open, content, close).
/// We filter false positives here by checking the original query_text: only emit
/// the diagnostic if the query actually contains multiline string literals.
///
/// NOTE: Rowan tree positions don't match query_text byte positions because the
/// SDBL lexer skips newlines in `tokenize_strings_mode`. We cannot use the HIR range
/// to index into query_text. Instead we check if query_text has ANY multiline strings.
pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::MultilineString { range } = diag {
        // Filter false positives: HIR lowering flags ALL non-empty SDBL strings
        // (since they all produce >2 tokens). Only emit if query_text actually
        // contains a multiline string literal (one spanning across \n).
        if !query_has_multiline_strings(query_text) {
            return;
        }
        crate::sdbl_utils::dispatch_simple(
            ctx,
            DiagnosticCode::MultilineStringInQuery,
            "Проверьте корректность многострочного литерала",
            *range,
            mapper,
            query_text,
            diagnostics,
        );
    }
}

/// Check if query_text contains ANY multiline string literal.
///
/// Scans the entire query_text for SDBL string literals (delimited by `"`)
/// and returns true if any of them span multiple lines.
/// This is independent of Rowan positions (which don't match query_text
/// due to newlines skipped by the SDBL lexer).
fn query_has_multiline_strings(query_text: &str) -> bool {
    let bytes = query_text.as_bytes();
    let mut pos = 0;

    while pos < bytes.len() {
        if bytes[pos] == b'"' {
            // Found opening quote - scan for closing quote
            pos += 1;
            let mut has_newline = false;
            loop {
                if pos >= bytes.len() {
                    // Unterminated string
                    if has_newline {
                        return true;
                    }
                    break;
                }
                if bytes[pos] == b'"' {
                    // Check for escaped "" (SDBL empty string or escape)
                    if pos + 1 < bytes.len() && bytes[pos + 1] == b'"' {
                        pos += 2; // Skip ""
                        continue;
                    }
                    // Closing quote found
                    pos += 1;
                    if has_newline {
                        return true;
                    }
                    break;
                }
                if bytes[pos] == b'\n' {
                    has_newline = true;
                }
                pos += 1;
            }
        } else {
            pos += 1;
        }
    }

    false
}

/// Runs the MultilineStringInQuery diagnostic (standalone, used in tests).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::MultilineStringInQuery,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_empty_string_in_query_creates_multiline() {
        // "" in SDBL query creates a multiline string literal (suspicious)
        // """" (4 quotes) is the correct empty string - produces no diagnostic
        let code = r#"Процедура Тест()

    ТекстЗапроса =
    "ВЫБРАТь
    |   Поле КАК Поле,
    |   "" КАК ПустаяСтрока,
    |   "" КАК ЕщеПустаяСтрока,
    |   "" как ТретьяПустаяСтрока,
    |   ЕСТЬNULL(Поле, """") КАК ПолеНеВСтроке
    |ИЗ
    |   Справочник.Справочник";

    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |	ПриходныйОрдерНоменклатура.Номенклатура КАК Номенклатура,
    |	ЕСТЬNULL(ПриходныйОрдерНоменклатура.Номенклатура.Код, "") КАК НоменклатураКод,
    |	ЕСТЬNULL(ПриходныйОрдерНоменклатура.Номенклатура.Наименование, "") КАК НоменклатураНаименование
    |ИЗ
    |	Документ.ПриходныйОрдер.Номенклатура КАК ПриходныйОрдерНоменклатура
    |ГДЕ
    |	ПриходныйОрдерНоменклатура.Ссылка = &Ссылка";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MultilineStringInQuery,
            expect![[r#"
                MultilineStringInQuery @ 6:9..7:6
                  message: Проверьте корректность многострочного литерала
                  severity: Critical
                MultilineStringInQuery @ 7:32..11:11
                  message: Проверьте корректность многострочного литерала
                  severity: Critical
                MultilineStringInQuery @ 16:61..17:69
                  message: Проверьте корректность многострочного литерала
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_no_diagnostic_for_string_literals_in_case() {
        // ""Ж"" in BSL string = "Ж" in SDBL = valid single-line string literal
        // Should NOT trigger multiline string diagnostic
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   ВЫБОР
    |       КОГДА Т.Пол = ЗНАЧЕНИЕ(Перечисление.ПолФизическогоЛица.Мужской)
    |           ТОГДА ""М""
    |       КОГДА Т.Пол = ЗНАЧЕНИЕ(Перечисление.ПолФизическогоЛица.Женский)
    |           ТОГДА ""Ж""
    |       ИНАЧЕ """"
    |   КОНЕЦ КАК Пол
    |ИЗ Справочник.ФизическиеЛица КАК Т";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MultilineStringInQuery,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_no_diagnostic_for_correct_empty_string() {
        // """" (4 quotes) is the correct way to represent empty string in SDBL
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   ЕСТЬNULL(Поле, """") КАК Поле
    |ИЗ Справочник.Справочник";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MultilineStringInQuery,
            expect![[r#""#]],
        );
    }
}
