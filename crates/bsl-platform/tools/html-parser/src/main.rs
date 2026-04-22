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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    constructors: Vec<ConstructorInfo>,
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

/// Constructor of a platform type (e.g. `Новый Массив(<КоличествоЭлементов>)`).
///
/// BSL types may expose multiple constructor overloads; each HBK
/// `ctors/ctor{N}.html` corresponds to one overload. The integer `N` from the
/// filename is used as `id` so the JSON output keeps a stable identity across
/// regenerations.
#[derive(Debug, Serialize, Deserialize)]
struct ConstructorInfo {
    /// Stable numeric id derived from `ctor{N}.html`.
    id: u32,
    /// English name of the enclosing platform type (e.g. "Array"), same shape
    /// as `MethodInfo::type_name`. Runtime code uses this to index constructors
    /// by type.
    type_name: String,
    /// Human-readable overload name from `<p class="V8SH_heading">`
    /// (e.g. "По количеству элементов"). `None` only when the HBK page is
    /// malformed; normal pages always have it.
    #[serde(skip_serializing_if = "Option::is_none")]
    variant_name: Option<String>,
    /// Constructor parameters; same extractor as regular methods.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    parameters: Vec<MethodParameter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<ContextAvailability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<ConstructorDocumentation>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConstructorDocumentation {
    syntax: String,
    description: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    param_descriptions: Vec<ParamDescription>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    examples: Vec<CodeExample>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    see_also: Vec<String>,
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
    let (types, methods, global_functions, constructors) = parse_shcntx_data(&shcntx_dir)
        .context("Failed to parse shcntx data")?;

    println!(
        "Parsed {} types, {} methods, {} global functions, and {} constructors",
        types.len(), methods.len(), global_functions.len(), constructors.len()
    );

    // Generate JSON
    let platform_data = PlatformData {
        keywords,
        types,
        methods,
        global_functions,
        constructors,
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

/// Parses shcntx directory for platform types, methods, global functions, and constructors.
fn parse_shcntx_data(
    shcntx_dir: &Path,
) -> Result<(Vec<TypeInfo>, Vec<MethodInfo>, Vec<GlobalFunctionInfo>, Vec<ConstructorInfo>)> {
    let mut types = Vec::new();
    let mut methods = Vec::new();
    let mut global_functions = Vec::new();
    let mut constructors = Vec::new();
    let mut method_id_counter = 0u32;
    let mut function_id_counter = 0u32;
    // Constructors use a monotonic counter instead of the `ctor{N}.html`
    // filename digit: not every HBK page is named `ctorN.html` — many types
    // ship `ctor_Auto.html`, which would collapse to id=0 and collide with
    // every other non-numeric page in `constructor_docs_by_id`.
    let mut ctor_id_counter = 0u32;

    let objects_dir = shcntx_dir.join("objects");
    if !objects_dir.exists() {
        return Ok((types, methods, global_functions, constructors));
    }

    // Check for "Global context" directory
    let global_context_dir = objects_dir.join("Global context");
    if global_context_dir.exists() {
        println!("Parsing Global context directory...");
        global_functions = parse_global_functions(&global_context_dir, &mut function_id_counter)?;
    }

    // Iterate through catalog directories.
    // Sort entries by name so constructor ids (and the JSON diff) stay stable
    // across regenerations on different filesystems / OSes.
    let mut catalog_entries: Vec<_> = fs::read_dir(&objects_dir)?.collect::<Result<_, _>>()?;
    catalog_entries.sort_by_key(|e| e.file_name());
    for catalog_entry in catalog_entries {
        let catalog_path = catalog_entry.path();

        if !catalog_path.is_dir() {
            continue;
        }

        // Skip "Global context" directory (already processed)
        if catalog_path.file_name().unwrap().to_string_lossy() == "Global context" {
            continue;
        }

        // Recursively parse this catalog
        parse_catalog_directory(
            &catalog_path,
            &mut types,
            &mut methods,
            &mut constructors,
            &mut method_id_counter,
            &mut ctor_id_counter,
        )?;
    }

    Ok((types, methods, global_functions, constructors))
}

/// Recursively parses a catalog directory for types, methods, and constructors.
fn parse_catalog_directory(
    catalog_path: &Path,
    types: &mut Vec<TypeInfo>,
    methods: &mut Vec<MethodInfo>,
    constructors: &mut Vec<ConstructorInfo>,
    method_id_counter: &mut u32,
    ctor_id_counter: &mut u32,
) -> Result<()> {
    // Iterate through entries in this catalog.
    // Sort so `fs::read_dir`'s platform-dependent order doesn't leak into
    // the generated JSON (constructor ids included).
    let mut entries: Vec<_> = fs::read_dir(catalog_path)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let entry_path = entry.path();

        // Skip non-directories
        if !entry_path.is_dir() {
            continue;
        }

        let dir_name = entry_path.file_name().unwrap().to_string_lossy().to_string();

        // If it's a nested catalog, recurse into it
        if dir_name.starts_with("catalog") {
            parse_catalog_directory(
                &entry_path,
                types,
                methods,
                constructors,
                method_id_counter,
                ctor_id_counter,
            )?;
            continue;
        }

        // Otherwise it's a type directory - find corresponding .html file
        let type_html = entry_path.with_extension("html");
        if type_html.exists() {
            if let Some(type_info) = parse_type_html(&type_html, &entry_path)? {
                println!("Found type: {} / {}", type_info.name, type_info.english_name);

                // Parse methods for this type
                let type_methods =
                    parse_type_methods(&entry_path, &type_info.english_name, method_id_counter)?;
                methods.extend(type_methods);

                // Parse constructors for this type (not all types have them —
                // missing ctors/ directory yields an empty Vec, not an error).
                let type_ctors = parse_type_constructors(
                    &entry_path,
                    &type_info.english_name,
                    ctor_id_counter,
                )?;
                constructors.extend(type_ctors);

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

/// Parses `{type_dir}/ctors/*.html` into `ConstructorInfo`s.
///
/// Absence of the `ctors/` directory is normal (not every platform type has
/// a constructor) and must not propagate as an error.
///
/// `ctor_id_counter` feeds a monotonic id for every parsed overload. We
/// intentionally don't derive ids from the `ctor{N}.html` filename number:
/// many HBK pages are named `ctor_Auto.html` (no digit), which would all
/// collapse to id=0 and collide in `constructor_docs_by_id`. Filenames are
/// sorted before reading so ids stay stable across platforms / regenerations.
fn parse_type_constructors(
    type_dir: &Path,
    type_name_en: &str,
    ctor_id_counter: &mut u32,
) -> Result<Vec<ConstructorInfo>> {
    let ctors_dir = type_dir.join("ctors");
    if !ctors_dir.exists() {
        return Ok(Vec::new());
    }

    let mut html_paths: Vec<std::path::PathBuf> = Vec::new();
    for entry in fs::read_dir(&ctors_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("html") {
            html_paths.push(path);
        }
    }
    html_paths.sort();

    let mut ctors = Vec::new();
    for path in html_paths {
        if let Some(info) = parse_constructor_html(&path, type_name_en, ctor_id_counter)? {
            ctors.push(info);
        }
    }
    Ok(ctors)
}

/// Parses a single ctor HTML file (`ctor13.html`, `ctor_Auto.html`, …).
///
/// Returns `None` when the file does not look like a constructor page at all
/// (no extractable variant name and no `<h1>` fallback).
fn parse_constructor_html(
    html_path: &Path,
    type_name_en: &str,
    ctor_id_counter: &mut u32,
) -> Result<Option<ConstructorInfo>> {
    let html_content = fs::read_to_string(html_path)?;

    let variant_name = scraper_parser::extract_constructor_variant_name(&html_content);
    if variant_name.is_none() {
        return Ok(None);
    }

    let id = *ctor_id_counter;
    *ctor_id_counter += 1;

    let parameters = scraper_parser::extract_parameters(&html_content);
    let min_version = scraper_parser::extract_version(&html_content);
    let context = scraper_parser::extract_context(&html_content);
    let documentation = extract_constructor_documentation(&html_content);

    Ok(Some(ConstructorInfo {
        id,
        type_name: type_name_en.to_string(),
        variant_name,
        parameters,
        min_version,
        context,
        documentation,
    }))
}

/// Collects the full documentation block of a constructor. Mirrors
/// [`extract_method_documentation`] — same extractors, different DTO.
fn extract_constructor_documentation(html_content: &str) -> Option<ConstructorDocumentation> {
    let syntax = scraper_parser::extract_syntax(html_content).unwrap_or_default();
    let description = scraper_parser::extract_description(html_content).unwrap_or_default();

    let param_descriptions = scraper_parser::extract_parameter_descriptions(html_content)
        .into_iter()
        .map(|p| ParamDescription {
            name: p.name,
            description: p.description,
            default_value: p.default_value,
        })
        .collect();

    let examples = scraper_parser::extract_examples(html_content);
    let notes = scraper_parser::extract_notes(html_content);
    let see_also = scraper_parser::extract_see_also(html_content);

    // Keep the doc block even when description is empty — constructors sometimes
    // only carry a syntax line (e.g. `Новый Структура()`). Empty-everything
    // still yields a doc so downstream can inspect syntax.
    Some(ConstructorDocumentation {
        syntax,
        description,
        param_descriptions,
        examples,
        notes,
        see_also,
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

// -----------------------------------------------------------------------------
// Tests — synthetic HTML only.
//
// We do NOT check vendor 1C HBK HTML into the repo (licensing). The fixtures
// here are minimal hand-rolled strings that reproduce just the CSS structure
// (`V8SH_*` classes, rubric shape) the real extractors depend on. End-to-end
// coverage against real HBK happens via `BSL_PLATFORM_PATH` on regeneration.
// -----------------------------------------------------------------------------
#[cfg(test)]
mod ctor_tests {
    use super::*;
    use std::path::PathBuf;

    fn unique_tmpdir(tag: &str) -> PathBuf {
        let pid = std::process::id();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("html-parser-ctor-{tag}-{pid}-{ts}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create tmp dir");
        dir
    }

    #[test]
    fn variant_name_prefers_heading_over_h1() {
        let html = r#"<html><body>
            <h1 class="V8SH_pagetitle">ФейкТип.H1-вариант</h1>
            <p class="V8SH_heading">Heading-вариант</p>
            </body></html>"#;
        assert_eq!(
            scraper_parser::extract_constructor_variant_name(html),
            Some("Heading-вариант".to_string())
        );
    }

    #[test]
    fn variant_name_falls_back_to_h1_after_dot() {
        let html = r#"<html><body>
            <h1 class="V8SH_pagetitle">ФейкТип.По ключам</h1>
            </body></html>"#;
        assert_eq!(
            scraper_parser::extract_constructor_variant_name(html),
            Some("По ключам".to_string())
        );
    }

    #[test]
    fn variant_name_returns_none_when_both_selectors_miss() {
        let html = "<html><body><p>unrelated</p></body></html>";
        assert!(scraper_parser::extract_constructor_variant_name(html).is_none());
    }

    /// Synthetic constructor HTML shaped like real HBK pages:
    /// `<h1>` with `РусТип.Вариант`, `V8SH_heading` with the canonical variant,
    /// chapter-blocks for «Синтаксис/Параметры/Описание/Использование в версии».
    fn synthetic_ctor_html(variant: &str, param_block: &str, description: &str) -> String {
        format!(
            r#"<html><body>
<h1 class="V8SH_pagetitle">ФейкТип.{variant}</h1>
<p class="V8SH_title">ФейкТип (FakeType)</p>
<p class="V8SH_heading">{variant}</p>
<div class="__SINCE_SHOW_STYLE__"><p class="not_used">Доступен, начиная с версии 8.3.10.</p></div>
<p class="V8SH_chapter">Синтаксис:</p>Новый ФейкТип(…)
<p class="V8SH_chapter">Параметры:</p>{param_block}
<p class="V8SH_chapter">Описание:</p><p>{description}</p>
<p class="V8SH_chapter">Использование в версии:</p><p class="V8SH_versionInfo">Доступен, начиная с версии 8.3.10.</p>
</body></html>"#
        )
    }

    #[test]
    fn parse_constructor_html_extracts_single_param() {
        let html = synthetic_ctor_html(
            "С одним параметром",
            r#"<div class="V8SH_rubric"> <p>&lt;Значение&gt; (необязательный)</div>Тип: <a href="x">Число</a>. <br>Описание значения.<br>"#,
            "Пример конструктора с одним параметром.",
        );

        let dir = unique_tmpdir("single");
        let path = dir.join("ctor42.html");
        fs::write(&path, &html).unwrap();

        let mut counter: u32 = 100;
        let info = parse_constructor_html(&path, "FakeType", &mut counter)
            .unwrap()
            .expect("got ctor info");
        assert_eq!(info.id, 100, "monotonic counter must hand out the current value");
        assert_eq!(counter, 101, "counter must advance after a successful parse");
        assert_eq!(info.type_name, "FakeType");
        assert_eq!(info.variant_name.as_deref(), Some("С одним параметром"));
        assert_eq!(info.parameters.len(), 1);
        assert_eq!(info.parameters[0].name, "Значение");
        assert_eq!(info.parameters[0].param_type.as_deref(), Some("Число"));
        assert!(info.parameters[0].is_optional);

        let docs = info.documentation.expect("docs present");
        assert!(docs.syntax.contains("Новый ФейкТип"));
        assert!(docs.description.contains("Пример конструктора"));
        assert_eq!(docs.param_descriptions.len(), 1);
        assert_eq!(docs.param_descriptions[0].name, "Значение");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_constructor_html_returns_none_without_variant_marker() {
        // No V8SH_heading and no dot in <h1> → unparseable.
        let html = "<html><body><h1 class=\"V8SH_pagetitle\">Plain</h1></body></html>";
        let dir = unique_tmpdir("no-variant");
        let path = dir.join("ctor7.html");
        fs::write(&path, html).unwrap();
        let mut counter: u32 = 0;
        assert!(parse_constructor_html(&path, "FakeType", &mut counter).unwrap().is_none());
        assert_eq!(counter, 0, "counter must not advance when parse returns None");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_type_constructors_returns_empty_when_dir_missing() {
        let dir = unique_tmpdir("no-ctors");
        // `dir/ctors` intentionally does not exist.
        let mut counter: u32 = 0;
        let got = parse_type_constructors(&dir, "FakeType", &mut counter).unwrap();
        assert!(got.is_empty());
        assert_eq!(counter, 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_type_constructors_picks_up_multi_overload_with_stable_order() {
        let type_dir = unique_tmpdir("multi");
        let ctors_dir = type_dir.join("ctors");
        fs::create_dir_all(&ctors_dir).unwrap();

        // Filenames mimic real HBK mix: numeric + `_Auto` variants. Sorted
        // alphabetically: ctor1, ctor2, ctor_Auto. The test asserts the
        // monotonic counter follows that deterministic order regardless of
        // the filesystem's `read_dir` order.
        fs::write(
            ctors_dir.join("ctor2.html"),
            synthetic_ctor_html(
                "По значению",
                r#"<div class="V8SH_rubric"> <p>&lt;Значение&gt;</div>Тип: <a href="x">Строка</a>. <br>"#,
                "По значению.",
            ),
        )
        .unwrap();
        fs::write(
            ctors_dir.join("ctor1.html"),
            synthetic_ctor_html("Пустой", "", "Без параметров."),
        )
        .unwrap();
        fs::write(
            ctors_dir.join("ctor_Auto.html"),
            synthetic_ctor_html("По умолчанию", "", "Автоконструктор."),
        )
        .unwrap();
        // Non-html file must be ignored:
        fs::write(ctors_dir.join("readme.txt"), "noise").unwrap();

        let mut counter: u32 = 500;
        let got = parse_type_constructors(&type_dir, "FakeType", &mut counter).unwrap();
        assert_eq!(got.len(), 3);
        // Stable sort by filename: ctor1, ctor2, ctor_Auto → ids 500, 501, 502.
        assert_eq!(got[0].id, 500);
        assert_eq!(got[0].variant_name.as_deref(), Some("Пустой"));
        assert_eq!(got[1].id, 501);
        assert_eq!(got[1].variant_name.as_deref(), Some("По значению"));
        assert_eq!(got[2].id, 502);
        assert_eq!(got[2].variant_name.as_deref(), Some("По умолчанию"));
        assert_eq!(counter, 503, "counter must advance once per parsed ctor");

        fs::remove_dir_all(&type_dir).ok();
    }
}
