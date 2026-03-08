//! MultilineStringInQuery diagnostic.
//!
//! Detects multiline string literals in SDBL queries.
//!
//! ## Why?
//! Multiline string literals in SDBL queries are very rare and usually indicate
//! an error from incorrect number of double quotes. In SDBL, to represent an
//! empty string you should use """" (4 quotes), not "" (2 quotes).
//!
//! ## Bad practice
//! ```bsl
//! Query.Text = "SELECT
//! |   ЕСТЬNULL(Field, "") AS Code  // Wrong: "" becomes multiline string
//! |FROM Table";
//! ```
//!
//! ## Good practice
//! ```bsl
//! Query.Text = "SELECT
//! |   ЕСТЬNULL(Field, """") AS Code  // Correct: """" is empty string in SDBL
//! |FROM Table";
//! ```
//!
//! ## Implementation
//!
//! Migrated to HIR-based approach for consistency with other SDBL diagnostics.
//! Diagnostics are collected during HIR lowering when processing string literals.

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
pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::MultilineString { range } = diag {
        crate::sdbl_utils::dispatch_simple(
            ctx,
            DiagnosticCode::MultilineStringInQuery,
            "Check if multiline literal is correct",
            *range,
            mapper,
            query_text,
            diagnostics,
        );
    }
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
    use super::check;
    use crate::test_utils::{assert_diagnostic_range_multiline, check_sdbl_diagnostic};
    use crate::{DiagnosticCode, Severity};

    fn check_metadata(diag: &crate::Diagnostic) {
        assert_eq!(diag.code, DiagnosticCode::MultilineStringInQuery);
        assert_eq!(diag.severity, Severity::Critical);
        assert_eq!(diag.message, "Check if multiline literal is correct");
    }

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
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 3);

        for diag in &diagnostics {
            check_metadata(diag);
        }

        assert_diagnostic_range_multiline(code, &diagnostics[0], 5, 8, 6, 5);
        assert_diagnostic_range_multiline(code, &diagnostics[1], 6, 31, 10, 10);
        assert_diagnostic_range_multiline(code, &diagnostics[2], 15, 60, 16, 68);
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }
}
