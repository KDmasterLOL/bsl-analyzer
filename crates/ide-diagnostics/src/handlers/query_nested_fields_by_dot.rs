//! QueryNestedFieldsByDot diagnostic.
//!
//! Reports nested dereference of reference fields through dot in SDBL queries.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Default minimum path depth for normal-context column-ref dereferences.
/// Matches BSL-LS default and preserves pre-config behaviour.
const DEFAULT_MIN_PATH_DEPTH: i64 = 3;

/// Single-pass dispatch for `QueryNestedFieldsByDot`.
///
/// Filtering rules:
/// - `parts_count = Some(n)` (normal-context column ref): emit only when `n >= minPathDepth`.
/// - `parts_count = None` (virtual-table parameters, CAST member chain): always emit;
///   threshold lives in the syntax of the construct itself.
pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::QueryNestedFieldsByDot { range, parts_count } = diag {
        if let Some(n) = parts_count {
            let min = ctx
                .config
                .get_int(DiagnosticCode::QueryNestedFieldsByDot, "minPathDepth")
                .unwrap_or(DEFAULT_MIN_PATH_DEPTH);
            if (*n as i64) < min {
                return;
            }
        }
        crate::sdbl_utils::dispatch_simple(
            ctx,
            DiagnosticCode::QueryNestedFieldsByDot,
            "Обнаружено разыменование ссылочного поля",
            *range,
            mapper,
            query_text,
            diagnostics,
        );
    }
}

/// Runs the QueryNestedFieldsByDot diagnostic (standalone, used in tests).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::QueryNestedFieldsByDot,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_sdbl_diagnostic};
    use crate::{DiagnosticCode, Severity};
    #[test]
    fn test_query_nested_fields_by_dot() {
        let code = r#"//Проверяемые кейсы:

//1.Базовое разыменование ссылочных полей в выборке (во временную таблицу или в результат запроса) Должна зафиксироваться ошибка
//2.Разыменование ссылочных полей в соединениях таблиц (Должна зафиксироваться ошибка)
//3.Разыменование ссылочных полей в виртуальных таблиц (Должна зафиксироваться ошибка)
//4.Агрегатные и иные функции над полем (Не должна фиксироваться ошибка)
//5.Конструкция "ВЫРАЗИТЬ" без разыменования получаемого поля (Не должна фиксироваться ошибка)
//6.Конструкция "ВЫРАЗИТЬ" с разыменованием получаемого поля (Должна фиксироваться ошибка)
//7.Разыменование ссылочных полей в секции "ГДЕ" (Должна фиксироваться ошибка)

Процедура ПолучениеДанныхЗаказовКлиентов()

	Запрос = Новый Запрос;
	Запрос.Текст =
	"ВЫБРАТЬ
	|	ЗаказКлиентаТовары.Ссылка КАК Ссылка,
	|	ЗаказКлиентаТовары.Номенклатура КАК Номенклатура,
	|	ЗаказКлиентаТовары.Характеристика КАК Характеристика,
	|	ЗаказКлиентаТовары.Упаковка КАК Упаковка,
	|	ЗаказКлиентаТовары.Серия КАК Серия,
	|	ЗаказКлиентаТовары.СуммаНДС + ЗаказКлиентаТовары.Сумма КАК СуммаСНДС,
	|	ЗаказКлиентаТовары.Ссылка.Организация КАК Организация, //Ошибка №1
	|	ЗаказКлиентаТовары.Ссылка.Контрагент КАК Контрагент, //Ошибка №1
	|	ЗаказКлиентаТовары.Ссылка.Партнер КАК Партнер, //Ошибка №1
	|	ЗаказКлиентаТовары.Ссылка.ОбъектРасчетов КАК ОбъектРасчетов //Ошибка №1
	|ПОМЕСТИТЬ ВТ_ДанныеЗаказовКлиента
	|ИЗ
	|	Документ.ЗаказКлиента.Товары КАК ЗаказКлиентаТовары
	|ГДЕ
	|	ЗаказКлиентаТовары.Ссылка.Дата МЕЖДУ &НачалоПериода И &КонецПериода //Ошибка №7
	|
	|ИНДЕКСИРОВАТЬ ПО
	|	Контрагент,
	|	Партнер,
	|	ОбъектРасчетов,
	|	Организация
	|;
	|
	|////////////////////////////////////////////////////////////////////////////////
	|ВЫБРАТЬ
	|	РасчетыСКлиентамиОбороты.АналитикаУчетаПоПартнерам КАК АналитикаУчетаПоПартнерам,
	|	РасчетыСКлиентамиОбороты.ОбъектРасчетов КАК ОбъектРасчетов,
	|	РасчетыСКлиентамиОбороты.Валюта КАК Валюта,
	|	РасчетыСКлиентамиОбороты.СуммаОборот КАК СуммаОборот,
	|	РасчетыСКлиентамиОбороты.КОплатеОборот КАК КОплатеОборот,
	|	РасчетыСКлиентамиОбороты.КОтгрузкеОборот КАК КОтгрузкеОборот,
	|	РасчетыСКлиентамиОбороты.ОтгружаетсяОборот КАК ОтгружаетсяОборот
	|ПОМЕСТИТЬ ВТ_РасчетыСКлиентами
	|ИЗ
	|	РегистрНакопления.РасчетыСКлиентами.Обороты(
	|			&НачалоПериода,
	|			&КонецПериода,
	|			,
	|			(АналитикаУчетаПоПартнерам.Партнер, АналитикаУчетаПоПартнерам.Контрагент, АналитикаУчетаПоПартнерам.Организация, ОбъектРасчетов) В //Ошибка №3
	|				(ВЫБРАТЬ
	|					ВТ_ДанныеЗаказовКлиента.Партнер КАК Партнер,
	|					ВТ_ДанныеЗаказовКлиента.Контрагент КАК Контрагент,
	|					ВТ_ДанныеЗаказовКлиента.Организация КАК Организация,
	|					ВТ_ДанныеЗаказовКлиента.ОбъектРасчетов КАК ОбъектРасчетов
	|				ИЗ
	|					ВТ_ДанныеЗаказовКлиента КАК ВТ_ДанныеЗаказовКлиента)) КАК РасчетыСКлиентамиОбороты
	|;
	|
	|////////////////////////////////////////////////////////////////////////////////
	|ВЫБРАТЬ
	|	РасчетыСКлиентамиПланОтгрузокОбороты.АналитикаУчетаПоПартнерам КАК АналитикаУчетаПоПартнерам,
	|	РасчетыСКлиентамиПланОтгрузокОбороты.ОбъектРасчетов КАК ОбъектРасчетов,
	|	РасчетыСКлиентамиПланОтгрузокОбороты.ДокументПлан КАК ДокументПлан,
	|	РасчетыСКлиентамиПланОтгрузокОбороты.Валюта КАК Валюта,
	|	РасчетыСКлиентамиПланОтгрузокОбороты.СуммаОборот КАК СуммаОборот
	|ПОМЕСТИТЬ ВТ_ПланОтгрузок
	|ИЗ
	|	РегистрНакопления.РасчетыСКлиентамиПланОтгрузок.Обороты(
	|			&НачалоПериода,
	|			&КонецПериода,
	|			,
	|			(ДокументПлан, ОбъектРасчетов) В
	|				(ВЫБРАТЬ
	|					ВТ_ДанныеЗаказовКлиента.Ссылка КАК Ссылка,
	|					ВТ_ДанныеЗаказовКлиента.ОбъектРасчетов КАК ОбъектРасчетов
	|				ИЗ
	|					ВТ_ДанныеЗаказовКлиента КАК ВТ_ДанныеЗаказовКлиента)) КАК РасчетыСКлиентамиПланОтгрузокОбороты
	|;
	|
	|////////////////////////////////////////////////////////////////////////////////
	|ВЫБРАТЬ
	|	ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам КАК АналитикаУчетаПоПартнерам,
	|	ВТ_РасчетыСКлиентами.ОбъектРасчетов КАК ОбъектРасчетов,
	|	ВТ_РасчетыСКлиентами.Валюта КАК Валюта,
	|	ВТ_РасчетыСКлиентами.СуммаОборот КАК СуммаОборот,
	|	ВТ_РасчетыСКлиентами.КОплатеОборот КАК КОплатеОборот,
	|	ВТ_РасчетыСКлиентами.КОтгрузкеОборот КАК КОтгрузкеОборот,
	|	ВТ_РасчетыСКлиентами.ОтгружаетсяОборот КАК ОтгружаетсяОборот,
	|	ВТ_ДанныеЗаказовКлиента.Номенклатура КАК Номенклатура,
	|	ВТ_ДанныеЗаказовКлиента.Характеристика КАК Характеристика,
	|	ВТ_ДанныеЗаказовКлиента.Упаковка КАК Упаковка,
	|	ЕСТЬNULL(ВТ_ДанныеЗаказовКлиента.Ссылка, ЗНАЧЕНИЕ(Документ.ЗаказКлиента.ПустаяСсылка)) КАК ЗаказКлиента
	|ПОМЕСТИТЬ ВТ_РасчетыСКлиентамиРасш
	|ИЗ
	|	ВТ_РасчетыСКлиентами КАК ВТ_РасчетыСКлиентами
	|		ЛЕВОЕ СОЕДИНЕНИЕ ВТ_ДанныеЗаказовКлиента КАК ВТ_ДанныеЗаказовКлиента
	|		ПО ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам.Партнер = ВТ_ДанныеЗаказовКлиента.Партнер //Ошибка №2
	|			И ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам.Контрагент = ВТ_ДанныеЗаказовКлиента.Контрагент //Ошибка №2
	|			И ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам.Организация = ВТ_ДанныеЗаказовКлиента.Организация //Ошибка №2
	|;
	|
	|////////////////////////////////////////////////////////////////////////////////
	|ВЫБРАТЬ
	|	ВТ_РасчетыСКлиентамиРасш.АналитикаУчетаПоПартнерам КАК АналитикаУчетаПоПартнерам,
	|	ВТ_РасчетыСКлиентамиРасш.ОбъектРасчетов КАК ОбъектРасчетов,
	|	ВТ_РасчетыСКлиентамиРасш.ЗаказКлиента КАК ЗаказКлиента,
	|	ВТ_РасчетыСКлиентамиРасш.Номенклатура КАК Номенклатура,
	|	ВТ_РасчетыСКлиентамиРасш.Характеристика КАК Характеристика,
	|	ВТ_РасчетыСКлиентамиРасш.Упаковка КАК Упаковка,
	|	ВЫРАЗИТЬ(ВТ_ПланОтгрузок.ДокументПлан КАК Документ.ЗаказКлиента).Валюта КАК ВалютаДокумента,
	|	ВЫРАЗИТЬ(ВТ_ПланОтгрузок.ДокументПлан КАК Документ.ЗаказКлиента).Валюта.Наценка КАК НаценкаВалюыДокумента //Ошибка №6
	|ИЗ
	|	ВТ_РасчетыСКлиентамиРасш КАК ВТ_РасчетыСКлиентамиРасш
	|		ВНУТРЕННЕЕ СОЕДИНЕНИЕ ВТ_ПланОтгрузок КАК ВТ_ПланОтгрузок
	|		ПО ВТ_РасчетыСКлиентамиРасш.АналитикаУчетаПоПартнерам = ВТ_ПланОтгрузок.АналитикаУчетаПоПартнерам
	|			И ВТ_РасчетыСКлиентамиРасш.ОбъектРасчетов = ВТ_ПланОтгрузок.ОбъектРасчетов
	|			И ВТ_РасчетыСКлиентамиРасш.ЗаказКлиента = ВТ_ПланОтгрузок.ДокументПлан";

	Запрос.УстановитьПараметр("КонецПериода", КонецГода(ТекущаяДата()));
	Запрос.УстановитьПараметр("НачалоПериода", НачалоГода(ТекущаяДата()));

	РезультатЗапроса = Запрос.Выполнить().Выгрузить();

	Запрос = Новый Запрос;
	Запрос.Текст =
		"ВЫБРАТЬ
		|	КурсыВалютСрезПоследних.Курс КАК Курс
		|ИЗ
		|	РегистрСведений.КурсыВалют.СрезПоследних(&Период, Валюта = &Валюта) КАК КурсыВалютСрезПоследних";

	Запрос.УстановитьПараметр("Валюта", Справочники.Валюты.ПустаяСсылка());
	Запрос.УстановитьПараметр("Период", ТекущаяДата());

	РезультатЗапроса = Запрос.Выполнить();

	ВыборкаДетальныеЗаписи = РезультатЗапроса.Выбрать();

КонецПроцедуры;

//Итоговое ожидаемое количество срабатываний:
//Номер тест кейса | Число срабатываний
// №1 | 4
// №2 | 3
// №3 | 3
// №4 | 0
// №5 | 0
// №6 | 1
// №7 | 1
//Итого: 12"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        // Expected: 12 diagnostics.
        //
        // Found diagnostics:
        // Query 1 (SELECT + WHERE):
        // - Line 22: ЗаказКлиентаТовары.Ссылка.Организация (3 parts)
        // - Line 23: ЗаказКлиентаТовары.Ссылка.Контрагент (3 parts)
        // - Line 24: ЗаказКлиентаТовары.Ссылка.Партнер (3 parts)
        // - Line 25: ЗаказКлиентаТовары.Ссылка.ОбъектРасчетов (3 parts)
        // - Line 30: ЗаказКлиентаТовары.Ссылка.Дата (3 parts, WHERE clause)
        //
        // Query 2 (virtual table params):
        // - Line 54: АналитикаУчетаПоПартнерам.Партнер (2 parts in virtual table)
        // - Line 55: АналитикаУчетаПоПартнерам.Контрагент (2 parts in virtual table)
        // - Line 56: АналитикаУчетаПоПартнерам.Организация (2 parts in virtual table)
        //
        // Query 4 (JOIN ON clause):
        // - Line 102: ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам.Партнер (3 parts)
        // - Line 103: ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам.Контрагент (3 parts)
        // - Line 104: ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам.Организация (3 parts)
        //
        // Query 5 (CAST member access):
        // - Line 116: ВЫРАЗИТЬ(...).Валюта.Наценка (2 fields after CAST)
        assert_eq!(diagnostics.len(), 12, "Expected 12 diagnostics, got {}", diagnostics.len());

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::QueryNestedFieldsByDot);
            assert_eq!(diag.severity, Severity::Warning);
            assert_eq!(diag.message, "Обнаружено разыменование ссылочного поля");
        }

        // Verify first diagnostic position (line 22 in BSL file, 0-indexed = 21)
        // "|<tab><tab>ЗаказКлиентаТовары.Ссылка.Организация " - col 3 to 41 (0-indexed)
        assert_diagnostic_range(code, &diagnostics[0], 21, 3, 41);
    }

    #[test]
    fn test_no_false_positives_for_mdo_types() {
        // Should NOT trigger for MDO type paths like "Справочник.Валюты.Код"
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ Справочник.Валюты.Код ИЗ Справочник.Валюты";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert!(diagnostics.is_empty(), "MDO type paths should not trigger diagnostic");
    }

    #[test]
    fn test_no_false_positives_for_two_parts() {
        // Should NOT trigger for simple 2-part paths like "T.Поле"
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ T.Ссылка ИЗ Документ.Заказ КАК T";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert!(diagnostics.is_empty(), "Two-part paths should not trigger diagnostic");
    }

    fn config_with_min_path_depth(depth: i64) -> crate::DiagnosticsConfig {
        let mut config = crate::DiagnosticsConfig::default();
        config.enabled.push(DiagnosticCode::QueryNestedFieldsByDot);
        config.parameters.insert(
            DiagnosticCode::QueryNestedFieldsByDot,
            serde_json::json!({ "minPathDepth": depth }),
        );
        config
    }

    #[test]
    fn test_min_path_depth_above_three_drops_three_part_normal_context() {
        // 3-part normal-context column ref: handler filter drops when minPathDepth > 3.
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ T.Ссылка.Контрагент ИЗ Документ.Заказ КАК T";
КонецПроцедуры
"#;
        let config = config_with_min_path_depth(4);
        let diagnostics = crate::test_utils::check_hir_diagnostic_with_config(code, config, check);
        assert!(
            diagnostics.is_empty(),
            "minPathDepth=4 must suppress 3-part normal-context column refs, got {:?}",
            diagnostics.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_min_path_depth_above_three_preserves_cast_member_chain() {
        // CAST chained `member_access` carries `parts_count: None` and is unaffected by config.
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫРАЗИТЬ(T.Ссылка КАК Документ.Заказ).Валюта.Курс
    |ИЗ Документ.Заказ КАК T";
КонецПроцедуры
"#;
        let config = config_with_min_path_depth(99);
        let diagnostics = crate::test_utils::check_hir_diagnostic_with_config(code, config, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "CAST member chain (parts_count=None) must emit regardless of minPathDepth"
        );
    }

    #[test]
    fn test_min_path_depth_two_emits_two_part_normal_context() {
        // 2-part normal-context column ref: silent at default (min=3), emits at min=2.
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ T.Поле ИЗ Документ.Заказ КАК T";
КонецПроцедуры
"#;
        let config = config_with_min_path_depth(2);
        let diagnostics = crate::test_utils::check_hir_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 1, "minPathDepth=2 must emit on a 2-part column ref");
    }
}
