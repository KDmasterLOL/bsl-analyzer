//! HTML parser using scraper library for more robust parsing.
//!
//! This module provides improved parsing functions using CSS selectors
//! instead of manual string searching.

use scraper::{Html, Selector, ElementRef};
use crate::{ContextAvailability, MethodParameter};

/// CSS selector for chapter headers (e.g., "Доступность:", "Возвращаемое значение:")
const CHAPTER_SELECTOR: &str = "p.V8SH_chapter";

/// CSS selector for parameter blocks
const RUBRIC_SELECTOR: &str = "div.V8SH_rubric";

/// Extracts version from HTML content using scraper
/// Looks for: "Доступен, начиная с версии 8.0." or "8.3.6."
pub fn extract_version(html_content: &str) -> Option<String> {
    let html = Html::parse_fragment(html_content);

    // Search in all text nodes for version pattern
    for text_node in html.root_element().descendants() {
        if let Some(text) = text_node.value().as_text() {
            if let Some(pos) = text.text.find("Доступен, начиная с версии ") {
                let after = &text.text[pos + "Доступен, начиная с версии ".len()..];
                // Find the period at the end of the sentence, not in version number
                // Version can be "8.0", "8.3.6", etc.
                // Look for pattern: numbers and dots until period + space or end
                let version_end = after.find(". ")
                    .or_else(|| {
                        // If no ". " found, look for period at end
                        if after.ends_with('.') {
                            Some(after.len() - 1)
                        } else {
                            after.find('.')
                        }
                    })
                    .unwrap_or(after.len());

                let version = after[..version_end].trim();
                if !version.is_empty() {
                    return Some(version.to_string());
                }
            }
        }
    }
    None
}

/// Extracts context availability from HTML content using scraper
/// Looks for: "<p class="V8SH_chapter">Доступность: </p><p>Тонкий клиент, сервер...</p>"
pub fn extract_context(html_content: &str) -> Option<ContextAvailability> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse(CHAPTER_SELECTOR).unwrap();

    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if text.contains("Доступность") {
            // Get the next sibling element after chapter
            let mut node = chapter.next_sibling();
            while let Some(n) = node {
                if let Some(elem) = n.value().as_element() {
                    if elem.name() == "p" {
                        let context_text = ElementRef::wrap(n)
                            .unwrap()
                            .text()
                            .collect::<String>()
                            .to_lowercase();

                        return Some(parse_context_flags(&context_text));
                    }
                }
                node = n.next_sibling();
            }
        }
    }
    None
}

/// Parses context flags from text
fn parse_context_flags(text: &str) -> ContextAvailability {
    ContextAvailability {
        thick_client: text.contains("толстый клиент"),
        thin_client: text.contains("тонкий клиент"),
        web_client: text.contains("веб-клиент"),
        server: text.contains("сервер"),
        mobile_client: text.contains("мобильный клиент"),
        external_connection: text.contains("внешнее соединение")
            || text.contains("интеграция"),
    }
}

/// Extracts return type from HTML content using scraper
/// Handles multiple patterns:
/// 1. "Тип: XXX" - explicit type specification
/// 2. Description without "Тип:" - extracts type from description or links
pub fn extract_return_type(html_content: &str) -> Option<String> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse(CHAPTER_SELECTOR).unwrap();

    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if text.contains("Возвращаемое значение") {
            // Collect content after chapter until next chapter
            let mut content = String::new();
            let mut html_content_section = String::new();
            let mut node = chapter.next_sibling();

            while let Some(n) = node {
                // Stop if we hit another chapter
                if let Some(elem) = n.value().as_element() {
                    if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                        break;
                    }
                }

                // Collect text from all nodes
                if let Some(text_node) = n.value().as_text() {
                    content.push_str(&text_node.text);
                    html_content_section.push_str(&text_node.text);
                } else if let Some(elem_ref) = ElementRef::wrap(n) {
                    // Collect both text and HTML (to extract type links)
                    content.push_str(&elem_ref.text().collect::<String>());
                    html_content_section.push_str(&elem_ref.inner_html());
                }

                node = n.next_sibling();
            }

            // Pattern 1: Extract "Тип: XXX"
            if let Some(type_pos) = content.find("Тип: ") {
                let after = &content[type_pos + "Тип: ".len()..];
                // Find end: period, <br>, or newline
                let end = after
                    .find('.')
                    .or_else(|| after.find('\n'))
                    .unwrap_or(after.len());
                let type_str = after[..end].trim();
                if !type_str.is_empty() {
                    return Some(type_str.to_string());
                }
            }

            // Pattern 2: No "Тип:" prefix - try to extract from description
            // Look for type links in the HTML content
            if let Some(type_from_link) = extract_type_from_html_section(&html_content_section) {
                return Some(type_from_link);
            }

            // Pattern 3: If description mentions "Произвольный" or similar keywords
            if content.to_lowercase().contains("произвольн") {
                return Some("Произвольный".to_string());
            }
        }
    }
    None
}

/// Extracts type from HTML section by finding type links
fn extract_type_from_html_section(html: &str) -> Option<String> {
    // Look for links to type definitions
    // Pattern: <a href="v8help://...">ТипЗначения</a>
    let html_fragment = Html::parse_fragment(html);
    let link_sel = Selector::parse("a[href]").unwrap();

    for link in html_fragment.select(&link_sel) {
        let href = link.value().attr("href")?;

        // Skip external links and links to specific sections
        if href.contains("1centerprise.com") || href.contains("#") {
            continue;
        }

        // Get link text - this is often the type name
        let link_text = link.text().collect::<String>();
        let trimmed = link_text.trim();

        // Skip common non-type keywords
        if trimmed.is_empty()
            || trimmed == "Неопределено" // This is a value, not a type
            || trimmed == "Undefined"
            || trimmed.contains("если") // Skip conditional text
            || trimmed.contains("if") {
            continue;
        }

        // If link is to a type definition, return it
        if href.contains("SyntaxHelperContext/objects")
            || href.contains("SyntaxHelperLanguage/def_") {
            return Some(trimmed.to_string());
        }
    }

    None
}

/// Extracts method parameters from HTML content using scraper
/// Looks for: "Параметры:</p><div class="V8SH_rubric">..."
pub fn extract_parameters(html_content: &str) -> Vec<MethodParameter> {
    let html = Html::parse_fragment(html_content);
    let rubric_sel = Selector::parse(RUBRIC_SELECTOR).unwrap();

    let mut parameters = Vec::new();

    for rubric in html.select(&rubric_sel) {
        let inner = rubric.inner_html();

        // Extract parameter name: &lt;ИмяПараметра&gt;
        if let Some(param_name) = extract_param_name(&inner) {
            let is_optional = inner.contains("(необязательный)");

            // Extract type from rubric or following content
            let param_type = extract_param_type(&rubric);

            parameters.push(MethodParameter {
                name: param_name,
                param_type,
                is_optional,
            });
        }
    }

    parameters
}

/// Extracts parameter name from HTML content
fn extract_param_name(html: &str) -> Option<String> {
    if let Some(start) = html.find("&lt;") {
        let after_start = &html[start + "&lt;".len()..];
        if let Some(end) = after_start.find("&gt;") {
            let name = after_start[..end].trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Extracts parameter type from element and following siblings
fn extract_param_type(element: &ElementRef) -> Option<String> {
    // First, try to find type inside the element itself
    let inner = element.inner_html();
    if let Some(type_str) = extract_type_from_text(&inner) {
        return Some(type_str);
    }

    // If not found, collect all following content until next rubric or too far
    // This handles cases where type is in mixed text/element nodes after </div>
    let mut collected_text = String::new();
    let mut node = element.next_sibling();
    let mut search_depth = 0;

    while let Some(n) = node {
        // Limit search depth to avoid going too far
        if search_depth > 10 {
            break;
        }
        search_depth += 1;

        // Stop if we hit another rubric
        if let Some(elem) = n.value().as_element() {
            if elem.attr("class") == Some("V8SH_rubric") {
                break;
            }
        }

        // Collect text from text nodes
        if let Some(text) = n.value().as_text() {
            collected_text.push_str(&text.text);
        }

        // Collect text from element nodes (including their children)
        if let Some(elem_ref) = ElementRef::wrap(n) {
            collected_text.push_str(&elem_ref.text().collect::<String>());
            collected_text.push(' ');
        }

        node = n.next_sibling();

        // Check if we have found type in collected text
        if let Some(type_str) = extract_type_from_text(&collected_text) {
            return Some(type_str);
        }
    }

    None
}

/// Extracts type from text containing "Тип: XXX"
fn extract_type_from_text(text: &str) -> Option<String> {
    if let Some(pos) = text.find("Тип: ") {
        let after = &text[pos + "Тип: ".len()..];
        // Find end: period, newline, or <br>
        let end = after
            .find('.')
            .or_else(|| after.find('\n'))
            .or_else(|| after.find("<br"))
            .unwrap_or_else(|| {
                // Limit to 200 chars
                after.char_indices()
                    .nth(200)
                    .map(|(idx, _)| idx)
                    .unwrap_or(after.len())
            });

        let cleaned = strip_html_tags(&after[..end]);
        let type_str = cleaned
            .trim()
            .trim_end_matches('.')
            .trim();

        if !type_str.is_empty() {
            return Some(type_str.to_string());
        }
    }
    None
}

/// Strips HTML tags from text (same as old implementation for compatibility)
fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut inside_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => result.push(ch),
            _ => {}
        }
    }

    result
}

/// Extracts syntax from HTML content using scraper
/// Looks for: "<p class="V8SH_chapter">Синтаксис:</p><p>...</p>"
pub fn extract_syntax(html_content: &str) -> Option<String> {
    extract_chapter_content(html_content, "Синтаксис")
}

/// Extracts description from HTML content using scraper
/// Looks for: "<p class="V8SH_chapter">Описание:</p><p>...</p>"
pub fn extract_description(html_content: &str) -> Option<String> {
    extract_chapter_content(html_content, "Описание")
}

/// Extracts notes from HTML content using scraper
/// Looks for: "<p class="V8SH_chapter">Примечание:</p><p>...</p>" or "Примечания:"
pub fn extract_notes(html_content: &str) -> Option<String> {
    extract_chapter_content(html_content, "Примечание")
        .or_else(|| extract_chapter_content(html_content, "Примечания"))
}

/// Generic function to extract content from a chapter section
fn extract_chapter_content(html_content: &str, chapter_title: &str) -> Option<String> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse(CHAPTER_SELECTOR).unwrap();

    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if text.contains(chapter_title) {
            // Collect content after chapter until next chapter
            let mut content = String::new();
            let mut node = chapter.next_sibling();

            while let Some(n) = node {
                // Stop if we hit another chapter
                if let Some(elem) = n.value().as_element() {
                    if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                        break;
                    }
                }

                // Collect text from text nodes
                if let Some(text_node) = n.value().as_text() {
                    content.push_str(&text_node.text);
                } else if let Some(elem_ref) = ElementRef::wrap(n) {
                    // Collect text from elements
                    let elem_text = elem_ref.text().collect::<String>();
                    if !elem_text.trim().is_empty() {
                        if !content.is_empty() && !content.ends_with('\n') {
                            content.push('\n');
                        }
                        content.push_str(&elem_text);
                    }
                }

                node = n.next_sibling();
            }

            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Parameter description with full text
#[derive(Debug, Clone)]
pub struct ParamDescription {
    pub name: String,
    pub description: String,
}

/// Extracts detailed parameter descriptions from HTML content
/// Looks for: "<p class="V8SH_chapter">Параметры:</p><div class="V8SH_rubric">..."
pub fn extract_parameter_descriptions(html_content: &str) -> Vec<ParamDescription> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse(CHAPTER_SELECTOR).unwrap();

    let mut param_descriptions = Vec::new();

    // Find "Параметры:" chapter
    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if text.contains("Параметры") {
            // Collect all rubrics after this chapter
            let mut node = chapter.next_sibling();

            while let Some(n) = node {
                // Stop if we hit another chapter
                if let Some(elem) = n.value().as_element() {
                    if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                        break;
                    }
                }

                // Check if this is a rubric element
                if let Some(elem_ref) = ElementRef::wrap(n) {
                    if elem_ref.value().attr("class") == Some("V8SH_rubric") {
                        // Extract parameter name from rubric
                        let inner = elem_ref.inner_html();
                        if let Some(param_name) = extract_param_name(&inner) {
                            // Collect all text after this rubric until next rubric or chapter
                            let description = collect_param_description(elem_ref);

                            param_descriptions.push(ParamDescription {
                                name: param_name,
                                description,
                            });
                        }
                    }
                }

                node = n.next_sibling();
            }
            break;
        }
    }

    param_descriptions
}

/// Collects parameter description text after a rubric element
fn collect_param_description(rubric: ElementRef) -> String {
    let mut description = String::new();
    let mut node = rubric.next_sibling();
    let mut depth = 0;

    while let Some(n) = node {
        // Limit depth to avoid going too far
        if depth > 20 {
            break;
        }
        depth += 1;

        // Stop if we hit another rubric or chapter
        if let Some(elem) = n.value().as_element() {
            if elem.attr("class") == Some("V8SH_rubric") {
                break;
            }
            if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                break;
            }
        }

        // Collect text from text nodes
        if let Some(text_node) = n.value().as_text() {
            description.push_str(&text_node.text);
        } else if let Some(elem_ref) = ElementRef::wrap(n) {
            // Collect text from elements
            let elem_text = elem_ref.text().collect::<String>();
            if !elem_text.trim().is_empty() {
                description.push_str(&elem_text);
                description.push(' ');
            }
        }

        node = n.next_sibling();
    }

    description.trim().to_string()
}

/// Code example with optional description
#[derive(Debug, Clone)]
pub struct CodeExample {
    pub code: String,
    pub description: Option<String>,
}

/// Extracts code examples from HTML content
/// Looks for: "<p class="V8SH_chapter">Пример:</p>" or "Примеры:"
pub fn extract_examples(html_content: &str) -> Vec<CodeExample> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse(CHAPTER_SELECTOR).unwrap();
    let pre_sel = Selector::parse("pre").unwrap();

    let mut examples = Vec::new();

    // Find "Пример:" or "Примеры:" chapter
    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if text.contains("Пример") {
            // Collect content after chapter until next chapter
            let mut node = chapter.next_sibling();
            let mut current_description = String::new();

            while let Some(n) = node {
                // Stop if we hit another chapter
                if let Some(elem) = n.value().as_element() {
                    if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                        break;
                    }
                }

                // Check for code tables or pre tags
                if let Some(elem_ref) = ElementRef::wrap(n) {
                    // Check if this is a TABLE with code
                    if elem_ref.value().name() == "table" ||
                       elem_ref.value().attr("bgcolor") == Some("#f7f7f7") {
                        // First try to extract from <pre> tags inside table
                        let mut found_code = false;
                        for pre in elem_ref.select(&pre_sel) {
                            let code = pre.text().collect::<String>();
                            if !code.trim().is_empty() {
                                let desc = if current_description.trim().is_empty() {
                                    None
                                } else {
                                    Some(current_description.trim().to_string())
                                };

                                examples.push(CodeExample {
                                    code: code.trim().to_string(),
                                    description: desc,
                                });

                                current_description.clear();
                                found_code = true;
                            }
                        }

                        // If no <pre> found, extract all text from table (1C uses <font> tags)
                        if !found_code {
                            let mut code_text = String::new();

                            // Extract text content, preserving line breaks
                            for node in elem_ref.descendants() {
                                if let Some(text) = node.value().as_text() {
                                    code_text.push_str(&text.text);
                                } else if let Some(elem) = node.value().as_element() {
                                    // BR tags become newlines
                                    if elem.name() == "br" {
                                        code_text.push('\n');
                                    }
                                }
                            }

                            let cleaned_code = code_text.trim();
                            if !cleaned_code.is_empty() {
                                let desc = if current_description.trim().is_empty() {
                                    None
                                } else {
                                    Some(current_description.trim().to_string())
                                };

                                examples.push(CodeExample {
                                    code: cleaned_code.to_string(),
                                    description: desc,
                                });

                                current_description.clear();
                            }
                        }
                    }
                    // Check if this is a pre tag directly
                    else if elem_ref.value().name() == "pre" {
                        let code = elem_ref.text().collect::<String>();
                        if !code.trim().is_empty() {
                            let desc = if current_description.trim().is_empty() {
                                None
                            } else {
                                Some(current_description.trim().to_string())
                            };

                            examples.push(CodeExample {
                                code: code.trim().to_string(),
                                description: desc,
                            });

                            current_description.clear();
                        }
                    }
                    // Otherwise, collect text as potential description
                    else if elem_ref.value().name() != "table" {
                        let elem_text = elem_ref.text().collect::<String>();
                        if !elem_text.trim().is_empty() {
                            if !current_description.is_empty() {
                                current_description.push(' ');
                            }
                            current_description.push_str(&elem_text);
                        }
                    }
                } else if let Some(text_node) = n.value().as_text() {
                    // Collect text nodes as potential description
                    if !text_node.text.trim().is_empty() {
                        if !current_description.is_empty() {
                            current_description.push(' ');
                        }
                        current_description.push_str(&text_node.text);
                    }
                }

                node = n.next_sibling();
            }
            break;
        }
    }

    examples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_context_with_integration() {
        let html = r#"
            <p class="V8SH_chapter">Доступность: </p>
            <p>Тонкий клиент, сервер, интеграция.</p>
        "#;

        let context = extract_context(html).unwrap();
        assert!(context.thin_client);
        assert!(context.server);
        assert!(context.external_connection); // интеграция
        assert!(!context.web_client);
    }

    #[test]
    fn test_extract_context_all_platforms() {
        let html = r#"
            <p class="V8SH_chapter">Доступность: </p>
            <p>Толстый клиент, тонкий клиент, веб-клиент, сервер, мобильный клиент, внешнее соединение.</p>
        "#;

        let context = extract_context(html).unwrap();
        assert!(context.thick_client);
        assert!(context.thin_client);
        assert!(context.web_client);
        assert!(context.server);
        assert!(context.mobile_client);
        assert!(context.external_connection);
    }

    #[test]
    fn test_extract_parameters_with_types() {
        let html = r#"
            <div class="V8SH_rubric">
                <p>&lt;Значение&gt; (обязательный)</p>
            </div>
            Тип: <a href="...">Число</a>. <br>
        "#;

        let params = extract_parameters(html);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "Значение");
        assert_eq!(params[0].param_type, Some("Число".to_string()));
        assert!(!params[0].is_optional);
    }

    #[test]
    fn test_extract_parameters_optional() {
        let html = r#"
            <div class="V8SH_rubric">
                <p>&lt;РежимЗаписи&gt; (необязательный)</p>
            </div>
            Тип: РежимЗаписиДокумента.
        "#;

        let params = extract_parameters(html);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "РежимЗаписи");
        assert!(params[0].is_optional);
        assert_eq!(params[0].param_type, Some("РежимЗаписиДокумента".to_string()));
    }

    #[test]
    fn test_extract_return_type() {
        let html = r#"
            <p class="V8SH_chapter">Возвращаемое значение:</p>
            <p>Тип: <a href="...">Число</a>.</p>
        "#;

        let return_type = extract_return_type(html);
        assert_eq!(return_type, Some("Число".to_string()));
    }

    #[test]
    fn test_extract_return_type_undefined() {
        let html = r#"
            <p class="V8SH_chapter">Возвращаемое значение:</p>
            <p>Тип: Неопределено.</p>
        "#;

        let return_type = extract_return_type(html);
        assert_eq!(return_type, Some("Неопределено".to_string()));
    }

    #[test]
    fn test_extract_return_type_without_type_prefix() {
        // Real-world case from Map.Get
        let html = r#"
            <p class="V8SH_chapter">Возвращаемое значение:</p>
            Значение элемента соответствия.<br>
            <a href="v8help://SyntaxHelperLanguage/def_Undefined">Неопределено</a> - если указанный ключ отсутствует.
        "#;

        let return_type = extract_return_type(html);
        // Should return None since the first link is "Неопределено" which is a value, not type
        // and there's no actual type specified (it's arbitrary)
        assert_eq!(return_type, None);
    }

    #[test]
    fn test_extract_return_type_from_link() {
        // Case where type is in a link without "Тип:" prefix
        let html = r#"
            <p class="V8SH_chapter">Возвращаемое значение:</p>
            <p><a href="v8help://SyntaxHelperContext/objects/catalog234/Array.html">Массив</a> элементов.</p>
        "#;

        let return_type = extract_return_type(html);
        assert_eq!(return_type, Some("Массив".to_string()));
    }

    #[test]
    fn test_extract_return_type_arbitrary() {
        // Case mentioning "произвольный"
        let html = r#"
            <p class="V8SH_chapter">Возвращаемое значение:</p>
            Произвольное значение элемента.
        "#;

        let return_type = extract_return_type(html);
        assert_eq!(return_type, Some("Произвольный".to_string()));
    }

    #[test]
    fn test_extract_version() {
        let html = r#"
            <p>Доступен, начиная с версии 8.3.6.</p>
        "#;

        let version = extract_version(html);
        assert_eq!(version, Some("8.3.6".to_string()));
    }

    #[test]
    fn test_extract_version_not_found() {
        let html = r#"
            <p>Some random text.</p>
        "#;

        let version = extract_version(html);
        assert_eq!(version, None);
    }

    #[test]
    fn test_extract_syntax() {
        let html = r#"
            <p class="V8SH_chapter">Синтаксис:</p>
            <p>НачатьТранзакцию(&lt;РежимБлокировок&gt;)</p>
        "#;

        let syntax = extract_syntax(html).unwrap();
        assert!(syntax.contains("НачатьТранзакцию"));
        assert!(syntax.contains("РежимБлокировок"));
    }

    #[test]
    fn test_extract_description() {
        let html = r#"
            <p class="V8SH_chapter">Описание:</p>
            <p>Открывает транзакцию. Транзакция предназначена для записи в информационную базу согласованных изменений.</p>
        "#;

        let desc = extract_description(html).unwrap();
        assert!(desc.contains("Открывает транзакцию"));
        assert!(desc.contains("согласованных изменений"));
    }

    #[test]
    fn test_extract_description_multiline() {
        let html = r#"
            <p class="V8SH_chapter">Описание:</p>
            <p>Первый параграф описания.</p>
            <p>Второй параграф описания.</p>
            <p class="V8SH_chapter">Следующая секция:</p>
        "#;

        let desc = extract_description(html).unwrap();
        assert!(desc.contains("Первый параграф"));
        assert!(desc.contains("Второй параграф"));
        assert!(!desc.contains("Следующая секция"));
    }

    #[test]
    fn test_extract_parameter_descriptions() {
        let html = r#"
            <p class="V8SH_chapter">Параметры:</p>
            <div class="V8SH_rubric">
                <p>&lt;РежимБлокировок&gt;</p>
            </div>
            Тип: РежимУправленияБлокировкойДанных.<br>
            Установка параметра имеет смысл, если для свойства конфигурации "Режим управления блокировкой данных" выбрано значение "Автоматический и Управляемый".
        "#;

        let params = extract_parameter_descriptions(html);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "РежимБлокировок");
        assert!(params[0].description.contains("РежимУправленияБлокировкойДанных"));
        assert!(params[0].description.contains("Автоматический и Управляемый"));
    }

    #[test]
    fn test_extract_parameter_descriptions_multiple() {
        let html = r#"
            <p class="V8SH_chapter">Параметры:</p>
            <div class="V8SH_rubric">
                <p>&lt;Строка&gt;</p>
            </div>
            Тип: Строка. Первый параметр.
            <div class="V8SH_rubric">
                <p>&lt;Число&gt;</p>
            </div>
            Тип: Число. Второй параметр.
        "#;

        let params = extract_parameter_descriptions(html);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "Строка");
        assert!(params[0].description.contains("Первый параметр"));
        assert_eq!(params[1].name, "Число");
        assert!(params[1].description.contains("Второй параметр"));
    }

    #[test]
    fn test_extract_examples() {
        let html = r##"
            <p class="V8SH_chapter">Пример:</p>
            <p>Увеличение закупочной цены на 5%</p>
            <TABLE bgColor="#f7f7f7">
                <TR><TD><pre>НачатьТранзакцию();
ВыборкаТоваров = Справочники.Номенклатура.Выбрать();
ЗафиксироватьТранзакцию();</pre></TD></TR>
            </TABLE>
        "##;

        let examples = extract_examples(html);
        assert_eq!(examples.len(), 1);
        assert!(examples[0].code.contains("НачатьТранзакцию"));
        assert!(examples[0].code.contains("ЗафиксироватьТранзакцию"));
        assert!(examples[0].description.as_ref().unwrap().contains("закупочной цены"));
    }

    #[test]
    fn test_extract_examples_multiple() {
        let html = r#"
            <p class="V8SH_chapter">Примеры:</p>
            <p>Первый пример</p>
            <pre>Пример1();</pre>
            <p>Второй пример</p>
            <pre>Пример2();</pre>
        "#;

        let examples = extract_examples(html);
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0].code, "Пример1();");
        assert_eq!(examples[0].description.as_ref().unwrap(), "Первый пример");
        assert_eq!(examples[1].code, "Пример2();");
        assert_eq!(examples[1].description.as_ref().unwrap(), "Второй пример");
    }

    #[test]
    fn test_extract_examples_without_description() {
        let html = r#"
            <p class="V8SH_chapter">Пример:</p>
            <pre>КодБезОписания();</pre>
        "#;

        let examples = extract_examples(html);
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].code, "КодБезОписания();");
        assert!(examples[0].description.is_none());
    }

    #[test]
    fn test_extract_notes() {
        let html = r#"
            <p class="V8SH_chapter">Примечание:</p>
            <p>Это важное примечание о методе.</p>
        "#;

        let notes = extract_notes(html).unwrap();
        assert!(notes.contains("важное примечание"));
    }

    #[test]
    fn test_extract_notes_plural() {
        let html = r#"
            <p class="V8SH_chapter">Примечания:</p>
            <p>Первое примечание.</p>
            <p>Второе примечание.</p>
        "#;

        let notes = extract_notes(html).unwrap();
        assert!(notes.contains("Первое примечание"));
        assert!(notes.contains("Второе примечание"));
    }

    #[test]
    fn test_real_begin_transaction_html() {
        // Real HTML from 1C platform for BeginTransaction
        let html = std::fs::read_to_string("/tmp/BeginTransaction.html");

        if html.is_err() {
            println!("Skipping test: /tmp/BeginTransaction.html not found");
            return;
        }

        let html = html.unwrap();

        // Test syntax extraction
        let syntax = extract_syntax(&html);
        println!("Syntax: {:?}", syntax);
        assert!(syntax.is_some());
        assert!(syntax.unwrap().contains("НачатьТранзакцию"));

        // Test description extraction
        let description = extract_description(&html);
        println!("Description: {:?}", description);
        assert!(description.is_some());
        let desc = description.unwrap();
        assert!(desc.contains("Открывает транзакцию"));

        // Test parameter descriptions
        let params = extract_parameter_descriptions(&html);
        println!("Params count: {}", params.len());
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "РежимБлокировок");
        assert!(params[0].description.contains("РежимУправленияБлокировкойДанных"));

        // Test examples
        let examples = extract_examples(&html);
        println!("Examples count: {}", examples.len());
        assert_eq!(examples.len(), 1);
        assert!(examples[0].code.contains("НачатьТранзакцию"));
        assert!(examples[0].code.contains("ЗафиксироватьТранзакцию"));
    }
}
