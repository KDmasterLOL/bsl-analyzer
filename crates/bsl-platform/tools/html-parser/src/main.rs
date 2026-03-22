//! HTML parser for extracting BSL platform documentation.
//!
//! Parses extracted .hbk files and generates JSON with:
//! - BSL keywords from shlang_ru.hbk
//! - Platform types and methods from shcntx_ru.hbk

mod scraper_parser;

use anyhow::{Context, Result};
use scraper_parser::{CodeExample, ParamDescription}; // Reuse from scraper_parser
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct PlatformData {
    keywords: Vec<KeywordInfo>,
    types: Vec<TypeInfo>,
    methods: Vec<MethodInfo>,
    global_functions: Vec<GlobalFunctionInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct KeywordInfo {
    /// Russian name (e.g., "Для", "ВызватьИсключение")
    keyword_ru: String,
    /// English name (e.g., "For", "Raise")
    keyword_en: String,
    /// Full documentation
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<KeywordDocumentation>,
}

#[derive(Debug, Serialize, Deserialize)]
struct KeywordDocumentation {
    syntax: String,
    description: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    param_descriptions: Vec<ParamDescription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_version: Option<String>,
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
    /// Full documentation
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<MethodDocumentation>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GlobalFunctionInfo {
    /// Function ID
    id: u32,
    /// Russian function name (e.g., "НачатьТранзакцию")
    name: String,
    /// English function name (e.g., "BeginTransaction")
    english_name: String,
    /// Return type (e.g., "Число", "Неопределено")
    #[serde(skip_serializing_if = "Option::is_none")]
    return_type: Option<String>,
    /// Function parameters
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    parameters: Vec<MethodParameter>,
    /// Minimum version (e.g., "8.0")
    #[serde(skip_serializing_if = "Option::is_none")]
    min_version: Option<String>,
    /// Context availability
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<ContextAvailability>,
    /// Full documentation
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<MethodDocumentation>,
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

#[derive(Debug, Serialize, Deserialize)]
struct MethodDocumentation {
    /// Syntax description
    syntax: String,
    /// Detailed description
    description: String,
    /// Parameter descriptions
    param_descriptions: Vec<ParamDescription>,
    /// Code examples
    examples: Vec<CodeExample>,
    /// Notes
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    /// See also links
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    see_also: Vec<String>,
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

    // Parse keywords from shlang (non-fatal if this fails)
    let keywords = match parse_shlang_keywords(&shlang_dir) {
        Ok(kw) => {
            println!("Parsed {} keywords", kw.len());
            kw
        }
        Err(e) => {
            eprintln!("Warning: Failed to parse shlang keywords: {}", e);
            eprintln!("Continuing with empty keyword list...");
            Vec::new()
        }
    };

    // Parse platform types and methods from shcntx
    let (types, methods, global_functions) = parse_shcntx_data(&shcntx_dir)
        .context("Failed to parse shcntx data")?;

    println!("Parsed {} types, {} methods, and {} global functions",
             types.len(), methods.len(), global_functions.len());

    // Generate JSON
    let platform_data = PlatformData {
        keywords,
        types,
        methods,
        global_functions,
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
    let mut st_file_count = 0;
    let mut html_found_count = 0;

    // Find all .st files (snippet templates)
    for entry in fs::read_dir(shlang_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("st") {
            st_file_count += 1;
            if let Some(keyword) = parse_keyword_file(&path)? {
                html_found_count += 1;
                keywords.push(keyword);
            }
        }
    }

    println!("Found {} .st files, {} with HTML documentation", st_file_count, html_found_count);

    Ok(keywords)
}

/// Parses a single keyword file (.st + corresponding HTML file).
fn parse_keyword_file(st_path: &Path) -> Result<Option<KeywordInfo>> {
    // Load corresponding HTML documentation (same name without .st extension)
    let html_path = st_path.with_extension("");
    if !html_path.exists() {
        // No HTML file - skip this keyword
        return Ok(None);
    }

    let html_content = fs::read_to_string(&html_path)?;

    // Use scraper parser to extract full documentation
    if let Some(keyword_doc) = scraper_parser::parse_keyword_html(&html_content) {
        // Convert scraper_parser::KeywordDocumentation to our KeywordDocumentation
        let documentation = KeywordDocumentation {
            syntax: keyword_doc.syntax,
            description: keyword_doc.description,
            param_descriptions: keyword_doc.params,
            min_version: keyword_doc.min_version,
        };

        Ok(Some(KeywordInfo {
            keyword_ru: keyword_doc.keyword_ru,
            keyword_en: keyword_doc.keyword_en,
            documentation: Some(documentation),
        }))
    } else {
        // Failed to parse - skip
        Ok(None)
    }
}

/// Parses shcntx directory for platform types, methods, and global functions.
fn parse_shcntx_data(shcntx_dir: &Path) -> Result<(Vec<TypeInfo>, Vec<MethodInfo>, Vec<GlobalFunctionInfo>)> {
    let mut types = Vec::new();
    let mut methods = Vec::new();
    let mut global_functions = Vec::new();
    let mut method_id_counter = 0u32;
    let mut function_id_counter = 0u32;

    let objects_dir = shcntx_dir.join("objects");
    if !objects_dir.exists() {
        return Ok((types, methods, global_functions));
    }

    // Check for "Global context" directory
    let global_context_dir = objects_dir.join("Global context");
    if global_context_dir.exists() {
        println!("Parsing Global context directory...");
        global_functions = parse_global_functions(&global_context_dir, &mut function_id_counter)?;
    }

    // Iterate through catalog directories
    for catalog_entry in fs::read_dir(&objects_dir)? {
        let catalog_entry = catalog_entry?;
        let catalog_path = catalog_entry.path();

        if !catalog_path.is_dir() {
            continue;
        }

        // Skip "Global context" directory (already processed)
        if catalog_path.file_name().unwrap().to_string_lossy() == "Global context" {
            continue;
        }

        // Recursively parse this catalog
        parse_catalog_directory(&catalog_path, &mut types, &mut methods, &mut method_id_counter)?;
    }

    Ok((types, methods, global_functions))
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

                    // Extract additional information using scraper
                    let min_version = scraper_parser::extract_version(&html_content);
                    let context = scraper_parser::extract_context(&html_content);

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

                            // Extract additional information using scraper
                            let return_type = scraper_parser::extract_return_type(&html_content);
                            let parameters = scraper_parser::extract_parameters(&html_content);
                            let min_version = scraper_parser::extract_version(&html_content);
                            let context = scraper_parser::extract_context(&html_content);

                            // Extract full documentation
                            let documentation = extract_method_documentation(&html_content);

                            return Ok(Some(MethodInfo {
                                id,
                                type_name: type_name.to_string(),
                                name: russian_method,
                                english_name: english_method,
                                return_type,
                                parameters,
                                min_version,
                                context,
                                documentation,
                            }));
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

/// Extracts full method documentation from HTML content
fn extract_method_documentation(html_content: &str) -> Option<MethodDocumentation> {
    let syntax = scraper_parser::extract_syntax(html_content).unwrap_or_default();
    let description = scraper_parser::extract_description(html_content)?;

    let param_descriptions_raw = scraper_parser::extract_parameter_descriptions(html_content);
    let param_descriptions = param_descriptions_raw
        .into_iter()
        .map(|p| ParamDescription {
            name: p.name,
            description: p.description,
            default_value: p.default_value,
        })
        .collect();

    let examples = scraper_parser::extract_examples(html_content);

    let notes = scraper_parser::extract_notes(html_content);

    // Return documentation even if description is empty string (shouldn't happen, but just in case)
    Some(MethodDocumentation {
        syntax,
        description,
        param_descriptions,
        examples,
        notes,
        see_also: scraper_parser::extract_see_also(html_content),
    })
}

/// Parses global functions from "Global context" directory.
fn parse_global_functions(
    global_context_dir: &Path,
    function_id_counter: &mut u32,
) -> Result<Vec<GlobalFunctionInfo>> {
    let mut functions = Vec::new();

    let methods_dir = global_context_dir.join("methods");
    if !methods_dir.exists() {
        return Ok(functions);
    }

    // Iterate through catalog directories in methods/
    for catalog_entry in fs::read_dir(&methods_dir)? {
        let catalog_entry = catalog_entry?;
        let catalog_path = catalog_entry.path();

        if !catalog_path.is_dir() {
            continue;
        }

        let dir_name = catalog_path.file_name().unwrap().to_string_lossy().to_string();

        // Only process catalog directories
        if !dir_name.starts_with("catalog") {
            continue;
        }

        // Parse all function HTML files in this catalog
        for function_entry in fs::read_dir(&catalog_path)? {
            let function_entry = function_entry?;
            let function_path = function_entry.path();

            // Only process .html files
            if function_path.extension().and_then(|s| s.to_str()) != Some("html") {
                continue;
            }

            if let Some(function_info) = parse_global_function_html(&function_path, function_id_counter)? {
                functions.push(function_info);
            }
        }
    }

    Ok(functions)
}

/// Parses global function HTML file to extract function information.
///
/// Expected title format: "Глобальный контекст.НачатьТранзакцию (Global context.BeginTransaction)"
fn parse_global_function_html(
    html_path: &Path,
    function_id_counter: &mut u32,
) -> Result<Option<GlobalFunctionInfo>> {
    let html_content = fs::read_to_string(html_path)?;

    // Extract function names from <h1 class="V8SH_pagetitle">Глобальный контекст.Формат (Global context.Format)</h1>
    if let Some(title_start) = html_content.find(r#"<h1 class="V8SH_pagetitle">"#) {
        let after_title = &html_content[title_start + r#"<h1 class="V8SH_pagetitle">"#.len()..];
        if let Some(title_end) = after_title.find("</h1>") {
            let title = &after_title[..title_end];

            // Parse "Глобальный контекст.Формат (Global context.Format)" format
            // Extract function names (after "Глобальный контекст." and "Global context.")

            // Find the dot after "Глобальный контекст"
            if let Some(first_dot) = title.find('.') {
                let after_first_dot = &title[first_dot + 1..];

                // Russian function name (before space and opening paren)
                let russian_function = if let Some(space_pos) = after_first_dot.find(" (") {
                    after_first_dot[..space_pos].trim().to_string()
                } else {
                    return Ok(None);
                };

                // English function name (inside parentheses, after "Global context.")
                if let Some(paren_start) = title.find("Global context.") {
                    let after_global_context = &title[paren_start + "Global context.".len()..];
                    if let Some(paren_end) = after_global_context.find(')') {
                        let english_function = after_global_context[..paren_end].trim().to_string();

                        let id = *function_id_counter;
                        *function_id_counter += 1;

                        // Extract additional information using scraper
                        let return_type = scraper_parser::extract_return_type(&html_content);
                        let parameters = scraper_parser::extract_parameters(&html_content);
                        let min_version = scraper_parser::extract_version(&html_content);
                        let context = scraper_parser::extract_context(&html_content);

                        // Extract full documentation
                        let documentation = extract_method_documentation(&html_content);

                        println!("  Found global function: {} / {}", russian_function, english_function);

                        return Ok(Some(GlobalFunctionInfo {
                            id,
                            name: russian_function,
                            english_name: english_function,
                            return_type,
                            parameters,
                            min_version,
                            context,
                            documentation,
                        }));
                    }
                }
            }
        }
    }

    Ok(None)
}
