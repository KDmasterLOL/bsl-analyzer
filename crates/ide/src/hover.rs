//! Hover information provider.
//!
//! This module provides hover information for BSL code, including:
//! - Platform types (Строка, Число, Массив, etc.)
//! - Platform methods with signatures and documentation
//! - User-defined symbols (future)

use bsl_platform::{
    platform_method_query, platform_type_query, type_methods_query, ContextAvailability,
    MethodLookupInput, PlatformMethod, TypeNameInput,
};
use ide_db::RootDatabase;
use syntax::{SyntaxKind, SyntaxToken, TextRange, TextSize};
use vfs::FileId;

use crate::HoverResult;

/// Returns hover information at the specified position.
pub(crate) fn hover(
    db: &dyn RootDatabase,
    file_id: FileId,
    offset: TextSize,
) -> Option<HoverResult> {
    let _span = tracing::info_span!("hover", ?file_id, ?offset).entered();

    // Parse the file
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    // Find token at position
    let token = root.token_at_offset(offset).right_biased()?;

    tracing::debug!(token_kind = ?token.kind(), token_text = ?token.text(), "Hover token");

    // Try platform type/method hover
    if let Some(result) = hover_platform(db, &token) {
        return Some(result);
    }

    // TODO: Add hover for user-defined symbols
    // TODO: Add hover for keywords
    // TODO: Add hover for literals

    None
}

/// Attempts to provide hover information for platform types and methods.
fn hover_platform(db: &dyn RootDatabase, token: &SyntaxToken) -> Option<HoverResult> {
    let token_text = token.text();

    // Check if this is an identifier
    if token.kind() != SyntaxKind::IDENT {
        return None;
    }

    // Try to determine context: is this a type reference or a method call?
    let parent = token.parent()?;
    let parent_kind = parent.kind();

    tracing::debug!(?parent_kind, "Parent node kind");

    // Check if it's a method call (e.g., Строка.ВРег())
    if let Some((type_name, method_name)) = try_extract_method_call(token) {
        return hover_for_platform_method(db, &type_name, &method_name, token.text_range());
    }

    // Check if it's a type reference (e.g., variable declaration type)
    // For now, just try to look it up as a platform type
    hover_for_platform_type(db, token_text, token.text_range())
}

/// Attempts to extract method call context (receiver type + method name).
///
/// Example: `Строка.ВРег()` -> Some(("Строка", "ВРег"))
fn try_extract_method_call(token: &SyntaxToken) -> Option<(String, String)> {
    let _parent = token.parent()?;

    // Check if we're in a method call expression
    // AST structure: MethodCallExpr -> NameRef (method name)
    // We need to traverse up to find the receiver

    // For MVP, we'll use a simple heuristic:
    // If there's a DOT before this identifier, check what's before the dot
    let mut prev_sibling = token.prev_sibling_or_token();

    // Skip whitespace
    while let Some(sibling) = &prev_sibling {
        if sibling.kind() == SyntaxKind::WHITESPACE {
            prev_sibling = sibling.prev_sibling_or_token();
        } else {
            break;
        }
    }

    // Check if previous is DOT
    if let Some(sibling) = prev_sibling {
        if sibling.kind() == SyntaxKind::DOT {
            // Get what's before the dot
            let mut prev = sibling.prev_sibling_or_token();

            // Skip whitespace
            while let Some(s) = &prev {
                if s.kind() == SyntaxKind::WHITESPACE {
                    prev = s.prev_sibling_or_token();
                } else {
                    break;
                }
            }

            if let Some(receiver) = prev {
                if receiver.kind() == SyntaxKind::IDENT {
                    let receiver_text = receiver.as_token()?.text().to_string();
                    let method_text = token.text().to_string();
                    return Some((receiver_text, method_text));
                }
            }
        }
    }

    None
}

/// Generates hover information for platform types.
///
/// Example output:
/// ```markdown
/// **Тип:** Строка / String
///
/// **Доступность:** Толстый клиент, Тонкий клиент, Веб-клиент, Сервер
///
/// **Версия:** 8.0+
///
/// **Методы:**
/// - ВРег() / Upper() -> Строка
/// - НРег() / Lower() -> Строка
/// - Длина() / Length() -> Число
/// ...
/// ```
fn hover_for_platform_type(
    db: &dyn RootDatabase,
    type_name: &str,
    range: TextRange,
) -> Option<HoverResult> {
    let input = TypeNameInput::new(db, type_name.to_string());
    let platform_type = platform_type_query(db, input)?;

    let mut markup = String::new();

    // Type header
    markup
        .push_str(&format!("**Тип:** {} / {}\n\n", platform_type.name, platform_type.english_name));

    // Context availability
    if let Some(ctx) = &platform_type.context {
        markup.push_str(&format!("**Доступность:** {}\n\n", format_context_availability(ctx)));
    }

    // Version info
    if let Some(version) = &platform_type.min_version {
        markup.push_str(&format!("**Версия:** {}+\n\n", version));
    }

    // Methods preview (first 10)
    let methods_input = TypeNameInput::new(db, type_name.to_string());
    let methods = type_methods_query(db, methods_input);

    if !methods.is_empty() {
        markup.push_str("**Методы:**\n");
        for method in methods.iter().take(10) {
            let sig = format_method_signature(method);
            markup.push_str(&format!("- {}\n", sig));
        }
        if methods.len() > 10 {
            markup.push_str(&format!("\n... и еще {} методов", methods.len() - 10));
        }
    }

    Some(HoverResult { markup, range: Some(range) })
}

/// Generates hover information for platform methods.
///
/// Example output:
/// ```markdown
/// **Метод:** ВРег / Upper
/// **Тип:** Строка
///
/// **Синтаксис:**
/// ```bsl
/// ВРег(<Строка>) -> Строка
/// Upper(<String>) -> String
/// ```
///
/// **Параметры:**
/// - Строка: Строка
///
/// **Возвращает:** Строка
///
/// **Доступность:** Все контексты
/// ```
fn hover_for_platform_method(
    db: &dyn RootDatabase,
    type_name: &str,
    method_name: &str,
    range: TextRange,
) -> Option<HoverResult> {
    let input = MethodLookupInput::new(db, type_name.to_string(), method_name.to_string());
    let method = platform_method_query(db, input)?;

    let mut markup = String::new();

    // Method header
    markup.push_str(&format!(
        "**Метод:** {} / {}\n\n**Тип:** {}\n\n",
        method.name, method.english_name, method.type_name
    ));

    // Syntax (bilingual)
    markup.push_str("**Синтаксис:**\n```bsl\n");
    markup.push_str(&format!("{}\n", format_method_signature(&method)));

    // English variant
    let english_sig = format_method_signature_english(&method);
    markup.push_str(&format!("{}\n", english_sig));
    markup.push_str("```\n\n");

    // Parameters
    if !method.parameters.is_empty() {
        markup.push_str("**Параметры:**\n");
        for param in &method.parameters {
            let optional = if param.is_optional { " (необязательный)" } else { "" };
            let param_type = param.param_type.as_deref().unwrap_or("Произвольный");
            markup.push_str(&format!("- {}: {}{}\n", param.name, param_type, optional));
        }
        markup.push('\n');
    }

    // Return type
    if let Some(ret_type) = &method.return_type {
        markup.push_str(&format!("**Возвращает:** {}\n\n", ret_type));
    }

    // Context availability
    if let Some(ctx) = &method.context {
        markup.push_str(&format!("**Доступность:** {}", format_context_availability(ctx)));
    }

    Some(HoverResult { markup, range: Some(range) })
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Formats method signature in Russian.
///
/// Example: `ВРег(<Строка>) -> Строка`
fn format_method_signature(method: &PlatformMethod) -> String {
    let params: Vec<_> = method
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

    let ret_part = method.return_type.as_ref().map(|r| format!(" -> {}", r)).unwrap_or_default();

    format!("{}({}){}", method.name, params.join(", "), ret_part)
}

/// Formats method signature in English.
///
/// Example: `Upper(<String>) -> String`
fn format_method_signature_english(method: &PlatformMethod) -> String {
    let params: Vec<_> = method
        .parameters
        .iter()
        .map(|p| {
            let ty = p.param_type.as_deref().unwrap_or("Arbitrary");
            if p.is_optional {
                format!("[{}]", ty)
            } else {
                format!("<{}>", ty)
            }
        })
        .collect();

    let ret_part = method.return_type.as_ref().map(|r| format!(" -> {}", r)).unwrap_or_default();

    format!("{}({}){}", method.english_name, params.join(", "), ret_part)
}

/// Formats context availability as human-readable string.
///
/// Example: "Толстый клиент, Тонкий клиент, Веб-клиент, Сервер"
fn format_context_availability(ctx: &ContextAvailability) -> String {
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

    if parts.is_empty() {
        "Недоступно".to_string()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_platform::PlatformDataInner;

    #[test]
    fn test_format_method_signature() {
        // Skip if no platform data available
        let data = PlatformDataInner::instance();
        if data.all_methods().is_empty() {
            println!("Skipping test: no platform methods available");
            return;
        }

        // Get first method with parameters
        let method = data
            .all_methods()
            .iter()
            .find(|m| !m.parameters.is_empty())
            .expect("Should have at least one method with parameters");

        let sig = format_method_signature(method);

        // Should contain method name and parentheses
        assert!(sig.contains(&method.name.to_string()));
        assert!(sig.contains('('));
        assert!(sig.contains(')'));
    }

    #[test]
    fn test_format_context_availability() {
        let ctx = ContextAvailability {
            thick_client: true,
            thin_client: true,
            web_client: false,
            server: true,
            mobile_client: false,
            external_connection: false,
        };

        let formatted = format_context_availability(&ctx);

        assert!(formatted.contains("Толстый клиент"));
        assert!(formatted.contains("Тонкий клиент"));
        assert!(formatted.contains("Сервер"));
        assert!(!formatted.contains("Веб-клиент"));
    }

    #[test]
    fn test_format_context_availability_empty() {
        let ctx = ContextAvailability {
            thick_client: false,
            thin_client: false,
            web_client: false,
            server: false,
            mobile_client: false,
            external_connection: false,
        };

        let formatted = format_context_availability(&ctx);

        assert_eq!(formatted, "Недоступно");
    }
}
