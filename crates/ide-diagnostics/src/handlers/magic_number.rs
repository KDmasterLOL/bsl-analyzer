use crate::define_metadata;
use crate::metadata::*;
use crate::AnalysisContext;
use crate::{Diagnostic, DiagnosticCode};
use hir::LocalRange;
use hir::MagicNumberContext;
use std::collections::HashSet;
use stdx::case::CaseExt;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    value: &str,
    range: LocalRange,
    context: &MagicNumberContext,
    ctx: &AnalysisContext,
) -> Option<Diagnostic<LocalRange>> {
    let code = DiagnosticCode::MagicNumber;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let config = Config::from_context(ctx);
    if is_authorized(value, &config) {
        return None;
    }

    match context {
        MagicNumberContext::InDefaultParam => return None,
        MagicNumberContext::InStructureInsert => return None,
        MagicNumberContext::InStructureConstructor => return None,
        MagicNumberContext::InPropertyAssignment => return None,
        MagicNumberContext::InSimpleAssignment => return None,
        MagicNumberContext::InTernaryBranch => return None,
        MagicNumberContext::InRoundPrecision => return None,
        MagicNumberContext::InForLoopBoundary => return None,
        MagicNumberContext::InArrayIndex => {
            if config.allow_magic_indexes {
                return None;
            }
        }
        MagicNumberContext::InConstructor { type_name } => {
            if config.excluded_constructors.contains(type_name) {
                return None;
            }
        }
        MagicNumberContext::InExpression
        | MagicNumberContext::InReturn
        | MagicNumberContext::InMethodCall
        | MagicNumberContext::Other => {}
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Магическое число {}. Замените число на константу с понятным названием.",
            value
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

const DEFAULT_AUTHORIZED_NUMBERS: &str = "-1,0,1,24,60,3600,86400";
const DEFAULT_ALLOW_MAGIC_INDEXES: bool = true;
const DEFAULT_EXCLUDED_CONSTRUCTORS: &str =
    "КвалификаторыЧисла,КвалификаторыСтроки,NumberQualifiers,StringQualifiers,Цвет,Color";

#[derive(Debug, Clone)]
struct Config {
    authorized_numbers: HashSet<String>,
    allow_magic_indexes: bool,
    excluded_constructors: HashSet<String>,
}

impl Config {
    fn from_context(ctx: &AnalysisContext) -> Self {
        let authorized_str = ctx.config_string(
            DiagnosticCode::MagicNumber,
            "authorizedNumbers",
            DEFAULT_AUTHORIZED_NUMBERS,
        );
        let authorized_numbers: HashSet<String> = authorized_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let allow_magic_indexes = ctx.config_bool(
            DiagnosticCode::MagicNumber,
            "allowMagicIndexes",
            DEFAULT_ALLOW_MAGIC_INDEXES,
        );

        let excluded_constructors_str = ctx.config_string(
            DiagnosticCode::MagicNumber,
            "excludedConstructors",
            DEFAULT_EXCLUDED_CONSTRUCTORS,
        );
        let excluded_constructors: HashSet<String> = excluded_constructors_str
            .split(',')
            .map(|s| s.trim().fold_lower())
            .filter(|s| !s.is_empty())
            .collect();

        tracing::debug!(
            authorized_count = authorized_numbers.len(),
            allow_indexes = allow_magic_indexes,
            excluded_constructors_count = excluded_constructors.len(),
            "MagicNumber config loaded"
        );

        Self { authorized_numbers, allow_magic_indexes, excluded_constructors }
    }
}

fn is_authorized(number: &str, config: &Config) -> bool {
    config.authorized_numbers.contains(number)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_hir_diagnostic, check_hir_diagnostic_with_config, format_diags};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::{expect, Expect};

    fn check_magic_number_snapshot(
        code: &str,
        diagnostics: Vec<crate::Diagnostic>,
        expected: Expect,
    ) {
        let diagnostics = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::MagicNumber)
            .collect::<Vec<_>>();
        expected.assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура ПроверкаЧисел()

    ПонятнаяПеременная = 6; // Нет ошибки
    СекундВЧасе = 60 * 60; // Ошибка на двух числах

    Если ТекущаяДатаИВремя > СекундВЧасе Тогда // Нет ошибки

        Результат = ?(Число1 = 11, Чтото, 3); // Ошибка на 11

    КонецЕсли;

    Если Описание = 4 Тогда // Ошибка на 4

        Возврат;

    КонецЕсли;

КонецПроцедуры

Процедура Б()
    Если Описание2 > 11 Тогда // Ошибка на 11

        Чтото = Чтото + 1; // Нет ошибки из-за исключения
        Чтото = Чтото + 14; // Ошибка на 14

    КонецЕсли;

    ЭтоВоскресенье = ДеньНедели = 7; // Тут ошибка, хоть и выглядит нормально.
    ДеньНеделиВоскресенье = 7;
    ЭтоВоскресенье = ДеньНедели = ДеньНеделиВоскресенье; // А вот тут уже ошибки нет

    ПроверочноеПеречисление = Новый Массив;
    ПроверочноеПеречисление.Добавить(1); // Нет ошибки из-за исключения
    ПроверочноеПеречисление.Добавить(2); // ошибка
    ПроверочноеПеречисление.Добавить(3); // ошибка

КонецПроцедуры

Процедура А(А = 566) // пропущенная ошибка

КонецПроцедуры

Функция КодОшибки()

    Возврат 12;

КонецФункции

Процедура Индексы()
    Индекс1 = Коллекция.Индексы[20]; // замечание при allowMagicIndexes = false
    Метод(Индексы[21]) // замечание при allowMagicIndexes = false
КонецПроцедуры"#;

        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(
            code,
            all,
            expect![[r#"
            MagicNumber @ 8:32..8:34
              message: Магическое число 11. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 12:21..12:22
              message: Магическое число 4. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 21:22..21:24
              message: Магическое число 11. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 24:25..24:27
              message: Магическое число 14. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 28:35..28:36
              message: Магическое число 7. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 34:38..34:39
              message: Магическое число 2. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 35:38..35:39
              message: Магическое число 3. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 45:13..45:15
              message: Магическое число 12. Замените число на константу с понятным названием.
              severity: Information"#]],
        );
    }

    #[test]
    fn test_authorized_numbers() {
        let code = r"
Процедура Тест()
    А = -1; // Authorized
    Б = 0;  // Authorized
    В = 1;  // Authorized
    Г = 2;  // Not authorized
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_allow_magic_indexes_true() {
        let code = r"
Процедура Тест()
    Индекс = Массив[20];
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_allow_magic_indexes_false() {
        let code = r"
Процедура Тест()
    Индекс = Массив[20];
    Элемент = Коллекция.Индексы[21];
КонецПроцедуры
        ";
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicNumber,
            serde_json::json!({
                "allowMagicIndexes": false
            }),
        );

        let all = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        check_magic_number_snapshot(
            code,
            all,
            expect![[r#"
            MagicNumber @ 3:21..3:23
              message: Магическое число 20. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 4:33..4:35
              message: Магическое число 21. Замените число на константу с понятным названием.
              severity: Information"#]],
        );
    }

    #[test]
    fn test_comprehensive_with_allow_magic_indexes_false() {
        let code = r#"Процедура ПроверкаЧисел()

    ПонятнаяПеременная = 6; // Нет ошибки
    СекундВЧасе = 60 * 60; // Ошибка на двух числах

    Если ТекущаяДатаИВремя > СекундВЧасе Тогда // Нет ошибки

        Результат = ?(Число1 = 11, Чтото, 3); // Ошибка на 11

    КонецЕсли;

    Если Описание = 4 Тогда // Ошибка на 4

        Возврат;

    КонецЕсли;

КонецПроцедуры

Процедура Б()
    Если Описание2 > 11 Тогда // Ошибка на 11

        Чтото = Чтото + 1; // Нет ошибки из-за исключения
        Чтото = Чтото + 14; // Ошибка на 14

    КонецЕсли;

    ЭтоВоскресенье = ДеньНедели = 7; // Тут ошибка, хоть и выглядит нормально.
    ДеньНеделиВоскресенье = 7;
    ЭтоВоскресенье = ДеньНедели = ДеньНеделиВоскресенье; // А вот тут уже ошибки нет

    ПроверочноеПеречисление = Новый Массив;
    ПроверочноеПеречисление.Добавить(1); // Нет ошибки из-за исключения
    ПроверочноеПеречисление.Добавить(2); // ошибка
    ПроверочноеПеречисление.Добавить(3); // ошибка

КонецПроцедуры

Процедура А(А = 566) // пропущенная ошибка

КонецПроцедуры

Функция КодОшибки()

    Возврат 12;

КонецФункции

Процедура Индексы()
    Индекс1 = Коллекция.Индексы[20]; // замечание при allowMagicIndexes = false
    Метод(Индексы[21]) // замечание при allowMagicIndexes = false
КонецПроцедуры"#;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicNumber,
            serde_json::json!({
                "allowMagicIndexes": false
            }),
        );

        let all = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        check_magic_number_snapshot(
            code,
            all,
            expect![[r#"
            MagicNumber @ 8:32..8:34
              message: Магическое число 11. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 12:21..12:22
              message: Магическое число 4. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 21:22..21:24
              message: Магическое число 11. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 24:25..24:27
              message: Магическое число 14. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 28:35..28:36
              message: Магическое число 7. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 34:38..34:39
              message: Магическое число 2. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 35:38..35:39
              message: Магическое число 3. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 45:13..45:15
              message: Магическое число 12. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 50:33..50:35
              message: Магическое число 20. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 51:19..51:21
              message: Магическое число 21. Замените число на константу с понятным названием.
              severity: Information"#]],
        );
    }

    #[test]
    fn test_return_statement_not_excluded() {
        let code = r"
Функция КодОшибки()
    Возврат 12;
КонецФункции
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(
            code,
            all,
            expect![[r#"
            MagicNumber @ 3:13..3:15
              message: Магическое число 12. Замените число на константу с понятным названием.
              severity: Information"#]],
        );
    }

    #[test]
    fn test_structure_insert_excluded() {
        let code = r#"
Процедура Тест()
    НоваяСтруктура = Новый Структура;
    НоваяСтруктура.Вставить("МояПеременная", 20);
    НоваяСтруктура.Вставить("ДругаяПеременная", 42);
КонецПроцедуры
        "#;
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_structure_constructor_excluded() {
        let code = r#"
Процедура Тест()
    Структура1 = Новый Структура("Поле1, Поле2", 5, 15);
    Структура2 = Новый ФиксированнаяСтруктура("Значение", 200);
КонецПроцедуры
        "#;
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_property_assignment_excluded() {
        let code = r#"
Процедура Тест()
    СтруктураСПолями = Новый Структура("МояПеременная");
    СтруктураСПолями.МояПеременная = 20;
КонецПроцедуры
        "#;
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_default_parameter_excluded() {
        let code = r"
Процедура А(А = 566)
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_custom_authorized_numbers() {
        let code = r"
Процедура Тест()
    СекундВМинуте = 60;
    МинутВЧасе = 60;
    ДнейВНеделе = 7;
КонецПроцедуры
        ";
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicNumber,
            serde_json::json!({
                "authorizedNumbers": "-1,0,1,60,7"
            }),
        );

        let all = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_simple_assignment_with_meaningful_name() {
        let code = r"
Процедура Тест()
    ДлительностьОперации = 120;
    МаксимальноеКоличествоПопыток = 5;
    ТаймаутСоединения = 30;
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_structure_insert_with_meaningful_key() {
        let code = r#"
Процедура Тест()
    Параметры = Новый Структура;
    Параметры.Вставить("Таймаут", 30);
    Параметры.Вставить("МаксимальныйРазмер", 1024);

    Сессия = Новый Структура;
    Сессия.Вставить("ВремяЖизни", 50);
    Сессия.Вставить("ПериодПроверки", 15);
КонецПроцедуры
        "#;
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_property_assignment_with_meaningful_name() {
        let code = r#"
Процедура Тест()
    Настройки = Новый Структура("Таймаут, Повторы");
    Настройки.Таймаут = 30;
    Настройки.Повторы = 5;
КонецПроцедуры
        "#;
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_magic_numbers_in_expressions() {
        let code = r"
Процедура Тест()
    СекундВЧасе = 60 * 60;
    Результат = Значение + 25;
    Если Счетчик > 100 Тогда
        Возврат 12;
    КонецЕсли;
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(
            code,
            all,
            expect![[r#"
            MagicNumber @ 4:28..4:30
              message: Магическое число 25. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 5:20..5:23
              message: Магическое число 100. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 6:17..6:19
              message: Магическое число 12. Замените число на константу с понятным названием.
              severity: Information"#]],
        );
    }

    #[test]
    fn test_for_loop_boundary_excluded() {
        // The numeric init (`= 2`) and bound (`По 38`) of a For loop are boundaries,
        // not magic numbers; a magic number in the loop body is still flagged.
        let code = r"
Процедура Тест()
    Для Сч = 2 По 38 Цикл
        Значение = Сч * 555;
    КонецЦикла;
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(
            code,
            all,
            expect![[r#"
                MagicNumber @ 4:25..4:28
                  message: Магическое число 555. Замените число на константу с понятным названием.
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_for_loop_compound_boundary_still_flagged() {
        // A magic number inside a compound boundary expression is not a bare boundary
        // literal and stays flagged, matching bsl-ls.
        let code = r"
Процедура Тест()
    Для Сч = 0 По Граница * 10 Цикл
    КонецЦикла;
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(
            code,
            all,
            expect![[r#"
                MagicNumber @ 3:29..3:31
                  message: Магическое число 10. Замените число на константу с понятным названием.
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_for_loop_boundary_edge_cases_match_bsl_ls() {
        // Verified against bsl-language-server 1.0.0: a negated literal (`По -5`) and a
        // parenthesized literal (`По (38)`) are pure-numeric boundaries and are NOT
        // flagged, while a non-trivial numeric expression (`По 2 + 3`) IS flagged.
        let code = r"
Процедура Тест()
    Для а = 0 По -5 Цикл
    КонецЦикла;
    Для б = 0 По (38) Цикл
    КонецЦикла;
    Для в = 0 По 2 + 3 Цикл
    КонецЦикла;
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(
            code,
            all,
            expect![[r#"
                MagicNumber @ 7:18..7:19
                  message: Магическое число 2. Замените число на константу с понятным названием.
                  severity: Information
                MagicNumber @ 7:22..7:23
                  message: Магическое число 3. Замените число на константу с понятным названием.
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_excluded_constructors_number_qualifiers() {
        let code = r"
Процедура Тест()
    Квалификатор = Новый КвалификаторыЧисла(10, 2);
    Квалификатор2 = Новый КвалификаторыЧисла(15, 3, ДопустимыйЗнак.Любой);
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_excluded_constructors_string_qualifiers() {
        let code = r"
Процедура Тест()
    Квалификатор = Новый КвалификаторыСтроки(100);
    Квалификатор2 = Новый КвалификаторыСтроки(255, ДопустимаяДлина.Переменная);
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_excluded_constructors_english_names() {
        let code = r"
Процедура Тест()
    Qualifier = New NumberQualifiers(10, 2);
    StrQualifier = New StringQualifiers(100);
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_excluded_constructors_color() {
        let code = r"
Процедура Тест()
    ЦветФона = Новый Цвет(1, 150, 150);
    BorderColor = New Color(255, 128, 0);
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_excluded_constructors_custom_list() {
        let code = r"
Процедура Тест()
    МетодОбработки(Новый Массив(100));
    МетодОбработки(Новый КвалификаторыЧисла(10, 2));
КонецПроцедуры
        ";

        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(
            code,
            all,
            expect![[r#"
            MagicNumber @ 3:33..3:36
              message: Магическое число 100. Замените число на константу с понятным названием.
              severity: Information"#]],
        );

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicNumber,
            serde_json::json!({
                "excludedConstructors": "КвалификаторыЧисла,КвалификаторыСтроки,Массив"
            }),
        );

        let all2 = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        check_magic_number_snapshot(code, all2, expect![[r#""#]]);
    }

    #[test]
    fn test_excluded_constructors_empty_disables() {
        let code = r"
Процедура Тест()
    МетодОбработки(Новый КвалификаторыЧисла(10, 2));
КонецПроцедуры
        ";

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicNumber,
            serde_json::json!({
                "excludedConstructors": ""
            }),
        );

        let all = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        check_magic_number_snapshot(
            code,
            all,
            expect![[r#"
            MagicNumber @ 3:45..3:47
              message: Магическое число 10. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 3:49..3:50
              message: Магическое число 2. Замените число на константу с понятным названием.
              severity: Information"#]],
        );
    }

    #[test]
    fn test_excluded_constructors_in_column_definition() {
        let code = r#"
Процедура Тест()
    ТаблицаДанных.Колонки.Добавить("ОстаткиПоЯчейкам", Новый ОписаниеТипов("Число", , , Новый КвалификаторыЧисла(10, 3)));
    ТаблицаДанных.Колонки.Добавить("ОстаткиПоЯчейкамВЕдИзм", Новый ОписаниеТипов("Число", , , Новый КвалификаторыЧисла(10, 3)));
    ТаблицаДанных.Колонки.Добавить("ОстаткиПоСкладу", Новый ОписаниеТипов("Число", , , Новый КвалификаторыЧисла(10, 3)));
    ТаблицаДанных.Колонки.Добавить("ОстаткиПоУчету", Новый ОписаниеТипов("Число", , , Новый КвалификаторыЧисла(10, 2)));
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_round_precision_excluded() {
        let code = r"
Процедура Тест()
    Результат = Окр(Значение, 2);
    Результат2 = Round(Value, 3);
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(code, all, expect![[r#""#]]);
    }

    #[test]
    fn test_round_first_arg_not_excluded() {
        let code = r"
Процедура Тест()
    Результат = Окр(3.14, 2);
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(
            code,
            all,
            expect![[r#"
            MagicNumber @ 3:21..3:25
              message: Магическое число 3.14. Замените число на константу с понятным названием.
              severity: Information"#]],
        );
    }

    #[test]
    fn test_other_constructors_not_excluded() {
        let code = r"
Процедура Тест()
    МетодОбработки(Новый Массив(100));
    Список = Новый СписокЗначений;
    Список.Добавить(42);
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        check_magic_number_snapshot(
            code,
            all,
            expect![[r#"
            MagicNumber @ 3:33..3:36
              message: Магическое число 100. Замените число на константу с понятным названием.
              severity: Information
            MagicNumber @ 5:21..5:23
              message: Магическое число 42. Замените число на константу с понятным названием.
              severity: Information"#]],
        );
    }
}
