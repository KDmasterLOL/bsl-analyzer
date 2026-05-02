//! HTML parser for extracting BSL platform documentation.
//!
//! Parses extracted .hbk files and generates JSON with:
//! - BSL keywords from shlang_ru.hbk
//! - Platform types and methods from shcntx_ru.hbk

mod scraper_parser;

use anyhow::{Context, Result};
use scraper_parser::{CodeExample, ParamDescription, ParameterVariant}; // Reuse from scraper_parser
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
    /// Properties of platform types (e.g. `Запрос.Параметры`, `Запрос.Текст`).
    ///
    /// Top-level vec mirroring `methods`: each entry carries `type_name` (the
    /// English name of the enclosing type) so the runtime can build a bilingual
    /// index keyed by `(type_name_en, property_name_{ru,en})` without walking
    /// a nested `TypeInfo.properties` path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    properties: Vec<PropertyInfo>,
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
    /// Method parameters — flattened union across all syntax variants
    /// (legacy field used by hover/completion). Multi-overload methods
    /// also populate `variants`; arity / type checks must consult
    /// `variants`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    parameters: Vec<MethodParameter>,
    /// Per-variant parameter lists when the HBK page declares multiple
    /// `<p class="V8SH_chapter">Вариант синтаксиса: …</p>` sections
    /// (e.g. `ЧтениеXML.ПолучитьАтрибут`, `ТаблицаЗначений.Скопировать`).
    /// Empty for single-overload methods — `parameters` already covers
    /// them.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    variants: Vec<MethodVariantInfo>,
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

/// One syntax variant of a multi-overload platform method.
///
/// Mirrors `crates/bsl-platform/src/types.rs::MethodVariant`.
#[derive(Debug, Serialize, Deserialize)]
struct MethodVariantInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    variant_name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    parameters: Vec<MethodParameter>,
}

impl From<ParameterVariant> for MethodVariantInfo {
    fn from(v: ParameterVariant) -> Self {
        Self { variant_name: v.variant_name, parameters: v.parameters }
    }
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
    /// Function parameters — flattened union across all syntax variants
    /// (legacy field used by hover/completion). Multi-overload functions
    /// also populate `variants`; arity / type checks must consult
    /// `variants`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    parameters: Vec<MethodParameter>,
    /// Per-variant parameter lists when the HBK page declares multiple
    /// `<p class="V8SH_chapter">Вариант синтаксиса: …</p>` sections.
    /// Empty for the vast majority of single-overload functions.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    variants: Vec<GlobalFunctionVariantInfo>,
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

/// One syntax variant of a multi-overload global function.
///
/// Mirrors `crates/bsl-platform/src/types.rs::GlobalFunctionVariant`.
#[derive(Debug, Serialize, Deserialize)]
struct GlobalFunctionVariantInfo {
    /// Variant name from the `Вариант синтаксиса:` chapter (e.g.
    /// `"По идентификатору"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    variant_name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    parameters: Vec<MethodParameter>,
}

impl From<ParameterVariant> for GlobalFunctionVariantInfo {
    fn from(v: ParameterVariant) -> Self {
        Self { variant_name: v.variant_name, parameters: v.parameters }
    }
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

/// Property of a platform type (e.g. `Запрос.Параметры`, `Запрос.Текст`).
///
/// Parsed from `{type_dir}/properties/<PropName>.html`. Carries the list of
/// declared value types (single entry for scalar properties, multiple for
/// union declarations like `МенеджерВременныхТаблиц, Неопределено`) and a
/// `is_readonly` flag derived from the `Использование:` chapter.
#[derive(Debug, Serialize, Deserialize)]
struct PropertyInfo {
    /// Stable numeric id assigned at parse time.
    id: u32,
    /// English name of the enclosing platform type (e.g. "Query"), same
    /// shape as `MethodInfo::type_name`.
    type_name: String,
    /// Russian property name (e.g. "Параметры").
    name: String,
    /// English property name (e.g. "Parameters").
    english_name: String,
    /// Declared value types in source order. Single-element for scalars,
    /// multi-element for union properties. Empty when the HBK page omits
    /// the `Тип:` marker (free-prose description).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    property_types: Vec<String>,
    /// `true` when the `Использование:` chapter states "Только чтение";
    /// `false` for read-write properties (the page says "Чтение и запись").
    is_readonly: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<ContextAvailability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<PropertyDocumentation>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PropertyDocumentation {
    /// Free-prose description (text after the `Описание:` chapter with the
    /// `Тип:` segment trimmed — the caller already has `property_types`).
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    see_also: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MethodParameter {
    /// Parameter name (e.g., "Значение")
    name: String,
    /// Parameter type (e.g., "Число", "Произвольный")
    #[serde(skip_serializing_if = "Option::is_none")]
    param_type: Option<String>,
    /// Is parameter optional
    is_optional: bool,
    /// Is the parameter the unbounded-variadic tail of the call. Lifted
    /// from the page's `Синтаксис:` chapter when it contains the
    /// separate-bracket ellipsis idiom `<X1>,...,<XN>` (substring
    /// `>,...,<` after HTML decoding). Defaults to `false`; the JSON
    /// omits the field when not set so the on-disk diff stays minimal
    /// for non-variadic functions.
    #[serde(default, skip_serializing_if = "is_false")]
    is_variadic: bool,
}

/// `serde` skip helper — see `MethodParameter::is_variadic`.
fn is_false(b: &bool) -> bool {
    !*b
}

/// If the page's `Синтаксис:` chapter encodes an unbounded variadic
/// tail with separate name brackets (`<X1>,...,<XN>` shape), set
/// `is_variadic = true` on the last parameter. Otherwise leave the list
/// untouched.
///
/// **Detection rule** — the substring `>,...,<` after HTML decoding.
/// Distinguishes:
/// - `<X1>,...,<XN>` (Мин, Макс, ПродолжитьВызов, Новый Массив, two
///   COMSafeArray ctors) — separate name groups, contains `>,...,<` →
///   mark.
/// - `<X1,...,XN>` (СтрШаблон, ФорматированнаяСтрока ctor 173) — single
///   name group, NO `>,...,<` substring → skip. The capped name idiom
///   `<word>N-<word>M` in `hir-ty/src/builtin.rs` handles those.
///
/// **Multi-variant assumption.** For pages with several `Вариант
/// синтаксиса:` chapters this looks at the page-level Syntax chapter
/// only. The current BSL platform corpus has zero multi-variant
/// methods/globals with the `,...,` shape (audit 2026-04-29) so this
/// is exact today; if a future regen produces such a case, refine into
/// per-variant detection.
fn mark_trailing_variadic_from_syntax(parameters: &mut [MethodParameter], html: &str) {
    if parameters.is_empty() {
        return;
    }
    let Some(syntax) = scraper_parser::extract_syntax(html) else {
        return;
    };
    if syntax.contains(">,...,<") {
        if let Some(last) = parameters.last_mut() {
            last.is_variadic = true;
        }
    }
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
    let (types, methods, global_functions, constructors, properties) =
        parse_shcntx_data(&shcntx_dir).context("Failed to parse shcntx data")?;

    println!(
        "Parsed {} types, {} methods, {} global functions, {} constructors, and {} properties",
        types.len(),
        methods.len(),
        global_functions.len(),
        constructors.len(),
        properties.len()
    );

    // Generate JSON
    let platform_data = PlatformData {
        keywords,
        types,
        methods,
        global_functions,
        constructors,
        properties,
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

/// Parses shcntx directory for platform types, methods, global functions, constructors,
/// and properties.
fn parse_shcntx_data(
    shcntx_dir: &Path,
) -> Result<(
    Vec<TypeInfo>,
    Vec<MethodInfo>,
    Vec<GlobalFunctionInfo>,
    Vec<ConstructorInfo>,
    Vec<PropertyInfo>,
)> {
    let mut types = Vec::new();
    let mut methods = Vec::new();
    let mut global_functions = Vec::new();
    let mut constructors = Vec::new();
    let mut properties = Vec::new();
    let mut method_id_counter = 0u32;
    let mut function_id_counter = 0u32;
    // Constructors use a monotonic counter instead of the `ctor{N}.html`
    // filename digit: not every HBK page is named `ctorN.html` — many types
    // ship `ctor_Auto.html`, which would collapse to id=0 and collide with
    // every other non-numeric page in `constructor_docs_by_id`.
    let mut ctor_id_counter = 0u32;
    // Property ids follow the same monotonic strategy as constructors. Many
    // HBK pages are named `<Name><N>.html` where `<N>` is not unique across
    // different types (e.g. both `Query/properties/Parameters7503.html` and
    // an unrelated type could ship the same digit suffix), so a content-derived
    // id wouldn't round-trip.
    let mut prop_id_counter = 0u32;

    let objects_dir = shcntx_dir.join("objects");
    if !objects_dir.exists() {
        return Ok((types, methods, global_functions, constructors, properties));
    }

    // Check for "Global context" directory
    let global_context_dir = objects_dir.join("Global context");
    if global_context_dir.exists() {
        println!("Parsing Global context directory...");
        global_functions = parse_global_functions(&global_context_dir, &mut function_id_counter)?;

        // Global properties (e.g. `ОбработкаОшибок`, `Метаданные`, `Справочники`)
        // are top-level identifiers whose declared type is the foreign key into
        // the regular type/method index. Reuses `parse_type_properties`: the
        // `properties/` subdirectory has the same flat layout as for any other
        // type, and `parse_property_html` extracts the title shape
        // `Глобальный контекст.X (Global context.X)` via the same dot-split.
        let global_properties = parse_type_properties(
            &global_context_dir,
            "Global context",
            &mut prop_id_counter,
        )?;
        println!("Parsed {} global context properties", global_properties.len());
        properties.extend(global_properties);
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
            &mut properties,
            &mut method_id_counter,
            &mut ctor_id_counter,
            &mut prop_id_counter,
        )?;
    }

    Ok((types, methods, global_functions, constructors, properties))
}

/// Recursively parses a catalog directory for types, methods, constructors,
/// and properties.
#[allow(clippy::too_many_arguments)]
fn parse_catalog_directory(
    catalog_path: &Path,
    types: &mut Vec<TypeInfo>,
    methods: &mut Vec<MethodInfo>,
    constructors: &mut Vec<ConstructorInfo>,
    properties: &mut Vec<PropertyInfo>,
    method_id_counter: &mut u32,
    ctor_id_counter: &mut u32,
    prop_id_counter: &mut u32,
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
                properties,
                method_id_counter,
                ctor_id_counter,
                prop_id_counter,
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

                // Parse properties for this type. Same "missing directory is
                // normal" contract as ctors — many primitives / purely-method
                // types have no `properties/` at all.
                let type_properties = parse_type_properties(
                    &entry_path,
                    &type_info.english_name,
                    prop_id_counter,
                )?;
                properties.extend(type_properties);

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
                            let mut parameters = scraper_parser::extract_parameters(&html_content);
                            mark_trailing_variadic_from_syntax(&mut parameters, &html_content);
                            // Per-variant breakdown for multi-overload pages
                            // (e.g. `ЧтениеXML.ПолучитьАтрибут`,
                            // `ТаблицаЗначений.Скопировать`). Empty for
                            // single-overload methods — `parameters`
                            // already covers them.
                            let raw_variants =
                                scraper_parser::extract_parameter_variants(&html_content);
                            let mut variants: Vec<MethodVariantInfo> =
                                if raw_variants.len() > 1 {
                                    raw_variants.into_iter().map(Into::into).collect()
                                } else {
                                    Vec::new()
                                };
                            for variant in variants.iter_mut() {
                                mark_trailing_variadic_from_syntax(
                                    &mut variant.parameters,
                                    &html_content,
                                );
                            }
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
                                variants,
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

    let mut parameters = scraper_parser::extract_parameters(&html_content);
    mark_trailing_variadic_from_syntax(&mut parameters, &html_content);
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

/// Parses `{type_dir}/properties/*.html` into `PropertyInfo`s.
///
/// Mirrors the shape of [`parse_type_methods`] and [`parse_type_constructors`]:
/// absence of the `properties/` directory is normal (not every platform type
/// has declared properties — many primitives and collections only expose
/// methods) and must not propagate as an error. Files are sorted before reading
/// so ids stay stable across platforms / regenerations.
fn parse_type_properties(
    type_dir: &Path,
    type_name_en: &str,
    prop_id_counter: &mut u32,
) -> Result<Vec<PropertyInfo>> {
    let properties_dir = type_dir.join("properties");
    if !properties_dir.exists() {
        return Ok(Vec::new());
    }

    let mut html_paths: Vec<std::path::PathBuf> = Vec::new();
    for entry in fs::read_dir(&properties_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("html") {
            html_paths.push(path);
        }
    }
    html_paths.sort();

    let mut out = Vec::new();
    for path in html_paths {
        if let Some(info) = parse_property_html(&path, type_name_en, prop_id_counter)? {
            out.push(info);
        }
    }
    Ok(out)
}

/// Parses a single property HTML page.
///
/// Expected title shape: `<h1 class="V8SH_pagetitle">Запрос.Параметры (Query.Parameters)</h1>`.
/// Returns `None` when the file does not look like a property page
/// (missing or malformed `<h1>`).
fn parse_property_html(
    html_path: &Path,
    type_name_en: &str,
    prop_id_counter: &mut u32,
) -> Result<Option<PropertyInfo>> {
    let html_content = fs::read_to_string(html_path)?;

    // Title format:
    // - Scalar type: `Запрос.Параметры (Query.Parameters)` — 1 dot per side.
    // - Composite type: `РегистрСведенийНаборЗаписей.<Имя регистра сведений>.Отбор
    //   (InformationRegisterRecordSet.<Information register name>.Filter)` —
    //   2+ dots per side, with `<…>` placeholder segments inside.
    //
    // Use `rfind('.')` to anchor on the LAST dot — the property name is
    // always the rightmost segment, regardless of how many composite
    // qualifiers precede it. Splitting on the first dot would break
    // composite types: the property fragment would start with `<Имя…`
    // and the placeholder filter below would silently drop the entry.
    let Some(title_start) = html_content.find(r#"<h1 class="V8SH_pagetitle">"#) else {
        return Ok(None);
    };
    let after_title = &html_content[title_start + r#"<h1 class="V8SH_pagetitle">"#.len()..];
    let Some(title_end) = after_title.find("</h1>") else {
        return Ok(None);
    };
    let title = &after_title[..title_end];

    // Russian half: everything before the opening paren.
    let Some(paren_start) = title.find('(') else {
        return Ok(None);
    };
    let russian_half = title[..paren_start].trim_end();
    let Some(last_dot_ru) = russian_half.rfind('.') else {
        return Ok(None);
    };
    let russian_prop = russian_half[last_dot_ru + 1..].trim().to_string();

    // English half: between the opening paren and the matching close.
    let after_paren = &title[paren_start + 1..];
    let Some(paren_end) = after_paren.rfind(')') else {
        return Ok(None);
    };
    let english_half = after_paren[..paren_end].trim();
    let Some(last_dot_en) = english_half.rfind('.') else {
        return Ok(None);
    };
    let english_prop = english_half[last_dot_en + 1..].trim().to_string();

    if russian_prop.is_empty() || english_prop.is_empty() {
        return Ok(None);
    }

    // Skip HBK placeholder pages. Manager-style composite types
    // (`Structure.<Имя ключа>`, `CatalogManager.<Имя>`, `CatalogRef.<Имя>`)
    // ship generic "any user-defined key/member" pages whose title
    // contains literal `<…>` markers — HTML-escaped as `&lt;…&gt;`. The
    // method side already uses the `<Имя>` placeholder convention, and
    // the scalar property index matches on real member names, not the
    // placeholder itself. Filtering here keeps completion from
    // surfacing noise like `&lt;Имя ключа` as a suggestion.
    let looks_like_placeholder = |s: &str| s.starts_with('<') || s.starts_with("&lt;");
    if looks_like_placeholder(&russian_prop) || looks_like_placeholder(&english_prop) {
        return Ok(None);
    }

    let id = *prop_id_counter;
    *prop_id_counter += 1;

    let property_types = scraper_parser::extract_property_types(&html_content);
    let is_readonly = scraper_parser::extract_readonly(&html_content);
    let min_version = scraper_parser::extract_version(&html_content);
    let context = scraper_parser::extract_context(&html_content);
    let documentation = extract_property_documentation(&html_content);

    Ok(Some(PropertyInfo {
        id,
        type_name: type_name_en.to_string(),
        name: russian_prop,
        english_name: english_prop,
        property_types,
        is_readonly,
        min_version,
        context,
        documentation,
    }))
}

/// Collects the free-prose description + notes + see-also block of a
/// property page.
///
/// The `Описание:` chapter starts with `Тип: …` followed by the prose
/// narrative. `property_types` already captures the structured type
/// reference, so we strip the leading `Тип: …` sentence from the
/// description here — otherwise hover would duplicate the type string
/// between the `**Тип:**` line rendered from `property_types` and the
/// first sentence of the narrative. When the page carries only the
/// `Тип:` marker without prose, the stripped description becomes empty
/// and (combined with empty `notes` / `see_also`) suppresses the doc
/// block entirely.
fn extract_property_documentation(html_content: &str) -> Option<PropertyDocumentation> {
    let description = scraper_parser::extract_description(html_content)
        .map(|d| strip_leading_tip_prefix(&d))
        .unwrap_or_default();
    let notes = scraper_parser::extract_notes(html_content);
    let see_also = scraper_parser::extract_see_also(html_content);

    if description.is_empty() && notes.is_none() && see_also.is_empty() {
        return None;
    }

    Some(PropertyDocumentation { description, notes, see_also })
}

/// Drop the leading `Тип: … .` segment from a property description.
///
/// Example input: `"Тип: Структура. Содержит значения параметров."`
/// Returns:       `"Содержит значения параметров."`
///
/// Leaves non-`Тип:` descriptions unchanged, and is tolerant to missing
/// terminal period or trailing whitespace around the segment.
fn strip_leading_tip_prefix(desc: &str) -> String {
    let trimmed = desc.trim_start();
    let Some(after_label) = trimmed.strip_prefix("Тип:") else {
        return desc.to_string();
    };
    let rest = after_label.trim_start();
    // Look for the first `.` that terminates the type list; anything
    // past it is the actual narrative.
    match rest.find('.') {
        Some(idx) => rest[idx + 1..].trim_start().to_string(),
        None => String::new(),
    }
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
                        let mut parameters = scraper_parser::extract_parameters(&html_content);
                        mark_trailing_variadic_from_syntax(&mut parameters, &html_content);
                        // Per-variant breakdown for multi-overload pages
                        // (`ПодключитьВнешнююКомпоненту`, `Дата`,
                        // `ОткрытьФорму`, …). Empty when the page has a
                        // single anonymous variant — `parameters` already
                        // covers it.
                        let raw_variants =
                            scraper_parser::extract_parameter_variants(&html_content);
                        let mut variants: Vec<GlobalFunctionVariantInfo> =
                            if raw_variants.len() > 1 {
                                raw_variants.into_iter().map(Into::into).collect()
                            } else {
                                Vec::new()
                            };
                        for variant in variants.iter_mut() {
                            mark_trailing_variadic_from_syntax(
                                &mut variant.parameters,
                                &html_content,
                            );
                        }
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
                            variants,
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

    // -----------------------------------------------------------------
    // mark_trailing_variadic_from_syntax — direct detector tests.
    //
    // Detection contract: the substring `>,...,<` (separate-bracket
    // ellipsis idiom) in the page's `Синтаксис:` chapter lifts the
    // last parameter's `is_variadic` flag. No marker → no flag.
    // The single-bracket idiom (`<X1,...,XN>` for СтрШаблон /
    // ФорматированнаяСтрока) intentionally does NOT trigger here —
    // that path is handled by the `<word>N-<word>M` name idiom
    // downstream in `hir-ty/src/builtin.rs`.
    // -----------------------------------------------------------------

    /// Build a minimal page with a `Синтаксис:` chapter and ONE rubric
    /// param. Caller injects the syntax body to exercise the detector.
    fn synthetic_page_with_syntax(syntax_body: &str, rubric_inner: &str) -> String {
        format!(
            r#"<html><body>
<h1 class="V8SH_pagetitle">Глобальный контекст.Тест (Global context.Test)</h1>
<p class="V8SH_chapter">Синтаксис:</p>{syntax_body}
<p class="V8SH_chapter">Параметры:</p><div class="V8SH_rubric"> <p>{rubric_inner}</div>Тип: <a href="x">Произвольный</a>.<br>
<p class="V8SH_chapter">Описание:</p><p>desc</p>
</body></html>"#
        )
    }

    /// `Мин(<Значение1>,...,<ЗначениеN>)` — separate brackets, the
    /// canonical unbounded-variadic shape. After HTML decode the
    /// substring `>,...,<` is present, so the flag must lift.
    #[test]
    fn detector_marks_separate_bracket_ellipsis() {
        let html = synthetic_page_with_syntax(
            "Тест(&lt;Значение1>,...,<ЗначениеN&gt;)",
            "&lt;Значение1>,...,<ЗначениеN&gt; (обязательный)",
        );
        let mut params = scraper_parser::extract_parameters(&html);
        assert!(!params.is_empty(), "rubric must yield at least one param");
        mark_trailing_variadic_from_syntax(&mut params, &html);
        assert!(
            params.last().unwrap().is_variadic,
            "Мин-style `<X1>,...,<XN>` must lift is_variadic"
        );
    }

    /// `ПродолжитьВызов(<<Значение1>,...,<ЗначениеN>>)` — the double-
    /// angle quirk found in HBK. The inner `<…>,...,<…>` still produces
    /// `>,...,<` after decoding, so the flag must lift even though the
    /// outer wrappers are doubled.
    #[test]
    fn detector_marks_double_angle_quirk() {
        let html = synthetic_page_with_syntax(
            "Тест(&lt;<Значение1>,...,<ЗначениеN>&gt;)",
            "&lt;Значение1>,...,<ЗначениеN&gt; (обязательный)",
        );
        let mut params = scraper_parser::extract_parameters(&html);
        mark_trailing_variadic_from_syntax(&mut params, &html);
        assert!(
            params.last().unwrap().is_variadic,
            "ПродолжитьВызов-style `<<X1>,...,<XN>>` must still lift is_variadic"
        );
    }

    /// `ФорматированнаяСтрока(<Содержимое1,...,СодержимоеN>)` — single
    /// bracket spans the whole ellipsis. NO `>,...,<` substring →
    /// detector must NOT lift; this case is handled by the name idiom.
    #[test]
    fn detector_skips_single_bracket_idiom() {
        let html = synthetic_page_with_syntax(
            "Тест(&lt;Содержимое1,...,СодержимоеN&gt;)",
            "&lt;Содержимое1,...,СодержимоеN&gt; (обязательный)",
        );
        let mut params = scraper_parser::extract_parameters(&html);
        mark_trailing_variadic_from_syntax(&mut params, &html);
        assert!(
            !params.last().unwrap().is_variadic,
            "single-bracket idiom must NOT lift is_variadic — name idiom owns this shape"
        );
    }

    /// Fixed-arity signature with no ellipsis whatsoever — flag stays
    /// false, locking the no-behaviour-change path for the bulk of the
    /// platform corpus.
    #[test]
    fn detector_skips_fixed_arity() {
        let html = synthetic_page_with_syntax(
            "Тест(&lt;Параметр&gt;)",
            "&lt;Параметр&gt; (обязательный)",
        );
        let mut params = scraper_parser::extract_parameters(&html);
        mark_trailing_variadic_from_syntax(&mut params, &html);
        assert!(
            !params.last().unwrap().is_variadic,
            "fixed-arity must NOT lift is_variadic"
        );
    }

    /// Empty parameter list — early-return guard: no last param to
    /// mark, no panic, no allocation churn.
    #[test]
    fn detector_handles_empty_params_list() {
        let html = synthetic_page_with_syntax(
            "Тест(&lt;X1&gt;,...,&lt;XN&gt;)",
            "&lt;X&gt;",
        );
        let mut params: Vec<MethodParameter> = Vec::new();
        mark_trailing_variadic_from_syntax(&mut params, &html);
        assert!(params.is_empty(), "empty input must remain empty");
    }

    /// Page without a `Синтаксис:` chapter — `extract_syntax` returns
    /// `None`. Detector must early-return without touching the params.
    #[test]
    fn detector_handles_missing_syntax_chapter() {
        let html = r#"<html><body>
<h1 class="V8SH_pagetitle">FakeType</h1>
<p class="V8SH_chapter">Параметры:</p><div class="V8SH_rubric"> <p>&lt;X&gt; (обязательный)</div>Тип: <a href="x">Число</a>.
</body></html>"#;
        let mut params = scraper_parser::extract_parameters(html);
        mark_trailing_variadic_from_syntax(&mut params, html);
        assert!(
            !params.last().unwrap().is_variadic,
            "absent Синтаксис: chapter must keep is_variadic at false"
        );
    }

    /// End-to-end through `parse_constructor_html`: `Новый Массив(
    /// <КоличествоЭлементов1>,...,<КоличествоЭлементовN>)` shape feeds
    /// the full pipeline (syntax extraction + detector + serialization
    /// of `is_variadic`). Locks the integration with the constructor
    /// callsite.
    #[test]
    fn parse_constructor_html_lifts_variadic_for_array_count_shape() {
        let html = synthetic_ctor_html(
            "По количеству элементов",
            r#"<div class="V8SH_rubric"> <p>&lt;КоличествоЭлементов1>,...,<КоличествоЭлементовN&gt; (необязательный)</div>Тип: <a href="x">Число</a>. <br>Размерности.<br>"#,
            "Создаёт массив указанной размерности.",
        );
        // synthetic_ctor_html hardcodes a non-variadic Синтаксис body
        // (`Новый ФейкТип(…)`); rewrite the syntax line so the
        // detector sees the real Array shape.
        let html = html.replace(
            "<p class=\"V8SH_chapter\">Синтаксис:</p>Новый ФейкТип(…)",
            "<p class=\"V8SH_chapter\">Синтаксис:</p>Новый ФейкТип(&lt;КоличествоЭлементов1>,...,<КоличествоЭлементовN&gt;)",
        );

        let dir = unique_tmpdir("array-variadic");
        let path = dir.join("ctor13.html");
        fs::write(&path, &html).unwrap();
        let mut counter: u32 = 0;
        let info = parse_constructor_html(&path, "FakeType", &mut counter)
            .unwrap()
            .expect("ctor info");
        assert_eq!(info.parameters.len(), 1);
        assert!(
            info.parameters[0].is_variadic,
            "Array's count-list ctor must mark its sole param as variadic"
        );
        fs::remove_dir_all(&dir).ok();
    }
}

// -----------------------------------------------------------------------------
// `parse_property_html` tests — pin the rfind-anchored title parsing so that
// composite type pages (`<Type>.<Имя>.<Property>`) emit real properties
// instead of being silently dropped by the placeholder filter.
// -----------------------------------------------------------------------------
#[cfg(test)]
mod property_tests {
    use super::*;
    use std::path::PathBuf;

    fn unique_tmpdir(tag: &str) -> PathBuf {
        let pid = std::process::id();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("html-parser-prop-{tag}-{pid}-{ts}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create tmp dir");
        dir
    }

    fn synthetic_property_page(title: &str) -> String {
        format!(
            r#"<html><body>
<h1 class="V8SH_pagetitle">{title}</h1>
<p class="V8SH_chapter">Использование:</p><p>Чтение и запись.</p>
<p class="V8SH_chapter">Описание:</p><p>Тип: Булево. Свойство для теста.</p>
</body></html>"#
        )
    }

    fn parse(title: &str, type_name_en: &str) -> Option<PropertyInfo> {
        let dir = unique_tmpdir("title");
        let path = dir.join("Prop.html");
        fs::write(&path, synthetic_property_page(title)).unwrap();
        let mut counter: u32 = 0;
        let info = parse_property_html(&path, type_name_en, &mut counter).unwrap();
        fs::remove_dir_all(&dir).ok();
        info
    }

    #[test]
    fn scalar_property_title_extracts_property_name() {
        // Baseline: scalar `Запрос.Параметры (Query.Parameters)` page —
        // single dot per side. Lock the rfind path on the simplest shape so
        // a regression here surfaces immediately.
        let info = parse("Запрос.Параметры (Query.Parameters)", "Query")
            .expect("scalar property page must parse");
        assert_eq!(info.name, "Параметры");
        assert_eq!(info.english_name, "Parameters");
        assert_eq!(info.type_name, "Query");
    }

    #[test]
    fn composite_property_title_extracts_rightmost_segment() {
        // Real HBK shape for register record-set / catalog-object / etc.:
        // `<Type>.<Имя ...>.<PropertyName> (<TypeEn>.<NameEn>.<PropertyEn>)`.
        // Anchor on the LAST dot so the property name doesn't degrade into
        // the `<Имя ...>` placeholder fragment (which the placeholder
        // filter would silently drop, hiding 7 real properties for every
        // record-set type).
        let info = parse(
            "РегистрСведенийНаборЗаписей.&lt;Имя регистра сведений&gt;.ДополнительныеСвойства \
             (InformationRegisterRecordSet.&lt;Information register name&gt;.AdditionalProperties)",
            "InformationRegisterRecordSet",
        )
        .expect("composite property page must parse");
        assert_eq!(info.name, "ДополнительныеСвойства");
        assert_eq!(info.english_name, "AdditionalProperties");
        assert_eq!(info.type_name, "InformationRegisterRecordSet");
    }

    #[test]
    fn composite_property_with_placeholder_segment_still_parses_real_name() {
        // The placeholder filter must only fire when the *property name*
        // itself starts with `<` / `&lt;` (e.g. catalog manager's literal
        // `<Имя>` member page). When the placeholder is the qualifier
        // (`<Имя регистра сведений>`) and the rightmost segment is a real
        // identifier, the entry must survive.
        let info = parse(
            "РегистрСведенийНаборЗаписей.&lt;Имя регистра сведений&gt;.Отбор \
             (InformationRegisterRecordSet.&lt;Information register name&gt;.Filter)",
            "InformationRegisterRecordSet",
        )
        .expect("composite property with bracketed qualifier must parse");
        assert_eq!(info.name, "Отбор");
        assert_eq!(info.english_name, "Filter");
    }

    #[test]
    fn composite_placeholder_property_name_is_still_dropped() {
        // Negative pin: when the property name itself IS a placeholder
        // (`<Имя ключа>` on `Структура` / `Соответствие`-style synthetic
        // pages), the filter must keep dropping it. Otherwise completion
        // surfaces noise like `<Имя ключа` as a real property suggestion.
        let info = parse(
            "Структура.&lt;Имя ключа&gt; (Structure.&lt;Key name&gt;)",
            "Structure",
        );
        assert!(info.is_none(), "placeholder-as-property-name must still be dropped");
    }
}
