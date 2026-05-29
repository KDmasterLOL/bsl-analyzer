use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Performance, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::LogicalOrInWhere { range } = diag {
        crate::sdbl_utils::dispatch_simple(ctx, DiagnosticCode::LogicalOrInTheWhereSectionOfQuery, "Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий", *range, mapper, query_text, diagnostics);
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_multi_case_where_or() {
        let code = r#"Процедура Тест1()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Таблица.Наименование
    |ИЗ Справочник.Товары КАК Таблица
    |ГДЕ
    |   Истина ИЛИ Цена = 2"; //<-- ошибка

КонецПроцедуры

Процедура Тест2()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Таблица.Наименование
    |ИЗ Справочник.Товары КАК Таблица
    |ГДЕ
    |   Таблица.Поле1 = &Значение1
    |   ИЛИ Таблица.Поле2 = &Значение2"; //<-- ошибка

КонецПроцедуры

Процедура Тест4()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Таблица.Наименование
    |ИЗ Справочник.Товары КАК Таблица
    |ГДЕ
    |   Таблица.Поле1 = &Значение1
    |   И (Таблица.Поле2 = &Значение2 ИЛИ Таблица.Поле3 = &Значение3)"; //<-- ошибка

КонецПроцедуры

Процедура Тест5()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Таблица.Наименование
    |ИЗ Справочник.Товары КАК Таблица
    |ГДЕ
    |   Таблица.Поле1 = &Значение1
    |   ИЛИ //<-- ошибка
    |   (Таблица.Поле2 = &Значение2 ИЛИ Таблица.Поле3 = &Значение3)"; //<-- ошибка

КонецПроцедуры

Процедура Тест6()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "Выбрать Истина КАК Поле1
    |ИЗ Документ.РеализацияТоваровУслуг КАК Т
    |Где Истина
    |И Т.Ссылка В
    |    (Выбрать СС.Ссылка
    |     Из Справочник.Справочник2 КАК СС
    |     Где Истина Или Ложь)"; //<-- ошибка

КонецПроцедуры

Процедура Тест7()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР КОГДА Таблица.Флаг ИЛИ Истина Тогда Истина Иначе Ложь КОНЕЦ
    |ИЗ Справочник.Товары КАК Таблица
    |РегистрНакопления.Склады.Остатки(, ) КАК Т
    |По Истина ИЛИ Таблица.Поле1 = Т.Товар
    |УПОРЯДОЧИТЬ ПО
    |   ВЫБОР КОГДА Таблица.Флаг ИЛИ Истина Тогда Истина Иначе Ложь КОНЕЦ";

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#"
                LogicalOrInTheWhereSectionOfQuery @ 8:16..8:19
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning
                LogicalOrInTheWhereSectionOfQuery @ 20:9..20:12
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning
                LogicalOrInTheWhereSectionOfQuery @ 32:39..32:42
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning
                LogicalOrInTheWhereSectionOfQuery @ 44:9..44:12
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning
                LogicalOrInTheWhereSectionOfQuery @ 45:37..45:40
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning
                LogicalOrInTheWhereSectionOfQuery @ 59:22..59:25
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_simple_or_in_where() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT Name FROM Products WHERE Type = 1 OR Category = 2";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#"
                LogicalOrInTheWhereSectionOfQuery @ 3:56..3:58
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_russian_or_keyword() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Товары ГДЕ Цена = 100 ИЛИ Количество = 0";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#"
                LogicalOrInTheWhereSectionOfQuery @ 3:50..3:53
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_or_in_parentheses() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T WHERE A = 1 AND (B = 2 OR C = 3)";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#"
                LogicalOrInTheWhereSectionOfQuery @ 3:54..3:56
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_multiple_or_in_where() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T WHERE A = 1 OR B = 2 OR C = 3";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#"
                LogicalOrInTheWhereSectionOfQuery @ 3:43..3:45
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning
                LogicalOrInTheWhereSectionOfQuery @ 3:52..3:54
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_nested_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T1 WHERE ID IN (SELECT ID FROM T2 WHERE A = 1 OR B = 2)";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#"
                LogicalOrInTheWhereSectionOfQuery @ 3:75..3:77
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_no_false_positives_case_expression() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT CASE WHEN Flag OR True THEN 1 ELSE 0 END AS Result FROM T WHERE ID = 1";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_no_false_positives_join_on() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T1 LEFT JOIN T2 ON T1.A = T2.A OR T1.B = T2.B WHERE T1.ID = 1";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_no_where_clause() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM Products";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_and_with_or_in_parentheses() {
        let code = r#"
Процедура Тест()
    Запрос.Текст = "ВЫБРАТЬ Таблица.Наименование
    |ИЗ Справочник.Товары КАК Таблица
    |ГДЕ
    |   Таблица.Поле1 = 1
    |   И (Таблица.Поле2 = 2 ИЛИ Таблица.Поле3 = 3)";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#"
                LogicalOrInTheWhereSectionOfQuery @ 7:30..7:33
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_sdbl_with_parameters() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T WHERE Field1 = &Param1 AND (Field2 = &Param2 OR Field3 = &Param3)";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#"
                LogicalOrInTheWhereSectionOfQuery @ 3:76..3:78
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning"#]],
        );
    }

    #[test]
    fn track3_or_in_russian_subquery_where_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
        "ВЫБРАТЬ Товары.Ссылка КАК Ссылка
        |ИЗ Справочник.Номенклатура КАК Товары
        |ГДЕ Товары.Ссылка В (
        |   ВЫБРАТЬ Остатки.Номенклатура КАК Номенклатура
        |   ИЗ РегистрНакопления.ОстаткиТоваров КАК Остатки
        |   ГДЕ Остатки.Склад = &Склад ИЛИ Остатки.Количество > 0
        |)";
КонецПроцедуры"#,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#"
                LogicalOrInTheWhereSectionOfQuery @ 9:40..9:43
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning"#]],
        );
    }

    #[test]
    fn track3_or_in_deep_subquery_where_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
        "SELECT Products.Ref AS Ref
        |FROM Catalog.Products AS Products
        |WHERE Products.Ref IN (
        |   SELECT Balances.Product AS Product
        |   FROM (
        |       SELECT Stock.Product AS Product
        |       FROM AccumulationRegister.Stock AS Stock
        |       WHERE Stock.Warehouse = &Warehouse OR Stock.Quantity > 0
        |   ) AS Balances
        |)";
КонецПроцедуры"#,
            DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
            expect![[r#"
                LogicalOrInTheWhereSectionOfQuery @ 11:52..11:54
                  message: Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий
                  severity: Warning"#]],
        );
    }
}
