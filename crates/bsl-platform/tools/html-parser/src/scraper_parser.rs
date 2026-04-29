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
        if let Some(p) = extract_parameter_from_rubric(&rubric) {
            parameters.push(p);
        }
    }

    parameters
}

/// One syntax variant on a multi-overload page (e.g. `ПодключитьВнешнююКомпоненту`,
/// `Дата`, `ОткрытьФорму`). Matches the JSON schema in `main.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterVariant {
    /// Variant name from the `Вариант синтаксиса:` chapter (e.g.
    /// `"По идентификатору"`). `None` for single-overload pages, where the
    /// extractor returns one anonymous variant whose `parameters` matches
    /// the legacy [`extract_parameters`] output.
    pub variant_name: Option<String>,
    pub parameters: Vec<MethodParameter>,
}

/// Partition the page's parameter rubrics by `<p class="V8SH_chapter">`
/// boundaries.
///
/// **Boundaries.** A chapter whose text starts with `Вариант синтаксиса:`
/// commits the in-flight variant and starts a new one labelled with the
/// rest of the chapter text. A chapter whose text starts with one of the
/// page-level end markers (`Возвращаемое значение`, `Описание` —
/// excluding `Описание варианта метода`, `Доступность`, `Прим…` for
/// `Примечание/Примечания/Пример/Примеры`, `См. также`, `Использование`)
/// commits the in-flight variant and stops collecting until the next
/// `Вариант синтаксиса:`.
///
/// **Anonymous fallback.** A `V8SH_rubric` encountered with no in-flight
/// variant lazily opens an anonymous one (`variant_name = None`) — this
/// covers single-overload pages, which is the vast majority. On those
/// pages this function returns exactly one variant whose `parameters`
/// vector equals the legacy [`extract_parameters`] output.
///
/// Compound selector `"p.V8SH_chapter, div.V8SH_rubric"` walks both kinds
/// of element in document order — scraper's `select` is depth-first
/// document order, which matches the source layout.
pub fn extract_parameter_variants(html_content: &str) -> Vec<ParameterVariant> {
    let html = Html::parse_fragment(html_content);
    let combined_sel =
        Selector::parse("p.V8SH_chapter, div.V8SH_rubric").unwrap();

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
                let variant_name = if name.is_empty() {
                    None
                } else {
                    Some(name.to_string())
                };
                current = Some(ParameterVariant {
                    variant_name,
                    parameters: Vec::new(),
                });
            } else if is_variant_end_marker(trimmed) {
                if let Some(v) = current.take() {
                    variants.push(v);
                }
            }
            // Other chapters (`Синтаксис:`, `Параметры:`,
            // `Описание варианта метода:`) are no-ops — they sit
            // inside the current variant section.
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

/// Extracts a single parameter from a `V8SH_rubric` element. Shared between
/// [`extract_parameters`] (flat list) and [`extract_parameter_variants`]
/// (per-variant lists).
fn extract_parameter_from_rubric(rubric: &ElementRef) -> Option<MethodParameter> {
    let inner = rubric.inner_html();
    let name = extract_param_name(&inner)?;
    let is_optional = inner.contains("(необязательный)");
    let param_type = extract_param_type(rubric);
    // `is_variadic` defaults to false here — `mark_trailing_variadic_from_syntax`
    // at the page level lifts the flag for the trailing param when the
    // page's `Синтаксис:` chapter shows the `<X1>,...,<XN>` shape.
    Some(MethodParameter { name, param_type, is_optional, is_variadic: false })
}

/// True for chapter labels that close any in-flight variant section.
///
/// `Описание варианта метода:` is explicitly NOT a boundary — it lives
/// inside a variant. All other `Описание…` chapters (page-level
/// description) ARE boundaries.
///
/// The `Прим…` family is enumerated explicitly (`Примечание`, `Примечания`,
/// `Пример`, `Примеры`) rather than matched by prefix — `Применение` and
/// other unrelated chapters share that prefix, and a too-broad match would
/// cause a real variant to commit early on a chapter we don't actually
/// understand.
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

/// Extracts parameter type from element and following siblings.
///
/// **Algorithm.** Builds a single accumulating buffer from the rubric's
/// `inner_html` plus following sibling text. After every accumulation
/// step the buffer is fed to the **strict** [`extract_type_from_text`],
/// which only returns `Some` once a real terminator (`.`, `\n`, `<br>`)
/// follows the `Тип: ` prefix. This is load-bearing: the prior
/// implementation accepted partial text via a 200-char fallback inside
/// `extract_type_from_text` and returned eagerly after the FIRST type-name
/// arrived in the buffer (e.g. `Тип: Число `, before later siblings
/// contributed `Строка, КолонкаТаблицыЗначений`). The result was that
/// every multi-type platform parameter in `platform_data.json` got
/// truncated to its first variant — which caused false-positive
/// `TypeMismatch` diagnostics on legitimate `String` arguments to
/// `ТаблицаЗначений.ВыгрузитьКолонку(<Колонка>)` and similar methods.
///
/// Once the sibling walk exhausts (next-rubric / depth limit / EOF) the
/// permissive companion [`extract_type_from_text_unterminated`] runs
/// once over the final buffer — capped at 200 chars to avoid bleeding
/// into description prose, but accepting whatever was collected.
fn extract_param_type(element: &ElementRef) -> Option<String> {
    // Seed the buffer with the rubric's own inner_html — types
    // sometimes live entirely inside the rubric block (e.g.
    // `<div class="V8SH_rubric">… Тип: <a>Number</a>.</div>`), and
    // sometimes straddle the closing `</div>` because the platform's
    // emitted HTML is loose. A unified buffer handles both.
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

        // Stop at the next rubric — never bleed into a sibling
        // parameter's `Тип: …` sentence.
        if let Some(elem) = n.value().as_element() {
            if elem.attr("class") == Some("V8SH_rubric") {
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

    // Final fallback after sibling walk is exhausted.
    extract_type_from_text_unterminated(&buffer)
}

/// Extracts the `Тип: <names>` substring **only when** a real terminator
/// (`.`, `\n`, `<br>`) follows the type list.
///
/// Returns `None` when no terminator is yet present in `text` — this is
/// load-bearing for incremental sibling-walk callers (see
/// [`extract_param_type`]). For the post-walk "use whatever we have"
/// behaviour, see [`extract_type_from_text_unterminated`].
fn extract_type_from_text(text: &str) -> Option<String> {
    let pos = text.find("Тип: ")?;
    let after = &text[pos + "Тип: ".len()..];
    let end = after
        .find('.')
        .or_else(|| after.find('\n'))
        .or_else(|| after.find("<br"))?;
    finalize_type_substring(&after[..end])
}

/// Permissive companion to [`extract_type_from_text`] — used as the
/// final fallback when the sibling walk is done and the strict pass
/// never matched.
///
/// Crucially this **still honours real terminators** when present —
/// only the truly-no-terminator branch falls back to a 200-char cap.
/// This protects against the `Тип: . <br>…` shape that some platform
/// HBK pages use to say "no specific parameter type" (e.g.
/// `FullTextSearchManager.УстановитьРежимПолнотекстовогоПоиска`):
/// the literal empty-type-with-`.` must surface as `None`, not as a
/// 200-char slice of the parameter description that happens to live
/// behind the `<br>`. Without the terminator-first pass here, the
/// corruption leaked through to `platform_data.json` as `param_type:
/// ".  Устанавливаемый режим …Описание:…"` (Codex flagged it at
/// stop-time on 2026-04-28).
fn extract_type_from_text_unterminated(text: &str) -> Option<String> {
    let pos = text.find("Тип: ")?;
    let after = &text[pos + "Тип: ".len()..];
    let end = after
        .find('.')
        .or_else(|| after.find('\n'))
        .or_else(|| after.find("<br"))
        .unwrap_or_else(|| {
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
/// text content, which combined with HTML's `", "` text nodes between
/// `<a>` links yields `Тип: Число , Строка , КолонкаТаблицыЗначений`
/// with stray pre-comma spaces. Downstream consumers
/// (`resolve_platform_type_union` splits on `,` and trims) are tolerant
/// of this, but the canonical platform_data.json form uses a tight
/// `", "` separator, so we collapse here at the source.
fn finalize_type_substring(raw: &str) -> Option<String> {
    let cleaned = strip_html_tags(raw);
    let trimmed = cleaned.trim().trim_end_matches('.').trim();
    if trimmed.is_empty() {
        return None;
    }
    // Drop empty segments that the sibling walk's trailing-space pushes
    // can leave between commas (`Тип: A, B, , ` → `["A", "B", "", ""]`),
    // and the trailing comma case (502+ JSON entries had `…, X, ` shapes
    // before this filter), so the canonical comma-tight form is emitted.
    let normalised: Vec<&str> =
        trimmed.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if normalised.is_empty() {
        None
    } else {
        Some(normalised.join(", "))
    }
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParamDescription {
    pub name: String,
    pub description: String,
    /// Default value (extracted from "Значение по умолчанию:" in description)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

/// Extracts "Значение по умолчанию:" from parameter description text.
///
/// Handles patterns like:
/// - "Значение по умолчанию: Неопределено."
/// - "Значение по умолчанию - 10."
fn extract_default_value(text: &str) -> Option<String> {
    const PATTERNS: &[&str] = &["Значение по умолчанию:", "Значение по умолчанию -"];

    for pattern in PATTERNS {
        if let Some(pos) = text.find(pattern) {
            let after = &text[pos + pattern.len()..];
            // Find end: period, newline, or end of string
            let end = after
                .find('.')
                .or_else(|| after.find('\n'))
                .unwrap_or(after.len());
            let value = after[..end].trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

    /// Real-world `ТаблицаЗначений.ВыгрузитьКолонку` shape — `Тип: …`
    /// after the rubric `</div>` carries three comma-separated link
    /// nodes (`Число`, `Строка`, `КолонкаТаблицыЗначений`) before the
    /// terminating `.<br>`. Before the strict-terminator fix, the
    /// extractor returned eagerly with just `Число` because the
    /// premature 200-char fallback fired after the first sibling
    /// added that single link to the buffer.
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

    /// Variant where the entire `Тип: ...` sentence sits inside the
    /// rubric block itself, before the closing `</div>`. The unified
    /// `inner_html`-seeded buffer must catch it on the first
    /// `extract_type_from_text` call.
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
        assert_eq!(
            params[0].param_type,
            Some("Число, Строка, КолонкаТаблицыЗначений".to_string()),
        );
    }

    /// Single-overload page (no `Вариант синтаксиса:` chapter): the
    /// variant extractor returns one anonymous variant whose parameters
    /// match the legacy [`extract_parameters`] output. Locks the
    /// "no behaviour change for 99% of platform pages" invariant.
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
        // Parity with the legacy flat extractor.
        let flat = extract_parameters(html);
        let flat_names: Vec<&str> = flat.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(flat_names, names);
    }

    /// Mini-replica of the real `ПодключитьВнешнююКомпоненту` HBK page —
    /// two `Вариант синтаксиса:` chapters, each with its own rubric set,
    /// terminated by a `Возвращаемое значение:` end-marker. The extractor
    /// must split into two variants whose union (across all rubrics)
    /// equals the legacy flat output, but whose individual parameter
    /// lists differ — slot 1 is `ИдентификаторОбъекта` in v1 and
    /// `Местоположение` in v2.
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
        // The legacy flat extractor sees the union (5 entries) — that is
        // the very bug `variants` exists to work around.
        assert_eq!(extract_parameters(html).len(), 5);
    }

    /// `XMLReader.GetAttribute` shape (3 variants). Locks the user-
    /// reported false-positive case: the `По полному имени` variant has
    /// a single `String` param, so calling `Чтение.ПолучитьАтрибут("…")`
    /// must accept once `MethodInfo::overloads` is populated.
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

    /// `Описание варианта метода:` is INSIDE a variant section — it
    /// must NOT close the variant. Without this carve-out, the variant
    /// would commit early and any rubrics later would land in a fresh
    /// anonymous variant, splitting one logical overload into two.
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

    /// When the sibling walk runs out of nodes without ever seeing a
    /// real terminator (`.`/`\n`/`<br>`), the permissive
    /// post-walk fallback must still extract whatever was collected
    /// after `Тип: `. This guards against regressions where dropping
    /// the premature fallback inside `extract_type_from_text` could
    /// lose the type entirely on truncated/loose markup.
    #[test]
    fn test_extract_parameters_no_terminator_falls_back_at_end() {
        // No period, no <br>, no newline — only the rubric and the
        // raw `Тип: …` text.
        let html = "<div class=\"V8SH_rubric\"><p>&lt;Имя&gt; (обязательный)</p></div>Тип: Строка";

        let params = extract_parameters(html);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "Имя");
        assert_eq!(params[0].param_type, Some("Строка".to_string()));
    }

    /// Trailing-comma normalisation: when the source HTML has
    /// `Тип: A, B, ` (with a stray trailing comma+space before the
    /// terminator) — typically because the sibling walk pushes an
    /// extra `' '` after each element — empty segments must be
    /// filtered, not joined back as `"A, B, "`.
    #[test]
    fn test_extract_parameters_trailing_comma_is_normalised() {
        let html = "<div class=\"V8SH_rubric\"><p>&lt;X&gt; (обязательный)</p></div>Тип: A, B, .";

        let params = extract_parameters(html);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].param_type, Some("A, B".to_string()));
    }

    /// Real-world HBK shape `Тип: . <br>…description…` — the platform
    /// docs use a literal empty type to say "no specific parameter
    /// type" (e.g. `FullTextSearchManager.УстановитьРежимПолнотекстовогоПоиска`).
    /// The unterminated fallback must NOT recover a 200-char prose
    /// slice from the description that follows the `<br>`. With
    /// terminator-first preference in the fallback, the empty type
    /// surfaces as `None` (no `param_type` emitted in JSON).
    #[test]
    fn test_extract_parameters_empty_type_with_dot_yields_none() {
        let html = "<div class=\"V8SH_rubric\"><p>&lt;Режим&gt; (обязательный)</p></div>Тип: . <br>Описание параметра.";

        let params = extract_parameters(html);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "Режим");
        assert_eq!(params[0].param_type, None,
            "literal empty type (Тип: .) must surface as None, not corrupt prose");
    }

    /// Strict mode: `extract_type_from_text` must return `None` when
    /// no terminator follows `Тип: ` — callers rely on this to keep
    /// accumulating sibling text instead of locking in a partial
    /// answer.
    #[test]
    fn test_extract_type_from_text_strict_requires_terminator() {
        // Single token after `Тип: ` with no terminator → None.
        assert_eq!(extract_type_from_text("Тип: Число"), None);
        // Terminated by period → matches.
        assert_eq!(
            extract_type_from_text("Тип: Число."),
            Some("Число".to_string())
        );
        // Multi-type, terminated → matches in full.
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
        // Colon variant
        let text = "Тип: Число. Значение по умолчанию: 10. Описание...";
        assert_eq!(extract_default_value(text), Some("10".to_string()));

        // Dash variant
        let text2 = "Тип: Строка. Значение по умолчанию - Пустая строка. Ещё текст.";
        assert_eq!(extract_default_value(text2), Some("Пустая строка".to_string()));

        // No default value
        let text3 = "Тип: Булево. Без значения по умолчанию.";
        assert_eq!(extract_default_value(text3), None);

        // With Неопределено
        let text4 = "Значение по умолчанию: Неопределено.";
        assert_eq!(extract_default_value(text4), Some("Неопределено".to_string()));
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

/// Extracts "See also" references from HTML content.
/// Looks for: `<p class="V8SH_chapter">См. также:</p>` followed by links.
///
/// Returns references as strings like "ТаблицаЗначений, метод Вставить".
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
            // Stop at next chapter section
            if let Some(elem) = n.value().as_element() {
                if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                    break;
                }
            }

            if let Some(text_node) = n.value().as_text() {
                // Trim start, preserve trailing space (e.g. ", метод ")
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
/// Format: "Для (For)" or "Процедура (Procedure)"
/// Returns (русское, english) tuple
pub fn extract_keyword_name(html_content: &str) -> Option<(String, String)> {
    let html = Html::parse_fragment(html_content);
    let h1_selector = Selector::parse("h1.V8SH_pagetitle").ok()?;

    for h1 in html.select(&h1_selector) {
        let text = h1.text().collect::<String>();
        // Parse format: "Для (For)" or "Для&nbsp;(For)"
        // Replace &nbsp; with space
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

/// Extracts constructor variant name from HTML page.
///
/// Source priority matches the real HBK layout observed in shcntx:
/// 1. `<p class="V8SH_heading">По количеству элементов</p>` — canonical,
///    strips the type prefix that `<h1>` keeps (`Массив.По ...`).
/// 2. Fallback: part of `<h1 class="V8SH_pagetitle">` after the first dot,
///    used when the page omits `V8SH_heading`.
///
/// Returns `None` when neither selector yields a non-empty string.
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

/// Extracts the raw chapter content for a chapter whose title is the
/// literal `Использование` block on a property page.
///
/// Property pages encode read-only semantics as plain text sibling to the
/// `Использование:` chapter header, e.g.
/// `<p class="V8SH_chapter">Использование:</p>Только чтение.` or
/// `…</p>Чтение и запись.`.
/// Returns `true` when the first non-empty text after the header contains
/// `Только чтение` (case-insensitive, Russian); otherwise `false`.
pub fn extract_readonly(html_content: &str) -> bool {
    let Some(text) = extract_chapter_content(html_content, "Использование") else {
        return false;
    };
    let lc = text.to_lowercase();
    lc.contains("только чтение")
}

/// Extracts the declared value types from a property page.
///
/// Property pages embed the property type inside the `Описание:` chapter
/// under the `Тип:` prefix, e.g.
/// `<p class="V8SH_chapter">Описание:</p>Тип: <a href="...">Структура</a>. <br>Содержит…`
/// or, for union returns,
/// `…Тип: <a>МенеджерВременныхТаблиц</a>, <a>Неопределено</a>. <br>…`.
///
/// Returns the list of Russian type names in source order. Empty when the
/// page omits the `Тип:` block or the description is free prose.
pub fn extract_property_types(html_content: &str) -> Vec<String> {
    let Some(desc) = extract_chapter_content(html_content, "Описание") else {
        return Vec::new();
    };
    let Some(pos) = desc.find("Тип:") else {
        return Vec::new();
    };
    let after = &desc[pos + "Тип:".len()..];
    let end = after
        .find('.')
        .or_else(|| after.find('\n'))
        .unwrap_or(after.len());
    let segment = after[..end].trim();
    segment
        .split(',')
        .map(|s| s.trim().trim_matches('.').trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parses keyword HTML file and extracts full documentation.
pub fn parse_keyword_html(html_content: &str) -> Option<KeywordDocumentation> {
    let (keyword_ru, keyword_en) = extract_keyword_name(html_content)?;

    let syntax = extract_syntax(html_content).unwrap_or_default();
    let description = extract_description(html_content).unwrap_or_default();
    let params = extract_parameter_descriptions(html_content);
    let min_version = extract_version(html_content);

    // Only return docs if we have at least description
    if description.is_empty() {
        return None;
    }

    Some(KeywordDocumentation {
        keyword_ru,
        keyword_en,
        syntax,
        description,
        params,
        min_version,
    })
}
