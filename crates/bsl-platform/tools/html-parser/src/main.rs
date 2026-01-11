//! HTML parser for extracting BSL platform documentation.
//!
//! Parses extracted .hbk files and generates JSON with:
//! - BSL keywords from shlang_ru.hbk
//! - Platform types and methods from shcntx_ru.hbk

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct PlatformData {
    keywords: Vec<KeywordInfo>,
    types: Vec<TypeInfo>,
    methods: Vec<MethodInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct KeywordInfo {
    /// Russian name (e.g., "Функция")
    russian: String,
    /// English name (e.g., "Function")
    english: String,
    /// Snippet template in Russian
    snippet_ru: String,
    /// Snippet template in English
    snippet_en: String,
    /// HTML documentation
    documentation: String,
    /// Category (def, struct, root, etc.)
    category: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TypeInfo {
    /// Russian name (e.g., "Массив")
    name: String,
    /// English name (e.g., "Array")
    english_name: String,
    /// Minimum version (e.g., "8.0")
    #[serde(skip_serializing_if = "Option::is_none")]
    min_version: Option<String>,
    /// Context availability
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<ContextAvailability>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MethodInfo {
    /// Method ID
    id: u32,
    /// Type name
    type_name: String,
    /// Russian method name
    name: String,
    /// English method name
    english_name: String,
    /// Return type (e.g., "Число", "Неопределено")
    #[serde(skip_serializing_if = "Option::is_none")]
    return_type: Option<String>,
    /// Method parameters
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    parameters: Vec<MethodParameter>,
    /// Minimum version (e.g., "8.0")
    #[serde(skip_serializing_if = "Option::is_none")]
    min_version: Option<String>,
    /// Context availability
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<ContextAvailability>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MethodParameter {
    /// Parameter name (e.g., "Значение")
    name: String,
    /// Parameter type (e.g., "Число", "Произвольный")
    #[serde(skip_serializing_if = "Option::is_none")]
    param_type: Option<String>,
    /// Is parameter optional
    is_optional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextAvailability {
    /// Available on thick client
    thick_client: bool,
    /// Available on thin client
    thin_client: bool,
    /// Available on web client
    web_client: bool,
    /// Available on server
    server: bool,
    /// Available on mobile client
    mobile_client: bool,
    /// Available on external connection
    external_connection: bool,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 4 {
        eprintln!("Usage: {} <shlang_dir> <shcntx_dir> <output_json>", args[0]);
        std::process::exit(1);
    }

    let shlang_dir = PathBuf::from(&args[1]);
    let shcntx_dir = PathBuf::from(&args[2]);
    let output_path = PathBuf::from(&args[3]);

    println!("Parsing shlang from: {}", shlang_dir.display());
    println!("Parsing shcntx from: {}", shcntx_dir.display());

    // Parse keywords from shlang
    let keywords = parse_shlang_keywords(&shlang_dir)
        .context("Failed to parse shlang keywords")?;

    println!("Parsed {} keywords", keywords.len());

    // Parse platform types and methods from shcntx
    let (types, methods) = parse_shcntx_data(&shcntx_dir)
        .context("Failed to parse shcntx data")?;

    println!("Parsed {} types and {} methods", types.len(), methods.len());

    // Generate JSON
    let platform_data = PlatformData {
        keywords,
        types,
        methods,
    };

    let json = serde_json::to_string_pretty(&platform_data)
        .context("Failed to serialize to JSON")?;

    fs::write(&output_path, json)
        .context("Failed to write output JSON")?;

    println!("Generated: {}", output_path.display());
    Ok(())
}

/// Parses shlang directory for BSL keywords.
fn parse_shlang_keywords(shlang_dir: &Path) -> Result<Vec<KeywordInfo>> {
    let mut keywords = Vec::new();

    // Find all .st files (snippet templates)
    for entry in fs::read_dir(shlang_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("st") {
            if let Some(keyword) = parse_keyword_file(&path)? {
                keywords.push(keyword);
            }
        }
    }

    Ok(keywords)
}

/// Parses a single keyword file (.st + corresponding HTML file).
fn parse_keyword_file(st_path: &Path) -> Result<Option<KeywordInfo>> {
    let file_name = st_path.file_stem().unwrap().to_string_lossy();

    // Parse category and base name from filename
    // Examples: "def_Func", "struct_For", "root_New"
    let parts: Vec<&str> = file_name.split('_').collect();
    if parts.len() < 2 {
        return Ok(None);
    }

    let category = parts[0].to_string();
    let _base_name = parts[1..].join("_");

    // Parse .st file to get snippets
    let st_content = fs::read_to_string(st_path)?;
    let (snippet_ru, snippet_en, russian, english) = parse_st_snippet(&st_content)?;

    // Load corresponding HTML documentation
    let html_path = st_path.with_extension("");
    let documentation = if html_path.exists() {
        fs::read_to_string(&html_path).unwrap_or_default()
    } else {
        String::new()
    };

    Ok(Some(KeywordInfo {
        russian,
        english,
        snippet_ru,
        snippet_en,
        documentation,
        category,
    }))
}

/// Parses .st file to extract snippet templates.
///
/// Format:
/// ```
/// {1,{2,
/// {"",1,0,"",""},
/// {0,{"ru",0,0,"","Функция <?>()..."}},
/// {0,{"en",0,0,"","Function <?>()..."}}
/// }}
/// ```
fn parse_st_snippet(content: &str) -> Result<(String, String, String, String)> {
    // Simple regex-like parsing of the structured format
    let mut snippet_ru = String::new();
    let mut snippet_en = String::new();
    let mut russian_name = String::new();
    let mut english_name = String::new();

    // Find Russian snippet: {"ru",0,0,"","..."}
    // The snippet is the 5th element (after 4 commas)
    if let Some(ru_start) = content.find(r#"{"ru",0,0,""#) {
        let after_pattern = &content[ru_start + r#"{"ru",0,0,""#.len()..];
        // Skip opening comma-quote: ,"
        if let Some(snippet_start) = after_pattern.find(r#","#) {
            let snippet_text = &after_pattern[snippet_start + 2..]; // Skip ,"
            if let Some(snippet_end) = snippet_text.find(r#""}"#) {
                snippet_ru = snippet_text[..snippet_end].to_string();
                // Extract first word as Russian name
                russian_name = snippet_ru
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
        }
    }

    // Find English snippet: {"en",0,0,"","..."}
    if let Some(en_start) = content.find(r#"{"en",0,0,""#) {
        let after_pattern = &content[en_start + r#"{"en",0,0,""#.len()..];
        // Skip opening comma-quote: ,"
        if let Some(snippet_start) = after_pattern.find(r#","#) {
            let snippet_text = &after_pattern[snippet_start + 2..]; // Skip ,"
            if let Some(snippet_end) = snippet_text.find(r#""}"#) {
                snippet_en = snippet_text[..snippet_end].to_string();
                // Extract first word as English name
                english_name = snippet_en
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
        }
    }

    Ok((snippet_ru, snippet_en, russian_name, english_name))
}

/// Parses shcntx directory for platform types and methods.
fn parse_shcntx_data(shcntx_dir: &Path) -> Result<(Vec<TypeInfo>, Vec<MethodInfo>)> {
    let mut types = Vec::new();
    let mut methods = Vec::new();
    let mut method_id_counter = 0u32;

    let objects_dir = shcntx_dir.join("objects");
    if !objects_dir.exists() {
        return Ok((types, methods));
    }

    // Iterate through catalog directories
    for catalog_entry in fs::read_dir(&objects_dir)? {
        let catalog_entry = catalog_entry?;
        let catalog_path = catalog_entry.path();

        if !catalog_path.is_dir() {
            continue;
        }

        // Recursively parse this catalog
        parse_catalog_directory(&catalog_path, &mut types, &mut methods, &mut method_id_counter)?;
    }

    Ok((types, methods))
}

/// Recursively parses a catalog directory for types and methods.
fn parse_catalog_directory(
    catalog_path: &Path,
    types: &mut Vec<TypeInfo>,
    methods: &mut Vec<MethodInfo>,
    method_id_counter: &mut u32,
) -> Result<()> {
    // Iterate through entries in this catalog
    for entry in fs::read_dir(catalog_path)? {
        let entry = entry?;
        let entry_path = entry.path();

        // Skip non-directories
        if !entry_path.is_dir() {
            continue;
        }

        let dir_name = entry_path.file_name().unwrap().to_string_lossy().to_string();

        // If it's a nested catalog, recurse into it
        if dir_name.starts_with("catalog") {
            parse_catalog_directory(&entry_path, types, methods, method_id_counter)?;
            continue;
        }

        // Otherwise it's a type directory - find corresponding .html file
        let type_html = entry_path.with_extension("html");
        if type_html.exists() {
            if let Some(type_info) = parse_type_html(&type_html, &entry_path)? {
                println!("Found type: {} / {}", type_info.name, type_info.english_name);

                // Parse methods for this type
                let type_methods = parse_type_methods(&entry_path, &type_info.english_name, method_id_counter)?;
                methods.extend(type_methods);

                types.push(type_info);
            }
        }
    }

    Ok(())
}

/// Parses type HTML file to extract type information.
fn parse_type_html(html_path: &Path, _type_dir: &Path) -> Result<Option<TypeInfo>> {
    let html_content = fs::read_to_string(html_path)?;

    // Extract type names from <h1 class="V8SH_pagetitle">Массив (Array)</h1>
    if let Some(title_start) = html_content.find(r#"<h1 class="V8SH_pagetitle">"#) {
        let after_title = &html_content[title_start + r#"<h1 class="V8SH_pagetitle">"#.len()..];
        if let Some(title_end) = after_title.find("</h1>") {
            let title = &after_title[..title_end];

            // Parse "Массив (Array)" format
            if let Some(paren_start) = title.find('(') {
                let russian = title[..paren_start].trim().to_string();
                let after_paren = &title[paren_start + 1..];
                if let Some(paren_end) = after_paren.find(')') {
                    let english = after_paren[..paren_end].trim().to_string();

                    // Extract additional information
                    let min_version = extract_version(&html_content);
                    let context = extract_context(&html_content);

                    return Ok(Some(TypeInfo {
                        name: russian,
                        english_name: english,
                        min_version,
                        context,
                    }));
                }
            }
        }
    }

    Ok(None)
}

/// Parses methods for a platform type.
fn parse_type_methods(type_dir: &Path, type_name: &str, method_id_counter: &mut u32) -> Result<Vec<MethodInfo>> {
    let mut methods = Vec::new();

    let methods_dir = type_dir.join("methods");
    if !methods_dir.exists() {
        return Ok(methods);
    }

    // Iterate through method HTML files
    for method_entry in fs::read_dir(&methods_dir)? {
        let method_entry = method_entry?;
        let method_path = method_entry.path();

        // Only process .html files
        if method_path.extension().and_then(|s| s.to_str()) != Some("html") {
            continue;
        }

        if let Some(method_info) = parse_method_html(&method_path, type_name, method_id_counter)? {
            methods.push(method_info);
        }
    }

    Ok(methods)
}

/// Parses method HTML file to extract method information.
fn parse_method_html(html_path: &Path, type_name: &str, method_id_counter: &mut u32) -> Result<Option<MethodInfo>> {
    let html_content = fs::read_to_string(html_path)?;

    // Extract method names from <h1 class="V8SH_pagetitle">Массив.Найти (Array.Find)</h1>
    if let Some(title_start) = html_content.find(r#"<h1 class="V8SH_pagetitle">"#) {
        let after_title = &html_content[title_start + r#"<h1 class="V8SH_pagetitle">"#.len()..];
        if let Some(title_end) = after_title.find("</h1>") {
            let title = &after_title[..title_end];

            // Parse "Массив.Найти (Array.Find)" format
            // Extract method names (after dot, before parenthesis)
            if let Some(dot_pos) = title.find('.') {
                let after_dot = &title[dot_pos + 1..];

                // Russian method name (before space and opening paren)
                let russian_method = if let Some(space_pos) = after_dot.find(' ') {
                    after_dot[..space_pos].trim().to_string()
                } else {
                    return Ok(None);
                };

                // English method name (inside parentheses, after dot)
                if let Some(paren_start) = title.find('(') {
                    let after_paren = &title[paren_start + 1..];
                    if let Some(dot_in_paren) = after_paren.find('.') {
                        let after_paren_dot = &after_paren[dot_in_paren + 1..];
                        if let Some(paren_end) = after_paren_dot.find(')') {
                            let english_method = after_paren_dot[..paren_end].trim().to_string();

                            let id = *method_id_counter;
                            *method_id_counter += 1;

                            // Extract additional information
                            let return_type = extract_return_type(&html_content);
                            let parameters = extract_parameters(&html_content);
                            let min_version = extract_version(&html_content);
                            let context = extract_context(&html_content);

                            return Ok(Some(MethodInfo {
                                id,
                                type_name: type_name.to_string(),
                                name: russian_method,
                                english_name: english_method,
                                return_type,
                                parameters,
                                min_version,
                                context,
                            }));
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

/// Extracts version from HTML content
/// Looks for: "Доступен, начиная с версии 8.0."
fn extract_version(html: &str) -> Option<String> {
    if let Some(start) = html.find("Доступен, начиная с версии ") {
        let after_start = &html[start + "Доступен, начиная с версии ".len()..];
        if let Some(end) = after_start.find('.') {
            let version = after_start[..end].trim();
            if !version.is_empty() {
                return Some(version.to_string());
            }
        }
    }
    None
}

/// Extracts context availability from HTML content
/// Looks for: "<p class="V8SH_chapter">Доступность: </p><p>Тонкий клиент, сервер...</p>"
fn extract_context(html: &str) -> Option<ContextAvailability> {
    if let Some(start) = html.find("Доступность:") {
        let after_start = &html[start..];
        if let Some(p_start) = after_start.find("</p>") {
            let after_p = &after_start[p_start + "</p>".len()..];
            if let Some(next_p_start) = after_p.find("<p>") {
                let content_start = &after_p[next_p_start + "<p>".len()..];
                if let Some(p_end) = content_start.find("</p>") {
                    let context_text = content_start[..p_end].to_lowercase();
                    return Some(ContextAvailability {
                        thick_client: context_text.contains("толстый клиент"),
                        thin_client: context_text.contains("тонкий клиент"),
                        web_client: context_text.contains("веб-клиент"),
                        server: context_text.contains("сервер"),
                        mobile_client: context_text.contains("мобильный клиент"),
                        external_connection: context_text.contains("внешнее соединение")
                            || context_text.contains("интеграция"),
                    });
                }
            }
        }
    }
    None
}

/// Extracts return type from HTML content
/// Looks for: "Возвращаемое значение:</p>Тип: Число, Неопределено."
fn extract_return_type(html: &str) -> Option<String> {
    if let Some(start) = html.find("Возвращаемое значение:") {
        let after_start = &html[start..];
        if let Some(type_start) = after_start.find("Тип: ") {
            let after_type = &after_start[type_start + "Тип: ".len()..];
            // Find text until <br> or </p> or period
            let end_pos = after_type
                .find("</p>")
                .or_else(|| after_type.find("<br>"))
                .or_else(|| after_type.find('.'))
                .unwrap_or(after_type.len());

            let type_text = &after_type[..end_pos];
            // Clean up HTML tags like <a href...>Число</a>
            let cleaned = strip_html_tags(type_text).trim().to_string();
            if !cleaned.is_empty() && cleaned != "." {
                return Some(cleaned);
            }
        }
    }
    None
}

/// Extracts method parameters from HTML content
/// Looks for: "Параметры:</p><div class="V8SH_rubric">..."
fn extract_parameters(html: &str) -> Vec<MethodParameter> {
    let mut parameters = Vec::new();

    if let Some(params_start) = html.find("Параметры:</p>") {
        let after_params = &html[params_start..];

        // Find all parameter blocks: <div class="V8SH_rubric">
        let mut search_from = 0;
        while let Some(rubric_start) = after_params[search_from..].find(r#"<div class="V8SH_rubric">"#) {
            let param_block = &after_params[search_from + rubric_start..];

            // Extract parameter name: &lt;Значение&gt; (обязательный)
            if let Some(name_start) = param_block.find("&lt;") {
                let after_name_start = &param_block[name_start + "&lt;".len()..];
                if let Some(name_end) = after_name_start.find("&gt;") {
                    let param_name = after_name_start[..name_end].trim().to_string();

                    // Check if optional: (необязательный) or (обязательный)
                    let is_optional = param_block.contains("(необязательный)");

                    // Extract type: "Тип: Число." or "Тип: <a>Число</a>."
                    // Type can be after </div>, search in first 600 chars max
                    let search_limit = param_block.len().min(
                        param_block.char_indices()
                            .nth(600)
                            .map(|(idx, _)| idx)
                            .unwrap_or(param_block.len())
                    );
                    let search_area = &param_block[..search_limit];

                    let param_type = if let Some(type_start) = search_area.find("Тип: ") {
                        let after_type = &search_area[type_start + "Тип: ".len()..];
                        // Find end: <br> is safer than period (which can be in href)
                        let type_end = after_type
                            .find("<br>")
                            .or_else(|| after_type.find('\n'))
                            .or_else(|| after_type.find("<p"))
                            .unwrap_or_else(|| {
                                // Fallback: take until period or 200 chars
                                after_type.find('.').unwrap_or_else(|| {
                                    after_type.char_indices()
                                        .nth(200)
                                        .map(|(idx, _)| idx)
                                        .unwrap_or(after_type.len())
                                })
                            });
                        let type_text = &after_type[..type_end];
                        // Clean HTML and remove trailing period
                        let cleaned = strip_html_tags(type_text).trim().trim_end_matches('.').trim().to_string();
                        if !cleaned.is_empty() {
                            Some(cleaned)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    parameters.push(MethodParameter {
                        name: param_name,
                        param_type,
                        is_optional,
                    });
                }
            }

            // Move search position forward safely (using char boundaries)
            let skip_chars = 50;  // Skip approximately 50 characters
            let new_pos = after_params[search_from + rubric_start..]
                .char_indices()
                .nth(skip_chars)
                .map(|(idx, _)| search_from + rubric_start + idx)
                .unwrap_or(after_params.len());
            search_from = new_pos;

            // Prevent infinite loop
            if search_from >= after_params.len() {
                break;
            }
        }
    }

    parameters
}

/// Strips HTML tags from text
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
    fn test_parse_st_snippet() {
        let content = r#"{1,
{2,
{"",1,0,"",""},
{0,
{"ru",0,0,"","Функция <?>()
Возврат ;
КонецФункции "}
},
{0,
{"en",0,0,"","Function <?>()
Return ;
EndFunction"}
}
}
}"#;

        let (snippet_ru, snippet_en, russian, english) = parse_st_snippet(content).unwrap();

        assert_eq!(russian, "Функция");
        assert_eq!(english, "Function");
        assert!(snippet_ru.contains("Функция <?>()"));
        assert!(snippet_en.contains("Function <?>()"));
    }
}
