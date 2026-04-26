//! MagicNumber diagnostic
//!
//! Detects hard-coded numeric literals in BSL code.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::MagicNumberContext;
use ide_db::TextRange;
use std::collections::HashSet;

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

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::MagicNumber` is encountered.
/// Applies configuration filtering:
/// - authorizedNumbers: numbers that are always allowed
/// - allowMagicIndexes: whether to allow numbers in array index access
/// - excludedConstructors: constructor types where numbers are allowed
pub fn from_hir(
    value: &str,
    range: TextRange,
    context: &MagicNumberContext,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::MagicNumber;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    // Apply authorizedNumbers configuration
    let config = Config::from_context(ctx);
    if is_authorized(value, &config) {
        return None;
    }

    // Apply context-based exclusions
    match context {
        MagicNumberContext::InDefaultParam => return None,
        MagicNumberContext::InStructureInsert => return None,
        MagicNumberContext::InStructureConstructor => return None,
        MagicNumberContext::InPropertyAssignment => return None,
        MagicNumberContext::InSimpleAssignment => return None,
        MagicNumberContext::InTernaryBranch => return None,
        MagicNumberContext::InRoundPrecision => return None,
        MagicNumberContext::InArrayIndex => {
            if config.allow_magic_indexes {
                return None;
            }
            // If not allowed, fall through to emit diagnostic
        }
        MagicNumberContext::InConstructor { type_name } => {
            if config.excluded_constructors.contains(type_name) {
                return None;
            }
            // If not excluded, fall through to emit diagnostic
        }
        // These contexts should emit diagnostics:
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

/// Configuration for the diagnostic
#[derive(Debug, Clone)]
struct Config {
    authorized_numbers: HashSet<String>,
    allow_magic_indexes: bool,
    excluded_constructors: HashSet<String>,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
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
            .map(|s| s.trim().to_lowercase())
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

/// Check if number is in authorized list
fn is_authorized(number: &str, config: &Config) -> bool {
    config.authorized_numbers.contains(number)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        assert_diagnostic_range, check_hir_diagnostic, check_hir_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};

    fn filter(diagnostics: &[crate::Diagnostic]) -> Vec<&crate::Diagnostic> {
        diagnostics.iter().filter(|d| d.code == DiagnosticCode::MagicNumber).collect()
    }

    #[test]
    fn test_comprehensive() {
        // Large inline regression fixture for magic-number coverage.
        // Uses 4-space indentation to match original column positions exactly.
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
        let diags = filter(&all);

        assert_eq!(diags.len(), 8, "Expected 8 diagnostics");

        assert_diagnostic_range(code, diags[0], 7, 31, 33); // 11
        assert_diagnostic_range(code, diags[1], 11, 20, 21); // 4
        assert_diagnostic_range(code, diags[2], 20, 21, 23); // 11
        assert_diagnostic_range(code, diags[3], 23, 24, 26); // 14
        assert_diagnostic_range(code, diags[4], 27, 34, 35); // 7
        assert_diagnostic_range(code, diags[5], 33, 37, 38); // 2
        assert_diagnostic_range(code, diags[6], 34, 37, 38); // 3
        assert_diagnostic_range(code, diags[7], 44, 12, 14); // 12
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
        let diags = filter(&all);

        assert_eq!(diags.len(), 0, "Should detect no numbers (2 is excluded by simple assignment)");
    }

    #[test]
    fn test_allow_magic_indexes_true() {
        let code = r"
Процедура Тест()
    Индекс = Массив[20];
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);

        assert_eq!(diags.len(), 0, "Array index should be excluded");
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
        let diags = filter(&all);

        assert_eq!(diags.len(), 2, "Array index should be detected when allowMagicIndexes = false");
    }

    #[test]
    fn test_comprehensive_with_allow_magic_indexes_false() {
        // Same code as test_comprehensive with 4-space indentation.
        // Lines 49-50 contain array index accesses with magic numbers.
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
        let diags = filter(&all);

        assert_eq!(diags.len(), 10, "Should find 10 diagnostics with allowMagicIndexes=false");

        assert_diagnostic_range(code, diags[8], 49, 32, 34);
        assert_diagnostic_range(code, diags[9], 50, 18, 20);
    }

    #[test]
    fn test_return_statement_not_excluded() {
        let code = r"
Функция КодОшибки()
    Возврат 12;
КонецФункции
        ";
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);

        assert_eq!(diags.len(), 1, "Return statement should NOT be excluded");
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
        let diags = filter(&all);

        assert_eq!(diags.len(), 0, "Structure.Insert() values should be excluded");
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
        let diags = filter(&all);

        assert_eq!(diags.len(), 0, "Structure constructor values should be excluded");
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
        let diags = filter(&all);

        assert_eq!(diags.len(), 0, "Property assignment values should be excluded");
    }

    #[test]
    fn test_default_parameter_excluded() {
        let code = r"
Процедура А(А = 566)
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);

        assert_eq!(diags.len(), 0, "Default parameter values should be excluded");
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
        let diags = filter(&all);

        assert_eq!(diags.len(), 0, "All numbers should be authorized with custom config");
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
        let diags = filter(&all);

        assert_eq!(
            diags.len(),
            0,
            "Simple assignments to meaningfully named variables should not be detected"
        );
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
        let diags = filter(&all);

        assert_eq!(
            diags.len(),
            0,
            "Structure.Insert() with meaningful keys should not be detected"
        );
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
        let diags = filter(&all);

        assert_eq!(
            diags.len(),
            0,
            "Property assignments with meaningful names should not be detected"
        );
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
        let diags = filter(&all);

        assert!(
            diags.len() >= 2,
            "Magic numbers in expressions should be detected, found {}",
            diags.len()
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
        let diags = filter(&all);

        assert_eq!(
            diags.len(),
            0,
            "NumberQualifiers constructor params should be excluded by default"
        );
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
        let diags = filter(&all);

        assert_eq!(
            diags.len(),
            0,
            "StringQualifiers constructor params should be excluded by default"
        );
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
        let diags = filter(&all);

        assert_eq!(diags.len(), 0, "English constructor names should be excluded by default");
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
        let diags = filter(&all);

        assert_eq!(diags.len(), 0, "Color constructor params should be excluded by default");
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
        let diags = filter(&all);
        assert_eq!(diags.len(), 1, "Array constructor should NOT be excluded by default");
        assert!(diags[0].message.contains("100"), "Should detect 100 in Array");

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MagicNumber,
            serde_json::json!({
                "excludedConstructors": "КвалификаторыЧисла,КвалификаторыСтроки,Массив"
            }),
        );

        let all2 = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diags2 = filter(&all2);
        assert_eq!(diags2.len(), 0, "Array constructor should be excluded with custom config");
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
        let diags = filter(&all);
        assert_eq!(
            diags.len(),
            2,
            "Empty excludedConstructors should detect all numbers in constructors"
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
        let diags = filter(&all);
        assert_eq!(
            diags.len(),
            0,
            "Numbers inside КвалификаторыЧисла in column definitions should be excluded"
        );
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
        let diags = filter(&all);

        assert_eq!(diags.len(), 0, "Round/Окр precision argument should be excluded");
    }

    #[test]
    fn test_round_first_arg_not_excluded() {
        let code = r"
Процедура Тест()
    Результат = Окр(3.14, 2);
КонецПроцедуры
        ";
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);

        assert_eq!(diags.len(), 1, "Round/Окр first argument should NOT be excluded");
        assert!(diags[0].message.contains("3.14"), "Should detect 3.14 as magic number");
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
        let diags = filter(&all);

        assert_eq!(diags.len(), 2, "Non-excluded constructors should be detected");
    }
}
