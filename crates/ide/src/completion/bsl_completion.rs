//! BSL code completion.
//!
//! Provides completion for BSL code context:
//! - Global platform functions (НачатьТранзакцию, Формат, Сообщить, etc.)
//! - BSL keywords (Процедура, Функция, Если, etc.)
//! - User-defined symbols (module functions, variables) - TODO

use bsl_platform::{GlobalFunction, PlatformData, PlatformDataInner};
use ide_db::RootDatabase;
use syntax::SyntaxKind;

use super::{CompletionItem, CompletionItemKind, CompletionPosition};

/// Attempts to provide BSL code completions.
///
/// Returns Some(items) if this is a BSL completion context (not after DOT),
/// otherwise returns None.
pub(super) fn bsl_completions(
    db: &dyn RootDatabase,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let _span = tracing::info_span!("bsl_completions").entered();

    tracing::info!("bsl_completions called");

    // Parse the file
    let parse = db.parse(position.file_id);
    let root = parse.syntax_node();

    // Find token at position
    let token = root.token_at_offset(position.offset).right_biased();

    if token.is_none() {
        tracing::info!("No token at position - returning None");
        return None;
    }

    let token = token.unwrap();

    tracing::debug!(token_kind = ?token.kind(), token_text = ?token.text(), "BSL completion token");

    // Check if we're in a method call context (after DOT) - skip BSL completion
    // Platform completion will handle this
    if let Some(prev) = token.prev_sibling_or_token() {
        if prev.kind() == SyntaxKind::DOT {
            tracing::info!("After DOT - skipping BSL completion");
            return None;
        }
    }

    tracing::debug!("Not after DOT, checking if typing...");

    // Check if we're typing something that could be a global function or keyword
    // This includes:
    // - IDENT tokens (user typing a new identifier)
    // - Keyword tokens (user typing inside a keyword like "ВызватьИсключение")
    let token_text = token.text();

    // Check if cursor is inside the token (not at the end)
    // This handles the case where user is typing "ВызватьИ" and lexer already
    // recognized full "ВызватьИсключение" as KW_RAISE
    let is_typing = if token.kind() == SyntaxKind::IDENT {
        // For identifiers, always provide completions
        true
    } else if token.kind().is_keyword() {
        // For keywords, check if cursor is inside the token (partial typing)
        let token_start = token.text_range().start();
        let cursor_in_token = position.offset.checked_sub(token_start);
        if let Some(offset_in_token) = cursor_in_token {
            // Cursor is inside the token if it's not at the end
            let offset_in_token: usize = offset_in_token.into();
            offset_in_token < token_text.len()
        } else {
            false
        }
    } else {
        // Other tokens - no completion
        false
    };

    tracing::debug!(is_typing = is_typing, "Checked is_typing");

    if is_typing {
        // Extract the prefix (text before cursor)
        let token_start = token.text_range().start();
        let cursor_in_token = position.offset.checked_sub(token_start).unwrap_or_default();
        let cursor_in_token: usize = cursor_in_token.into();

        // Get prefix (text from token start to cursor)
        let prefix = &token_text[..cursor_in_token.min(token_text.len())];

        tracing::info!(
            prefix = ?prefix,
            token_kind = ?token.kind(),
            full_text = ?token_text,
            "Completing with prefix"
        );

        let mut completions = Vec::new();

        // If typing inside a keyword, offer the keyword itself as completion
        if token.kind().is_keyword() {
            let (detail, documentation) = get_keyword_info(token_text);
            let keyword_item = CompletionItem {
                label: token_text.to_string(),
                detail: Some(detail),
                kind: CompletionItemKind::Keyword,
                insert_text: token_text.to_string(),
                documentation: Some(documentation),
            };
            completions.push(keyword_item);
        }

        // Also add global functions that match the prefix
        completions.extend(complete_global_functions(prefix));

        tracing::info!(count = completions.len(), "Returning BSL completions");
        return Some(completions);
    }

    // No BSL completion context
    tracing::info!("No BSL completion context - returning None");
    None
}

/// Completes global platform functions with optional prefix filter.
///
/// Example: For prefix "Начать", shows: НачатьТранзакцию, etc.
fn complete_global_functions(prefix: &str) -> Vec<CompletionItem> {
    let data = PlatformDataInner::instance();
    let all_functions = data.all_global_functions();

    let prefix_lower = prefix.to_lowercase();

    // Filter functions by prefix (case-insensitive)
    let matching: Vec<_> = all_functions
        .iter()
        .filter(|f| {
            f.name.to_lowercase().starts_with(&prefix_lower)
                || f.english_name.to_lowercase().starts_with(&prefix_lower)
        })
        .collect();

    tracing::debug!(
        total_functions = all_functions.len(),
        matching_count = matching.len(),
        prefix = ?prefix,
        "Filtered global functions"
    );

    matching.iter().map(|f| render_global_function(f)).collect()
}

/// Renders a global function as a completion item.
///
/// Generates a completion item with:
/// - Label: Russian function name (e.g., "НачатьТранзакцию")
/// - Detail: Signature with return type
/// - Insert text: Snippet with parameter placeholders
/// - Documentation: Bilingual signature + parameters
fn render_global_function(function: &GlobalFunction) -> CompletionItem {
    // Label: Russian name
    let label = function.name.to_string();

    // Detail: Signature with return type
    let detail = format_function_signature(function);

    // Insert text: Snippet with placeholders
    let insert_text = generate_function_snippet(function);

    // Documentation: Bilingual signature + parameters
    let documentation = Some(format_function_documentation(function));

    CompletionItem {
        label,
        detail: Some(detail),
        kind: CompletionItemKind::Function,
        insert_text,
        documentation,
    }
}

/// Formats function signature for the detail field.
///
/// Example: `НачатьТранзакцию([РежимБлокировок])`
fn format_function_signature(function: &GlobalFunction) -> String {
    let params: Vec<_> = function
        .parameters
        .iter()
        .map(|p| {
            let ty = p.param_type.as_deref().unwrap_or("Произвольный");
            if p.is_optional {
                format!("[{}]", ty)
            } else {
                format!("<{}>", ty)
            }
        })
        .collect();

    let ret_part = function.return_type.as_ref().map(|r| format!(" -> {}", r)).unwrap_or_default();

    format!("{}({}){}", function.name, params.join(", "), ret_part)
}

/// Generates function snippet with parameter placeholders.
///
/// LSP snippet format with tab stops:
/// - $1, $2, $3 - Tab stop positions
/// - ${1:placeholder} - Tab stop with placeholder text
/// - $0 - Final cursor position
///
/// Example: `НачатьТранзакцию(${1:[РежимБлокировок]})$0`
fn generate_function_snippet(function: &GlobalFunction) -> String {
    if function.parameters.is_empty() {
        // No parameters: just function name with parentheses and final cursor
        return format!("{}()$0", function.name);
    }

    // Generate snippet with parameter placeholders
    let mut snippet = format!("{}(", function.name);

    for (idx, param) in function.parameters.iter().enumerate() {
        if idx > 0 {
            snippet.push_str(", ");
        }

        let param_type = param.param_type.as_deref().unwrap_or("Произвольный");
        let placeholder =
            if param.is_optional { format!("[{}]", param_type) } else { param_type.to_string() };

        snippet.push_str(&format!("${{{}:{}}}", idx + 1, placeholder));
    }

    snippet.push_str(")$0");
    snippet
}

/// Formats function documentation for the completion item.
///
/// Example output:
/// ```text
/// НачатьТранзакцию / BeginTransaction
///
/// Параметры:
/// - РежимБлокировок: РежимУправленияБлокировкойДанных (необязательный)
///
/// Доступность: Сервер, Толстый клиент, Внешнее соединение
/// ```
fn format_function_documentation(function: &GlobalFunction) -> String {
    // Try to get full documentation
    if let Some(full_docs) = PlatformData::instance().get_global_function_docs(function.id) {
        return format_function_documentation_full(function, &full_docs);
    }

    // Fallback to basic documentation
    format_function_documentation_basic(function)
}

/// Formats global function with full documentation from platform data.
fn format_function_documentation_full(
    function: &GlobalFunction,
    docs: &bsl_platform::MethodDocs,
) -> String {
    let mut doc = format!("{} / {}\n\n", function.name, function.english_name);

    // Description
    if !docs.description.is_empty() {
        doc.push_str(&docs.description);
        doc.push_str("\n\n");
    }

    // Parameters with detailed descriptions
    if !docs.params.is_empty() {
        doc.push_str("Параметры:\n");
        for param in &docs.params {
            doc.push_str(&format!("- {}", param.name));
            if !param.description.is_empty() {
                doc.push_str(&format!(": {}", param.description));
            }
            doc.push('\n');
        }
        doc.push('\n');
    }

    // Return type
    if let Some(ret_type) = &function.return_type {
        doc.push_str(&format!("Возвращает: {}\n\n", ret_type));
    }

    // Examples (first example only for completion)
    if !docs.examples.is_empty() {
        if let Some(example) = docs.examples.first() {
            doc.push_str("Пример:\n");
            doc.push_str(&example.code);
            doc.push_str("\n\n");
        }
    }

    // Context availability
    if let Some(ctx) = &function.context {
        let mut parts = Vec::new();
        if ctx.thick_client {
            parts.push("Толстый клиент");
        }
        if ctx.thin_client {
            parts.push("Тонкий клиент");
        }
        if ctx.web_client {
            parts.push("Веб-клиент");
        }
        if ctx.server {
            parts.push("Сервер");
        }
        if ctx.mobile_client {
            parts.push("Мобильный клиент");
        }
        if ctx.external_connection {
            parts.push("Внешнее соединение");
        }

        if !parts.is_empty() {
            doc.push_str(&format!("Доступность: {}", parts.join(", ")));
        }
    }

    doc
}

/// Formats global function with basic documentation (fallback).
fn format_function_documentation_basic(function: &GlobalFunction) -> String {
    let mut doc = format!("{} / {}\n\n", function.name, function.english_name);

    if !function.parameters.is_empty() {
        doc.push_str("Параметры:\n");
        for param in &function.parameters {
            let param_type = param.param_type.as_deref().unwrap_or("Произвольный");
            let optional = if param.is_optional { " (необязательный)" } else { "" };
            doc.push_str(&format!("- {}: {}{}\n", param.name, param_type, optional));
        }
        doc.push('\n');
    }

    if let Some(ret_type) = &function.return_type {
        doc.push_str(&format!("Возвращает: {}\n\n", ret_type));
    }

    // Context availability
    if let Some(ctx) = &function.context {
        let mut parts = Vec::new();
        if ctx.thick_client {
            parts.push("Толстый клиент");
        }
        if ctx.thin_client {
            parts.push("Тонкий клиент");
        }
        if ctx.web_client {
            parts.push("Веб-клиент");
        }
        if ctx.server {
            parts.push("Сервер");
        }
        if ctx.mobile_client {
            parts.push("Мобильный клиент");
        }
        if ctx.external_connection {
            parts.push("Внешнее соединение");
        }

        if !parts.is_empty() {
            doc.push_str(&format!("Доступность: {}", parts.join(", ")));
        }
    }

    doc
}

/// Returns detail and documentation for a BSL keyword.
fn get_keyword_info(keyword: &str) -> (String, String) {
    // Try to get full keyword documentation from platform data
    if let Some(keyword_docs) = bsl_platform::PlatformData::instance().get_keyword_docs(keyword) {
        let mut doc = format!("{} / {}\n\n", keyword_docs.keyword_ru, keyword_docs.keyword_en);

        // Syntax
        if !keyword_docs.syntax.is_empty() {
            doc.push_str("**Синтаксис:**\n```bsl\n");
            doc.push_str(&keyword_docs.syntax);
            doc.push_str("\n```\n\n");
        }

        // Description
        if !keyword_docs.description.is_empty() {
            doc.push_str(&keyword_docs.description);
            doc.push_str("\n\n");
        }

        // Parameters
        if !keyword_docs.params.is_empty() {
            doc.push_str("**Параметры:**\n");
            for param in &keyword_docs.params {
                doc.push_str(&format!("- **{}**: {}\n", param.name, param.description));
            }
            doc.push('\n');
        }

        // Version
        if let Some(ref version) = keyword_docs.min_version {
            doc.push_str(&format!("**Доступен с версии:** {}", version));
        }

        return ("Ключевое слово BSL".to_string(), doc);
    }

    // Fallback for keywords without documentation
    ("Ключевое слово BSL".to_string(), format!("**{}**\n\nКлючевое слово языка BSL.", keyword))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_function_signature() {
        use bsl_platform::PlatformDataInner;

        // Skip if no platform data available
        let data = PlatformDataInner::instance();
        if data.all_global_functions().is_empty() {
            println!("Skipping test: no global functions available");
            return;
        }

        // Get НачатьТранзакцию function
        let function = data.get_global_function("НачатьТранзакцию");
        assert!(function.is_some(), "Should find НачатьТранзакцию");

        let function = function.unwrap();
        let sig = format_function_signature(function);

        println!("Signature: {}", sig);
        assert!(sig.contains("НачатьТранзакцию"));
        assert!(sig.contains('('));
        assert!(sig.contains(')'));
    }

    #[test]
    fn test_generate_function_snippet() {
        use bsl_platform::PlatformDataInner;

        let data = PlatformDataInner::instance();
        if data.all_global_functions().is_empty() {
            println!("Skipping test: no global functions available");
            return;
        }

        let function = data.get_global_function("НачатьТранзакцию").unwrap();
        let snippet = generate_function_snippet(function);

        println!("Snippet: {}", snippet);
        assert!(snippet.starts_with("НачатьТранзакцию("));
        assert!(snippet.ends_with(")$0"));
    }

    #[test]
    fn test_complete_global_functions_with_prefix() {
        use bsl_platform::PlatformDataInner;

        let data = PlatformDataInner::instance();
        if data.all_global_functions().is_empty() {
            println!("Skipping test: no global functions available");
            return;
        }

        // Test with prefix "Начать" - should find НачатьТранзакцию
        let items = complete_global_functions("Начать");

        println!("Found {} completions for 'Начать'", items.len());
        assert!(!items.is_empty(), "Should find functions starting with 'Начать'");

        // Should contain НачатьТранзакцию
        let has_begin_transaction = items.iter().any(|i| i.label == "НачатьТранзакцию");
        assert!(has_begin_transaction, "Should contain НачатьТранзакцию");

        // All should be functions
        for item in &items {
            assert_eq!(item.kind, CompletionItemKind::Function);
        }
    }

    #[test]
    fn test_complete_global_functions_case_insensitive() {
        use bsl_platform::PlatformDataInner;

        let data = PlatformDataInner::instance();
        if data.all_global_functions().is_empty() {
            println!("Skipping test: no global functions available");
            return;
        }

        // Test with lowercase prefix
        let items_lower = complete_global_functions("начать");
        let items_upper = complete_global_functions("НАЧАТЬ");
        let items_mixed = complete_global_functions("Начать");

        // Should find the same functions regardless of case
        assert_eq!(items_lower.len(), items_upper.len());
        assert_eq!(items_lower.len(), items_mixed.len());
    }

    #[test]
    fn test_render_global_function() {
        use bsl_platform::PlatformDataInner;

        let data = PlatformDataInner::instance();
        if data.all_global_functions().is_empty() {
            println!("Skipping test: no global functions available");
            return;
        }

        let function = data.get_global_function("НачатьТранзакцию").unwrap();
        let item = render_global_function(function);

        assert_eq!(item.label, "НачатьТранзакцию");
        assert_eq!(item.kind, CompletionItemKind::Function);
        assert!(item.detail.is_some());
        assert!(item.documentation.is_some());

        // Snippet should end with $0
        assert!(item.insert_text.ends_with("$0"));
    }
}
