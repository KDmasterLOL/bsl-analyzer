//! Built-in function signatures for 1C:Enterprise platform.
//!
//! This module provides type signatures for ~100+ platform functions like:
//! - String functions: СтрДлина, НСтр, СтрШаблон, СтрЗаменить
//! - Type functions: ТипЗнч, Тип
//! - Date functions: ТекущаяДата, Год, Месяц
//! - Math functions: Окр, Цел, Макс, Мин
//! - Platform functions: ПредопределенноеЗначение, Выполнить, ОписаниеТипов
//! - Error handling: ИнформацияОбОшибке, ПодробноеПредставлениеОшибки
//! - Collection constructors: Новый
//!
//! Function list based on real-world usage statistics from doc3 project (6,540 BSL files).

use hir_def::ty::{FunctionSignature, Ty};
use rustc_hash::FxHashMap;
use std::sync::OnceLock;

/// Global registry of built-in platform functions.
///
/// Initialized once on first access and reused for all subsequent calls.
static BUILTIN_FUNCTIONS: OnceLock<BuiltinFunctions> = OnceLock::new();

/// Get the global built-in functions registry.
pub fn builtin_functions() -> &'static BuiltinFunctions {
    BUILTIN_FUNCTIONS.get_or_init(BuiltinFunctions::new)
}

/// Registry of built-in platform function signatures.
///
/// Contains type signatures for all standard 1C:Enterprise platform functions.
/// Functions are indexed by their lowercase name for case-insensitive lookup.
#[derive(Debug)]
pub struct BuiltinFunctions {
    /// Signatures indexed by lowercase function name.
    signatures: FxHashMap<String, FunctionSignature>,
}

impl BuiltinFunctions {
    /// Create and populate the built-in functions registry.
    ///
    /// This is called once during initialization.
    fn new() -> Self {
        let mut signatures = FxHashMap::default();

        // String functions
        Self::add_string_functions(&mut signatures);

        // Type functions
        Self::add_type_functions(&mut signatures);

        // Date functions
        Self::add_date_functions(&mut signatures);

        // Math functions
        Self::add_math_functions(&mut signatures);

        // Conversion functions
        Self::add_conversion_functions(&mut signatures);

        // Collection functions
        Self::add_collection_functions(&mut signatures);

        // System functions
        Self::add_system_functions(&mut signatures);

        // Platform-specific functions
        Self::add_platform_functions(&mut signatures);

        // Error handling functions
        Self::add_error_functions(&mut signatures);

        tracing::debug!("initialized {} built-in function signatures", signatures.len());

        Self { signatures }
    }

    /// Get function signature by name (case-insensitive).
    pub fn get(&self, name: &str) -> Option<&FunctionSignature> {
        let name_lower = name.to_lowercase();
        self.signatures.get(&name_lower)
    }

    /// Add string manipulation functions.
    fn add_string_functions(sigs: &mut FxHashMap<String, FunctionSignature>) {
        // СтрДлина(Строка) -> Число
        Self::add_fn(sigs, "стрдлина", vec![Ty::String], Ty::Number);
        Self::add_fn(sigs, "strlen", vec![Ty::String], Ty::Number);

        // ВРег(Строка) -> Строка
        Self::add_fn(sigs, "врег", vec![Ty::String], Ty::String);
        Self::add_fn(sigs, "upper", vec![Ty::String], Ty::String);

        // НРег(Строка) -> Строка
        Self::add_fn(sigs, "нрег", vec![Ty::String], Ty::String);
        Self::add_fn(sigs, "lower", vec![Ty::String], Ty::String);

        // ТРег(Строка) -> Строка
        Self::add_fn(sigs, "трег", vec![Ty::String], Ty::String);
        Self::add_fn(sigs, "title", vec![Ty::String], Ty::String);

        // СокрЛП(Строка) -> Строка
        Self::add_fn(sigs, "сокрлп", vec![Ty::String], Ty::String);
        Self::add_fn(sigs, "trimall", vec![Ty::String], Ty::String);

        // СокрЛ(Строка) -> Строка
        Self::add_fn(sigs, "сокрл", vec![Ty::String], Ty::String);
        Self::add_fn(sigs, "triml", vec![Ty::String], Ty::String);

        // СокрП(Строка) -> Строка
        Self::add_fn(sigs, "сокрп", vec![Ty::String], Ty::String);
        Self::add_fn(sigs, "trimr", vec![Ty::String], Ty::String);

        // Лев(Строка, Число) -> Строка
        Self::add_fn(sigs, "лев", vec![Ty::String, Ty::Number], Ty::String);
        Self::add_fn(sigs, "left", vec![Ty::String, Ty::Number], Ty::String);

        // Прав(Строка, Число) -> Строка
        Self::add_fn(sigs, "прав", vec![Ty::String, Ty::Number], Ty::String);
        Self::add_fn(sigs, "right", vec![Ty::String, Ty::Number], Ty::String);

        // Сред(Строка, Число, Число) -> Строка
        Self::add_fn(sigs, "сред", vec![Ty::String, Ty::Number, Ty::Number], Ty::String);
        Self::add_fn(sigs, "mid", vec![Ty::String, Ty::Number, Ty::Number], Ty::String);

        // СтрНайти(Строка, Строка) -> Число
        Self::add_fn(sigs, "стрнайти", vec![Ty::String, Ty::String], Ty::Number);
        Self::add_fn(sigs, "strfind", vec![Ty::String, Ty::String], Ty::Number);

        // СтрЗаменить(Строка, Строка, Строка) -> Строка
        Self::add_fn(sigs, "стрзаменить", vec![Ty::String, Ty::String, Ty::String], Ty::String);
        Self::add_fn(sigs, "strreplace", vec![Ty::String, Ty::String, Ty::String], Ty::String);

        // СтрРазделить(Строка, Строка) -> Массив
        Self::add_fn(sigs, "стрразделить", vec![Ty::String, Ty::String], Ty::Array);
        Self::add_fn(sigs, "strsplit", vec![Ty::String, Ty::String], Ty::Array);

        // СтрСоединить(Массив, Строка) -> Строка
        Self::add_fn(sigs, "стрсоединить", vec![Ty::Array, Ty::String], Ty::String);
        Self::add_fn(sigs, "strconcat", vec![Ty::Array, Ty::String], Ty::String);

        // ПустаяСтрока(Строка) -> Булево
        Self::add_fn(sigs, "пустаястрока", vec![Ty::String], Ty::Boolean);
        Self::add_fn(sigs, "isblankstring", vec![Ty::String], Ty::Boolean);

        // СтрШаблон(Строка, ...) -> Строка
        // Note: СтрШаблон has variadic parameters, for now use Unknown for params
        Self::add_fn(sigs, "стршаблон", vec![Ty::String], Ty::String);
        Self::add_fn(sigs, "strtemplate", vec![Ty::String], Ty::String);

        // НСтр(Строка) -> Строка - multi-language strings (40,552 uses in real code)
        Self::add_fn(sigs, "нстр", vec![Ty::String], Ty::String);
        Self::add_fn(sigs, "nstr", vec![Ty::String], Ty::String);

        // ПодставитьПараметрыВСтроку(Строка, ...) -> Строка (5,894 uses)
        // Note: variadic parameters
        Self::add_fn(sigs, "подставитьпараметрывстроку", vec![Ty::String], Ty::String);
        Self::add_fn(sigs, "substituteparameterstostring", vec![Ty::String], Ty::String);
    }

    /// Add type-related functions.
    fn add_type_functions(sigs: &mut FxHashMap<String, FunctionSignature>) {
        // ТипЗнч(Произвольный) -> Тип
        Self::add_fn(sigs, "типзнч", vec![Ty::Unknown], Ty::Type);
        Self::add_fn(sigs, "typeof", vec![Ty::Unknown], Ty::Type);

        // Тип(Строка) -> Тип
        Self::add_fn(sigs, "тип", vec![Ty::String], Ty::Type);
        Self::add_fn(sigs, "type", vec![Ty::String], Ty::Type);

        // ТипЗначенияСтр(Тип) -> Строка
        Self::add_fn(sigs, "типзначениястр", vec![Ty::Type], Ty::String);
        Self::add_fn(sigs, "typevaluetostr", vec![Ty::Type], Ty::String);
    }

    /// Add date/time functions.
    fn add_date_functions(sigs: &mut FxHashMap<String, FunctionSignature>) {
        // ТекущаяДата() -> Дата
        Self::add_fn(sigs, "текущаядата", vec![], Ty::Date);
        Self::add_fn(sigs, "currentdate", vec![], Ty::Date);

        // Год(Дата) -> Число
        Self::add_fn(sigs, "год", vec![Ty::Date], Ty::Number);
        Self::add_fn(sigs, "year", vec![Ty::Date], Ty::Number);

        // Месяц(Дата) -> Число
        Self::add_fn(sigs, "месяц", vec![Ty::Date], Ty::Number);
        Self::add_fn(sigs, "month", vec![Ty::Date], Ty::Number);

        // День(Дата) -> Число
        Self::add_fn(sigs, "день", vec![Ty::Date], Ty::Number);
        Self::add_fn(sigs, "day", vec![Ty::Date], Ty::Number);

        // Час(Дата) -> Число
        Self::add_fn(sigs, "час", vec![Ty::Date], Ty::Number);
        Self::add_fn(sigs, "hour", vec![Ty::Date], Ty::Number);

        // Минута(Дата) -> Число
        Self::add_fn(sigs, "минута", vec![Ty::Date], Ty::Number);
        Self::add_fn(sigs, "minute", vec![Ty::Date], Ty::Number);

        // Секунда(Дата) -> Число
        Self::add_fn(sigs, "секунда", vec![Ty::Date], Ty::Number);
        Self::add_fn(sigs, "second", vec![Ty::Date], Ty::Number);

        // НачалоГода(Дата) -> Дата
        Self::add_fn(sigs, "началогода", vec![Ty::Date], Ty::Date);
        Self::add_fn(sigs, "begofyear", vec![Ty::Date], Ty::Date);

        // КонецГода(Дата) -> Дата
        Self::add_fn(sigs, "конецгода", vec![Ty::Date], Ty::Date);
        Self::add_fn(sigs, "endofyear", vec![Ty::Date], Ty::Date);

        // ДобавитьМесяц(Дата, Число) -> Дата
        Self::add_fn(sigs, "добавитьмесяц", vec![Ty::Date, Ty::Number], Ty::Date);
        Self::add_fn(sigs, "addmonth", vec![Ty::Date, Ty::Number], Ty::Date);
    }

    /// Add mathematical functions.
    fn add_math_functions(sigs: &mut FxHashMap<String, FunctionSignature>) {
        // Окр(Число, Число) -> Число
        Self::add_fn(sigs, "окр", vec![Ty::Number, Ty::Number], Ty::Number);
        Self::add_fn(sigs, "round", vec![Ty::Number, Ty::Number], Ty::Number);

        // Цел(Число) -> Число
        Self::add_fn(sigs, "цел", vec![Ty::Number], Ty::Number);
        Self::add_fn(sigs, "int", vec![Ty::Number], Ty::Number);

        // Макс(Число, Число) -> Число
        Self::add_fn(sigs, "макс", vec![Ty::Number, Ty::Number], Ty::Number);
        Self::add_fn(sigs, "max", vec![Ty::Number, Ty::Number], Ty::Number);

        // Мин(Число, Число) -> Число
        Self::add_fn(sigs, "мин", vec![Ty::Number, Ty::Number], Ty::Number);
        Self::add_fn(sigs, "min", vec![Ty::Number, Ty::Number], Ty::Number);

        // Sqrt(Число) -> Число
        Self::add_fn(sigs, "sqrt", vec![Ty::Number], Ty::Number);
        Self::add_fn(sigs, "кв", vec![Ty::Number], Ty::Number);

        // Pow(Число, Число) -> Число
        Self::add_fn(sigs, "pow", vec![Ty::Number, Ty::Number], Ty::Number);
        Self::add_fn(sigs, "степень", vec![Ty::Number, Ty::Number], Ty::Number);

        // Abs(Число) -> Число
        Self::add_fn(sigs, "abs", vec![Ty::Number], Ty::Number);

        // Log(Число) -> Число
        Self::add_fn(sigs, "log", vec![Ty::Number], Ty::Number);
        Self::add_fn(sigs, "ln", vec![Ty::Number], Ty::Number);

        // Log10(Число) -> Число
        Self::add_fn(sigs, "log10", vec![Ty::Number], Ty::Number);
        Self::add_fn(sigs, "lg", vec![Ty::Number], Ty::Number);
    }

    /// Add conversion functions.
    fn add_conversion_functions(sigs: &mut FxHashMap<String, FunctionSignature>) {
        // Строка(Произвольный) -> Строка
        Self::add_fn(sigs, "строка", vec![Ty::Unknown], Ty::String);
        Self::add_fn(sigs, "string", vec![Ty::Unknown], Ty::String);

        // Число(Строка) -> Число
        Self::add_fn(sigs, "число", vec![Ty::String], Ty::Number);
        Self::add_fn(sigs, "number", vec![Ty::String], Ty::Number);

        // Булево(Произвольный) -> Булево
        Self::add_fn(sigs, "булево", vec![Ty::Unknown], Ty::Boolean);
        Self::add_fn(sigs, "boolean", vec![Ty::Unknown], Ty::Boolean);

        // Дата(Строка) -> Дата
        Self::add_fn(sigs, "дата", vec![Ty::String], Ty::Date);
        Self::add_fn(sigs, "date", vec![Ty::String], Ty::Date);

        // Формат(Произвольный, Строка) -> Строка
        Self::add_fn(sigs, "формат", vec![Ty::Unknown, Ty::String], Ty::String);
        Self::add_fn(sigs, "format", vec![Ty::Unknown, Ty::String], Ty::String);
    }

    /// Add collection-related functions.
    fn add_collection_functions(sigs: &mut FxHashMap<String, FunctionSignature>) {
        // Новый is handled specially in inference since it's a constructor
        // But we can add it here for completeness
        Self::add_fn(sigs, "новый", vec![Ty::Type], Ty::Unknown);
        Self::add_fn(sigs, "new", vec![Ty::Type], Ty::Unknown);
    }

    /// Add system functions.
    fn add_system_functions(sigs: &mut FxHashMap<String, FunctionSignature>) {
        // Сообщить(Строка)
        Self::add_proc(sigs, "сообщить", vec![Ty::String]);
        Self::add_proc(sigs, "message", vec![Ty::String]);

        // Предупреждение(Строка)
        Self::add_proc(sigs, "предупреждение", vec![Ty::String]);
        Self::add_proc(sigs, "warning", vec![Ty::String]);

        // ВызватьИсключение(Строка)
        Self::add_proc(sigs, "вызватьисключение", vec![Ty::String]);
        Self::add_proc(sigs, "raise", vec![Ty::String]);

        // ЗначениеЗаполнено(Произвольный) -> Булево
        Self::add_fn(sigs, "значениезаполнено", vec![Ty::Unknown], Ty::Boolean);
        Self::add_fn(sigs, "valuefilled", vec![Ty::Unknown], Ty::Boolean);

        // ПустоеЗначение(Тип) -> Произвольный
        Self::add_fn(sigs, "пустоезначение", vec![Ty::Type], Ty::Unknown);
        Self::add_fn(sigs, "emptyvalue", vec![Ty::Type], Ty::Unknown);
    }

    /// Add platform-specific functions.
    fn add_platform_functions(sigs: &mut FxHashMap<String, FunctionSignature>) {
        // ПредопределенноеЗначение(Строка) -> Произвольный (6,182 uses)
        Self::add_fn(sigs, "предопределенноезначение", vec![Ty::String], Ty::Unknown);
        Self::add_fn(sigs, "predefinedvalue", vec![Ty::String], Ty::Unknown);

        // ОписаниеТипов(...) -> ОписаниеТипов (3,877 uses)
        // Note: variadic parameters for type description constructor
        Self::add_fn(sigs, "описаниетипов", vec![], Ty::Unknown);
        Self::add_fn(sigs, "typedescription", vec![], Ty::Unknown);

        // ЗаполнитьЗначенияСвойств(Объект, Источник) (4,585 uses)
        Self::add_proc(sigs, "заполнитьзначениясвойств", vec![Ty::Unknown, Ty::Unknown]);
        Self::add_proc(sigs, "fillpropertyvalues", vec![Ty::Unknown, Ty::Unknown]);

        // Выполнить(Строка) -> Произвольный (5,801 uses)
        // Dynamic code execution - returns Unknown
        Self::add_fn(sigs, "выполнить", vec![Ty::String], Ty::Unknown);
        Self::add_fn(sigs, "execute", vec![Ty::String], Ty::Unknown);
    }

    /// Add error handling functions.
    fn add_error_functions(sigs: &mut FxHashMap<String, FunctionSignature>) {
        // ИнформацияОбОшибке() -> ИнформацияОбОшибке (3,266 uses)
        Self::add_fn(sigs, "информацияобошибке", vec![], Ty::Unknown);
        Self::add_fn(sigs, "errorinfo", vec![], Ty::Unknown);

        // ПодробноеПредставлениеОшибки(ИнформацияОбОшибке) -> Строка (2,269 uses)
        Self::add_fn(sigs, "подробноепредставлениеошибки", vec![Ty::Unknown], Ty::String);
        Self::add_fn(sigs, "detailederrordescription", vec![Ty::Unknown], Ty::String);

        // КраткоеПредставлениеОшибки(ИнформацияОбОшибке) -> Строка
        Self::add_fn(sigs, "краткоепредставлениеошибки", vec![Ty::Unknown], Ty::String);
        Self::add_fn(sigs, "brieferrordescription", vec![Ty::Unknown], Ty::String);
    }

    /// Helper: Add a function signature.
    fn add_fn(
        sigs: &mut FxHashMap<String, FunctionSignature>,
        name: &str,
        params: Vec<Ty>,
        ret: Ty,
    ) {
        let sig = FunctionSignature::function(params, ret);
        sigs.insert(name.to_lowercase(), sig);
    }

    /// Helper: Add a procedure signature (returns Undefined).
    fn add_proc(sigs: &mut FxHashMap<String, FunctionSignature>, name: &str, params: Vec<Ty>) {
        let sig = FunctionSignature::procedure(params);
        sigs.insert(name.to_lowercase(), sig);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_functions_initialization() {
        let builtins = builtin_functions();
        // Should have many signatures
        assert!(builtins.signatures.len() > 50);
    }

    #[test]
    fn test_string_functions() {
        let builtins = builtin_functions();

        // СтрДлина(Строка) -> Число
        let sig = builtins.get("стрдлина").unwrap();
        assert_eq!(sig.params.len(), 1);
        assert_eq!(sig.params[0], Ty::String);
        assert_eq!(*sig.ret, Ty::Number);

        // English variant
        let sig_en = builtins.get("strlen").unwrap();
        assert_eq!(*sig_en.ret, Ty::Number);

        // ВРег(Строка) -> Строка
        let sig = builtins.get("врег").unwrap();
        assert_eq!(*sig.ret, Ty::String);
    }

    #[test]
    fn test_type_functions() {
        let builtins = builtin_functions();

        // ТипЗнч(Произвольный) -> Тип
        let sig = builtins.get("типзнч").unwrap();
        assert_eq!(sig.params.len(), 1);
        assert_eq!(*sig.ret, Ty::Type);

        // Тип(Строка) -> Тип
        let sig = builtins.get("тип").unwrap();
        assert_eq!(sig.params[0], Ty::String);
        assert_eq!(*sig.ret, Ty::Type);
    }

    #[test]
    fn test_date_functions() {
        let builtins = builtin_functions();

        // ТекущаяДата() -> Дата
        let sig = builtins.get("текущаядата").unwrap();
        assert_eq!(sig.params.len(), 0);
        assert_eq!(*sig.ret, Ty::Date);

        // Год(Дата) -> Число
        let sig = builtins.get("год").unwrap();
        assert_eq!(sig.params[0], Ty::Date);
        assert_eq!(*sig.ret, Ty::Number);
    }

    #[test]
    fn test_math_functions() {
        let builtins = builtin_functions();

        // Окр(Число, Число) -> Число
        let sig = builtins.get("окр").unwrap();
        assert_eq!(sig.params.len(), 2);
        assert_eq!(sig.params[0], Ty::Number);
        assert_eq!(sig.params[1], Ty::Number);
        assert_eq!(*sig.ret, Ty::Number);

        // Макс(Число, Число) -> Число
        let sig = builtins.get("макс").unwrap();
        assert_eq!(*sig.ret, Ty::Number);
    }

    #[test]
    fn test_case_insensitive_lookup() {
        let builtins = builtin_functions();

        // Should work with any case
        assert!(builtins.get("СТРДЛИНА").is_some());
        assert!(builtins.get("стрдлина").is_some());
        assert!(builtins.get("СтрДлина").is_some());
        assert!(builtins.get("strlen").is_some());
        assert!(builtins.get("STRLEN").is_some());
    }

    #[test]
    fn test_procedures() {
        let builtins = builtin_functions();

        // Сообщить(Строка) - procedure returns Undefined
        let sig = builtins.get("сообщить").unwrap();
        assert_eq!(sig.params.len(), 1);
        assert_eq!(sig.params[0], Ty::String);
        assert_eq!(*sig.ret, Ty::Undefined);
    }

    #[test]
    fn test_most_used_functions() {
        let builtins = builtin_functions();

        // НСтр - most used (40,552 in real code)
        let sig = builtins.get("нстр").unwrap();
        assert_eq!(sig.params.len(), 1);
        assert_eq!(sig.params[0], Ty::String);
        assert_eq!(*sig.ret, Ty::String);

        // ПредопределенноеЗначение - 6,182 uses
        let sig = builtins.get("предопределенноезначение").unwrap();
        assert_eq!(sig.params.len(), 1);
        assert_eq!(sig.params[0], Ty::String);
        assert_eq!(*sig.ret, Ty::Unknown);

        // ПодставитьПараметрыВСтроку - 5,894 uses
        let sig = builtins.get("подставитьпараметрывстроку").unwrap();
        assert_eq!(*sig.ret, Ty::String);

        // Выполнить - 5,801 uses
        let sig = builtins.get("выполнить").unwrap();
        assert_eq!(sig.params[0], Ty::String);
        assert_eq!(*sig.ret, Ty::Unknown);

        // ОписаниеТипов - 3,877 uses
        let sig = builtins.get("описаниетипов").unwrap();
        assert_eq!(*sig.ret, Ty::Unknown);

        // ИнформацияОбОшибке - 3,266 uses
        let sig = builtins.get("информацияобошибке").unwrap();
        assert_eq!(sig.params.len(), 0);
        assert_eq!(*sig.ret, Ty::Unknown);

        // ПодробноеПредставлениеОшибки - 2,269 uses
        let sig = builtins.get("подробноепредставлениеошибки").unwrap();
        assert_eq!(sig.params.len(), 1);
        assert_eq!(*sig.ret, Ty::String);

        // ЗаполнитьЗначенияСвойств - 4,585 uses (procedure)
        let sig = builtins.get("заполнитьзначениясвойств").unwrap();
        assert_eq!(sig.params.len(), 2);
        assert_eq!(*sig.ret, Ty::Undefined);
    }
}
