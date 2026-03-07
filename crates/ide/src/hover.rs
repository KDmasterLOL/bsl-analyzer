//! Hover information provider.
//!
//! This module provides hover information for BSL code, including:
//! - Platform types (Строка, Число, Массив, etc.)
//! - Platform methods with signatures and documentation
//! - User-defined symbols (methods, variables, parameters)

use bsl_platform::{
    global_function_query, platform_method_query, platform_type_query, type_methods_query,
    ContextAvailability, GlobalFunction, MethodDocs, MethodLookupInput, PlatformData,
    PlatformMethod, TypeNameInput,
};
use ide_db::RootDatabase;
use syntax::{SyntaxKind, SyntaxToken, TextRange, TextSize};
use vfs::FileId;

use crate::HoverResult;

/// Returns hover information at the specified position.
pub(crate) fn hover<DB: RootDatabase>(
    db: &DB,
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

    // Try user-defined symbols (via Definition API) FIRST
    // This has higher priority than platform symbols (local shadowing)
    if let Some(result) = hover_user_defined(db, file_id, &token) {
        return Some(result);
    }

    // Try platform type/method hover
    if let Some(result) = hover_platform(db, &token) {
        return Some(result);
    }

    // Try keyword hover
    if let Some(result) = hover_keyword(&token) {
        return Some(result);
    }

    // TODO: Add hover for literals

    None
}

/// Attempts to provide hover information for user-defined symbols (via Definition API).
///
/// This includes:
/// - Methods (procedures and functions)
/// - Variables (module-level and local)
/// - Parameters
///
/// Returns `None` for symbols that aren't user-defined or can't be resolved.
fn hover_user_defined<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    token: &SyntaxToken,
) -> Option<HoverResult> {
    // Only process identifiers
    if token.kind() != SyntaxKind::IDENT {
        return None;
    }

    // Use unified Semantics API
    let sema = hir::Semantics::new(db);
    let definition = sema.resolve_name_to_definition(file_id, token)?;

    // Convert Definition to HoverResult
    definition_to_hover(db, &definition, token.text_range())
}

/// Converts a Definition to HoverResult.
fn definition_to_hover<DB: RootDatabase>(
    db: &DB,
    definition: &hir::Definition,
    range: TextRange,
) -> Option<HoverResult> {
    let mut markup = String::new();

    match definition {
        hir::Definition::Method(_method_id) => {
            // Get method signature
            let label = definition.label(db);
            markup.push_str(&format!("**{}**\n\n", label));

            // Add export info if present
            if definition.is_export(db) {
                markup.push_str("*Экспортная*\n\n");
            }

            // Add documentation if available
            if let Some(docs) = definition.docs(db) {
                // Purpose
                if let Some(ref purpose) = docs.purpose {
                    if !purpose.is_empty() {
                        markup.push_str("**Назначение:**\n");
                        markup.push_str(purpose);
                        markup.push_str("\n\n");
                    }
                }

                // Parameters
                if !docs.parameters.is_empty() {
                    markup.push_str("**Параметры:**\n");
                    for param in &docs.parameters {
                        markup.push_str(&format!("- **{}**", param.name));

                        // Format types
                        if !param.types.is_empty() {
                            let type_names: Vec<_> =
                                param.types.iter().map(|t| t.name.as_str()).collect();
                            markup.push_str(&format!(": {}", type_names.join(", ")));
                        }

                        // Add description from first type if available
                        if let Some(first_type) = param.types.first() {
                            if let Some(ref desc) = first_type.description {
                                if !desc.is_empty() {
                                    markup.push_str(&format!(" - {}", desc));
                                }
                            }
                        }

                        markup.push('\n');
                    }
                    markup.push('\n');
                }

                // Return value
                if !docs.returned_value.is_empty() {
                    markup.push_str("**Возвращаемое значение:**\n");
                    let type_names: Vec<_> =
                        docs.returned_value.iter().map(|t| t.name.as_str()).collect();
                    markup.push_str(&format!("Тип: {}\n", type_names.join(", ")));

                    // Add description from first type if available
                    if let Some(first_type) = docs.returned_value.first() {
                        if let Some(ref desc) = first_type.description {
                            if !desc.is_empty() {
                                markup.push_str(&format!("{}\n", desc));
                            }
                        }
                    }
                    markup.push('\n');
                }

                // Examples
                if !docs.examples.is_empty() {
                    markup.push_str("**Примеры:**\n");
                    for (idx, example) in docs.examples.iter().enumerate() {
                        markup.push_str(&format!("{}. {}\n\n", idx + 1, example));
                    }
                }
            }
        }

        hir::Definition::Variable(_) => {
            if let Some(name) = definition.name(db) {
                markup.push_str(&format!("**Перем {}**\n\n", name.as_str()));

                if definition.is_export(db) {
                    markup.push_str("*Экспортная*\n\n");
                }
            } else {
                markup.push_str("**Переменная**\n\n");
            }

            // TODO: Add variable type info when available
        }

        hir::Definition::Parameter { param_name, .. } => {
            markup.push_str(&format!("**Параметр {}**\n\n", param_name.as_str()));
            // TODO: Add parameter type info when available
        }

        hir::Definition::Local { var_name, .. } => {
            markup.push_str(&format!("**Локальная переменная {}**\n\n", var_name.as_str()));
            // TODO: Add local variable type info when available
        }

        hir::Definition::Module(_module_id) => {
            markup.push_str("**Модуль**\n\n");
        }

        hir::Definition::MdoCollectionType(mdo_type) => {
            markup.push_str(&format!("**Тип метаданных:** {}\n\n", mdo_type.russian_name()));
            markup.push_str("*Коллекция объектов метаданных*");
        }

        hir::Definition::MdoObject { mdo_type, object_name } => {
            markup.push_str(&format!(
                "**{}.{}**\n\n",
                mdo_type.russian_name(),
                object_name.as_str()
            ));
            markup.push_str("*Объект метаданных*");
        }

        hir::Definition::MdoManagerModule { mdo_type, object_name, .. } => {
            markup.push_str(&format!(
                "**Менеджер модуль: {}.{}**\n\n",
                mdo_type.russian_name(),
                object_name.as_str()
            ));
            markup.push_str("*Модуль менеджера объекта метаданных*");
        }

        // Don't show hover for builtins (they're handled by hover_platform)
        hir::Definition::BuiltinFunction(_)
        | hir::Definition::BuiltinMethod { .. }
        | hir::Definition::VirtualTableField { .. }
        | hir::Definition::Unresolved => return None,
    }

    Some(HoverResult { markup, range: Some(range) })
}

/// Attempts to provide hover information for platform types and methods.
fn hover_platform<DB: RootDatabase>(db: &DB, token: &SyntaxToken) -> Option<HoverResult> {
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

    // Check if it's a global function (e.g., НачатьТранзакцию())
    if let Some(result) = hover_for_global_function(db, token_text, token.text_range()) {
        return Some(result);
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
fn hover_for_platform_type<DB: RootDatabase>(
    db: &DB,
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
fn hover_for_platform_method<DB: RootDatabase>(
    db: &DB,
    type_name: &str,
    method_name: &str,
    range: TextRange,
) -> Option<HoverResult> {
    let input = MethodLookupInput::new(db, type_name.to_string(), method_name.to_string());
    let method = platform_method_query(db, input)?;

    // Try to get full documentation
    let docs = PlatformData::instance().get_method_docs(method.id);

    let mut markup = String::new();

    if let Some(docs) = docs {
        // Format with full documentation
        format_method_with_full_docs(&mut markup, &method, &docs);
    } else {
        // Fallback to basic information
        format_method_basic(&mut markup, &method);
    }

    Some(HoverResult { markup, range: Some(range) })
}

/// Generates hover information for global platform functions.
///
/// Example output:
/// ```markdown
/// **Глобальная функция:** НачатьТранзакцию / BeginTransaction
///
/// **Синтаксис:**
/// ```bsl
/// НачатьТранзакцию([РежимБлокировок])
/// BeginTransaction([DataLockControlMode])
/// ```
///
/// **Параметры:**
/// - РежимБлокировок: РежимУправленияБлокировкойДанных (необязательный)
///
/// **Доступность:** Сервер, Толстый клиент, Внешнее соединение
/// ```
fn hover_for_global_function<DB: RootDatabase>(
    db: &DB,
    function_name: &str,
    range: TextRange,
) -> Option<HoverResult> {
    let input = TypeNameInput::new(db, function_name.to_string());
    let function = global_function_query(db, input)?;

    // Try to get full documentation
    let docs = PlatformData::instance().get_global_function_docs(function.id);

    let mut markup = String::new();

    if let Some(docs) = docs {
        // Format with full documentation
        format_global_function_with_full_docs(&mut markup, &function, &docs);
    } else {
        // Fallback to basic information
        format_global_function_basic(&mut markup, &function);
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

/// Formats global function signature in Russian.
///
/// Example: `НачатьТранзакцию([РежимБлокировок])`
fn format_global_function_signature(function: &GlobalFunction) -> String {
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

/// Formats global function signature in English.
///
/// Example: `BeginTransaction([DataLockControlMode])`
fn format_global_function_signature_english(function: &GlobalFunction) -> String {
    let params: Vec<_> = function
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

    let ret_part = function.return_type.as_ref().map(|r| format!(" -> {}", r)).unwrap_or_default();

    format!("{}({}){}", function.english_name, params.join(", "), ret_part)
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

/// Formats a method with full documentation.
fn format_method_with_full_docs(markup: &mut String, method: &PlatformMethod, docs: &MethodDocs) {
    // Method header
    markup.push_str(&format!(
        "**Метод:** {} / {}\n**Тип:** {}\n\n",
        method.name, method.english_name, method.type_name
    ));

    // Syntax from documentation
    if !docs.syntax.is_empty() {
        markup.push_str("**Синтаксис:**\n```bsl\n");
        markup.push_str(&docs.syntax);
        markup.push_str("\n```\n\n");
    }

    // Description
    if !docs.description.is_empty() {
        markup.push_str("**Описание:**\n");
        markup.push_str(&docs.description);
        markup.push_str("\n\n");
    }

    // Parameters with detailed descriptions
    if !docs.params.is_empty() {
        markup.push_str("**Параметры:**\n");
        for param in &docs.params {
            markup.push_str(&format!("- **{}**", param.name));
            if !param.description.is_empty() {
                markup.push_str(&format!(": {}", param.description));
            }
            markup.push('\n');
        }
        markup.push('\n');
    }

    // Return type
    if let Some(ref ret_type) = method.return_type {
        markup.push_str(&format!("**Возвращает:** {}\n\n", ret_type));
    }

    // Examples
    if !docs.examples.is_empty() {
        markup.push_str("**Примеры:**\n");
        for (idx, example) in docs.examples.iter().enumerate() {
            if let Some(ref desc) = example.description {
                markup.push_str(&format!("{}. {}\n", idx + 1, desc));
            }
            markup.push_str("```bsl\n");
            markup.push_str(&example.code);
            markup.push_str("\n```\n\n");
        }
    }

    // Notes
    if let Some(ref notes) = docs.notes {
        markup.push_str("**Примечание:**\n");
        markup.push_str(notes);
        markup.push_str("\n\n");
    }

    // Context availability
    if let Some(ref ctx) = method.context {
        markup.push_str(&format!("**Доступность:** {}", format_context_availability(ctx)));
    }
}

/// Formats a method with basic information (fallback when no full docs available).
fn format_method_basic(markup: &mut String, method: &PlatformMethod) {
    // Method header
    markup.push_str(&format!(
        "**Метод:** {} / {}\n\n**Тип:** {}\n\n",
        method.name, method.english_name, method.type_name
    ));

    // Syntax (bilingual)
    markup.push_str("**Синтаксис:**\n```bsl\n");
    markup.push_str(&format!("{}\n", format_method_signature(method)));

    // English variant
    let english_sig = format_method_signature_english(method);
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
}

/// Formats a global function with full documentation.
fn format_global_function_with_full_docs(
    markup: &mut String,
    function: &GlobalFunction,
    docs: &MethodDocs,
) {
    // Function header
    markup.push_str(&format!(
        "**Глобальная функция:** {} / {}\n\n",
        function.name, function.english_name
    ));

    // Syntax from documentation
    if !docs.syntax.is_empty() {
        markup.push_str("**Синтаксис:**\n```bsl\n");
        markup.push_str(&docs.syntax);
        markup.push_str("\n```\n\n");
    }

    // Description
    if !docs.description.is_empty() {
        markup.push_str("**Описание:**\n");
        markup.push_str(&docs.description);
        markup.push_str("\n\n");
    }

    // Parameters with detailed descriptions
    if !docs.params.is_empty() {
        markup.push_str("**Параметры:**\n");
        for param in &docs.params {
            markup.push_str(&format!("- **{}**", param.name));
            if !param.description.is_empty() {
                markup.push_str(&format!(": {}", param.description));
            }
            markup.push('\n');
        }
        markup.push('\n');
    }

    // Return type
    if let Some(ref ret_type) = function.return_type {
        markup.push_str(&format!("**Возвращает:** {}\n\n", ret_type));
    }

    // Examples
    if !docs.examples.is_empty() {
        markup.push_str("**Примеры:**\n");
        for (idx, example) in docs.examples.iter().enumerate() {
            if let Some(ref desc) = example.description {
                markup.push_str(&format!("{}. {}\n", idx + 1, desc));
            }
            markup.push_str("```bsl\n");
            markup.push_str(&example.code);
            markup.push_str("\n```\n\n");
        }
    }

    // Notes
    if let Some(ref notes) = docs.notes {
        markup.push_str("**Примечание:**\n");
        markup.push_str(notes);
        markup.push_str("\n\n");
    }

    // Context availability
    if let Some(ref ctx) = function.context {
        markup.push_str(&format!("**Доступность:** {}", format_context_availability(ctx)));
    }
}

/// Formats a global function with basic information (fallback when no full docs available).
fn format_global_function_basic(markup: &mut String, function: &GlobalFunction) {
    // Function header
    markup.push_str(&format!(
        "**Глобальная функция:** {} / {}\n\n",
        function.name, function.english_name
    ));

    // Syntax (bilingual)
    markup.push_str("**Синтаксис:**\n```bsl\n");
    markup.push_str(&format!("{}\n", format_global_function_signature(function)));

    // English variant
    let english_sig = format_global_function_signature_english(function);
    markup.push_str(&format!("{}\n", english_sig));
    markup.push_str("```\n\n");

    // Parameters
    if !function.parameters.is_empty() {
        markup.push_str("**Параметры:**\n");
        for param in &function.parameters {
            let optional = if param.is_optional { " (необязательный)" } else { "" };
            let param_type = param.param_type.as_deref().unwrap_or("Произвольный");
            markup.push_str(&format!("- {}: {}{}\n", param.name, param_type, optional));
        }
        markup.push('\n');
    }

    // Return type
    if let Some(ret_type) = &function.return_type {
        markup.push_str(&format!("**Возвращает:** {}\n\n", ret_type));
    }

    // Context availability
    if let Some(ctx) = &function.context {
        markup.push_str(&format!("**Доступность:** {}", format_context_availability(ctx)));
    }
}

/// Provides hover information for BSL keywords.
fn hover_keyword(token: &SyntaxToken) -> Option<HoverResult> {
    // Check if this is a keyword token
    if !token.kind().is_keyword() {
        return None;
    }

    let keyword_text = token.text();

    // Try to get keyword documentation
    let keyword_docs = bsl_platform::PlatformData::instance().get_keyword_docs(keyword_text)?;

    let mut markup = String::new();

    // Header
    markup.push_str(&format!(
        "**{}** / **{}**\n\n",
        keyword_docs.keyword_ru, keyword_docs.keyword_en
    ));

    // Syntax
    if !keyword_docs.syntax.is_empty() {
        markup.push_str("**Синтаксис:**\n```bsl\n");
        markup.push_str(&keyword_docs.syntax);
        markup.push_str("\n```\n\n");
    }

    // Description
    if !keyword_docs.description.is_empty() {
        markup.push_str(&keyword_docs.description);
        markup.push_str("\n\n");
    }

    // Parameters
    if !keyword_docs.params.is_empty() {
        markup.push_str("**Параметры:**\n");
        for param in &keyword_docs.params {
            markup.push_str(&format!("- **{}**: {}\n", param.name, param.description));
        }
        markup.push('\n');
    }

    // Version
    if let Some(ref version) = keyword_docs.min_version {
        markup.push_str(&format!("**Доступен с версии:** {}", version));
    }

    Some(HoverResult { markup, range: Some(token.text_range()) })
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

    #[test]
    fn test_format_global_function_signature() {
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
        let sig = format_global_function_signature(function);

        // Should contain function name and parentheses
        assert!(sig.contains("НачатьТранзакцию"));
        assert!(sig.contains('('));
        assert!(sig.contains(')'));

        println!("Russian signature: {}", sig);

        // Test English signature
        let sig_en = format_global_function_signature_english(function);
        assert!(sig_en.contains("BeginTransaction"));
        println!("English signature: {}", sig_en);
    }
}
