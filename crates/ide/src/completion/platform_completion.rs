//! Platform method completion.
//!
//! Provides completion for platform types and methods:
//! - Method completion after DOT (e.g., `Строка.` shows ВРег, НРег, etc.)
//! - Snippets with parameter placeholders

use bsl_platform::{type_methods_query, PlatformData, PlatformMethod, TypeNameInput};
use hir::{InferenceResult, Name, Ty};
use ide_db::RootDatabase;
use syntax::ast::{self, AstNode};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use super::{CompletionItem, CompletionItemKind, CompletionPosition};

/// Attempts to provide platform method completions.
///
/// Returns Some(items) if this is a method completion context (after DOT),
/// otherwise returns None to allow other completion providers to handle it.
///
/// Supports:
/// - Simple variable: `МойМассив.` → methods of Массив
/// - Direct type: `Строка.` → methods of Строка
/// - Fluent chains: `Запрос.Выполнить().Выбрать().` → methods of return type
pub(super) fn platform_completions(
    db: &dyn RootDatabase,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let _span = tracing::info_span!("platform_completions").entered();

    let parse = db.parse(position.file_id);
    let root = parse.syntax_node();

    let token = root.token_at_offset(position.offset).left_biased()?;

    tracing::debug!(token_kind = ?token.kind(), token_text = ?token.text(), "Completion token");

    if token.kind() != SyntaxKind::DOT {
        return None;
    }

    // Resolve the type of the expression before the DOT
    let receiver_expr = find_receiver_expr(&token)?;
    let infer_result = db.infer(position.file_id);

    let receiver_ty = resolve_syntax_expr_type(&receiver_expr, &infer_result);
    tracing::debug!(receiver_ty = ?receiver_ty, "Resolved receiver type");

    let type_name = receiver_ty.platform_type_name()?;
    tracing::debug!(type_name = ?type_name, "Platform type for completion");

    Some(complete_platform_methods(db, type_name))
}

/// Find the receiver expression node before the DOT token.
///
/// Walks up the syntax tree from DOT to find the parent expression,
/// then returns its first child (the receiver).
///
/// Handles cases like:
/// - `ident.` → parent is FIELD_EXPR, receiver is IDENT
/// - `expr.Method().` → parent is FIELD_EXPR, receiver is CALL_EXPR
fn find_receiver_expr(dot_token: &SyntaxToken) -> Option<SyntaxNode> {
    let parent = dot_token.parent()?;

    // DOT is inside a FIELD_EXPR: the first child node is the receiver
    if parent.kind() == SyntaxKind::FIELD_EXPR {
        return parent.children().next();
    }

    // Fallback: look at the previous sibling node of the DOT
    for sibling in dot_token.siblings_with_tokens(syntax::Direction::Prev).skip(1) {
        if sibling.kind() == SyntaxKind::WHITESPACE {
            continue;
        }
        if let Some(node) = sibling.as_node() {
            return Some(node.clone());
        }
        if let Some(token) = sibling.as_token() {
            return token.parent();
        }
        return None;
    }

    None
}

/// Recursively resolve the type of a syntax expression node.
///
/// This is a lightweight syntax-level type resolver for completion.
/// It uses inference results (var_types) for variables and platform data
/// for method return types to support fluent chains.
fn resolve_syntax_expr_type(node: &SyntaxNode, infer_result: &InferenceResult) -> Ty {
    match node.kind() {
        // Simple identifier — look up in var_types or treat as type name
        SyntaxKind::IDENT | SyntaxKind::EXPR => {
            if let Some(ident_token) = node.first_token() {
                if ident_token.kind() == SyntaxKind::IDENT {
                    return resolve_ident_type(ident_token.text(), infer_result);
                }
            }
            Ty::Unknown
        }

        // Call expression: callee(args) — resolve callee type
        // For method calls like `obj.Method()`, this is a CALL_EXPR wrapping a FIELD_EXPR
        SyntaxKind::CALL_EXPR => resolve_call_expr_type(node, infer_result),

        // Field expression: base.field — resolve for intermediate chains
        SyntaxKind::FIELD_EXPR => resolve_field_expr_type(node, infer_result),

        // New expression: Новый Type(...)
        SyntaxKind::NEW_EXPR => {
            if let Some(new_expr) = ast::NewExpr::cast(node.clone()) {
                Ty::from_new_expr(&new_expr)
            } else {
                Ty::Unknown
            }
        }

        // Parenthesized expression
        SyntaxKind::PAREN_EXPR => node
            .children()
            .next()
            .map(|child| resolve_syntax_expr_type(&child, infer_result))
            .unwrap_or(Ty::Unknown),

        _ => {
            // Try to find an IDENT token in this node (fallback)
            if let Some(ident) = node.first_token().filter(|t| t.kind() == SyntaxKind::IDENT) {
                return resolve_ident_type(ident.text(), infer_result);
            }
            Ty::Unknown
        }
    }
}

/// Resolve type of an identifier (variable name or type name).
fn resolve_ident_type(name: &str, infer_result: &InferenceResult) -> Ty {
    let key = name.to_lowercase();

    // Check var_types from inference
    if let Some(ty) = infer_result.var_types.get(&key) {
        if !ty.is_unknown() {
            return ty.clone();
        }
    }

    // Check if it's a known platform type name directly (e.g., `Строка.`)
    let ty = Ty::from_type_name(name);
    if !ty.is_unknown() {
        return ty;
    }

    // Check if it's a known platform type (e.g., `Запрос.`)
    let data = PlatformData::instance();
    if !data.get_type_methods(name).is_empty() {
        return Ty::PlatformObject(Name::new(name));
    }

    Ty::Unknown
}

/// Resolve type of a CALL_EXPR node.
///
/// Structure: CALL_EXPR → [callee_expr, ARG_LIST]
/// If callee is a FIELD_EXPR (method call), resolve receiver type + method return type.
fn resolve_call_expr_type(node: &SyntaxNode, infer_result: &InferenceResult) -> Ty {
    let callee = node.children().next();
    let callee = match callee {
        Some(c) => c,
        None => return Ty::Unknown,
    };

    // Method call: callee is FIELD_EXPR (base.method)
    if callee.kind() == SyntaxKind::FIELD_EXPR {
        let mut children = callee.children();
        let base = children.next();
        // Skip to find method name (IDENT after DOT)
        let method_name = callee
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::IDENT)
            .last();

        if let (Some(base_node), Some(method_token)) = (base, method_name) {
            let base_ty = resolve_syntax_expr_type(&base_node, infer_result);
            if let Some(type_name) = base_ty.platform_type_name() {
                let data = PlatformData::instance();
                if let Some(method) = data.get_method(type_name, method_token.text()) {
                    if let Some(ret_type) = &method.return_type {
                        let ty = Ty::from_type_name(ret_type);
                        if !ty.is_unknown() {
                            return ty;
                        }
                        return Ty::PlatformObject(Name::new(ret_type));
                    }
                    return Ty::Undefined;
                }
            }
        }
    }

    // Simple function call — could check global function return types
    // For now, return Unknown
    Ty::Unknown
}

/// Resolve type of a FIELD_EXPR node (base.field).
///
/// Used for intermediate chain resolution.
fn resolve_field_expr_type(node: &SyntaxNode, infer_result: &InferenceResult) -> Ty {
    // FIELD_EXPR has: base_expr, DOT, IDENT(field_name)
    let base = node.children().next();
    let field_name = node
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .last();

    if let (Some(base_node), Some(_field_token)) = (base, field_name) {
        // For now, resolve base type (field access on platform types returns Unknown)
        resolve_syntax_expr_type(&base_node, infer_result)
    } else {
        Ty::Unknown
    }
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
