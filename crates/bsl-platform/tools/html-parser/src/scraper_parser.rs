use crate::{ContextAvailability, MethodParameter};
use scraper::{ElementRef, Html, Selector};

const CHAPTER_SELECTOR: &str = "p.V8SH_chapter";

const RUBRIC_SELECTOR: &str = "div.V8SH_rubric";

pub fn extract_version(html_content: &str) -> Option<String> {
    let html = Html::parse_fragment(html_content);

    for text_node in html.root_element().descendants() {
        if let Some(text) = text_node.value().as_text() {
            if let Some(pos) = text.text.find("Доступен, начиная с версии ") {
                let after = &text.text[pos + "Доступен, начиная с версии ".len()..];
                let version_end = after
                    .find(". ")
                    .or_else(|| {
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

/// Extract the XDTO type name a type page declares (e.g. `ГрафическаяСхема`
/// carries `Имя типа XDTO: FlowchartContextType`). Configuration XML serializes
/// some attribute types by this XDTO name rather than the class name, so it is
/// needed to resolve such tokens back to the platform type.
pub fn extract_xdto_name(html_content: &str) -> Option<String> {
    const MARKER: &str = "Имя типа XDTO: ";
    let html = Html::parse_fragment(html_content);

    for text_node in html.root_element().descendants() {
        if let Some(text) = text_node.value().as_text() {
            if let Some(pos) = text.text.find(MARKER) {
                // The marker is followed by `<Name>.` (a sentence period), so the
                // name runs up to the first non-identifier char. `.` must NOT be
                // accepted here or the trailing period would be captured; every
                // real 1C XDTO name is a plain alnum/underscore identifier.
                let name: String = text.text[pos + MARKER.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
    }
    None
}

pub fn extract_context(html_content: &str) -> Option<ContextAvailability> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse(CHAPTER_SELECTOR).unwrap();

    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if text.contains("Доступность") {
            let mut node = chapter.next_sibling();
            while let Some(n) = node {
                if let Some(elem) = n.value().as_element() {
                    if elem.name() == "p" {
                        let context_text =
                            ElementRef::wrap(n).unwrap().text().collect::<String>().to_lowercase();

                        return Some(parse_context_flags(&context_text));
                    }
                }
                node = n.next_sibling();
            }
        }
    }
    None
}

fn parse_context_flags(text: &str) -> ContextAvailability {
    ContextAvailability {
        thick_client: text.contains("толстый клиент"),
        thin_client: text.contains("тонкий клиент"),
        web_client: text.contains("веб-клиент"),
        server: text.contains("сервер"),
        mobile_client: text.contains("мобильный клиент"),
        external_connection: text.contains("внешнее соединение") || text.contains("интеграция"),
    }
}

pub fn extract_return_type(html_content: &str) -> Option<String> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse(CHAPTER_SELECTOR).unwrap();

    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if text.contains("Возвращаемое значение") {
            let mut content = String::new();
            let mut html_content_section = String::new();
            let mut node = chapter.next_sibling();

            while let Some(n) = node {
                if let Some(elem) = n.value().as_element() {
                    if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                        break;
                    }
                }

                if let Some(text_node) = n.value().as_text() {
                    content.push_str(&text_node.text);
                    html_content_section.push_str(&text_node.text);
                } else if let Some(elem_ref) = ElementRef::wrap(n) {
                    content.push_str(&elem_ref.text().collect::<String>());
                    html_content_section.push_str(&elem_ref.inner_html());
                }

                node = n.next_sibling();
            }

            if let Some(type_pos) = content.find("Тип: ") {
                let after = &content[type_pos + "Тип: ".len()..];
                let end = after.find('.').or_else(|| after.find('\n')).unwrap_or(after.len());
                let type_str = after[..end].trim();
                if !type_str.is_empty() {
                    return Some(type_str.to_string());
                }
            }

            if let Some(type_from_link) = extract_type_from_html_section(&html_content_section) {
                return Some(type_from_link);
            }

            if content.to_lowercase().contains("произвольн") {
                return Some("Произвольный".to_string());
            }
        }
    }
    None
}

pub fn extract_iter_element_types(html_content: &str) -> Vec<String> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse(CHAPTER_SELECTOR).unwrap();

    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if !text.contains("Элементы коллекции") {
            continue;
        }

        let mut content = String::new();
        let mut node = chapter.next_sibling();
        while let Some(n) = node {
            if let Some(elem) = n.value().as_element() {
                if elem.name() == "br" {
                    break;
                }
                if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                    break;
                }
            }
            if let Some(text_node) = n.value().as_text() {
                content.push_str(&text_node.text);
            } else if let Some(elem_ref) = ElementRef::wrap(n) {
                content.push_str(&elem_ref.text().collect::<String>());
            }
            node = n.next_sibling();
        }

        let normalised = content.replace(" или ", ", ").replace(" or ", ", ");
        return normalised
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    Vec::new()
}

fn extract_type_from_html_section(html: &str) -> Option<String> {
    let html_fragment = Html::parse_fragment(html);
    let link_sel = Selector::parse("a[href]").unwrap();

    for link in html_fragment.select(&link_sel) {
        let href = link.value().attr("href")?;

        if href.contains("1centerprise.com") || href.contains("#") {
            continue;
        }

        let link_text = link.text().collect::<String>();
        let trimmed = link_text.trim();

        if trimmed.is_empty()
            || trimmed == "Неопределено"
            || trimmed == "Undefined"
            || trimmed.contains("если")
            || trimmed.contains("if")
        {
            continue;
        }

        if href.contains("SyntaxHelperContext/objects")
            || href.contains("SyntaxHelperLanguage/def_")
        {
            return Some(trimmed.to_string());
        }
    }

    None
}

pub fn extract_parameters(html_content: &str) -> Vec<MethodParameter> {
    let html = Html::parse_fragment(html_content);
    let rubric_sel = Selector::parse(RUBRIC_SELECTOR).unwrap();

    let mut parameters = Vec::new();

    for rubric in html.select(&rubric_sel) {
        if let Some(p) = extract_parameter_from_rubric(&rubric) {
            parameters.push(p);
        }
    }

    parameters
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterVariant {
    pub variant_name: Option<String>,
    pub parameters: Vec<MethodParameter>,
}

pub fn extract_parameter_variants(html_content: &str) -> Vec<ParameterVariant> {
    let html = Html::parse_fragment(html_content);
    let combined_sel = Selector::parse("p.V8SH_chapter, div.V8SH_rubric").unwrap();

    let mut variants: Vec<ParameterVariant> = Vec::new();
    let mut current: Option<ParameterVariant> = None;

    for el in html.select(&combined_sel) {
        let class = el.value().attr("class").unwrap_or("");
        if class.contains("V8SH_chapter") {
            let text = el.text().collect::<String>();
            let trimmed = text.trim();

            if let Some(rest) = trimmed.strip_prefix("Вариант синтаксиса:") {
                if let Some(v) = current.take() {
                    variants.push(v);
                }
                let name = rest.trim();
                let variant_name = if name.is_empty() { None } else { Some(name.to_string()) };
                current = Some(ParameterVariant { variant_name, parameters: Vec::new() });
            } else if is_variant_end_marker(trimmed) {
                if let Some(v) = current.take() {
                    variants.push(v);
                }
            }
        } else if class.contains("V8SH_rubric") {
            if let Some(p) = extract_parameter_from_rubric(&el) {
                let bucket = current.get_or_insert_with(|| ParameterVariant {
                    variant_name: None,
                    parameters: Vec::new(),
                });
                bucket.parameters.push(p);
            }
        }
    }

    if let Some(v) = current.take() {
        variants.push(v);
    }
    variants
}

fn extract_parameter_from_rubric(rubric: &ElementRef) -> Option<MethodParameter> {
    let inner = rubric.inner_html();
    let name = extract_param_name(&inner)?;
    let is_optional = inner.contains("(необязательный)");
    let param_type = extract_param_type(rubric);
    Some(MethodParameter { name, param_type, is_optional, is_variadic: false })
}

fn is_variant_end_marker(trimmed: &str) -> bool {
    if trimmed.starts_with("Описание варианта") {
        return false;
    }
    trimmed.starts_with("Возвращаемое значение")
        || trimmed.starts_with("Описание")
        || trimmed.starts_with("Доступность")
        || trimmed.starts_with("Примечание")
        || trimmed.starts_with("Примечания")
        || trimmed.starts_with("Пример")
        || trimmed.starts_with("Примеры")
        || trimmed.starts_with("См. также")
        || trimmed.starts_with("Использование")
}

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

fn extract_param_type(element: &ElementRef) -> Option<String> {
    let mut buffer = element.inner_html();
    if let Some(type_str) = extract_type_from_text(&buffer) {
        return Some(type_str);
    }

    let mut node = element.next_sibling();
    let mut search_depth = 0;

    while let Some(n) = node {
        if search_depth > 10 {
            break;
        }
        search_depth += 1;

        if let Some(elem) = n.value().as_element() {
            if elem.attr("class") == Some("V8SH_rubric")
                || (elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter"))
            {
                break;
            }
        }

        if let Some(text) = n.value().as_text() {
            buffer.push_str(&text.text);
        }
        if let Some(elem_ref) = ElementRef::wrap(n) {
            buffer.push_str(&elem_ref.text().collect::<String>());
            buffer.push(' ');
        }

        node = n.next_sibling();

        if let Some(type_str) = extract_type_from_text(&buffer) {
            return Some(type_str);
        }
    }

    extract_type_from_text_unterminated(&buffer)
}

fn extract_type_from_text(text: &str) -> Option<String> {
    let pos = text.find("Тип: ")?;
    let after = &text[pos + "Тип: ".len()..];
    let end = find_type_end(after)?;
    finalize_type_substring(&after[..end])
}

enum TypeTerminator {
    Period,
    LineBreak(usize),
}

fn find_type_end(text: &str) -> Option<usize> {
    let mut search_from = 0;

    while let Some((end, terminator)) = find_next_type_terminator(text, search_from) {
        match terminator {
            TypeTerminator::Period => return Some(end),
            TypeTerminator::LineBreak(line_break_len)
                if strip_html_tags(&text[..end]).trim_end().ends_with(',') =>
            {
                search_from = end + line_break_len;
                continue;
            }
            TypeTerminator::LineBreak(_) => return Some(end),
        }
    }

    None
}

fn find_next_type_terminator(text: &str, search_from: usize) -> Option<(usize, TypeTerminator)> {
    let remaining = &text[search_from..];
    let mut terminator = remaining.find('.').map(|pos| (search_from + pos, TypeTerminator::Period));

    for (pos, len) in [(remaining.find('\n'), 1), (remaining.find("<br"), 3)] {
        if let Some(pos) = pos {
            let candidate = (search_from + pos, TypeTerminator::LineBreak(len));
            if terminator.as_ref().is_none_or(|current| candidate.0 < current.0) {
                terminator = Some(candidate);
            }
        }
    }

    terminator
}

/// Permissive companion to [`extract_type_from_text`] — used as the
/// final fallback when the sibling walk is done and the strict pass
/// never matched.
///
/// Crucially this **still honours real terminators** when present —
/// only the truly-no-terminator branch falls back to a 200-char cap.
/// This protects against the `Тип: . <br>…` shape that some platform
/// HBK pages use to say "no specific parameter type": the literal
/// empty-type-with-`.` must surface as `None`, not as a 200-char slice
/// of the description that happens to live behind the `<br>`.
fn extract_type_from_text_unterminated(text: &str) -> Option<String> {
    let pos = text.find("Тип: ")?;
    let after = &text[pos + "Тип: ".len()..];
    let end = find_type_end(after).unwrap_or_else(|| {
        after.char_indices().nth(200).map(|(idx, _)| idx).unwrap_or(after.len())
    });
    finalize_type_substring(&after[..end])
}

/// Strip HTML tags, trim, drop the trailing dot, normalise spacing
/// around comma separators. Shared between the strict and permissive
/// type-extraction paths.
///
/// Comma normalisation is needed because the sibling walk in
/// [`extract_param_type`] pushes a trailing `' '` after every element's
/// text content, which combined with HTML's whitespace can leave empty
/// segments between commas that must be dropped.
fn finalize_type_substring(raw: &str) -> Option<String> {
    let cleaned = strip_html_tags(raw);
    let trimmed = cleaned.trim().trim_end_matches('.').trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalised: Vec<&str> =
        trimmed.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if normalised.is_empty() {
        None
    } else {
        Some(normalised.join(", "))
    }
}

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

pub fn extract_syntax(html_content: &str) -> Option<String> {
    extract_chapter_content(html_content, "Синтаксис")
}

pub fn extract_description(html_content: &str) -> Option<String> {
    extract_chapter_content(html_content, "Описание")
}

pub fn extract_notes(html_content: &str) -> Option<String> {
    extract_chapter_content(html_content, "Примечание")
        .or_else(|| extract_chapter_content(html_content, "Примечания"))
}

fn extract_chapter_content(html_content: &str, chapter_title: &str) -> Option<String> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse(CHAPTER_SELECTOR).unwrap();

    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if text.contains(chapter_title) {
            let mut content = String::new();
            let mut node = chapter.next_sibling();

            while let Some(n) = node {
                if let Some(elem) = n.value().as_element() {
                    if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                        break;
                    }
                }

                if let Some(text_node) = n.value().as_text() {
                    content.push_str(&text_node.text);
                } else if let Some(elem_ref) = ElementRef::wrap(n) {
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParamDescription {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

fn extract_default_value(text: &str) -> Option<String> {
    const PATTERNS: &[&str] = &["Значение по умолчанию:", "Значение по умолчанию -"];

    for pattern in PATTERNS {
        if let Some(pos) = text.find(pattern) {
            let after = &text[pos + pattern.len()..];
            let end = after.find('.').or_else(|| after.find('\n')).unwrap_or(after.len());
            let value = after[..end].trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub fn extract_parameter_descriptions(html_content: &str) -> Vec<ParamDescription> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse(CHAPTER_SELECTOR).unwrap();

    let mut param_descriptions = Vec::new();

    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if text.contains("Параметры") {
            let mut node = chapter.next_sibling();

            while let Some(n) = node {
                if let Some(elem) = n.value().as_element() {
                    if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                        break;
                    }
                }

                if let Some(elem_ref) = ElementRef::wrap(n) {
                    if elem_ref.value().attr("class") == Some("V8SH_rubric") {
                        let inner = elem_ref.inner_html();
                        if let Some(param_name) = extract_param_name(&inner) {
                            let description = collect_param_description(elem_ref);

                            let default_value = extract_default_value(&description);
                            param_descriptions.push(ParamDescription {
                                name: param_name,
                                description,
                                default_value,
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

fn collect_param_description(rubric: ElementRef) -> String {
    let mut description = String::new();
    let mut node = rubric.next_sibling();
    let mut depth = 0;

    while let Some(n) = node {
        if depth > 20 {
            break;
        }
        depth += 1;

        if let Some(elem) = n.value().as_element() {
            if elem.attr("class") == Some("V8SH_rubric") {
                break;
            }
            if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                break;
            }
        }

        if let Some(text_node) = n.value().as_text() {
            description.push_str(&text_node.text);
        } else if let Some(elem_ref) = ElementRef::wrap(n) {
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeExample {
    pub code: String,
    pub description: Option<String>,
}

pub fn extract_examples(html_content: &str) -> Vec<CodeExample> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse(CHAPTER_SELECTOR).unwrap();
    let pre_sel = Selector::parse("pre").unwrap();

    let mut examples = Vec::new();

    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if text.contains("Пример") {
            let mut node = chapter.next_sibling();
            let mut current_description = String::new();

            while let Some(n) = node {
                if let Some(elem) = n.value().as_element() {
                    if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                        break;
                    }
                }

                if let Some(elem_ref) = ElementRef::wrap(n) {
                    if elem_ref.value().name() == "table"
                        || elem_ref.value().attr("bgcolor") == Some("#f7f7f7")
                    {
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

                        if !found_code {
                            let mut code_text = String::new();

                            for node in elem_ref.descendants() {
                                if let Some(text) = node.value().as_text() {
                                    code_text.push_str(&text.text);
                                } else if let Some(elem) = node.value().as_element() {
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
                    } else if elem_ref.value().name() != "table" {
                        let elem_text = elem_ref.text().collect::<String>();
                        if !elem_text.trim().is_empty() {
                            if !current_description.is_empty() {
                                current_description.push(' ');
                            }
                            current_description.push_str(&elem_text);
                        }
                    }
                } else if let Some(text_node) = n.value().as_text() {
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
        assert!(context.external_connection);
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
    fn test_extract_parameters_multi_type_links() {
        let html = r#"
            <div class="V8SH_rubric">
                <p>&lt;Колонка&gt; (обязательный)</p>
            </div>
            Тип: <a href="v8help://SyntaxHelperLanguage/def_Number">Число</a>, <a href="v8help://SyntaxHelperLanguage/def_String">Строка</a>, <a href="v8help://SyntaxHelperContext/objects/.../ValueTableColumn.html">КолонкаТаблицыЗначений</a>. <br>
            Колонка, значения которой необходимо выгрузить.
        "#;

        let params = extract_parameters(html);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "Колонка");
        assert_eq!(
            params[0].param_type,
            Some("Число, Строка, КолонкаТаблицыЗначений".to_string()),
            "all three types must survive — premature fallback used to drop the tail"
        );
    }

    #[test]
    fn test_extract_parameters_multi_type_in_inner() {
        let html = r#"
            <div class="V8SH_rubric">
                <p>&lt;Колонка&gt; (обязательный)</p>
                Тип: <a>Число</a>, <a>Строка</a>, <a>КолонкаТаблицыЗначений</a>.
            </div>
        "#;

        let params = extract_parameters(html);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "Колонка");
        assert_eq!(params[0].param_type, Some("Число, Строка, КолонкаТаблицыЗначений".to_string()),);
    }

    #[test]
    fn test_extract_parameters_continues_comma_terminated_type_list_after_br() {
        let html = r#"
            <div class="V8SH_rubric">
                <p>&lt;Значение&gt; (обязательный)</p>
                Тип: A, B,<br></div>C, D.
        "#;

        let params = extract_parameters(html);

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].param_type, Some("A, B, C, D".to_string()));
    }

    #[test]
    fn test_extract_parameters_continues_comma_terminated_type_list_into_sibling_element() {
        let html = r#"
            <div class="V8SH_rubric">
                <p>&lt;Значение&gt; (обязательный)</p>
                Тип: A, B,<br></div><span>C, D.</span>
        "#;

        let params = extract_parameters(html);

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].param_type, Some("A, B, C, D".to_string()));
    }

    #[test]
    fn test_extract_parameters_stops_comma_terminated_type_list_at_next_chapter() {
        let html = r#"
            <div class="V8SH_rubric"><p>&lt;Значение&gt; (обязательный)</p></div>
            Тип: A, B,
            <p class="V8SH_chapter">Описание:</p>
            C, D.
        "#;

        let params = extract_parameters(html);

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].param_type, Some("A, B".to_string()));
    }

    #[test]
    fn test_extract_parameters_unterminated_type_respects_character_cap() {
        let type_name = "A".repeat(201);
        let html = format!(
            "<div class=\"V8SH_rubric\"><p>&lt;Значение&gt; (обязательный)</p></div>Тип: {type_name}"
        );

        let params = extract_parameters(&html);

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].param_type.as_deref().map(str::len), Some(200));
    }

    #[test]
    fn test_extract_parameter_variants_single_overload_anonymous() {
        let html = r#"
            <p class="V8SH_chapter">Параметры:</p>
            <div class="V8SH_rubric"><p>&lt;Значение&gt; (обязательный)</p></div>Тип: Число.
            <div class="V8SH_rubric"><p>&lt;Флаг&gt; (необязательный)</p></div>Тип: Булево.
            <p class="V8SH_chapter">Возвращаемое значение:</p>Тип: Число.
        "#;
        let variants = extract_parameter_variants(html);
        assert_eq!(variants.len(), 1, "single-overload page must yield one variant");
        assert!(variants[0].variant_name.is_none(), "no Вариант chapter → anonymous");
        let names: Vec<&str> = variants[0].parameters.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Значение", "Флаг"]);
        assert!(!variants[0].parameters[0].is_optional);
        assert!(variants[0].parameters[1].is_optional);
        let flat = extract_parameters(html);
        let flat_names: Vec<&str> = flat.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(flat_names, names);
    }

    #[test]
    fn test_extract_parameter_variants_attach_addin_two_variants() {
        let html = r#"
            <p class="V8SH_chapter">Вариант синтаксиса: По идентификатору</p>
            <p class="V8SH_chapter">Параметры:</p>
            <div class="V8SH_rubric"><p>&lt;ИдентификаторОбъекта&gt; (обязательный)</p></div>Тип: Строка.
            <p class="V8SH_chapter">Описание варианта метода:</p>
            <p class="V8SH_chapter">Вариант синтаксиса: По имени и местоположению</p>
            <p class="V8SH_chapter">Параметры:</p>
            <div class="V8SH_rubric"><p>&lt;Местоположение&gt; (обязательный)</p></div>Тип: Строка.
            <div class="V8SH_rubric"><p>&lt;Имя&gt; (обязательный)</p></div>Тип: Строка.
            <div class="V8SH_rubric"><p>&lt;Тип&gt; (необязательный)</p></div>Тип: ТипВнешнейКомпоненты.
            <div class="V8SH_rubric"><p>&lt;ТипПодключения&gt; (необязательный)</p></div>Тип: ТипПодключенияВнешнейКомпоненты.
            <p class="V8SH_chapter">Возвращаемое значение:</p>Тип: Булево.
        "#;
        let variants = extract_parameter_variants(html);
        assert_eq!(variants.len(), 2, "two `Вариант синтаксиса:` chapters → two variants");
        assert_eq!(variants[0].variant_name.as_deref(), Some("По идентификатору"));
        assert_eq!(
            variants[0].parameters.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["ИдентификаторОбъекта"]
        );
        assert_eq!(variants[1].variant_name.as_deref(), Some("По имени и местоположению"));
        assert_eq!(
            variants[1].parameters.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["Местоположение", "Имя", "Тип", "ТипПодключения"]
        );
        assert!(!variants[1].parameters[0].is_optional);
        assert!(variants[1].parameters[3].is_optional);
        assert_eq!(extract_parameters(html).len(), 5);
    }

    #[test]
    fn test_extract_parameter_variants_three_variants() {
        let html = r#"
            <p class="V8SH_chapter">Вариант синтаксиса: По номеру</p>
            <p class="V8SH_chapter">Параметры:</p>
            <div class="V8SH_rubric"><p>&lt;НомерАтрибута&gt; (обязательный)</p></div>Тип: Число.
            <p class="V8SH_chapter">Вариант синтаксиса: По полному имени</p>
            <p class="V8SH_chapter">Параметры:</p>
            <div class="V8SH_rubric"><p>&lt;ПолноеИмяАтрибута&gt; (обязательный)</p></div>Тип: Строка.
            <p class="V8SH_chapter">Вариант синтаксиса: По локальному имени и пространству имен</p>
            <p class="V8SH_chapter">Параметры:</p>
            <div class="V8SH_rubric"><p>&lt;ЛокальноеИмяАтрибута&gt; (обязательный)</p></div>Тип: Строка.
            <div class="V8SH_rubric"><p>&lt;URIПространстваИмен&gt; (обязательный)</p></div>Тип: Строка.
            <p class="V8SH_chapter">Возвращаемое значение:</p>Тип: Строка, Неопределено.
        "#;
        let variants = extract_parameter_variants(html);
        assert_eq!(variants.len(), 3);
        assert_eq!(variants[0].parameters.len(), 1);
        assert_eq!(variants[0].parameters[0].param_type.as_deref(), Some("Число"));
        assert_eq!(variants[1].parameters.len(), 1);
        assert_eq!(variants[1].parameters[0].param_type.as_deref(), Some("Строка"));
        assert_eq!(variants[2].parameters.len(), 2);
    }

    #[test]
    fn test_extract_parameter_variants_method_description_does_not_close() {
        let html = r#"
            <p class="V8SH_chapter">Вариант синтаксиса: Один</p>
            <p class="V8SH_chapter">Параметры:</p>
            <div class="V8SH_rubric"><p>&lt;Первый&gt; (обязательный)</p></div>Тип: Число.
            <p class="V8SH_chapter">Описание варианта метода:</p>
            <p class="V8SH_chapter">Возвращаемое значение:</p>Тип: Булево.
        "#;
        let variants = extract_parameter_variants(html);
        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0].variant_name.as_deref(), Some("Один"));
        assert_eq!(variants[0].parameters.len(), 1);
    }

    #[test]
    fn test_extract_parameters_no_terminator_falls_back_at_end() {
        let html = "<div class=\"V8SH_rubric\"><p>&lt;Имя&gt; (обязательный)</p></div>Тип: Строка";

        let params = extract_parameters(html);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "Имя");
        assert_eq!(params[0].param_type, Some("Строка".to_string()));
    }

    #[test]
    fn test_extract_parameters_trailing_comma_is_normalised() {
        let html = "<div class=\"V8SH_rubric\"><p>&lt;X&gt; (обязательный)</p></div>Тип: A, B, .";

        let params = extract_parameters(html);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].param_type, Some("A, B".to_string()));
    }

    #[test]
    fn test_extract_parameters_trailing_comma_without_continuation_is_normalised() {
        let html = "<div class=\"V8SH_rubric\"><p>&lt;X&gt; (обязательный)</p>Тип: A, B,<br></div>";

        let params = extract_parameters(html);

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].param_type, Some("A, B".to_string()));
    }

    #[test]
    fn test_extract_parameters_continues_wrapped_type_list_through_nested_tags() {
        let html = r#"
            <div class="V8SH_rubric">
                <p>&lt;Значение&gt; (обязательный)</p>
                Тип: <strong>A, B,</strong><br></div><span><em>C</em>, <a>D</a>.</span>
        "#;

        let params = extract_parameters(html);

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].param_type, Some("A, B, C, D".to_string()));
    }

    #[test]
    fn test_extract_parameters_empty_type_with_dot_yields_none() {
        let html = "<div class=\"V8SH_rubric\"><p>&lt;Режим&gt; (обязательный)</p></div>Тип: . <br>Описание параметра.";

        let params = extract_parameters(html);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "Режим");
        assert_eq!(
            params[0].param_type, None,
            "literal empty type (Тип: .) must surface as None, not corrupt prose"
        );
    }

    #[test]
    fn test_extract_type_from_text_strict_requires_terminator() {
        assert_eq!(extract_type_from_text("Тип: Число"), None);
        assert_eq!(extract_type_from_text("Тип: Число."), Some("Число".to_string()));
        assert_eq!(
            extract_type_from_text("Тип: Число, Строка."),
            Some("Число, Строка".to_string())
        );
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
        let html = r#"
            <p class="V8SH_chapter">Возвращаемое значение:</p>
            Значение элемента соответствия.<br>
            <a href="v8help://SyntaxHelperLanguage/def_Undefined">Неопределено</a> - если указанный ключ отсутствует.
        "#;

        let return_type = extract_return_type(html);
        assert_eq!(return_type, None);
    }

    #[test]
    fn test_extract_return_type_from_link() {
        let html = r#"
            <p class="V8SH_chapter">Возвращаемое значение:</p>
            <p><a href="v8help://SyntaxHelperContext/objects/catalog234/Array.html">Массив</a> элементов.</p>
        "#;

        let return_type = extract_return_type(html);
        assert_eq!(return_type, Some("Массив".to_string()));
    }

    #[test]
    fn test_extract_return_type_arbitrary() {
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
    fn test_extract_see_also() {
        let html = r#"
            <p class="V8SH_chapter">См. также:</p>
            <a href="v8help://SyntaxHelperContext/objects/catalog234/catalog236/ValueTable.html">ТаблицаЗначений</a>, метод <a href="v8help://SyntaxHelperContext/objects/catalog234/catalog236/ValueTable/methods/Insert582.html">Вставить</a><br>
            <a href="v8help://SyntaxHelperContext/objects/catalog234/catalog236/ValueTable.html">ТаблицаЗначений</a>, метод <a href="v8help://SyntaxHelperContext/objects/catalog234/catalog236/ValueTable/methods/Find602.html">Найти</a><br>
            <p class="V8SH_chapter">Использование в версии:</p>
        "#;

        let see_also = extract_see_also(html);
        assert_eq!(see_also.len(), 2);
        assert_eq!(see_also[0], "ТаблицаЗначений, метод Вставить");
        assert_eq!(see_also[1], "ТаблицаЗначений, метод Найти");
    }

    #[test]
    fn test_extract_see_also_with_property() {
        let html = r#"
            <p class="V8SH_chapter">См. также:</p>
            <a href="v8help://...">ПолеСводнойТаблицы</a>, свойство <a href="v8help://...">Имя</a><br>
            <p class="V8SH_chapter">Использование в версии:</p>
        "#;

        let see_also = extract_see_also(html);
        assert_eq!(see_also.len(), 1);
        assert_eq!(see_also[0], "ПолеСводнойТаблицы, свойство Имя");
    }

    #[test]
    fn test_extract_see_also_empty() {
        let html = r#"
            <p class="V8SH_chapter">Описание:</p>
            <p>Просто описание без см. также.</p>
        "#;

        let see_also = extract_see_also(html);
        assert!(see_also.is_empty());
    }

    #[test]
    fn test_extract_readonly_true() {
        let html = r#"
            <p class="V8SH_chapter">Использование:</p>Только чтение.
            <p class="V8SH_chapter">Описание:</p>Тип: Структура.
        "#;
        assert!(extract_readonly(html));
    }

    #[test]
    fn test_extract_readonly_read_write() {
        let html = r#"
            <p class="V8SH_chapter">Использование:</p>Чтение и запись.
            <p class="V8SH_chapter">Описание:</p>Тип: Строка.
        "#;
        assert!(!extract_readonly(html));
    }

    #[test]
    fn test_extract_readonly_missing_chapter() {
        let html = r#"
            <p class="V8SH_chapter">Описание:</p>Тип: Строка.
        "#;
        assert!(!extract_readonly(html));
    }

    #[test]
    fn test_extract_property_types_single() {
        let html = r#"
            <p class="V8SH_chapter">Использование:</p>Только чтение.
            <p class="V8SH_chapter">Описание:</p>Тип: <a href="...">Структура</a>. <br>
            Содержит значения параметров.
        "#;
        assert_eq!(extract_property_types(html), vec!["Структура".to_string()]);
    }

    #[test]
    fn test_extract_property_types_union() {
        let html = r#"
            <p class="V8SH_chapter">Описание:</p>Тип: <a>МенеджерВременныхТаблиц</a>, <a>Неопределено</a>. <br>Содержит…
        "#;
        assert_eq!(
            extract_property_types(html),
            vec!["МенеджерВременныхТаблиц".to_string(), "Неопределено".to_string()]
        );
    }

    #[test]
    fn test_extract_property_types_missing_tip() {
        let html = r#"
            <p class="V8SH_chapter">Описание:</p>Свободное описание без метки Тип.
        "#;
        assert!(extract_property_types(html).is_empty());
    }

    #[test]
    fn test_extract_default_value() {
        let text = "Тип: Число. Значение по умолчанию: 10. Описание...";
        assert_eq!(extract_default_value(text), Some("10".to_string()));

        let text2 = "Тип: Строка. Значение по умолчанию - Пустая строка. Ещё текст.";
        assert_eq!(extract_default_value(text2), Some("Пустая строка".to_string()));

        let text3 = "Тип: Булево. Без значения по умолчанию.";
        assert_eq!(extract_default_value(text3), None);

        let text4 = "Значение по умолчанию: Неопределено.";
        assert_eq!(extract_default_value(text4), Some("Неопределено".to_string()));
    }

    #[test]
    fn test_real_begin_transaction_html() {
        let html = std::fs::read_to_string("/tmp/BeginTransaction.html");

        if html.is_err() {
            println!("Skipping test: /tmp/BeginTransaction.html not found");
            return;
        }

        let html = html.unwrap();

        let syntax = extract_syntax(&html);
        println!("Syntax: {:?}", syntax);
        assert!(syntax.is_some());
        assert!(syntax.unwrap().contains("НачатьТранзакцию"));

        let description = extract_description(&html);
        println!("Description: {:?}", description);
        assert!(description.is_some());
        let desc = description.unwrap();
        assert!(desc.contains("Открывает транзакцию"));

        let params = extract_parameter_descriptions(&html);
        println!("Params count: {}", params.len());
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "РежимБлокировок");
        assert!(params[0].description.contains("РежимУправленияБлокировкойДанных"));

        let examples = extract_examples(&html);
        println!("Examples count: {}", examples.len());
        assert_eq!(examples.len(), 1);
        assert!(examples[0].code.contains("НачатьТранзакцию"));
        assert!(examples[0].code.contains("ЗафиксироватьТранзакцию"));
    }

    #[test]
    fn iter_elements_array_returns_arbitrary() {
        let html = r#"<p class="V8SH_chapter">Элементы коллекции:</p>Произвольный<br>"#;
        assert_eq!(extract_iter_element_types(html), vec!["Произвольный".to_string()]);
    }

    #[test]
    fn iter_elements_register_record_set_with_placeholder() {
        let html = r#"<p class="V8SH_chapter">Элементы коллекции:</p><a href="x">РегистрСведенийЗапись.&lt;Имя регистра сведений&gt;</a><br>Для объекта доступен обход коллекции…<br>"#;
        assert_eq!(
            extract_iter_element_types(html),
            vec!["РегистрСведенийЗапись.<Имя регистра сведений>".to_string()]
        );
    }

    #[test]
    fn iter_elements_multi_type_with_links() {
        let html = r#"<p class="V8SH_chapter">Элементы коллекции:</p><a href="x">ВыражениеСхемыЗапроса</a>, <a href="y">ВложеннаяТаблицаСхемыЗапроса</a>, <a href="z">Неопределено</a><br>Для объекта доступен обход коллекции…<br>"#;
        assert_eq!(
            extract_iter_element_types(html),
            vec![
                "ВыражениеСхемыЗапроса".to_string(),
                "ВложеннаяТаблицаСхемыЗапроса".to_string(),
                "Неопределено".to_string(),
            ]
        );
    }

    #[test]
    fn iter_elements_no_chapter_returns_empty() {
        let html = r#"<p class="V8SH_chapter">Описание:</p><p>Тип строки.</p>"#;
        assert!(extract_iter_element_types(html).is_empty());
    }

    #[test]
    fn iter_elements_trims_nbsp_and_whitespace() {
        let html =
            "<p class=\"V8SH_chapter\">Элементы коллекции:</p>\u{00A0} КлючИЗначение \u{00A0}<br>";
        assert_eq!(extract_iter_element_types(html), vec!["КлючИЗначение".to_string()]);
    }

    #[test]
    fn iter_elements_splits_on_russian_or_conjunction() {
        let html = r#"<p class="V8SH_chapter">Элементы коллекции:</p>ОбластьЯчеекТабличногоДокумента или РисунокТабличногоДокумента<br>"#;
        assert_eq!(
            extract_iter_element_types(html),
            vec![
                "ОбластьЯчеекТабличногоДокумента".to_string(),
                "РисунокТабличногоДокумента".to_string(),
            ]
        );
    }

    #[test]
    fn iter_elements_splits_on_english_or_conjunction() {
        let html = r#"<p class="V8SH_chapter">Элементы коллекции:</p>A or B, C<br>"#;
        assert_eq!(
            extract_iter_element_types(html),
            vec!["A".to_string(), "B".to_string(), "C".to_string()]
        );
    }
}

pub fn extract_see_also(html_content: &str) -> Vec<String> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse(CHAPTER_SELECTOR).unwrap();

    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if !text.contains("См. также") {
            continue;
        }

        let mut references = Vec::new();
        let mut current_ref = String::new();
        let mut node = chapter.next_sibling();

        while let Some(n) = node {
            if let Some(elem) = n.value().as_element() {
                if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                    break;
                }
            }

            if let Some(text_node) = n.value().as_text() {
                let t = text_node.text.trim_start();
                if !t.is_empty() {
                    current_ref.push_str(t);
                }
            } else if let Some(elem_ref) = ElementRef::wrap(n) {
                if elem_ref.value().name() == "a" {
                    let link_text = elem_ref.text().collect::<String>();
                    let trimmed = link_text.trim();
                    if !trimmed.is_empty() {
                        current_ref.push_str(trimmed);
                    }
                } else if elem_ref.value().name() == "br" {
                    // <br> ends current reference
                    let trimmed = current_ref.trim().trim_end_matches(',').trim().to_string();
                    if !trimmed.is_empty() {
                        references.push(trimmed);
                    }
                    current_ref.clear();
                }
            }

            node = n.next_sibling();
        }

        // Don't forget last reference (if no trailing <br>)
        let trimmed = current_ref.trim().trim_end_matches(',').trim().to_string();
        if !trimmed.is_empty() {
            references.push(trimmed);
        }

        return references;
    }

    Vec::new()
}

/// Keyword documentation structure (intermediate format for JSON)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeywordDocumentation {
    pub keyword_ru: String,
    pub keyword_en: String,
    pub syntax: String,
    pub description: String,
    pub params: Vec<ParamDescription>,
    pub min_version: Option<String>,
}

/// Extracts keyword name from H1 title.
///
/// Format: "Для (For)" or "Процедура (Procedure)"; returns (russian, english).
pub fn extract_keyword_name(html_content: &str) -> Option<(String, String)> {
    let html = Html::parse_fragment(html_content);
    let h1_selector = Selector::parse("h1.V8SH_pagetitle").ok()?;

    for h1 in html.select(&h1_selector) {
        let text = h1.text().collect::<String>();
        let text = text.replace('\u{00A0}', " ");

        if let Some(open_paren) = text.find('(') {
            if let Some(close_paren) = text.find(')') {
                let russian = text[..open_paren].trim().to_string();
                let english = text[open_paren + 1..close_paren].trim().to_string();

                if !russian.is_empty() && !english.is_empty() {
                    return Some((russian, english));
                }
            }
        }
    }

    None
}

pub fn extract_constructor_variant_name(html_content: &str) -> Option<String> {
    let html = Html::parse_fragment(html_content);
    let heading_sel = Selector::parse("p.V8SH_heading").ok()?;
    if let Some(heading) = html.select(&heading_sel).next() {
        let txt = heading.text().collect::<String>().replace('\u{00A0}', " ");
        let trimmed = txt.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let h1_sel = Selector::parse("h1.V8SH_pagetitle").ok()?;
    if let Some(h1) = html.select(&h1_sel).next() {
        let txt = h1.text().collect::<String>().replace('\u{00A0}', " ");
        if let Some((_, after)) = txt.split_once('.') {
            let trimmed = after.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}

pub fn extract_readonly(html_content: &str) -> bool {
    let Some(text) = extract_chapter_content(html_content, "Использование") else {
        return false;
    };
    let lc = text.to_lowercase();
    lc.contains("только чтение")
}

pub fn extract_property_types(html_content: &str) -> Vec<String> {
    let Some(desc) = extract_chapter_content(html_content, "Описание") else {
        return Vec::new();
    };
    let Some(pos) = desc.find("Тип:") else {
        return Vec::new();
    };
    let after = &desc[pos + "Тип:".len()..];
    let end = after.find('.').or_else(|| after.find('\n')).unwrap_or(after.len());
    let segment = after[..end].trim();
    segment
        .split(',')
        .map(|s| s.trim().trim_matches('.').trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn parse_keyword_html(html_content: &str) -> Option<KeywordDocumentation> {
    let (keyword_ru, keyword_en) = extract_keyword_name(html_content)?;

    let syntax = extract_syntax(html_content).unwrap_or_default();
    let description = extract_description(html_content).unwrap_or_default();
    let params = extract_parameter_descriptions(html_content);
    let min_version = extract_version(html_content);

    if description.is_empty() {
        return None;
    }

    Some(KeywordDocumentation { keyword_ru, keyword_en, syntax, description, params, min_version })
}
