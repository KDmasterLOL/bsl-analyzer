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
}
