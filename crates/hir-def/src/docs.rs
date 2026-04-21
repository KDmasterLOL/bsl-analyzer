//! Method documentation structures and parsing.
//!
//! This module provides HIR-level representation of BSL method documentation
//! (comments before procedures and functions). Documentation is parsed once
//! and cached via Salsa queries, then used by:
//! - Diagnostics (e.g., MissingReturnedValueDescription)
//! - LSP features (hover, signature help, completion)
//!
//! ## Architecture
//!
//! ```text
//! Parse → ItemTree → method_docs() query → MethodDocs (cached) → Use everywhere
//! ```
//!
//! Documentation is a first-class HIR concept.

use crate::item_tree::ModItem;
use crate::{DefDatabase, MethodId};
use std::sync::Arc;
use syntax::{extract_leading_comments_at_offset, SyntaxNode};

/// Parsed documentation for a BSL method (procedure or function).
///
/// All fields are optional because documentation sections may be missing.
///
/// ## Example
///
/// ```bsl
/// // Вычисляет сумму двух чисел.
/// //
/// // Параметры:
/// //   А - Число - первое слагаемое
/// //   Б - Число - второе слагаемое
/// //
/// // Возвращаемое значение:
/// //   Число - результат сложения
/// //
/// Функция Сумма(А, Б) Экспорт
///     Возврат А + Б;
/// КонецФункции
/// ```
///
/// Parsed as:
/// ```ignore
/// MethodDocs {
///     raw: "// Вычисляет сумму...",
///     purpose: Some("Вычисляет сумму двух чисел."),
///     parameters: vec![
///         ParameterDoc { name: "А", types: [TypeDoc { name: "Число", description: Some("первое слагаемое"), ... }] },
///         ParameterDoc { name: "Б", types: [TypeDoc { name: "Число", description: Some("второе слагаемое"), ... }] },
///     ],
///     returned_value: vec![
///         TypeDoc { name: "Число", description: Some("результат сложения"), ... }
///     ],
///     ...
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodDocs {
    /// Full raw text of all documentation comments.
    ///
    /// This is the concatenation of all leading comment lines before the method,
    /// with "//" prefixes removed. Used for fallback display.
    pub raw: String,

    /// Purpose/description section (text before any section keywords).
    ///
    /// This is the main description of what the method does.
    /// Example: "Вычисляет сумму двух чисел."
    pub purpose: Option<String>,

    /// Parameters with their types and descriptions.
    ///
    /// Extracted from "Параметры:" / "Parameters:" section.
    pub parameters: Vec<ParameterDoc>,

    /// Return value types and descriptions.
    ///
    /// Extracted from "Возвращаемое значение:" / "Returns:" section.
    pub returned_value: Vec<TypeDoc>,

    /// Examples section.
    ///
    /// Extracted from "Пример:" / "Example:" section.
    /// Each string is one line/paragraph of the example.
    pub examples: Vec<String>,

    /// Call options section.
    ///
    /// Extracted from "Варианты вызова:" / "Call options:" section.
    pub call_options: Vec<String>,

    /// Deprecation info (if "Устарела:" / "Deprecated:" keyword present).
    ///
    /// Contains the text after the deprecation keyword explaining why
    /// the method is deprecated and what to use instead.
    pub deprecation: Option<String>,

    /// Hyperlink reference to another method (if "См." / "See" keyword present).
    ///
    /// Example: "См. ДругойМетод()" → link = Some("ДругойМетод()")
    /// When present, this indicates documentation is delegated to another method.
    pub link: Option<String>,
}

impl MethodDocs {
    /// Create empty documentation.
    pub fn empty() -> Self {
        Self {
            raw: String::new(),
            purpose: None,
            parameters: Vec::new(),
            returned_value: Vec::new(),
            examples: Vec::new(),
            call_options: Vec::new(),
            deprecation: None,
            link: None,
        }
    }

    /// Check if documentation is empty (no meaningful content).
    pub fn is_empty(&self) -> bool {
        self.purpose.is_none()
            && self.parameters.is_empty()
            && self.returned_value.is_empty()
            && self.examples.is_empty()
            && self.call_options.is_empty()
            && self.deprecation.is_none()
            && self.link.is_none()
    }

    /// Check if this is a hyperlink reference (documentation delegated to another method).
    pub fn is_hyperlink(&self) -> bool {
        self.link.is_some()
    }

    /// Check if method is marked as deprecated.
    pub fn is_deprecated(&self) -> bool {
        self.deprecation.is_some()
    }
}

/// Documentation for a single parameter.
///
/// A parameter can have multiple possible types (union types).
///
/// ## Examples
///
/// Single type:
/// ```bsl
/// // Параметры:
/// //   Значение - Число - значение для обработки
/// ```
/// → `ParameterDoc { name: "Значение", types: [TypeDoc { name: "Число", description: Some("значение для обработки"), ... }] }`
///
/// Multiple types:
/// ```bsl
/// // Параметры:
/// //   Значение - Число, Строка - значение для обработки
/// ```
/// → `ParameterDoc { name: "Значение", types: [TypeDoc { name: "Число", ... }, TypeDoc { name: "Строка", ... }] }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterDoc {
    /// Parameter name (as it appears in the signature).
    pub name: String,

    /// Possible types for this parameter.
    ///
    /// Usually one type, but can be multiple for union types.
    pub types: Vec<TypeDoc>,
}

impl ParameterDoc {
    /// Create a new parameter doc.
    pub fn new(name: String, types: Vec<TypeDoc>) -> Self {
        Self { name, types }
    }
}

/// Documentation for a type.
///
/// Types can be simple (e.g., "Строка") or structured with sub-parameters
/// (e.g., "Структура:" followed by "* Field - Type - description").
///
/// ## Examples
///
/// Simple type:
/// ```bsl
/// // Возвращаемое значение:
/// //   Строка - имя пользователя
/// ```
/// → `TypeDoc { name: "Строка", description: Some("имя пользователя"), parameters: [], ... }`
///
/// Structured type:
/// ```bsl
/// // Возвращаемое значение:
/// //   Структура:
/// //     * Имя - Строка - имя пользователя
/// //     * Возраст - Число - возраст пользователя
/// ```
/// → `TypeDoc { name: "Структура", description: None, parameters: [...], ... }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDoc {
    /// Type name.
    ///
    /// Examples: "Строка", "Число", "Булево", "Структура", "Массив", "Соответствие", etc.
    pub name: String,

    /// Description of this type (optional).
    ///
    /// For simple types: describes what this value represents.
    /// For structured types: describes the structure as a whole.
    pub description: Option<String>,

    /// Sub-parameters for structured types.
    ///
    /// For "Структура:", "Соответствие:", "Массив:" - contains the nested fields/keys.
    /// Each field is documented with "* FieldName - Type - description" syntax.
    pub parameters: Vec<ParameterDoc>,

    /// Is this a hyperlink reference?
    ///
    /// `true` if the type is "См. Method()" / "See Method()".
    /// In this case, `name` contains the full hyperlink text.
    pub is_hyperlink: bool,
}

impl TypeDoc {
    /// Create a simple type doc (no sub-parameters).
    pub fn simple(name: String, description: Option<String>) -> Self {
        Self { name, description, parameters: Vec::new(), is_hyperlink: false }
    }

    /// Create a structured type doc (with sub-parameters).
    pub fn structured(
        name: String,
        description: Option<String>,
        parameters: Vec<ParameterDoc>,
    ) -> Self {
        Self { name, description, parameters, is_hyperlink: false }
    }

    /// Create a hyperlink type doc.
    pub fn hyperlink(link: String) -> Self {
        Self { name: link, description: None, parameters: Vec::new(), is_hyperlink: true }
    }
}

/// Parse method documentation from comments.
///
/// This is a Salsa query that:
/// 1. Extracts leading comments before the method
/// 2. Parses all documentation sections (purpose, parameters, returns, etc.)
/// 3. Returns structured `MethodDocs` or `None` if no documentation exists
///
/// ## Caching
///
/// This query is cached by Salsa. It's only recomputed when:
/// - The file containing the method changes
/// - The parse tree changes (which implies comments changed)
///
/// ## Example
///
/// ```ignore
/// let docs = db.method_docs(method_id)?;
/// println!("Purpose: {}", docs.purpose.unwrap_or_default());
/// for param in &docs.parameters {
///     println!("  Param {}: {:?}", param.name, param.types);
/// }
/// ```
pub fn method_docs_query(db: &dyn DefDatabase, method: MethodId) -> Option<Arc<MethodDocs>> {
    let tree = db.item_tree(method.module.file_id);

    // Get method's source_range directly from ItemTree (no AST traversal needed!)
    let items = tree.top_level_items();
    let item = items.get(method.local_id as usize)?;

    let source_range = match item {
        ModItem::Procedure(idx) => tree.procedure(*idx).source_range,
        ModItem::Function(idx) => tree.function(*idx).source_range,
        ModItem::Variable(_) => return None,
    };

    // Extract leading comments using offset directly (O(1) instead of O(n))
    let file_text = db.file_text_input(method.module.file_id);
    let source_text = file_text.text(db);
    let offset: usize = source_range.start().into();
    let comments = extract_leading_comments_at_offset(offset, &source_text)?;

    // Parse documentation from comments
    let docs = parse_method_docs(&comments)?;

    Some(Arc::new(docs))
}

/// Compute method documentation without database.
///
/// This is a public wrapper around the private `method_docs_query` logic,
/// allowing StreamingProvider to compute docs without Salsa.
///
/// Optimized: Uses source_range from ItemTree directly, no AST traversal needed.
///
/// # Arguments
///
/// * `_parse` - Unused (kept for API compatibility)
/// * `tree` - ItemTree for the file
/// * `method_id` - ID of the method to get docs for
/// * `file_text` - Source text of the file
pub fn compute_method_docs(
    _parse: &syntax::Parse<SyntaxNode>,
    tree: &crate::item_tree::ItemTree,
    method_id: MethodId,
    file_text: &str,
) -> Option<Arc<MethodDocs>> {
    // Get method's source_range directly from ItemTree (no AST traversal needed!)
    let items = tree.top_level_items();
    let item = items.get(method_id.local_id as usize)?;

    let source_range = match item {
        ModItem::Procedure(idx) => tree.procedure(*idx).source_range,
        ModItem::Function(idx) => tree.function(*idx).source_range,
        ModItem::Variable(_) => return None,
    };

    // Extract leading comments using offset directly (O(1) instead of O(n))
    let offset: usize = source_range.start().into();
    let comments = extract_leading_comments_at_offset(offset, file_text)?;

    // Parse documentation
    let docs = parse_method_docs(&comments)?;

    Some(Arc::new(docs))
}

/// Parse method documentation from comment lines.
///
/// This is the core parsing logic that converts raw comment strings into
/// structured `MethodDocs`.
///
/// ## Implementation Strategy
///
/// The parser uses pattern matching to identify sections:
/// - "Параметры:" / "Parameters:" → parameter section
/// - "Возвращаемое значение:" / "Returns:" → return value section
/// - "Пример:" / "Example:" → examples section
/// - "Варианты вызова:" / "Call options:" → call options section
/// - "Устарела:" / "Deprecated:" → deprecation info
/// - "См." / "See " at start → hyperlink reference
fn parse_method_docs(comments: &[String]) -> Option<MethodDocs> {
    if comments.is_empty() {
        return None;
    }

    // Join all comments into raw text
    let raw = comments.join("\n");

    let mut docs = MethodDocs {
        raw,
        purpose: None,
        parameters: Vec::new(),
        returned_value: Vec::new(),
        examples: Vec::new(),
        call_options: Vec::new(),
        deprecation: None,
        link: None,
    };

    // Check for hyperlink reference at the start (overrides everything)
    if let Some(first_non_empty) = comments.iter().find(|c| !c.trim().is_empty()) {
        if is_hyperlink_line(first_non_empty.trim()) {
            docs.link = Some(first_non_empty.trim().to_string());
            return Some(docs);
        }
    }

    // Find section indices
    let mut section_indices = Vec::new();
    for (i, line) in comments.iter().enumerate() {
        let lower = line.trim().to_lowercase();

        if is_parameters_keyword(&lower) {
            section_indices.push((i, Section::Parameters));
        } else if is_returns_keyword(&lower) {
            section_indices.push((i, Section::Returns));
        } else if is_example_keyword(&lower) {
            section_indices.push((i, Section::Examples));
        } else if is_call_options_keyword(&lower) {
            section_indices.push((i, Section::CallOptions));
        } else if is_deprecated_keyword(&lower) {
            section_indices.push((i, Section::Deprecated));
        }
    }

    // Parse purpose (everything before first section)
    let purpose_end = section_indices.first().map(|(i, _)| *i).unwrap_or(comments.len());
    if purpose_end > 0 {
        let purpose_lines: Vec<_> =
            comments[..purpose_end].iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

        if !purpose_lines.is_empty() {
            docs.purpose = Some(purpose_lines.join("\n"));
        }
    }

    // Parse each section
    for (idx, (start, section)) in section_indices.iter().enumerate() {
        let end = section_indices.get(idx + 1).map(|(i, _)| *i).unwrap_or(comments.len());
        let section_lines = &comments[*start + 1..end];

        match section {
            Section::Parameters => {
                docs.parameters = parse_parameters(section_lines);
            }
            Section::Returns => {
                docs.returned_value = parse_returns(section_lines);
            }
            Section::Examples => {
                docs.examples = parse_simple_section(section_lines);
            }
            Section::CallOptions => {
                docs.call_options = parse_simple_section(section_lines);
            }
            Section::Deprecated => {
                docs.deprecation = parse_deprecated_section(&comments[*start], section_lines);
            }
        }
    }

    Some(docs)
}

/// Section type for documentation parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Parameters,
    Returns,
    Examples,
    CallOptions,
    Deprecated,
}

/// Check if line contains "Параметры:" / "Parameters:" keyword.
fn is_parameters_keyword(lower_line: &str) -> bool {
    lower_line.contains("параметры:") || lower_line.contains("parameters:")
}

/// Check if line contains "Возвращаемое значение:" / "Returns:" keyword.
fn is_returns_keyword(lower_line: &str) -> bool {
    lower_line.contains("возвращаемое значение:") || lower_line.contains("returns:")
}

/// Check if line contains "Пример:" / "Example:" keyword.
fn is_example_keyword(lower_line: &str) -> bool {
    lower_line.contains("пример:") || lower_line.contains("example:")
}

/// Check if line contains "Варианты вызова:" / "Call options:" keyword.
fn is_call_options_keyword(lower_line: &str) -> bool {
    lower_line.contains("варианты вызова:") || lower_line.contains("call options:")
}

/// Check if line contains "Устарела" / "Deprecated" keyword.
///
/// Matches various formats:
/// - "Устарела." / "Deprecated."
/// - "Устарела:" / "Deprecated:"
/// - "Устарела" / "Deprecated" (standalone)
fn is_deprecated_keyword(lower_line: &str) -> bool {
    lower_line.contains("устарела") || lower_line.contains("deprecated")
}

/// Check if a line is a hyperlink reference.
///
/// Matches:
/// - "См. MethodName()"
/// - "See MethodName()"
fn is_hyperlink_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.starts_with("см.") || lower.starts_with("see ")
}

/// Parse parameters section.
///
/// Format:
/// ```text
/// Параметры:
///   ParamName - Type - description
///   AnotherParam - Type1, Type2 - description
/// ```
fn parse_parameters(lines: &[String]) -> Vec<ParameterDoc> {
    let mut parameters = Vec::new();
    let mut current_param: Option<(String, Vec<TypeDoc>)> = None;

    for line in lines {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Check if this is a sub-parameter line (starts with *)
        if trimmed.starts_with('*') {
            // Sub-parameter - add to last type's parameters
            if let Some((_, types)) = &mut current_param {
                if let Some(last_type) = types.last_mut() {
                    if let Some(sub_param) = parse_sub_parameter(trimmed) {
                        last_type.parameters.push(sub_param);
                    }
                }
            }
            continue;
        }

        // Continuation line for a union-typed parameter:
        //   ParamName - TypeA - description A
        //             - TypeB - description B
        // After trim the second line is "- TypeB - description B" and must extend
        // the current parameter instead of starting a new one.
        // Only accept it as a type continuation when the would-be type name actually
        // looks like a type — otherwise a description bullet like "- примечание"
        // would be silently absorbed as a phantom union member.
        if current_param.is_some() && trimmed.starts_with('-') {
            if let Some((type_name, description)) = parse_type_line(trimmed) {
                if is_likely_type_name(&type_name) {
                    if let Some((_, types)) = &mut current_param {
                        types.push(TypeDoc::simple(type_name, description));
                    }
                    continue;
                }
            }
        }

        // Try to parse as parameter line: "ParamName - Type - description"
        if let Some((param_name, types)) = parse_parameter_line(trimmed) {
            // Save previous parameter if exists
            if let Some((name, types)) = current_param.take() {
                parameters.push(ParameterDoc { name, types });
            }
            current_param = Some((param_name, types));
        }
    }

    // Save last parameter
    if let Some((name, types)) = current_param {
        parameters.push(ParameterDoc { name, types });
    }

    parameters
}

/// Parse a parameter line: "ParamName - Type - description" or "ParamName - Type".
fn parse_parameter_line(line: &str) -> Option<(String, Vec<TypeDoc>)> {
    let parts: Vec<&str> = line.splitn(3, " - ").collect();

    if parts.len() < 2 {
        return None;
    }

    let param_name = parts[0].trim().to_string();
    let type_part = parts[1].trim();
    let description = parts.get(2).map(|s| s.trim().to_string());

    // Parse types (might be comma-separated)
    let types = if type_part.contains(',') {
        type_part
            .split(',')
            .map(|t| TypeDoc::simple(t.trim().to_string(), description.clone()))
            .collect()
    } else {
        vec![TypeDoc::simple(type_part.to_string(), description)]
    };

    Some((param_name, types))
}

/// Parse a sub-parameter line: "* FieldName - Type - description".
fn parse_sub_parameter(line: &str) -> Option<ParameterDoc> {
    let without_star = line.strip_prefix('*')?.trim();
    let (name, types) = parse_parameter_line(without_star)?;
    Some(ParameterDoc { name, types })
}

/// Parse returns section (return value types).
///
/// Format:
/// ```text
/// Возвращаемое значение:
///   Type - description
///   Структура:
///     * Field1 - Type1 - description
///     * Field2 - Type2 - description
/// ```
fn parse_returns(lines: &[String]) -> Vec<TypeDoc> {
    let mut types = Vec::new();
    let mut current_type: Option<TypeDoc> = None;

    // Check for hyperlink first
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_hyperlink_line(trimmed) {
            return vec![TypeDoc::hyperlink(trimmed.to_string())];
        }
        break;
    }

    for line in lines {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Check if this is a sub-parameter line (starts with *)
        if trimmed.starts_with('*') {
            // Sub-parameter - add to current type
            if let Some(ref mut type_doc) = current_type {
                if let Some(sub_param) = parse_sub_parameter(trimmed) {
                    type_doc.parameters.push(sub_param);
                }
            }
            continue;
        }

        // Try to parse as type line
        if let Some((type_name, description)) = parse_type_line(trimmed) {
            // Save previous type if exists
            if let Some(type_doc) = current_type.take() {
                types.push(type_doc);
            }
            current_type = Some(TypeDoc::simple(type_name, description));
        }
    }

    // Save last type
    if let Some(type_doc) = current_type {
        types.push(type_doc);
    }

    types
}

/// Parse a type line: "Type - description" or "Type" or "Type:".
///
/// Returns: (type_name, description)
fn parse_type_line(line: &str) -> Option<(String, Option<String>)> {
    let trimmed = line.trim();

    // Skip structured type fields (e.g., "* FieldName - Type - description")
    if trimmed.starts_with('*') {
        return None;
    }

    // Pattern 0: "Type:" (type name with colon, no description)
    // Example: "Структура:", "Массив:"
    if trimmed.ends_with(':') && !trimmed.contains(" - ") {
        let type_name = trimmed.trim_end_matches(':').trim();
        if !type_name.is_empty() && is_likely_type_name(type_name) {
            return Some((type_name.to_string(), None));
        }
        return None;
    }

    // Pattern 1: "- Type - description" (leading dash with description)
    if let Some(without_dash) = trimmed.strip_prefix('-') {
        let without_dash = without_dash.trim();
        if trimmed.matches('-').count() >= 2 {
            if let Some(sep_pos) = without_dash.find(" - ") {
                let type_name = without_dash[..sep_pos].trim().to_string();
                let description = without_dash[sep_pos + 3..].trim().to_string();
                return Some((type_name, Some(description)));
            }
        }
    }

    // Pattern 2: "Type - description" (no leading dash)
    if let Some(sep_pos) = trimmed.find(" - ") {
        let type_name = trimmed[..sep_pos].trim();
        if type_name.is_empty() || type_name.starts_with('*') {
            return None;
        }
        let type_name = type_name.to_string();
        let description = trimmed[sep_pos + 3..].trim().to_string();
        return Some((type_name, Some(description)));
    }

    // Pattern 3: "- Type" (leading dash without description)
    if let Some(without_dash) = trimmed.strip_prefix('-') {
        let type_name = without_dash.trim().to_string();
        if !type_name.is_empty() {
            return Some((type_name, None));
        }
    }

    // Pattern 4: "Type" (just type name)
    if !trimmed.is_empty() && is_likely_type_name(trimmed) {
        return Some((trimmed.to_string(), None));
    }

    None
}

/// Check if a string is likely a type name.
///
/// A BSL type name is a single identifier, optionally dotted (e.g.
/// `Справочники.Партнеры`). Multi-word strings, prose with punctuation,
/// or anything containing spaces is NOT a type — this matters because
/// `parse_type_line`'s last-resort branch otherwise treats a description
/// continuation line like `Если передано имя ...` as a phantom type.
fn is_likely_type_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    if !s.chars().all(is_type_name_char) {
        return false;
    }

    let first_char = s.chars().next().unwrap();
    if first_char.is_uppercase() {
        return true;
    }

    // Lower-case fallback for known BSL primitives written in lowercase.
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

/// Characters that may appear inside a BSL type name token.
///
/// Identifiers + `.` for qualified names like `Справочники.Партнеры`.
fn is_type_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

/// Parse a simple section (examples, call options, etc.) as list of strings.
fn parse_simple_section(lines: &[String]) -> Vec<String> {
    lines.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string()).collect()
}

/// Parse deprecated section.
///
/// Extracts deprecation info from the keyword line and following lines.
/// Supports formats:
/// - "Устарела." / "Deprecated." (just marker, empty info)
/// - "Устарела. Используйте X" / "Deprecated. Use X" (info on same line)
/// - "Устарела:\n Используйте X" (info on next line)
fn parse_deprecated_section(keyword_line: &str, following_lines: &[String]) -> Option<String> {
    let lower = keyword_line.to_lowercase();

    // Find position after the keyword
    let after_keyword = if let Some(pos) = lower.find("устарела") {
        &keyword_line[pos + "устарела".len()..]
    } else if let Some(pos) = lower.find("deprecated") {
        &keyword_line[pos + "deprecated".len()..]
    } else {
        ""
    };

    // Remove leading punctuation and whitespace
    let info_on_same_line = after_keyword
        .trim_start_matches(|c: char| c == '.' || c == ':' || c.is_whitespace())
        .trim();

    if !info_on_same_line.is_empty() {
        return Some(info_on_same_line.to_string());
    }

    // Check following lines
    let following_info = parse_simple_section(following_lines);
    if !following_info.is_empty() {
        return Some(following_info.join("\n"));
    }

    // Just the marker, no additional info - still deprecated
    Some(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_docs_empty() {
        let docs = MethodDocs::empty();
        assert!(docs.is_empty());
        assert!(!docs.is_hyperlink());
        assert!(!docs.is_deprecated());
    }

    #[test]
    fn test_type_doc_simple() {
        let type_doc = TypeDoc::simple("Строка".to_string(), Some("описание".to_string()));
        assert_eq!(type_doc.name, "Строка");
        assert_eq!(type_doc.description, Some("описание".to_string()));
        assert!(type_doc.parameters.is_empty());
        assert!(!type_doc.is_hyperlink);
    }

    #[test]
    fn test_type_doc_structured() {
        let params = vec![ParameterDoc::new(
            "Поле".to_string(),
            vec![TypeDoc::simple("Число".to_string(), Some("значение".to_string()))],
        )];
        let type_doc = TypeDoc::structured("Структура".to_string(), None, params);
        assert_eq!(type_doc.name, "Структура");
        assert_eq!(type_doc.parameters.len(), 1);
        assert!(!type_doc.is_hyperlink);
    }

    #[test]
    fn test_type_doc_hyperlink() {
        let type_doc = TypeDoc::hyperlink("См. ДругойМетод()".to_string());
        assert!(type_doc.is_hyperlink);
        assert_eq!(type_doc.name, "См. ДругойМетод()");
    }

    #[test]
    fn test_parse_empty_comments() {
        let docs = parse_method_docs(&[]);
        assert!(docs.is_none());
    }

    #[test]
    fn test_parse_minimal_comments() {
        let comments = vec!["Описание метода".to_string()];
        let docs = parse_method_docs(&comments);
        assert!(docs.is_some());
        let docs = docs.unwrap();
        assert_eq!(docs.raw, "Описание метода");
        assert_eq!(docs.purpose, Some("Описание метода".to_string()));
    }

    #[test]
    fn test_parse_complete_documentation() {
        let comments = vec![
            "Вычисляет сумму двух чисел.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  А - Число - первое слагаемое".to_string(),
            "  Б - Число - второе слагаемое".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Число - результат сложения".to_string(),
            "".to_string(),
            "Пример:".to_string(),
            "  Результат = Сумма(2, 3); // Результат = 5".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        // Purpose
        assert_eq!(docs.purpose, Some("Вычисляет сумму двух чисел.".to_string()));

        // Parameters
        assert_eq!(docs.parameters.len(), 2);
        assert_eq!(docs.parameters[0].name, "А");
        assert_eq!(docs.parameters[0].types[0].name, "Число");
        assert_eq!(docs.parameters[0].types[0].description, Some("первое слагаемое".to_string()));
        assert_eq!(docs.parameters[1].name, "Б");
        assert_eq!(docs.parameters[1].types[0].name, "Число");

        // Return value
        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Число");
        assert_eq!(docs.returned_value[0].description, Some("результат сложения".to_string()));

        // Examples
        assert_eq!(docs.examples.len(), 1);
        assert!(docs.examples[0].contains("Результат = Сумма(2, 3)"));
    }

    #[test]
    fn test_parse_structured_return_value() {
        let comments = vec![
            "Возвращает информацию о пользователе.".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Структура:".to_string(),
            "    * Имя - Строка - имя пользователя".to_string(),
            "    * Возраст - Число - возраст пользователя".to_string(),
            "    * Email - Строка - адрес электронной почты".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Структура");

        // Check sub-parameters
        let sub_params = &docs.returned_value[0].parameters;
        assert_eq!(sub_params.len(), 3);
        assert_eq!(sub_params[0].name, "Имя");
        assert_eq!(sub_params[0].types[0].name, "Строка");
        assert_eq!(sub_params[1].name, "Возраст");
        assert_eq!(sub_params[2].name, "Email");
    }

    #[test]
    fn test_parse_returns_does_not_swallow_description_continuation() {
        // The description after the type can span multiple lines. Continuation lines
        // must NOT be promoted to phantom union members of the return type.
        // Real-world example: ОбщегоНазначения.ЗначениеРеквизитаОбъекта.
        let comments = vec![
            "Возвращает значения реквизита.".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Произвольный - если передана пустая ссылка, возвращается Неопределено.".to_string(),
            "                 Если передана ссылка несуществующего объекта (битая ссылка),"
                .to_string(),
            "                 то возвращается Неопределено.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(
            docs.returned_value.len(),
            1,
            "Description continuation must not be parsed as additional return types, got: {:?}",
            docs.returned_value
        );
        assert_eq!(docs.returned_value[0].name, "Произвольный");
    }

    #[test]
    fn test_parse_hyperlink() {
        let comments = vec!["См. ДругойМетод()".to_string()];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.is_hyperlink());
        assert_eq!(docs.link, Some("См. ДругойМетод()".to_string()));
    }

    #[test]
    fn test_parse_deprecated() {
        let comments = vec![
            "Старый метод.".to_string(),
            "".to_string(),
            "Устарела:".to_string(),
            "Используйте НовыйМетод() вместо этого метода.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.is_deprecated());
        assert_eq!(
            docs.deprecation,
            Some("Используйте НовыйМетод() вместо этого метода.".to_string())
        );
    }

    #[test]
    fn test_parse_deprecated_with_dot() {
        // Format: "Устарела." with dot
        let comments = vec!["Устарела.".to_string()];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.is_deprecated());
        assert_eq!(docs.deprecation, Some("".to_string()));
    }

    #[test]
    fn test_parse_deprecated_with_dot_and_info() {
        // Format: "Устарела. Используйте X" - info on same line
        let comments = vec!["Устарела. Используйте НовыйМетод().".to_string()];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.is_deprecated());
        assert_eq!(docs.deprecation, Some("Используйте НовыйМетод().".to_string()));
    }

    #[test]
    fn test_parse_deprecated_english() {
        let comments = vec!["Deprecated.".to_string()];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.is_deprecated());
    }

    #[test]
    fn test_parse_parameters_with_multiple_types() {
        let comments = vec![
            "Обрабатывает значение.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  Значение - Число, Строка - значение для обработки".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 1);
        assert_eq!(docs.parameters[0].name, "Значение");
        assert_eq!(docs.parameters[0].types.len(), 2);
        assert_eq!(docs.parameters[0].types[0].name, "Число");
        assert_eq!(docs.parameters[0].types[1].name, "Строка");
    }

    #[test]
    fn test_parse_call_options() {
        let comments = vec![
            "Выполняет операцию.".to_string(),
            "".to_string(),
            "Варианты вызова:".to_string(),
            "  Вариант 1: Выполнить(Параметр1)".to_string(),
            "  Вариант 2: Выполнить(Параметр1, Параметр2)".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.call_options.len(), 2);
        assert!(docs.call_options[0].contains("Вариант 1"));
        assert!(docs.call_options[1].contains("Вариант 2"));
    }

    #[test]
    fn test_parse_english_documentation() {
        let comments = vec![
            "Calculates the sum.".to_string(),
            "".to_string(),
            "Parameters:".to_string(),
            "  A - Number - first addend".to_string(),
            "  B - Number - second addend".to_string(),
            "".to_string(),
            "Returns:".to_string(),
            "  Number - sum result".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.purpose, Some("Calculates the sum.".to_string()));
        assert_eq!(docs.parameters.len(), 2);
        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Number");
    }

    #[test]
    fn test_parse_multiline_purpose() {
        let comments = vec![
            "Первая строка описания.".to_string(),
            "Вторая строка описания.".to_string(),
            "Третья строка описания.".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Булево".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.purpose.is_some());
        let purpose = docs.purpose.unwrap();
        assert!(purpose.contains("Первая строка"));
        assert!(purpose.contains("Вторая строка"));
        assert!(purpose.contains("Третья строка"));
    }

    #[test]
    fn test_parse_parameters_with_multiline_union_types() {
        // Format used in 1C standard libraries (e.g. ОбщегоНазначения.ЗначениеРеквизитаОбъекта):
        // a parameter has several alternative types listed on consecutive lines
        // aligned with the first dash.
        let comments = vec![
            "Возвращает значения реквизита.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  Ссылка       - ЛюбаяСсылка - объект, значения реквизитов которого получить."
                .to_string(),
            "               - Строка      - полное имя предопределенного элемента.".to_string(),
            "  ИмяРеквизита - Строка      - имя получаемого реквизита.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 2, "Expected exactly 2 parameters, no phantom '-'");

        assert_eq!(docs.parameters[0].name, "Ссылка");
        assert_eq!(docs.parameters[0].types.len(), 2);
        assert_eq!(docs.parameters[0].types[0].name, "ЛюбаяСсылка");
        assert_eq!(
            docs.parameters[0].types[0].description.as_deref(),
            Some("объект, значения реквизитов которого получить.")
        );
        assert_eq!(docs.parameters[0].types[1].name, "Строка");
        assert_eq!(
            docs.parameters[0].types[1].description.as_deref(),
            Some("полное имя предопределенного элемента.")
        );

        assert_eq!(docs.parameters[1].name, "ИмяРеквизита");
        assert_eq!(docs.parameters[1].types.len(), 1);
        assert_eq!(docs.parameters[1].types[0].name, "Строка");
    }

    #[test]
    fn test_parse_parameters_continuation_does_not_swallow_bullet_descriptions() {
        // A bullet in a description that happens to start with "-" must not be
        // interpreted as an extra union type for the previous parameter.
        let comments = vec![
            "Описание.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  Параметр - Число - значение, особенности:".to_string(),
            "             - дополнительное примечание ниже описания".to_string(),
            "  Другой - Строка - имя".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 2);
        assert_eq!(docs.parameters[0].name, "Параметр");
        assert_eq!(
            docs.parameters[0].types.len(),
            1,
            "Bullet line must NOT be absorbed as an extra type, got: {:?}",
            docs.parameters[0].types
        );
        assert_eq!(docs.parameters[0].types[0].name, "Число");
        assert_eq!(docs.parameters[1].name, "Другой");
    }

    #[test]
    fn test_parse_parameter_with_structured_type() {
        let comments = vec![
            "Обрабатывает настройки.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  Настройки - Структура - настройки подключения".to_string(),
            "    * Сервер - Строка - адрес сервера".to_string(),
            "    * Порт - Число - номер порта".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 1);
        assert_eq!(docs.parameters[0].name, "Настройки");
        assert_eq!(docs.parameters[0].types[0].name, "Структура");

        // Check nested fields
        let nested = &docs.parameters[0].types[0].parameters;
        assert_eq!(nested.len(), 2);
        assert_eq!(nested[0].name, "Сервер");
        assert_eq!(nested[1].name, "Порт");
    }
}
