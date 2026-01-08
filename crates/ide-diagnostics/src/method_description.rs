//! Parser for BSL method description comments.
//!
//! This module provides utilities to parse documentation comments for BSL methods,
//! specifically extracting return value descriptions.
//!
//! This is a simplified regex-based parser that handles the common cases without
//! requiring a full ANTLR4 grammar port.

/// Information about a method's return value description in documentation comments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnBlockInfo {
    /// Does the comment contain "Возвращаемое значение:" or "Returns:" keyword?
    pub has_return_keyword: bool,

    /// Is it a hyperlink reference like "См. Method()" or "See Method()"?
    pub is_hyperlink: bool,

    /// Types found with their descriptions.
    /// Each tuple is (type_name, description).
    /// Description is None if only type name is present without description text.
    pub types: Vec<(String, Option<String>)>,
}

impl ReturnBlockInfo {
    /// Create an empty return block info.
    pub fn new() -> Self {
        Self { has_return_keyword: false, is_hyperlink: false, types: Vec::new() }
    }
}

impl Default for ReturnBlockInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse return value description from method documentation comments.
///
/// Extracts structured information about return value descriptions using
/// regex patterns to match common documentation formats.
///
/// # Examples
///
/// ```text
/// // Описание метода
/// // Возвращаемое значение:
/// // Строка - описание результата
/// ```
///
/// Returns: `ReturnBlockInfo { has_return_keyword: true, is_hyperlink: false, types: [("Строка", Some("описание результата"))] }`
///
/// # Arguments
/// * `comments` - Lines of comments extracted from method documentation
pub fn parse_return_block_simple(comments: &[String]) -> ReturnBlockInfo {
    let mut info = ReturnBlockInfo::new();

    // Find the line with return keyword
    let return_keyword_index = comments.iter().position(|line| is_return_keyword_line(line));

    if return_keyword_index.is_none() {
        return info;
    }

    info.has_return_keyword = true;
    let start_index = return_keyword_index.unwrap() + 1;

    // Check if next non-empty line is a hyperlink reference
    for comment in comments.iter().skip(start_index) {
        let line = comment.trim();
        if line.is_empty() {
            continue;
        }

        // Check for hyperlink pattern: "См. ..." or "See ..."
        if is_hyperlink_line(line) {
            info.is_hyperlink = true;
            return info;
        }

        // Not a hyperlink, break to parse types
        break;
    }

    // Parse type descriptions from lines after return keyword
    for comment in comments.iter().skip(start_index) {
        let line = comment.trim();

        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Stop at next section keyword
        if is_section_keyword(line) {
            break;
        }

        // Parse type line
        if let Some((type_name, description)) = parse_type_line(line) {
            info.types.push((type_name, description));
        }
    }

    info
}

/// Check if a line contains a return value keyword.
///
/// Matches (case-insensitive):
/// - "Возвращаемое значение:"
/// - "Returns:"
fn is_return_keyword_line(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    lower.contains("возвращаемое значение:") || lower.contains("returns:")
}

/// Check if a line is a hyperlink reference.
///
/// Matches:
/// - "См. MethodName()"
/// - "See MethodName()"
/// - "см. MethodName"
fn is_hyperlink_line(line: &str) -> bool {
    let trimmed = line.trim();
    let lower = trimmed.to_lowercase();
    lower.starts_with("см.") || lower.starts_with("see ")
}

/// Check if a line is a section keyword (start of another documentation section).
///
/// Matches:
/// - "Параметры:"
/// - "Parameters:"
/// - "Пример:"
/// - "Example:"
fn is_section_keyword(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("параметры:")
        || lower.contains("parameters:")
        || lower.contains("пример:")
        || lower.contains("example:")
}

/// Parse a type description line.
///
/// Handles various patterns:
/// 1. "Строка - описание текста" → ("Строка", Some("описание текста"))
/// 2. "- Строка - описание" → ("Строка", Some("описание"))
/// 3. "Строка" → ("Строка", None)
/// 4. "* FieldName - Type - description" → Skip (structured type field)
/// 5. "  Structure:" → Skip (type definition)
///
/// # Returns
/// `Some((type_name, description))` if line contains a type, `None` otherwise.
fn parse_type_line(line: &str) -> Option<(String, Option<String>)> {
    let trimmed = line.trim();

    // Skip structured type fields (e.g., "* FieldName - Type - description")
    if trimmed.starts_with('*') {
        return None;
    }

    // Pattern 0: "Type:" (type name with colon, no description)
    // This is used to declare structured types before listing their fields
    // Example: "Структура:", "Structure:", "Массив:"
    if trimmed.ends_with(':') && !trimmed.contains(" - ") {
        let type_name = trimmed.trim_end_matches(':').trim();
        if !type_name.is_empty() && is_likely_type_name(type_name) {
            return Some((type_name.to_string(), None));
        }
        // Not a type name - skip this line
        return None;
    }

    // Pattern 1: "- Type - description" (leading dash with description)
    if let Some(without_dash) = trimmed.strip_prefix('-') {
        let without_dash = without_dash.trim();
        if trimmed.matches('-').count() >= 2 {
            if let Some(sep_pos) = without_dash.find(" - ") {
                let type_name = without_dash[..sep_pos].trim().to_string();
                let mut description = without_dash[sep_pos + 3..].trim().to_string();
                // Remove trailing colon if present (e.g., "описание:")
                if description.ends_with(':') {
                    description.pop();
                    description = description.trim_end().to_string();
                }
                return Some((type_name, Some(description)));
            }
        }
    }

    // Pattern 2: "Type - description" (no leading dash)
    if let Some(sep_pos) = trimmed.find(" - ") {
        let type_name = trimmed[..sep_pos].trim();
        // Skip if type name is empty or starts with * (structured field)
        if type_name.is_empty() || type_name.starts_with('*') {
            return None;
        }
        let type_name = type_name.to_string();
        let mut description = trimmed[sep_pos + 3..].trim().to_string();
        // Remove trailing colon if present (e.g., "описание:")
        if description.ends_with(':') {
            description.pop();
            description = description.trim_end().to_string();
        }
        return Some((type_name, Some(description)));
    }

    // Pattern 3: "- Type" (leading dash without description)
    if let Some(without_dash) = trimmed.strip_prefix('-') {
        let type_name = without_dash.trim().to_string();
        if !type_name.is_empty() {
            return Some((type_name, None));
        }
    }

    // Pattern 4: "Type" (just type name, no dash, no description)
    // Only accept if it looks like a type name (starts with capital letter or is a known type)
    if !trimmed.is_empty() && is_likely_type_name(trimmed) {
        return Some((trimmed.to_string(), None));
    }

    None
}

/// Check if a string is likely a type name.
///
/// Heuristics:
/// - Starts with a capital letter (e.g., "Строка", "Boolean")
/// - Is a known BSL type (case-insensitive)
fn is_likely_type_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    // Check if starts with uppercase (Cyrillic or Latin)
    let first_char = s.chars().next().unwrap();
    if first_char.is_uppercase() {
        return true;
    }

    // Check against known BSL types (case-insensitive)
    let lower = s.to_lowercase();
    matches!(
        lower.as_str(),
        "строка"
            | "string"
            | "число"
            | "number"
            | "булево"
            | "boolean"
            | "дата"
            | "date"
            | "неопределено"
            | "undefined"
            | "null"
            | "произвольный"
            | "arbitrary"
            | "структура"
            | "structure"
            | "массив"
            | "array"
            | "соответствие"
            | "map"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_return_keyword() {
        let comments = vec!["Описание метода".to_string(), "Еще описание".to_string()];

        let info = parse_return_block_simple(&comments);

        assert!(!info.has_return_keyword);
        assert!(!info.is_hyperlink);
        assert!(info.types.is_empty());
    }

    #[test]
    fn test_return_keyword_with_type_and_description() {
        let comments = vec![
            "Описание метода".to_string(),
            "Возвращаемое значение:".to_string(),
            "Строка - описание результата".to_string(),
        ];

        let info = parse_return_block_simple(&comments);

        assert!(info.has_return_keyword);
        assert!(!info.is_hyperlink);
        assert_eq!(info.types.len(), 1);
        assert_eq!(info.types[0].0, "Строка");
        assert_eq!(info.types[0].1, Some("описание результата".to_string()));
    }

    #[test]
    fn test_return_keyword_empty_block() {
        let comments = vec!["Описание метода".to_string(), "Возвращаемое значение:".to_string()];

        let info = parse_return_block_simple(&comments);

        assert!(info.has_return_keyword);
        assert!(!info.is_hyperlink);
        assert!(info.types.is_empty());
    }

    #[test]
    fn test_hyperlink_reference_russian() {
        let comments = vec!["Возвращаемое значение:".to_string(), "См. OtherMethod()".to_string()];

        let info = parse_return_block_simple(&comments);

        assert!(info.has_return_keyword);
        assert!(info.is_hyperlink);
        assert!(info.types.is_empty());
    }

    #[test]
    fn test_hyperlink_reference_english() {
        let comments = vec!["Returns:".to_string(), "See OtherMethod()".to_string()];

        let info = parse_return_block_simple(&comments);

        assert!(info.has_return_keyword);
        assert!(info.is_hyperlink);
        assert!(info.types.is_empty());
    }

    #[test]
    fn test_type_without_description() {
        let comments = vec!["Возвращаемое значение:".to_string(), "Строка".to_string()];

        let info = parse_return_block_simple(&comments);

        assert!(info.has_return_keyword);
        assert!(!info.is_hyperlink);
        assert_eq!(info.types.len(), 1);
        assert_eq!(info.types[0].0, "Строка");
        assert_eq!(info.types[0].1, None);
    }

    #[test]
    fn test_multiple_types_with_dash() {
        let comments = vec![
            "Возвращаемое значение:".to_string(),
            "- Строка - описание строки".to_string(),
            "- булево - описание булево".to_string(),
            "- Неопределено - если неизвестно".to_string(),
        ];

        let info = parse_return_block_simple(&comments);

        assert!(info.has_return_keyword);
        assert!(!info.is_hyperlink);
        assert_eq!(info.types.len(), 3);
        assert_eq!(info.types[0].0, "Строка");
        assert_eq!(info.types[0].1, Some("описание строки".to_string()));
        assert_eq!(info.types[1].0, "булево");
        assert_eq!(info.types[1].1, Some("описание булево".to_string()));
        assert_eq!(info.types[2].0, "Неопределено");
        assert_eq!(info.types[2].1, Some("если неизвестно".to_string()));
    }

    #[test]
    fn test_multiple_types_without_description() {
        let comments = vec![
            "Возвращаемое значение:".to_string(),
            "- Строка".to_string(),
            "- булево".to_string(),
        ];

        let info = parse_return_block_simple(&comments);

        assert!(info.has_return_keyword);
        assert!(!info.is_hyperlink);
        assert_eq!(info.types.len(), 2);
        assert_eq!(info.types[0].0, "Строка");
        assert_eq!(info.types[0].1, None);
        assert_eq!(info.types[1].0, "булево");
        assert_eq!(info.types[1].1, None);
    }

    #[test]
    fn test_structured_type_with_fields() {
        let comments = vec![
            "Возвращаемое значение:".to_string(),
            "Структура:".to_string(),
            "* FieldName - Строка - описание поля".to_string(),
            "* AnotherField - Число - еще поле".to_string(),
        ];

        let info = parse_return_block_simple(&comments);

        assert!(info.has_return_keyword);
        assert!(!info.is_hyperlink);
        // "Структура:" is now parsed as a type (Pattern 0: type with colon)
        // Structured fields (with *) are still skipped as they represent nested structure
        assert_eq!(info.types.len(), 1);
        assert_eq!(info.types[0].0, "Структура");
        assert_eq!(info.types[0].1, None); // No description for the type itself
    }

    #[test]
    fn test_english_keywords() {
        let comments = vec!["Returns:".to_string(), "String - description text".to_string()];

        let info = parse_return_block_simple(&comments);

        assert!(info.has_return_keyword);
        assert!(!info.is_hyperlink);
        assert_eq!(info.types.len(), 1);
        assert_eq!(info.types[0].0, "String");
        assert_eq!(info.types[0].1, Some("description text".to_string()));
    }

    #[test]
    fn test_stop_at_next_section() {
        let comments = vec![
            "Возвращаемое значение:".to_string(),
            "Строка - результат".to_string(),
            "Параметры:".to_string(),
            "Параметр1 - Число".to_string(),
        ];

        let info = parse_return_block_simple(&comments);

        assert!(info.has_return_keyword);
        assert_eq!(info.types.len(), 1);
        assert_eq!(info.types[0].0, "Строка");
        // Should stop at "Параметры:", not parse "Параметр1"
    }

    #[test]
    fn test_empty_lines_ignored() {
        let comments = vec![
            "Возвращаемое значение:".to_string(),
            "".to_string(),
            "".to_string(),
            "Строка - результат".to_string(),
        ];

        let info = parse_return_block_simple(&comments);

        assert!(info.has_return_keyword);
        assert_eq!(info.types.len(), 1);
        assert_eq!(info.types[0].0, "Строка");
    }

    #[test]
    fn test_parse_structure_with_nested_fields() {
        let comments = vec![
            "Возвращает структуру с доступными публикациями HTTP-сервисов ERP.".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Структура - Структура с ключами-названиями сервисов и значениями-URL путями к публикациям:".to_string(),
            "    * ПОЗК - Строка - Публикация для работы с производственными заказами.".to_string(),
            "    * ДанныеДО - Строка - Публикация для получения данных документооборота.".to_string(),
            "    * ДанныеДООтветственный - Строка - Публикация для получения данных об ответственных.".to_string(),
            "    * Рецептура - Строка - Публикация для работы с рецептурами.".to_string(),
            "".to_string(),
        ];

        let info = parse_return_block_simple(&comments);

        eprintln!("has_return_keyword: {}", info.has_return_keyword);
        eprintln!("is_hyperlink: {}", info.is_hyperlink);
        eprintln!("types found: {}", info.types.len());
        for (type_name, desc) in &info.types {
            eprintln!("  - {}: {:?}", type_name, desc);
        }

        assert!(info.has_return_keyword, "Should find return keyword");
        assert!(!info.is_hyperlink, "Should not be hyperlink");
        assert!(!info.types.is_empty(), "Should find at least one type");

        // Should find "Структура" type
        assert!(info.types.iter().any(|(t, _)| t == "Структура"), "Should find 'Структура' type");
    }
}
