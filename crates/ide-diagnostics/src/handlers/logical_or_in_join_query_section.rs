use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsConfig, DiagnosticsContext};

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

pub(crate) fn dispatch(
    config: &DiagnosticsConfig,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::LogicalOrInJoin { range } = diag {
        crate::sdbl_utils::dispatch_simple(
            config,
            DiagnosticCode::LogicalOrInJoinQuerySection,
            "Обнаружен оператор 'ИЛИ' в условии соединения",
            *range,
            mapper,
            query_text,
            diagnostics,
        );
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::LogicalOrInJoinQuerySection,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_logical_or_in_join_query_section() {
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

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInJoinQuerySection,
            expect![[r#"
                LogicalOrInJoinQuerySection @ 13:63..13:66
                  message: Обнаружен оператор 'ИЛИ' в условии соединения
                  severity: Warning
                LogicalOrInJoinQuerySection @ 13:109..13:112
                  message: Обнаружен оператор 'ИЛИ' в условии соединения
                  severity: Warning
                LogicalOrInJoinQuerySection @ 20:16..20:19
                  message: Обнаружен оператор 'ИЛИ' в условии соединения
                  severity: Warning
                LogicalOrInJoinQuerySection @ 25:15..25:18
                  message: Обнаружен оператор 'ИЛИ' в условии соединения
                  severity: Warning
                LogicalOrInJoinQuerySection @ 27:15..27:18
                  message: Обнаружен оператор 'ИЛИ' в условии соединения
                  severity: Warning
                LogicalOrInJoinQuerySection @ 28:15..28:18
                  message: Обнаружен оператор 'ИЛИ' в условии соединения
                  severity: Warning
                LogicalOrInJoinQuerySection @ 30:15..30:18
                  message: Обнаружен оператор 'ИЛИ' в условии соединения
                  severity: Warning
                LogicalOrInJoinQuerySection @ 31:15..31:18
                  message: Обнаружен оператор 'ИЛИ' в условии соединения
                  severity: Warning"#]],
        );
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

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInJoinQuerySection,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_or_in_select_no_trigger() {
        let code = r#"
Процедура Тест()
    Запрос.Текст = "SELECT Field1 > 0 OR Field2 > 0 FROM Table1";
КонецПроцедуры
"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInJoinQuerySection,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_multiple_fields_trigger() {
        let code = r#"
Процедура Тест()
    Запрос.Текст = "SELECT * FROM T1 INNER JOIN T2 ON T1.ID = T2.ID AND (T1.Amount > 100 OR T2.Price > 500)";
КонецПроцедуры
"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInJoinQuerySection,
            expect![[r#"
                LogicalOrInJoinQuerySection @ 3:90..3:92
                  message: Обнаружен оператор 'ИЛИ' в условии соединения
                  severity: Warning"#]],
        );
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

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInJoinQuerySection,
            expect![[r#"
                LogicalOrInJoinQuerySection @ 5:36..5:38
                  message: Обнаружен оператор 'ИЛИ' в условии соединения
                  severity: Warning"#]],
        );
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

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInJoinQuerySection,
            expect![[r#"
                LogicalOrInJoinQuerySection @ 5:34..5:37
                  message: Обнаружен оператор 'ИЛИ' в условии соединения
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_three_part_field_path_no_leak() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1
             |ВНУТРЕННЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.ID = Т2.ID
             |   И (Т1.Поле.SubField = 1 ИЛИ Т1.Поле.SubField = 2)";
КонецПроцедуры
"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInJoinQuerySection,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_undefined_literal_in_or_is_not_field() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1
             |ВНУТРЕННЕЕ СОЕДИНЕНИЕ Т2 ПО Т1.ID = Т2.ID
             |   И (Т2.Статус = НЕОПРЕДЕЛЕНО ИЛИ Т2.Статус = 1)";
КонецПроцедуры
"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInJoinQuerySection,
            expect![[r#""#]],
        );
    }
}
