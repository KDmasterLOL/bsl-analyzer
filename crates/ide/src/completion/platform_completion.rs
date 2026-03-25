//! Platform method completion.
//!
//! Provides completion for platform types and methods:
//! - Method completion after DOT (e.g., `Строка.` shows ВРег, НРег, etc.)
//! - Snippets with parameter placeholders

use bsl_platform::{type_methods_query, PlatformData, PlatformMethod, TypeNameInput};
use ide_db::RootDatabase;
use syntax::{SyntaxKind, SyntaxToken};

use super::{CompletionItem, CompletionItemKind, CompletionPosition};

/// Attempts to provide platform method completions.
///
/// Returns Some(items) if this is a method completion context (after DOT),
/// otherwise returns None to allow other completion providers to handle it.
pub(super) fn platform_completions(
    db: &dyn RootDatabase,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let _span = tracing::info_span!("platform_completions").entered();

    // Parse the file
    let parse = db.parse(position.file_id);
    let root = parse.syntax_node();

    // Find token at position (left-biased to catch DOT)
    let token = root.token_at_offset(position.offset).left_biased()?;

    tracing::debug!(token_kind = ?token.kind(), token_text = ?token.text(), "Completion token");

    // Check if we're right after a DOT
    if token.kind() == SyntaxKind::DOT {
        // Get the receiver (what's before the dot)
        if let Some(receiver_type) = extract_receiver_type(&token) {
            tracing::debug!(receiver_type = ?receiver_type, "Detected method completion context");
            return Some(complete_platform_methods(db, &receiver_type));
        }
    }

    // Not a platform method completion context
    None
}

/// Extracts the receiver type from the token before DOT.
///
/// Example: `Строка.` -> Some("Строка")
fn extract_receiver_type(dot_token: &SyntaxToken) -> Option<String> {
    // Get previous sibling (skip whitespace)
    let mut prev = dot_token.prev_sibling_or_token();

    // Skip whitespace
    while let Some(sibling) = &prev {
        if sibling.kind() == SyntaxKind::WHITESPACE {
            prev = sibling.prev_sibling_or_token();
        } else {
            break;
        }
    }

    // Check if it's an identifier
    if let Some(receiver) = prev {
        if receiver.kind() == SyntaxKind::IDENT {
            // Try to get as token first
            if let Some(token) = receiver.as_token() {
                let receiver_text = token.text().to_string();
                tracing::debug!(receiver = ?receiver_text, "Found receiver (token)");
                return Some(receiver_text);
            }

            // If it's a node, get the first token child
            if let Some(node) = receiver.as_node() {
                if let Some(token) = node.first_token() {
                    let receiver_text = token.text().to_string();
                    tracing::debug!(receiver = ?receiver_text, "Found receiver (from node)");
                    return Some(receiver_text);
                }
            }
        }
    }

    None
}

/// Completes platform methods for a receiver type.
///
/// Example: For receiver "Строка", shows: ВРег, НРег, Длина, etc.
fn complete_platform_methods(db: &dyn RootDatabase, receiver_type: &str) -> Vec<CompletionItem> {
    let input = TypeNameInput::new(db, receiver_type.to_string());
    let methods = type_methods_query(db, input);

    tracing::debug!(method_count = methods.len(), "Found platform methods");

    methods.iter().map(render_platform_method).collect()
}

/// Renders a platform method as a completion item.
///
/// Generates a completion item with:
/// - Label: Russian method name (e.g., "ВРег")
/// - Detail: Signature with return type
/// - Insert text: Snippet with parameter placeholders
/// - Documentation: Short description
fn render_platform_method(method: &PlatformMethod) -> CompletionItem {
    // Label: Russian name
    let label = method.name.to_string();

    // Detail: Signature with return type
    let detail = format_method_signature(method);

    // Insert text: Snippet with placeholders
    let insert_text = generate_method_snippet(method);

    // Documentation: Bilingual signature + parameters
    let documentation = Some(format_method_documentation(method));

    CompletionItem {
        label,
        detail: Some(detail),
        kind: CompletionItemKind::Method,
        insert_text,
        documentation,
        sort_text: None,
        filter_text: None,
        source: None,
    }
}

/// Formats method signature for the detail field.
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

/// Generates method snippet with parameter placeholders.
///
/// LSP snippet format with tab stops:
/// - $1, $2, $3 - Tab stop positions
/// - ${1:placeholder} - Tab stop with placeholder text
/// - $0 - Final cursor position
///
/// Example: `ВРег(${1:Строка})$0`
fn generate_method_snippet(method: &PlatformMethod) -> String {
    if method.parameters.is_empty() {
        // No parameters: just method name with parentheses and final cursor
        return format!("{}()$0", method.name);
    }

    // Generate snippet with parameter placeholders
    let mut snippet = format!("{}(", method.name);

    for (idx, param) in method.parameters.iter().enumerate() {
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

/// Formats method documentation for the completion item.
///
/// Example output:
/// ```text
/// ВРег / Upper
///
/// Параметры:
/// - Строка: Строка
///
/// Возвращает: Строка
/// ```
fn format_method_documentation(method: &PlatformMethod) -> String {
    // Try to get full documentation
    if let Some(full_docs) = PlatformData::instance().get_method_docs(method.id) {
        return format_method_documentation_full(method, &full_docs);
    }

    // Fallback to basic documentation
    format_method_documentation_basic(method)
}

/// Formats platform method with full documentation from platform data.
fn format_method_documentation_full(
    method: &PlatformMethod,
    docs: &bsl_platform::MethodDocs,
) -> String {
    let mut doc = format!("{} / {}\n\n", method.name, method.english_name);

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
    if let Some(ret_type) = &method.return_type {
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

    doc
}

/// Formats platform method with basic documentation (fallback).
fn format_method_documentation_basic(method: &PlatformMethod) -> String {
    let mut doc = format!("{} / {}\n\n", method.name, method.english_name);

    if !method.parameters.is_empty() {
        doc.push_str("Параметры:\n");
        for param in &method.parameters {
            let param_type = param.param_type.as_deref().unwrap_or("Произвольный");
            let optional = if param.is_optional { " (необязательный)" } else { "" };
            doc.push_str(&format!("- {}: {}{}\n", param.name, param_type, optional));
        }
        doc.push('\n');
    }

    if let Some(ret_type) = &method.return_type {
        doc.push_str(&format!("Возвращает: {}", ret_type));
    }

    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_platform::{ContextAvailability, MethodParam, PlatformMethod};

    fn create_test_method() -> PlatformMethod {
        PlatformMethod {
            id: 999999, // Use invalid ID to ensure fallback to basic docs
            type_name: "Строка".into(),
            name: "ВРег".into(),
            english_name: "Upper".into(),
            return_type: Some("Строка".into()),
            parameters: vec![MethodParam {
                name: "Значение".into(),
                param_type: Some("Строка".into()),
                is_optional: false,
            }],
            min_version: Some("8.0".into()),
            context: Some(ContextAvailability {
                thick_client: true,
                thin_client: true,
                web_client: true,
                server: true,
                mobile_client: false,
                external_connection: true,
            }),
        }
    }

    #[test]
    fn test_format_method_signature() {
        let method = create_test_method();
        let sig = format_method_signature(&method);

        assert_eq!(sig, "ВРег(<Строка>) -> Строка");
    }

    #[test]
    fn test_generate_method_snippet() {
        let method = create_test_method();
        let snippet = generate_method_snippet(&method);

        assert_eq!(snippet, "ВРег(${1:Строка})$0");
    }

    #[test]
    fn test_generate_snippet_no_params() {
        let method = PlatformMethod {
            id: 999998,
            type_name: "Число".into(),
            name: "Цел".into(),
            english_name: "Int".into(),
            return_type: Some("Число".into()),
            parameters: vec![],
            min_version: None,
            context: None,
        };

        let snippet = generate_method_snippet(&method);

        assert_eq!(snippet, "Цел()$0");
    }

    #[test]
    fn test_generate_snippet_optional_param() {
        let method = PlatformMethod {
            id: 999997,
            type_name: "Строка".into(),
            name: "Лев".into(),
            english_name: "Left".into(),
            return_type: Some("Строка".into()),
            parameters: vec![
                MethodParam {
                    name: "Строка".into(),
                    param_type: Some("Строка".into()),
                    is_optional: false,
                },
                MethodParam {
                    name: "Длина".into(),
                    param_type: Some("Число".into()),
                    is_optional: true,
                },
            ],
            min_version: None,
            context: None,
        };

        let snippet = generate_method_snippet(&method);

        assert_eq!(snippet, "Лев(${1:Строка}, ${2:[Число]})$0");
    }

    #[test]
    fn test_format_method_documentation() {
        let method = create_test_method();
        let doc = format_method_documentation(&method);

        // Should contain method name
        assert!(doc.contains("ВРег / Upper"));
        // Should use fallback docs (since id=999999 doesn't exist)
        assert!(doc.contains("Параметры:"));
        assert!(doc.contains("Значение: Строка"));
        assert!(doc.contains("Возвращает: Строка"));
    }

    #[test]
    fn test_render_platform_method() {
        let method = create_test_method();
        let item = render_platform_method(&method);

        assert_eq!(item.label, "ВРег");
        assert_eq!(item.kind, CompletionItemKind::Method);
        assert_eq!(item.insert_text, "ВРег(${1:Строка})$0");
        assert!(item.detail.is_some());
        assert!(item.documentation.is_some());
    }

    #[test]
    fn test_end_to_end_platform_completion() {
        use bsl_platform::PlatformDataInner;
        use ide_db::base_db::SourceDatabase;
        use ide_db::RootDatabaseImpl;
        use syntax::TextSize;
        use vfs::FileId;

        // Skip if no platform data available
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() || data.all_methods().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        // Create database and add file
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // BSL code with cursor after DOT
        // Use "XBase" type which has 30 methods in platform data
        let code = r#"Процедура Тест()
    Результат = XBase.
КонецПроцедуры"#;

        // Set file content
        db.set_file_text(file_id, code);

        // Position is right after the DOT (end of "XBase.")
        // We want left_biased to catch the DOT token
        let dot_end = code.find("XBase.").unwrap() + "XBase.".len();
        let offset = TextSize::from(dot_end as u32);

        // Request completions at the DOT position
        let position = CompletionPosition { file_id, offset, workspace_root: None };

        let items = platform_completions(&db, position);

        // Should have platform method completions
        assert!(items.is_some(), "Expected platform completions after DOT on XBase type");

        let items = items.unwrap();
        assert!(!items.is_empty(), "Expected at least one method completion");

        // All items should be methods
        for item in &items {
            assert_eq!(item.kind, CompletionItemKind::Method);
        }

        // Should contain common String methods if platform data is available
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        println!("Found {} method completions", labels.len());

        // If we have methods, verify snippet format
        for item in &items {
            // All methods should have snippets ending with $0
            assert!(
                item.insert_text.ends_with("$0"),
                "Method snippet should end with $0: {}",
                item.insert_text
            );

            // All methods should have parentheses
            assert!(
                item.insert_text.contains('(') && item.insert_text.contains(')'),
                "Method snippet should have parentheses: {}",
                item.insert_text
            );
        }
    }
}
