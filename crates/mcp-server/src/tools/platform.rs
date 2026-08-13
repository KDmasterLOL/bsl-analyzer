use crate::tools::response::{structured_with_text, truncate_text_to_budget};
use bsl_platform::PlatformDataInner;
use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{json, Value};
use std::fmt::Write;

/// A large platform type unfolds its constructors, methods and properties in full, so the
/// card goes out through the output budget. The note points at the cheaper follow-up: one
/// member's card via `type_name` instead of the whole type.
const BUDGET_NOTE: &str = "\n-- карточка усечена под max_output_tokens; повысьте бюджет или \
                           запросите один метод: name=\"ИмяМетода\", type_name=\"ИмяТипа\" --\n";

/// Both flags are always serialized: the published `outputSchema` requires them, and a client
/// validating the card against that schema must not have to treat an absent flag as `false`.
#[derive(JsonSchema, Serialize)]
pub(crate) struct SyntaxHelpResponse {
    schema_version: SyntaxHelpSchemaVersion,
    #[serde(flatten)]
    item: SyntaxHelpItem,
    text_truncated: bool,
    budget_exhausted: bool,
}

#[derive(JsonSchema, Serialize)]
enum SyntaxHelpSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

#[derive(JsonSchema, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SyntaxHelpItem {
    Type {
        name: String,
        english_name: String,
        min_version: Option<String>,
        contexts: Vec<SyntaxContext>,
        iterable_element_types: Vec<String>,
        xdto_name: Option<String>,
        constructors: Vec<SyntaxCallable>,
        methods: Vec<SyntaxMethodSummary>,
    },
    Method {
        matches: Vec<SyntaxCallable>,
    },
    GlobalFunction {
        function: SyntaxCallable,
    },
    Keyword {
        name: String,
        english_name: String,
        syntax: String,
        description: String,
        parameters: Vec<SyntaxDocParameter>,
        min_version: Option<String>,
    },
}

#[derive(JsonSchema, Serialize)]
struct SyntaxCallable {
    name: String,
    english_name: Option<String>,
    owner_type: Option<String>,
    variant_name: Option<String>,
    return_type: Option<String>,
    parameters: Vec<SyntaxParameter>,
    variants: Vec<SyntaxVariant>,
    min_version: Option<String>,
    contexts: Vec<SyntaxContext>,
    documentation: Option<SyntaxDocumentation>,
}

#[derive(JsonSchema, Serialize)]
struct SyntaxMethodSummary {
    name: String,
    english_name: String,
    return_type: Option<String>,
    min_version: Option<String>,
    contexts: Vec<SyntaxContext>,
}

#[derive(JsonSchema, Serialize)]
struct SyntaxVariant {
    name: Option<String>,
    parameters: Vec<SyntaxParameter>,
}

#[derive(JsonSchema, Serialize)]
struct SyntaxParameter {
    name: String,
    parameter_type: Option<String>,
    optional: bool,
    variadic: bool,
}

#[derive(JsonSchema, Serialize)]
struct SyntaxDocumentation {
    syntax: String,
    description: String,
    parameters: Vec<SyntaxDocParameter>,
    examples: Vec<SyntaxExample>,
    notes: Option<String>,
    see_also: Vec<String>,
}

#[derive(JsonSchema, Serialize)]
struct SyntaxDocParameter {
    name: String,
    description: String,
    default_value: Option<String>,
}

#[derive(JsonSchema, Serialize)]
struct SyntaxExample {
    code: String,
    description: Option<String>,
}

#[derive(JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
enum SyntaxContext {
    ThickClient,
    ThinClient,
    WebClient,
    Server,
    MobileClient,
    ExternalConnection,
}

pub fn bsl_syntax_help(
    name: &str,
    type_name: Option<&str>,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let (mut text, item) = syntax_help_card(name, type_name)?;
    let text_truncated = truncate_text_to_budget(&mut text, max_output_tokens, BUDGET_NOTE);
    // The flag is a property of the serialized card, so the card is built first and the flag
    // written into it afterwards rather than guessed before the trimming that decides it.
    let mut body = serde_json::to_value(SyntaxHelpResponse {
        schema_version: SyntaxHelpSchemaVersion::V1,
        item,
        text_truncated,
        budget_exhausted: false,
    })
    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    let budget_exhausted = fit_card_to_budget(&mut body, max_output_tokens, text.len());
    body["budget_exhausted"] = json!(budget_exhausted);
    Ok(structured_with_text(text, body))
}

/// Fit the card into what the Markdown left of `max_output_tokens`, reporting whether the pair
/// still exceeds it.
///
/// The response carries the rendering and the card together, so both are charged to one ceiling
/// — budgeting the text alone overshoots it by the card's size, which for a large platform type
/// is the larger half (see [`crate::tools::response::structured_with_text`]). The text is served
/// first, because clients have parsed it since before the card existed; the card's listings then
/// take what is left, in the order and with the floors [`LISTINGS`] gives them. A pair that is
/// still over the ceiling says so through this flag instead of going out silently oversized.
fn fit_card_to_budget(body: &mut Value, max_output_tokens: usize, text_bytes: usize) -> bool {
    let ceiling = max_output_tokens.saturating_mul(4);
    let mut listings = Vec::new();
    for (key, keep_one) in LISTINGS {
        if let Some(array) = body.get_mut(key).and_then(Value::as_array_mut) {
            listings.push((key, std::mem::take(array), keep_one));
        }
    }

    // With the listings taken out, what remains is the identity of the card — the kind, the
    // names, the flags — which no budget may drop: a card that cannot say what it describes is
    // worse than a card that says it is partial.
    let mut left = ceiling.saturating_sub(text_bytes + serialized_bytes(body));
    let mut dropped = false;
    for (key, mut items, keep_one) in listings {
        let (used, cut) = fit_listing(&mut items, left, keep_one);
        left = left.saturating_sub(used);
        dropped |= cut;
        body[key] = Value::Array(items);
    }
    dropped || text_bytes + serialized_bytes(body) > ceiling
}

/// The card's listings, in the order they are paid for, each with whether its first entry
/// survives a budget too small for it.
///
/// `matches` is the answer to the lookup itself, and an empty one would read as "nothing found";
/// a type's constructors and methods are supplementary, and the Markdown note already points at
/// the single-member lookup that returns them affordably.
const LISTINGS: [(&str, bool); 3] =
    [("matches", true), ("constructors", false), ("methods", false)];

/// Keep the leading entries of a listing that fit `budget_bytes`, reporting the bytes kept and
/// whether anything was dropped.
fn fit_listing(items: &mut Vec<Value>, budget_bytes: usize, keep_one: bool) -> (usize, bool) {
    let mut used = 0usize;
    let mut keep = 0usize;
    for (i, item) in items.iter().enumerate() {
        let next = used + serialized_bytes(item) + usize::from(i > 0);
        if next > budget_bytes && !(keep_one && keep == 0) {
            break;
        }
        used = next;
        keep = i + 1;
    }
    let dropped = keep < items.len();
    items.truncate(keep);
    (used, dropped)
}

fn serialized_bytes<T: Serialize>(value: &T) -> usize {
    serde_json::to_string(value).map(|s| s.len()).unwrap_or(0)
}

fn syntax_help_card(
    name: &str,
    type_name: Option<&str>,
) -> Result<(String, SyntaxHelpItem), McpError> {
    let platform = PlatformDataInner::instance();

    if let Some(tn) = type_name {
        return search_method(platform, tn, name);
    }

    if let Some(func) = platform.get_global_function(name) {
        return Ok((
            format_global_function(platform, func),
            SyntaxHelpItem::GlobalFunction {
                function: callable_from_global_function(platform, func),
            },
        ));
    }

    let types = platform.all_types();
    let name_lower = name.to_lowercase();
    if let Some(pt) = types.iter().find(|t| {
        t.name.to_lowercase() == name_lower || t.english_name.to_lowercase() == name_lower
    }) {
        return Ok((
            format_type_info(platform, pt),
            SyntaxHelpItem::Type {
                name: pt.name.to_string(),
                english_name: pt.english_name.to_string(),
                min_version: pt.min_version.as_ref().map(ToString::to_string),
                contexts: context_names(pt.context),
                iterable_element_types: pt
                    .iter_element_types
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                xdto_name: pt.xdto_name.as_ref().map(ToString::to_string),
                constructors: platform
                    .get_constructors(&pt.name)
                    .into_iter()
                    .map(|ctor| callable_from_constructor(platform, ctor))
                    .collect(),
                methods: platform
                    .get_type_methods(&pt.name)
                    .into_iter()
                    .map(method_summary)
                    .collect(),
            },
        ));
    }

    let all_methods = platform.all_methods();
    let matches: Vec<_> = all_methods
        .iter()
        .filter(|m| {
            m.name.to_lowercase() == name_lower || m.english_name.to_lowercase() == name_lower
        })
        .collect();

    if !matches.is_empty() {
        let mut out = format!("# Метод: {name}\n\n");
        if matches.len() > 1 {
            let _ = writeln!(out, "Найден у нескольких типов:\n");
        }
        for m in &matches {
            let _ = writeln!(
                out,
                "## {}.{} / {}.{}\n",
                m.type_name, m.name, m.type_name, m.english_name
            );
            format_method_signature(&mut out, &m.name, &m.parameters, m.return_type.as_deref());
            if let Some(docs) = platform.get_method_docs(m.id) {
                format_docs(&mut out, &docs);
            }
            out.push('\n');
        }
        return Ok((
            out,
            SyntaxHelpItem::Method {
                matches: matches
                    .into_iter()
                    .map(|method| callable_from_method(platform, method))
                    .collect(),
            },
        ));
    }

    if let Some(kw) = platform.get_keyword_docs(name) {
        return Ok((
            format_keyword_docs(&kw),
            SyntaxHelpItem::Keyword {
                name: kw.keyword_ru.to_string(),
                english_name: kw.keyword_en.to_string(),
                syntax: kw.syntax.clone(),
                description: kw.description.clone(),
                parameters: kw.params.iter().map(doc_parameter).collect(),
                min_version: kw.min_version.clone(),
            },
        ));
    }

    Err(McpError::invalid_params(
        format!("'{name}' не найдено среди типов, методов, глобальных функций и ключевых слов платформы"),
        None,
    ))
}

fn search_method(
    platform: &PlatformDataInner,
    type_name: &str,
    method_name: &str,
) -> Result<(String, SyntaxHelpItem), McpError> {
    if let Some(method) = platform.get_method(type_name, method_name) {
        let mut out = format!(
            "# {}.{} / {}.{}\n\n",
            method.type_name, method.name, method.type_name, method.english_name
        );
        format_method_signature(
            &mut out,
            &method.name,
            &method.parameters,
            method.return_type.as_deref(),
        );
        if let Some(docs) = platform.get_method_docs(method.id) {
            format_docs(&mut out, &docs);
        }
        Ok((out, SyntaxHelpItem::Method { matches: vec![callable_from_method(platform, method)] }))
    } else {
        Err(McpError::invalid_params(
            format!("Метод '{method_name}' не найден у типа '{type_name}'"),
            None,
        ))
    }
}

fn callable_from_method(
    platform: &PlatformDataInner,
    method: &bsl_platform::PlatformMethod,
) -> SyntaxCallable {
    SyntaxCallable {
        name: method.name.to_string(),
        english_name: Some(method.english_name.to_string()),
        owner_type: Some(method.type_name.to_string()),
        variant_name: None,
        return_type: method.return_type.as_ref().map(ToString::to_string),
        parameters: method.parameters.iter().map(parameter).collect(),
        variants: method
            .variants
            .iter()
            .map(|variant| SyntaxVariant {
                name: variant.variant_name.as_ref().map(ToString::to_string),
                parameters: variant.parameters.iter().map(parameter).collect(),
            })
            .collect(),
        min_version: method.min_version.as_ref().map(ToString::to_string),
        contexts: context_names(method.context),
        documentation: platform.get_method_docs(method.id).as_ref().map(documentation),
    }
}

fn callable_from_global_function(
    platform: &PlatformDataInner,
    function: &bsl_platform::GlobalFunction,
) -> SyntaxCallable {
    SyntaxCallable {
        name: function.name.to_string(),
        english_name: Some(function.english_name.to_string()),
        owner_type: None,
        variant_name: None,
        return_type: function.return_type.as_ref().map(ToString::to_string),
        parameters: function.parameters.iter().map(parameter).collect(),
        variants: function
            .variants
            .iter()
            .map(|variant| SyntaxVariant {
                name: variant.variant_name.as_ref().map(ToString::to_string),
                parameters: variant.parameters.iter().map(parameter).collect(),
            })
            .collect(),
        min_version: function.min_version.as_ref().map(ToString::to_string),
        contexts: context_names(function.context),
        documentation: platform.get_global_function_docs(function.id).as_ref().map(documentation),
    }
}

fn callable_from_constructor(
    platform: &PlatformDataInner,
    constructor: &bsl_platform::PlatformConstructor,
) -> SyntaxCallable {
    SyntaxCallable {
        name: "Новый".to_owned(),
        english_name: Some("New".to_owned()),
        owner_type: Some(constructor.type_name.to_string()),
        variant_name: constructor.variant_name.as_ref().map(ToString::to_string),
        return_type: Some(constructor.type_name.to_string()),
        parameters: constructor.parameters.iter().map(parameter).collect(),
        variants: Vec::new(),
        min_version: constructor.min_version.as_ref().map(ToString::to_string),
        contexts: context_names(constructor.context),
        documentation: platform.get_constructor_docs(constructor.id).as_ref().map(|docs| {
            SyntaxDocumentation {
                syntax: docs.syntax.clone(),
                description: docs.description.clone(),
                parameters: docs.params.iter().map(doc_parameter).collect(),
                examples: docs.examples.iter().map(example).collect(),
                notes: docs.notes.clone(),
                see_also: docs.see_also.clone(),
            }
        }),
    }
}

fn method_summary(method: &bsl_platform::PlatformMethod) -> SyntaxMethodSummary {
    SyntaxMethodSummary {
        name: method.name.to_string(),
        english_name: method.english_name.to_string(),
        return_type: method.return_type.as_ref().map(ToString::to_string),
        min_version: method.min_version.as_ref().map(ToString::to_string),
        contexts: context_names(method.context),
    }
}

fn parameter(param: &bsl_platform::MethodParam) -> SyntaxParameter {
    SyntaxParameter {
        name: param.name.to_string(),
        parameter_type: param.param_type.as_ref().map(ToString::to_string),
        optional: param.is_optional,
        variadic: param.is_variadic,
    }
}

fn documentation(docs: &bsl_platform::MethodDocs) -> SyntaxDocumentation {
    SyntaxDocumentation {
        syntax: docs.syntax.clone(),
        description: docs.description.clone(),
        parameters: docs.params.iter().map(doc_parameter).collect(),
        examples: docs.examples.iter().map(example).collect(),
        notes: docs.notes.clone(),
        see_also: docs.see_also.clone(),
    }
}

fn doc_parameter(param: &bsl_platform::ParamDocs) -> SyntaxDocParameter {
    SyntaxDocParameter {
        name: param.name.to_string(),
        description: param.description.clone(),
        default_value: param.default_value.clone(),
    }
}

fn example(example: &bsl_platform::CodeExample) -> SyntaxExample {
    SyntaxExample { code: example.code.clone(), description: example.description.clone() }
}

/// An entry with no availability markup is available everywhere, so an empty list here means
/// exactly one thing: the platform marked the entry available in no context at all.
fn context_names(context: Option<bsl_platform::ContextAvailability>) -> Vec<SyntaxContext> {
    let context = bsl_platform::ContextAvailability::effective(context.as_ref());
    [
        (context.thick_client, SyntaxContext::ThickClient),
        (context.thin_client, SyntaxContext::ThinClient),
        (context.web_client, SyntaxContext::WebClient),
        (context.server, SyntaxContext::Server),
        (context.mobile_client, SyntaxContext::MobileClient),
        (context.external_connection, SyntaxContext::ExternalConnection),
    ]
    .into_iter()
    .filter(|(available, _)| *available)
    .map(|(_, name)| name)
    .collect()
}

fn format_global_function(
    platform: &PlatformDataInner,
    func: &bsl_platform::GlobalFunction,
) -> String {
    let mut out = format!("# {} / {}\n\n", func.name, func.english_name);
    format_method_signature(&mut out, &func.name, &func.parameters, func.return_type.as_deref());
    if let Some(docs) = platform.get_global_function_docs(func.id) {
        format_docs(&mut out, &docs);
    }
    out
}

fn format_type_info(platform: &PlatformDataInner, pt: &bsl_platform::PlatformType) -> String {
    let mut out = format!("# {} / {}\n\n", pt.name, pt.english_name);

    format_constructors(&mut out, platform, &pt.name);

    let methods = platform.get_type_methods(&pt.name);
    if methods.is_empty() {
        // Only call a type method-less if it also has no constructor surface — otherwise
        // the constructor section above already carries its real API.
        if platform.get_constructors(&pt.name).is_empty() {
            let _ = writeln!(out, "Методов нет.");
        }
    } else {
        let _ = writeln!(out, "## Методы ({})\n", methods.len());
        let _ = writeln!(out, "| Имя | English | Возвращает |");
        let _ = writeln!(out, "|-----|---------|------------|");
        for m in &methods {
            let ret = m.return_type.as_deref().unwrap_or("—");
            let _ = writeln!(out, "| {} | {} | {ret} |", m.name, m.english_name);
        }
        let _ = writeln!(out, "\nИспользуйте `bsl_syntax_help(name=\"ИмяМетода\", type_name=\"{}\")` для подробной справки.", pt.name);
    }

    out
}

fn format_keyword_docs(kw: &bsl_platform::KeywordDocs) -> String {
    let mut out = format!("# {} / {}\n\n", kw.keyword_ru, kw.keyword_en);
    let _ = writeln!(out, "## Синтаксис\n\n```bsl\n{}\n```\n", kw.syntax);
    let _ = writeln!(out, "## Описание\n\n{}", kw.description);
    if !kw.params.is_empty() {
        let _ = writeln!(out, "\n## Параметры\n");
        for p in &kw.params {
            let _ = writeln!(out, "- **{}**: {}", p.name, p.description);
        }
    }
    out
}

fn format_method_signature(
    out: &mut String,
    name: &str,
    params: &[bsl_platform::MethodParam],
    return_type: Option<&str>,
) {
    let _ = write!(out, "```bsl\n{name}(");
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            let _ = write!(out, ", ");
        }
        if p.is_optional {
            let _ = write!(out, "[");
        }
        let _ = write!(out, "{}", p.name);
        if let Some(ref pt) = p.param_type {
            let _ = write!(out, ": {pt}");
        }
        if p.is_optional {
            let _ = write!(out, "]");
        }
    }
    let _ = write!(out, ")");
    if let Some(ret) = return_type {
        let _ = write!(out, ": {ret}");
    }
    let _ = writeln!(out, "\n```\n");
}

fn format_docs(out: &mut String, docs: &bsl_platform::MethodDocs) {
    format_doc_body(out, &docs.description, &docs.params, &docs.examples, docs.notes.as_deref());
}

/// Render the shared documentation body (description / params / examples / notes).
/// Methods and constructors carry the same doc fields, so both render through here.
fn format_doc_body(
    out: &mut String,
    description: &str,
    params: &[bsl_platform::ParamDocs],
    examples: &[bsl_platform::CodeExample],
    notes: Option<&str>,
) {
    if !description.is_empty() {
        let _ = writeln!(out, "## Описание\n\n{description}\n");
    }
    if !params.is_empty() {
        let _ = writeln!(out, "## Параметры\n");
        for p in params {
            if let Some(ref def) = p.default_value {
                let _ = writeln!(out, "- **{}** (по умолчанию: {def}): {}", p.name, p.description);
            } else {
                let _ = writeln!(out, "- **{}**: {}", p.name, p.description);
            }
        }
        out.push('\n');
    }
    if !examples.is_empty() {
        let _ = writeln!(out, "## Примеры\n");
        for ex in examples {
            if let Some(ref desc) = ex.description {
                let _ = writeln!(out, "{desc}\n");
            }
            let _ = writeln!(out, "```bsl\n{}\n```\n", ex.code);
        }
    }
    if let Some(notes) = notes {
        let _ = writeln!(out, "## Примечания\n\n{notes}\n");
    }
}

/// Render a type's constructors (`Новый Тип(…)`). A type whose entire API is its
/// constructor (`ОписаниеОповещения`, `Граница`, `ОписаниеТипов`) would otherwise show
/// only "Методов нет." and hide its real surface.
fn format_constructors(out: &mut String, platform: &PlatformDataInner, type_name: &str) {
    let constructors = platform.get_constructors(type_name);
    if constructors.is_empty() {
        return;
    }
    let _ = writeln!(out, "## Конструкторы ({})\n", constructors.len());
    let many = constructors.len() > 1;
    for ctor in &constructors {
        if many {
            if let Some(ref variant) = ctor.variant_name {
                let _ = writeln!(out, "### {variant}\n");
            }
        }
        let docs = platform.get_constructor_docs(ctor.id);
        match docs.as_ref().filter(|d| !d.syntax.is_empty()) {
            Some(d) => {
                let _ = writeln!(out, "```bsl\n{}\n```\n", d.syntax);
            }
            None => format_constructor_signature(out, type_name, &ctor.parameters),
        }
        if let Some(d) = docs.as_ref() {
            format_doc_body(out, &d.description, &d.params, &d.examples, d.notes.as_deref());
        }
    }
}

/// Fallback constructor signature built from parameter metadata when prose docs carry
/// no ready-made `syntax` string.
fn format_constructor_signature(
    out: &mut String,
    type_name: &str,
    params: &[bsl_platform::MethodParam],
) {
    let _ = write!(out, "```bsl\nНовый {type_name}(");
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            let _ = write!(out, ", ");
        }
        if p.is_optional {
            let _ = write!(out, "[");
        }
        let _ = write!(out, "{}", p.name);
        if let Some(ref pt) = p.param_type {
            let _ = write!(out, ": {pt}");
        }
        if p.is_optional {
            let _ = write!(out, "]");
        }
    }
    let _ = writeln!(out, ")\n```\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_text(result: &CallToolResult) -> &str {
        result.content[0].raw.as_text().expect("expected text content").text.as_str()
    }

    fn structured(result: &CallToolResult) -> &serde_json::Value {
        result.structured_content.as_ref().expect("expected structuredContent")
    }

    #[test]
    fn test_syntax_help_type_lookup() {
        let platform = PlatformDataInner::instance();
        if platform.all_types().is_empty() {
            return;
        }

        let result = bsl_syntax_help("Массив", None, 6000).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("Массив"), "should find Array type");
        assert!(text.contains("Array"), "should show english name");
        let body = structured(&result);
        assert_eq!(body["schema_version"], "1");
        assert_eq!(body["kind"], "type");
        assert_eq!(body["name"], "Массив");
        assert!(body["constructors"].is_array());
        assert!(body["methods"].is_array());
    }

    #[test]
    fn test_syntax_help_type_english() {
        let platform = PlatformDataInner::instance();
        if platform.all_types().is_empty() {
            return;
        }

        let result = bsl_syntax_help("Array", None, 6000).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("Массив") || text.contains("Array"), "should find by english name");
    }

    #[test]
    fn test_syntax_help_constructor_only_type_renders_constructor() {
        let platform = PlatformDataInner::instance();
        // Skip only when no platform data is generated at all. Do NOT guard on the
        // constructor lookup itself — that would mask a bilingual-keying regression (the
        // RU type name `ОписаниеОповещения` must resolve to the constructor keyed under
        // the EN `CallbackDescription`), which is exactly what this test protects.
        if platform.all_constructors().is_empty() {
            return;
        }
        assert!(
            !platform.get_constructors("ОписаниеОповещения").is_empty(),
            "RU type name must resolve to the CallbackDescription constructor"
        );

        let result = bsl_syntax_help("ОписаниеОповещения", None, 6000).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("Конструктор"), "constructor-only type must show its constructor");
        assert!(
            text.contains("Новый") || text.contains("ИмяПроцедуры"),
            "should show the Новый syntax or its parameters"
        );
        assert!(
            !text.contains("Методов нет."),
            "must not claim no surface when a constructor exists"
        );
    }

    #[test]
    fn test_syntax_help_method_with_type() {
        let platform = PlatformDataInner::instance();
        if platform.all_methods().is_empty() {
            return;
        }

        let method = &platform.all_methods()[0];
        let result = bsl_syntax_help(&method.name, Some(&method.type_name), 6000).unwrap();
        let text = extract_text(&result);
        assert!(text.contains(method.name.as_str()), "should contain method name");
        assert!(text.contains("```bsl"), "should have code block");
        let body = structured(&result);
        assert_eq!(body["kind"], "method");
        assert_eq!(body["matches"][0]["name"], method.name.as_str());
        assert_eq!(body["matches"][0]["owner_type"], method.type_name.as_str());
        assert!(body["matches"][0]["parameters"].is_array());
    }

    #[test]
    fn test_syntax_help_global_function() {
        let platform = PlatformDataInner::instance();
        if platform.all_global_functions().is_empty() {
            return;
        }

        let result = bsl_syntax_help("Сообщить", None, 6000).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("Сообщить") || text.contains("Message"), "should find global fn");
        let body = structured(&result);
        assert_eq!(body["kind"], "global_function");
        assert_eq!(body["function"]["name"], "Сообщить");
    }

    #[test]
    fn test_syntax_help_card_is_bounded_by_the_output_budget() {
        let platform = PlatformDataInner::instance();
        if platform.all_types().is_empty() {
            return;
        }

        let full = extract_text(&bsl_syntax_help("Массив", None, 6000).unwrap()).to_string();
        let clipped = bsl_syntax_help("Массив", None, 100).unwrap();
        let clipped = extract_text(&clipped);
        assert!(clipped.len() < full.len(), "a 100-token budget must clip the card");
        assert!(clipped.contains("карточка усечена"), "must carry the note: {clipped}");
        assert!(
            clipped.len() <= 100 * 4,
            "the note is reserved out of the budget, not added on top: {}",
            clipped.len()
        );
        assert_eq!(
            structured(&bsl_syntax_help("Массив", None, 100).unwrap())["text_truncated"],
            true
        );
    }

    fn pair_bytes(result: &CallToolResult) -> usize {
        extract_text(result).len() + serde_json::to_string(structured(result)).unwrap().len()
    }

    /// The response carries the Markdown and the card, so the ceiling covers both. `ЧтениеДанных`
    /// is the input that shows it: charging the text alone shipped 76 KB against a declared
    /// budget of 6000 tokens, of which 60 KB was the card nobody bounded.
    #[test]
    fn the_card_and_the_markdown_share_one_budget() {
        for name in ["ЧтениеДанных", "COMSafeArray", "Массив"] {
            let result = bsl_syntax_help(name, None, 6000).unwrap();

            assert!(pair_bytes(&result) <= 6000 * 4, "{name}: {} bytes", pair_bytes(&result));
        }
        assert_eq!(
            structured(&bsl_syntax_help("ЧтениеДанных", None, 6000).unwrap())["budget_exhausted"],
            true
        );
    }

    /// What a budget can never take away is the card's identity — its kind and names. A budget
    /// smaller than that goes out over the ceiling by the identity's size and says so, instead of
    /// answering with a card that cannot name what it describes.
    #[test]
    fn a_budget_below_the_cards_identity_overshoots_by_that_much_and_says_so() {
        let result = bsl_syntax_help("Массив", None, 600).unwrap();
        let card = structured(&result);

        assert_eq!(card["budget_exhausted"], true);
        assert_eq!(card["name"], "Массив");
        assert!(card["methods"].as_array().expect("methods listing").is_empty());
        assert!(pair_bytes(&result) <= 600 * 4 + 1024, "{} bytes", pair_bytes(&result));
    }

    /// A lookup that found something never answers with an empty listing: `matches` is the answer
    /// itself, and emptying it would read as "nothing found" rather than "trimmed".
    #[test]
    fn a_matched_lookup_keeps_one_entry_at_any_budget() {
        let platform = PlatformDataInner::instance();
        let method = &platform.all_methods()[0];

        let result = bsl_syntax_help(&method.name, Some(&method.type_name), 1).unwrap();
        let card = structured(&result);

        assert_eq!(card["budget_exhausted"], true);
        assert_eq!(card["matches"].as_array().expect("matches listing").len(), 1);
    }

    /// The syntax helper marks availability only where the platform limits it, so an unmarked
    /// entry is available everywhere. Not hypothetical: every constructor in the data is
    /// unmarked, and an empty list would tell a machine consumer the exact opposite.
    #[test]
    fn an_unmarked_availability_reads_as_every_context() {
        let card = structured(&bsl_syntax_help("Граница", None, 6000).unwrap()).clone();

        assert_eq!(
            card["constructors"][0]["contexts"],
            json!([
                "thick_client",
                "thin_client",
                "web_client",
                "server",
                "mobile_client",
                "external_connection"
            ])
        );
    }

    /// Nothing was cut, so nothing may claim it was: a flag that is true either way tells a
    /// consumer nothing about whether the card it holds is complete.
    #[test]
    fn a_card_within_the_budget_is_not_flagged() {
        let card = structured(&bsl_syntax_help("Если", None, 6000).unwrap()).clone();

        assert_eq!(card["budget_exhausted"], false);
        assert_eq!(card["text_truncated"], false);
    }

    #[test]
    fn test_syntax_help_not_found() {
        let result = bsl_syntax_help("НесуществующийТипМетодФункция", None, 6000);
        assert!(result.is_err(), "should return error for unknown name");
    }

    #[test]
    fn test_syntax_help_method_not_found_on_type() {
        let platform = PlatformDataInner::instance();
        if platform.all_types().is_empty() {
            return;
        }

        let result = bsl_syntax_help("НесуществующийМетод", Some("Массив"), 6000);
        assert!(result.is_err(), "should return error for unknown method");
    }
}
