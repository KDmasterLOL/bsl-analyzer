//! LogicalOrInJoinQuerySection diagnostic.
//!
//! Detects `OR` / `ИЛИ` operators in SDBL join conditions when they compare
//! different fields.
//!

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Performance, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Single-pass dispatch for LogicalOrInJoinQuerySection.
pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::LogicalOrInJoin { range } = diag {
        crate::sdbl_utils::dispatch_simple(
            ctx,
            DiagnosticCode::LogicalOrInJoinQuerySection,
            "Обнаружен оператор 'ИЛИ' в условии соединения",
            *range,
            mapper,
            query_text,
            diagnostics,
        );
    }
}

/// Runs the LogicalOrInJoinQuerySection diagnostic (standalone, used in tests).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::LogicalOrInJoinQuerySection,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_sdbl_diagnostic};
    use crate::{DiagnosticCode, Severity};
    #[test]
    fn test_logical_or_in_join_query_section() {
        // Large inline regression fixture for OR-in-JOIN coverage.
        // 8 OR diagnostics in JOIN conditions across nested joins.
        let code = r#"Процедура ПолучиттьРеализациюТовара()

	Запрос = Новый Запрос;
	Запрос.Текст =
	     "ВЫБРАТЬ
         |	РеализацияТоваровУслугТовары.Ссылка КАК Ссылка,
         |	РеализацияТоваровУслугТовары.Сумма > 0
         |		ИЛИ РеализацияТоваровУслугТовары.СуммаСНДС > 0 КАК НенулеваяСумма
         |ИЗ
         |	Документ.РеализацияТоваровУслуг.Товары КАК РеализацияТоваровУслугТовары
         |      ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.РеализацияТоваровУслуг КАК РеализацияТоваровУслуг
         |      ПО РеализацияТоваровУслугТовары.Ссылка = РеализацияТоваровУслуг.Ссылка
         |          И (РеализацияТоваровУслугТовары.Сумма > 0 ИЛИ РеализацияТоваровУслугТовары.СуммаНДС > 0 ИЛИ РеализацияТоваровУслугТовары.СуммаСНДС > 0) //Ошибка (2 срабатывания)
         |		ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Номенклатура КАК СправочникНоменклатура
         |			ЛЕВОЕ СОЕДИНЕНИЕ Справочник.ВидыНоменклатуры КАК ВидыНоменклатуры //Тест работы на вложенном соединении
         |			ПО СправочникНоменклатура.ВидНоменклатуры = ВидыНоменклатуры.Ссылка
         |				И (СправочникНоменклатура.СрокГодности > 1
         |					ИЛИ СправочникНоменклатура.СрокГодности < 10)
         |				И (СправочникНоменклатура.СрокГодности > 1
         |					ИЛИ ВидыНоменклатуры.ЗапрещенаПродажаЧерезПатент = ИСТИНА) //Ошибка
         |		ПО РеализацияТоваровУслугТовары.Номенклатура = СправочникНоменклатура.Ссылка
         |			И (СправочникНоменклатура.КодПоКВПД = ""1122""
         |				ИЛИ СправочникНоменклатура.КодПоКВПД = ""1133"")
         |			И (СправочникНоменклатура.Артикул = ""0011""
         |				ИЛИ СправочникНоменклатура.КодТРУ = ""0111"") //Ошибка
         |			И (СправочникНоменклатура.Артикул = ""0022""
         |				ИЛИ СправочникНоменклатура.КодТРУ = ""0222""
         |				ИЛИ СправочникНоменклатура.КодПоКВПД = ""2233"") //Ошибка (2 срабатывания)
         |			И (СправочникНоменклатура.КодПоКВПД = ""1122""
         |				ИЛИ СправочникНоменклатура.КодПоКВПД = ""1133""
         |				ИЛИ СправочникНоменклатура.КодТРУ = ""0222"")"; //Ошибка (2 срабатывания)

	РезультатЗапроса = Запрос.Выполнить();

КонецПроцедуры

//Диагностика должна зафиксировать ошибку
// при использовании оператора "ИЛИ" в условии над различными полями таблицы.
// Если оператор "ИЛИ" в условии над одним полем, то ошибка не фиксируется,
// так как планировщик запросов имеет возможность преобразовывать такое условие в IN, тем самым оптимизируя.

//Итоговое количество срабатываний - 8."#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        // Expect exactly 8 diagnostics matching reference implementation
        assert_eq!(
            diagnostics.len(),
            8,
            "Expected 8 diagnostics matching reference implementation"
        );

        // Verify all are on correct code
        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::LogicalOrInJoinQuerySection);
            // CodeSmell + Major → Warning (per metadata mapping)
            assert_eq!(diag.severity, Severity::Warning);
            assert!(diag.message.contains("ИЛИ"));
        }

        // Line 13 (0-based, fixture line index): first OR in JOIN condition
        assert_diagnostic_range(code, &diagnostics[0], 12, 62, 65);

        // Line 13: second OR in same expression
        assert_diagnostic_range(code, &diagnostics[1], 12, 108, 111);
    }

    #[test]
    fn test_same_field_no_trigger() {
        let code = r#"
Процедура Тест()
    Запрос.Текст = "SELECT * FROM T1
                   |LEFT JOIN T2 ON T1.ID = T2.ID
                   |   AND (T2.Status = 1 OR T2.Status = 2)";
КонецПроцедуры
"#;

        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Same field OR should not trigger diagnostic");
    }

    #[test]
    fn test_or_in_select_no_trigger() {
        let code = r#"
Процедура Тест()
    Запрос.Текст = "SELECT Field1 > 0 OR Field2 > 0 FROM Table1";
КонецПроцедуры
"#;

        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "OR in SELECT should not trigger diagnostic");
    }

    #[test]
    fn test_multiple_fields_trigger() {
        // Test on single line first
        let code = r#"
Процедура Тест()
    Запрос.Текст = "SELECT * FROM T1 INNER JOIN T2 ON T1.ID = T2.ID AND (T1.Amount > 100 OR T2.Price > 500)";
КонецПроцедуры
"#;

        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Multiple fields with OR should trigger diagnostic");
        assert_eq!(diagnostics[0].code, DiagnosticCode::LogicalOrInJoinQuerySection);
    }

    #[test]
    fn test_bilingual_english() {
        let code = r#"
Procedure Test()
    Query = "SELECT * FROM T1
            |INNER JOIN T2 ON T1.ID = T2.ID
            |   AND (T1.Field1 = 1 OR T2.Field2 = 2)";
EndProcedure
"#;

        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "English OR should trigger diagnostic");
    }

    #[test]
    fn test_bilingual_russian() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1
             |ВНУТРЕННЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.ID = Т2.ID
             |   И (Т1.Поле1 = 1 ИЛИ Т2.Поле2 = 2)";
КонецПроцедуры
"#;

        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Russian ИЛИ should trigger diagnostic");
    }
}
