//! JSDoc-style type annotation parser for BSL.
//!
//! Parses type hints from BSL doc comments like:
//! ```bsl
//! // Параметры:
//! //   Param1 - Строка - описание параметра
//! //   Param2 - Число - описание параметра
//! // Возвращаемое значение:
//! //   Булево - описание возвращаемого значения
//! ```

use crate::ty::Ty;
use crate::Name;

/// Type hints extracted from method documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodTypeHints {
    /// Parameter type hints: (parameter_name, type)
    pub params: Vec<(Name, Ty)>,

    /// Return type hint
    pub ret: Ty,
}

impl Default for MethodTypeHints {
    fn default() -> Self {
        Self { params: Vec::new(), ret: Ty::Undefined }
    }
}

/// Parse type hints from method documentation comment.
///
/// Recognizes the following patterns:
/// - Russian: "Параметры:", "Возвращаемое значение:"
/// - English: "Parameters:", "Return value:", "Returns:"
///
/// ## Example
/// ```text
/// // Параметры:
/// //   Строка1 - Строка - первая строка
/// //   Число1 - Число - какое-то число
/// // Возвращаемое значение:
/// //   Булево - результат сравнения
/// ```
pub fn parse_method_doc_types(doc_comment: &str) -> Option<MethodTypeHints> {
    let _span = tracing::trace_span!("parse_method_doc_types").entered();

    let mut hints = MethodTypeHints::default();
    let mut in_params_section = false;
    let mut in_return_section = false;

    for line in doc_comment.lines() {
        // Strip comment markers and whitespace
        let line = line.trim();
        // BSL uses only // comments, not ///
        let line = line.strip_prefix("//").unwrap_or(line).trim();

        // Check for section headers
        let line_lower = line.to_lowercase();

        if is_params_header(&line_lower) {
            in_params_section = true;
            in_return_section = false;
            continue;
        }

        if is_return_header(&line_lower) {
            in_params_section = false;
            in_return_section = true;
            continue;
        }

        // Parse parameter line
        if in_params_section {
            if let Some((name, ty)) = parse_param_line(line) {
                hints.params.push((name, ty));
            }
        }

        // Parse return type line
        if in_return_section {
            if let Some(ty) = parse_return_line(line) {
                hints.ret = ty;
                in_return_section = false; // Only first non-empty line
            }
        }
    }

    // Return None if no hints were found
    if hints.params.is_empty() && hints.ret == Ty::Undefined {
        tracing::trace!("no type hints found in doc comment");
        return None;
    }

    tracing::trace!("parsed {} parameter types, return type: {:?}", hints.params.len(), hints.ret);

    Some(hints)
}

/// Check if line is a parameters section header.
fn is_params_header(line_lower: &str) -> bool {
    line_lower.starts_with("параметры:")
        || line_lower.starts_with("parameters:")
        || line_lower == "параметры"
        || line_lower == "parameters"
}

/// Check if line is a return value section header.
fn is_return_header(line_lower: &str) -> bool {
    line_lower.starts_with("возвращаемое значение:")
        || line_lower.starts_with("return value:")
        || line_lower.starts_with("returns:")
        || line_lower == "возвращаемое значение"
        || line_lower == "return value"
        || line_lower == "returns"
}

/// Parse a parameter line: "Param1 - Строка - описание"
///
/// Expected format:
/// - Parameter name
/// - " - " separator
/// - Type name
/// - Optional " - " separator and description
fn parse_param_line(line: &str) -> Option<(Name, Ty)> {
    // Skip empty lines
    if line.is_empty() {
        return None;
    }

    // Split by " - " separator
    let parts: Vec<&str> = line.split(" - ").collect();
    if parts.len() < 2 {
        return None;
    }

    let param_name = parts[0].trim();
    let type_name = parts[1].trim();

    // Skip empty names or types
    if param_name.is_empty() || type_name.is_empty() {
        return None;
    }

    // Parse type
    let ty = parse_type_name(type_name);

    Some((Name::new(param_name), ty))
}

/// Parse a return type line: "Булево - описание"
///
/// Expected format:
/// - Type name
/// - Optional " - " separator and description
fn parse_return_line(line: &str) -> Option<Ty> {
    // Skip empty lines
    if line.is_empty() {
        return None;
    }

    // Split by " - " separator (description is optional)
    let type_name = if let Some(dash_pos) = line.find(" - ") { &line[..dash_pos] } else { line };

    let type_name = type_name.trim();
    if type_name.is_empty() {
        return None;
    }

    Some(parse_type_name(type_name))
}

/// Parse a type name string into a Ty.
///
/// Supports both Russian and English type names, case-insensitive.
fn parse_type_name(name: &str) -> Ty {
    let name_lower = name.to_lowercase();

    match name_lower.as_str() {
        // Primitive types
        "число" | "number" => Ty::Number,
        "строка" | "string" => Ty::String,
        "булево" | "boolean" => Ty::Boolean,
        "дата" | "date" => Ty::Date,
        "неопределено" | "undefined" => Ty::Undefined,

        // Collection types
        "массив" | "array" => Ty::Array,
        "структура" | "structure" => Ty::Structure,
        "соответствие" | "map" => Ty::Map,

        // Platform types
        "тип" | "type" => Ty::Type,
        "таблицазначений" | "valuetable" => Ty::ValueTable,
        "списокзначений" | "valuelist" => Ty::ValueList,

        // Special types
        "произвольный" | "any" | "arbitrary" => Ty::Unknown,

        // Unknown type name
        _ => {
            tracing::trace!("unknown type name in doc comment: {}", name);
            Ty::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_russian_doc() {
        let doc = r#"
// Выполняет сложение двух чисел
//
// Параметры:
//   Левый - Число - первое слагаемое
//   Правый - Число - второе слагаемое
// Возвращаемое значение:
//   Число - сумма двух чисел
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 2);
        assert_eq!(hints.params[0].0.as_str(), "Левый");
        assert_eq!(hints.params[0].1, Ty::Number);
        assert_eq!(hints.params[1].0.as_str(), "Правый");
        assert_eq!(hints.params[1].1, Ty::Number);
        assert_eq!(hints.ret, Ty::Number);
    }

    #[test]
    fn test_parse_english_doc() {
        let doc = r#"
// Checks if a string is empty
//
// Parameters:
//   Text - String - the string to check
// Returns:
//   Boolean - true if empty
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 1);
        assert_eq!(hints.params[0].0.as_str(), "Text");
        assert_eq!(hints.params[0].1, Ty::String);
        assert_eq!(hints.ret, Ty::Boolean);
    }

    #[test]
    fn test_parse_mixed_types() {
        let doc = r#"
// Параметры:
//   Строка1 - Строка - текст
//   Число1 - Число - количество
//   Флаг - Булево - признак
//   Дата1 - Дата - дата события
// Возвращаемое значение:
//   Массив - результат
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 4);
        assert_eq!(hints.params[0].1, Ty::String);
        assert_eq!(hints.params[1].1, Ty::Number);
        assert_eq!(hints.params[2].1, Ty::Boolean);
        assert_eq!(hints.params[3].1, Ty::Date);
        assert_eq!(hints.ret, Ty::Array);
    }

    #[test]
    fn test_parse_no_params() {
        let doc = r#"
// Получает текущую дату
//
// Возвращаемое значение:
//   Дата - текущая дата
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 0);
        assert_eq!(hints.ret, Ty::Date);
    }

    #[test]
    fn test_parse_no_return() {
        let doc = r#"
// Выводит сообщение
//
// Параметры:
//   Текст - Строка - текст сообщения
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 1);
        assert_eq!(hints.ret, Ty::Undefined);
    }

    #[test]
    fn test_parse_no_hints() {
        let doc = r#"
// Просто комментарий без типов
// Еще одна строка
"#;

        let hints = parse_method_doc_types(doc);
        assert!(hints.is_none());
    }

    #[test]
    fn test_parse_unknown_type() {
        let doc = r#"
// Параметры:
//   Объект - СправочникСсылка.Номенклатура - объект
// Возвращаемое значение:
//   Произвольный - результат
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 1);
        assert_eq!(hints.params[0].1, Ty::Unknown); // Unknown complex type
        assert_eq!(hints.ret, Ty::Unknown); // Произвольный = Unknown
    }

    #[test]
    fn test_parse_case_insensitive() {
        let doc = r#"
// Параметры:
//   Param1 - СТРОКА - текст
//   param2 - число - число
// ВОЗВРАЩАЕМОЕ ЗНАЧЕНИЕ:
//   булево - результат
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 2);
        assert_eq!(hints.params[0].1, Ty::String);
        assert_eq!(hints.params[1].1, Ty::Number);
        assert_eq!(hints.ret, Ty::Boolean);
    }
}
