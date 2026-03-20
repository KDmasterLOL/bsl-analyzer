//! Platform reference tool: BSL syntax help.

use bsl_platform::PlatformDataInner;
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use std::fmt::Write;

/// Returns documentation for a platform type, method, or global function.
pub fn bsl_syntax_help(name: &str, type_name: Option<&str>) -> Result<CallToolResult, McpError> {
    let platform = PlatformDataInner::instance();

    // If type_name is provided, search for a method on that type
    if let Some(tn) = type_name {
        return search_method(platform, tn, name);
    }

    // Try global function first
    if let Some(func) = platform.get_global_function(name) {
        return format_global_function(platform, func);
    }

    // Try as type name — show type info and its methods
    let types = platform.all_types();
    let name_lower = name.to_lowercase();
    if let Some(pt) = types.iter().find(|t| {
        t.name.to_lowercase() == name_lower || t.english_name.to_lowercase() == name_lower
    }) {
        return format_type_info(platform, pt);
    }

    // Try as method name across all types
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
        return Ok(CallToolResult::success(vec![Content::text(out)]));
    }

    // Try as keyword
    if let Some(kw) = platform.get_keyword_docs(name) {
        return format_keyword_docs(&kw);
    }

    Err(McpError::invalid_params(
        format!("'{name}' не найдено среди типов, методов, глобальных функций и ключевых слов платформы"),
        None,
    ))
}

/// Full-text search across platform documentation.
pub fn bsl_syntax_search(query: &str) -> Result<CallToolResult, McpError> {
    let platform = PlatformDataInner::instance();
    let results = platform.search(query);

    if results.is_empty() {
        return Err(McpError::invalid_params(
            format!("По запросу '{query}' ничего не найдено"),
            None,
        ));
    }

    let mut out = format!("# Результаты поиска: \"{query}\"\n\n");

    for (i, result) in results.iter().enumerate() {
        match result.kind {
            bsl_platform::DocKind::Type => {
                if let Some(ty) = platform.all_types().get(result.index) {
                    let _ =
                        writeln!(out, "## {}. {} / {} (тип)\n", i + 1, ty.name, ty.english_name);
                    let methods = platform.get_type_methods(&ty.name);
                    if !methods.is_empty() {
                        let names: Vec<&str> =
                            methods.iter().take(5).map(|m| m.name.as_str()).collect();
                        let _ = write!(out, "Методы: {}", names.join(", "));
                        if methods.len() > 5 {
                            let _ = write!(out, " и ещё {}", methods.len() - 5);
                        }
                        out.push_str("\n\n");
                    }
                }
            }
            bsl_platform::DocKind::Method => {
                if let Some(method) = platform.all_methods().get(result.index) {
                    let _ = writeln!(
                        out,
                        "## {}. {}.{} / {}.{}\n",
                        i + 1,
                        method.type_name,
                        method.name,
                        method.type_name,
                        method.english_name
                    );
                    if let Some(docs) = platform.get_method_docs(method.id) {
                        let desc = truncate_description(&docs.description, 150);
                        if !desc.is_empty() {
                            let _ = writeln!(out, "{desc}\n");
                        }
                    }
                }
            }
            bsl_platform::DocKind::GlobalFunction => {
                if let Some(func) = platform.all_global_functions().get(result.index) {
                    let _ = writeln!(
                        out,
                        "## {}. {} / {} (глобальная функция)\n",
                        i + 1,
                        func.name,
                        func.english_name
                    );
                    if let Some(docs) = platform.get_global_function_docs(func.id) {
                        let desc = truncate_description(&docs.description, 150);
                        if !desc.is_empty() {
                            let _ = writeln!(out, "{desc}\n");
                        }
                    }
                }
            }
        }
    }

    let _ = writeln!(
        out,
        "---\nИспользуйте `bsl_syntax_help(name=\"ИмяМетода\", type_name=\"ИмяТипа\")` для подробной справки."
    );

    Ok(CallToolResult::success(vec![Content::text(out)]))
}

fn truncate_description(desc: &str, max_chars: usize) -> &str {
    if desc.len() <= max_chars {
        return desc;
    }
    // Find a safe char boundary near max_chars
    let mut end = max_chars;
    while end > 0 && !desc.is_char_boundary(end) {
        end -= 1;
    }
    // Try to break at last space
    if let Some(space_pos) = desc[..end].rfind(' ') {
        &desc[..space_pos]
    } else {
        &desc[..end]
    }
}

fn search_method(
    platform: &PlatformDataInner,
    type_name: &str,
    method_name: &str,
) -> Result<CallToolResult, McpError> {
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
        Ok(CallToolResult::success(vec![Content::text(out)]))
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
) -> Result<CallToolResult, McpError> {
    let mut out = format!("# {} / {}\n\n", func.name, func.english_name);
    format_method_signature(&mut out, &func.name, &func.parameters, func.return_type.as_deref());
    if let Some(docs) = platform.get_global_function_docs(func.id) {
        format_docs(&mut out, &docs);
    }
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

fn format_type_info(
    platform: &PlatformDataInner,
    pt: &bsl_platform::PlatformType,
) -> Result<CallToolResult, McpError> {
    let mut out = format!("# {} / {}\n\n", pt.name, pt.english_name);

    let methods = platform.get_type_methods(&pt.name);
    if methods.is_empty() {
        let _ = writeln!(out, "Методов нет.");
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

    Ok(CallToolResult::success(vec![Content::text(out)]))
}

fn format_keyword_docs(kw: &bsl_platform::KeywordDocs) -> Result<CallToolResult, McpError> {
    let mut out = format!("# {} / {}\n\n", kw.keyword_ru, kw.keyword_en);
    let _ = writeln!(out, "## Синтаксис\n\n```bsl\n{}\n```\n", kw.syntax);
    let _ = writeln!(out, "## Описание\n\n{}", kw.description);
    if !kw.params.is_empty() {
        let _ = writeln!(out, "\n## Параметры\n");
        for p in &kw.params {
            let _ = writeln!(out, "- **{}**: {}", p.name, p.description);
        }
    }
    Ok(CallToolResult::success(vec![Content::text(out)]))
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
    if !docs.description.is_empty() {
        let _ = writeln!(out, "## Описание\n\n{}\n", docs.description);
    }
    if !docs.params.is_empty() {
        let _ = writeln!(out, "## Параметры\n");
        for p in &docs.params {
            if let Some(ref def) = p.default_value {
                let _ = writeln!(out, "- **{}** (по умолчанию: {def}): {}", p.name, p.description);
            } else {
                let _ = writeln!(out, "- **{}**: {}", p.name, p.description);
            }
        }
        out.push('\n');
    }
    if !docs.examples.is_empty() {
        let _ = writeln!(out, "## Примеры\n");
        for ex in &docs.examples {
            if let Some(ref desc) = ex.description {
                let _ = writeln!(out, "{desc}\n");
            }
            let _ = writeln!(out, "```bsl\n{}\n```\n", ex.code);
        }
    }
    if let Some(ref notes) = docs.notes {
        let _ = writeln!(out, "## Примечания\n\n{notes}\n");
    }
}
