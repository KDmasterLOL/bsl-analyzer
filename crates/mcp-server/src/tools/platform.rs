use crate::tools::response::text_within_budget;
use bsl_platform::PlatformDataInner;
use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;
use std::fmt::Write;

/// A large platform type unfolds its constructors, methods and properties in full, so the
/// card goes out through the output budget. The note points at the cheaper follow-up: one
/// member's card via `type_name` instead of the whole type.
const BUDGET_NOTE: &str = "\n-- карточка усечена под max_output_tokens; повысьте бюджет или \
                           запросите один метод: name=\"ИмяМетода\", type_name=\"ИмяТипа\" --\n";

pub fn bsl_syntax_help(
    name: &str,
    type_name: Option<&str>,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let card = syntax_help_card(name, type_name)?;
    Ok(text_within_budget(card, max_output_tokens, BUDGET_NOTE))
}

fn syntax_help_card(name: &str, type_name: Option<&str>) -> Result<String, McpError> {
    let platform = PlatformDataInner::instance();

    if let Some(tn) = type_name {
        return search_method(platform, tn, name);
    }

    if let Some(func) = platform.get_global_function(name) {
        return Ok(format_global_function(platform, func));
    }

    let types = platform.all_types();
    let name_lower = name.to_lowercase();
    if let Some(pt) = types.iter().find(|t| {
        t.name.to_lowercase() == name_lower || t.english_name.to_lowercase() == name_lower
    }) {
        return Ok(format_type_info(platform, pt));
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
        return Ok(out);
    }

    if let Some(kw) = platform.get_keyword_docs(name) {
        return Ok(format_keyword_docs(&kw));
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
) -> Result<String, McpError> {
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
        Ok(out)
    } else {
        Err(McpError::invalid_params(
            format!("Метод '{method_name}' не найден у типа '{type_name}'"),
            None,
        ))
    }
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
