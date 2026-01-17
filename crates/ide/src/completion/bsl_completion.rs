//! BSL code completion.
//!
//! Provides completion for BSL code context:
//! - Global platform functions (НачатьТранзакцию, Формат, Сообщить, etc.)
//! - BSL keywords (Процедура, Функция, Если, etc.)
//! - User-defined symbols (module functions, variables)

use bsl_platform::{GlobalFunction, PlatformData, PlatformDataInner};
use ide_db::RootDatabase;
use syntax::SyntaxKind;

use super::{CompletionItem, CompletionItemKind, CompletionPosition};

/// Attempts to provide BSL code completions.
///
/// Returns Some(items) if this is a BSL completion context (not after DOT),
/// otherwise returns None.
pub(super) fn bsl_completions<DB: RootDatabase>(
    db: &DB,
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
                sort_text: None,
                filter_text: None,
            };
            completions.push(keyword_item);
        }

        // Add user-defined symbols (module methods, variables)
        completions.extend(complete_user_defined_symbols(db, position.file_id, prefix));

        // Add MDO types and objects from metadata
        completions.extend(complete_mdo_symbols(db, position.file_id, prefix));

        // Also add global functions that match the prefix
        completions.extend(complete_global_functions(prefix));

        tracing::info!(count = completions.len(), "Returning BSL completions");
        return Some(completions);
    }

    // No BSL completion context
    tracing::info!("No BSL completion context - returning None");
    None
}

/// Helper: Get plural Russian form for MDO type.
fn mdo_type_plural_ru(mdo_type: &bsl_metadata::MdoType) -> &'static str {
    match mdo_type {
        bsl_metadata::MdoType::Document => "Документы",
        bsl_metadata::MdoType::Catalog => "Справочники",
        bsl_metadata::MdoType::InformationRegister => "РегистрыСведений",
        bsl_metadata::MdoType::AccumulationRegister => "РегистрыНакопления",
        bsl_metadata::MdoType::AccountingRegister => "РегистрыБухгалтерии",
        bsl_metadata::MdoType::CalculationRegister => "РегистрыРасчета",
        bsl_metadata::MdoType::ChartOfCharacteristicTypes => "ПланыВидовХарактеристик",
        bsl_metadata::MdoType::ChartOfAccounts => "ПланыСчетов",
        bsl_metadata::MdoType::ChartOfCalculationTypes => "ПланыВидовРасчета",
        bsl_metadata::MdoType::BusinessProcess => "БизнесПроцессы",
        bsl_metadata::MdoType::Task => "Задачи",
        bsl_metadata::MdoType::Enum => "Перечисления",
        bsl_metadata::MdoType::ExchangePlan => "ПланыОбмена",
        bsl_metadata::MdoType::ExternalDataSource => "ВнешниеИсточникиДанных",
        bsl_metadata::MdoType::Cube => "Кубы",
        bsl_metadata::MdoType::DimensionTable => "ТаблицыИзмерения",
        bsl_metadata::MdoType::Constant => "Константы",
        bsl_metadata::MdoType::DataProcessor => "Обработки",
        bsl_metadata::MdoType::Report => "Отчеты",
        bsl_metadata::MdoType::CommonModule => "ОбщиеМодули",
    }
}

/// Helper: Get plural English form for MDO type.
fn mdo_type_plural_en(mdo_type: &bsl_metadata::MdoType) -> &'static str {
    match mdo_type {
        bsl_metadata::MdoType::Document => "Documents",
        bsl_metadata::MdoType::Catalog => "Catalogs",
        bsl_metadata::MdoType::InformationRegister => "InformationRegisters",
        bsl_metadata::MdoType::AccumulationRegister => "AccumulationRegisters",
        bsl_metadata::MdoType::AccountingRegister => "AccountingRegisters",
        bsl_metadata::MdoType::CalculationRegister => "CalculationRegisters",
        bsl_metadata::MdoType::ChartOfCharacteristicTypes => "ChartsOfCharacteristicTypes",
        bsl_metadata::MdoType::ChartOfAccounts => "ChartsOfAccounts",
        bsl_metadata::MdoType::ChartOfCalculationTypes => "ChartsOfCalculationTypes",
        bsl_metadata::MdoType::BusinessProcess => "BusinessProcesses",
        bsl_metadata::MdoType::Task => "Tasks",
        bsl_metadata::MdoType::Enum => "Enums",
        bsl_metadata::MdoType::ExchangePlan => "ExchangePlans",
        bsl_metadata::MdoType::ExternalDataSource => "ExternalDataSources",
        bsl_metadata::MdoType::Cube => "Cubes",
        bsl_metadata::MdoType::DimensionTable => "DimensionTables",
        bsl_metadata::MdoType::Constant => "Constants",
        bsl_metadata::MdoType::DataProcessor => "DataProcessors",
        bsl_metadata::MdoType::Report => "Reports",
        bsl_metadata::MdoType::CommonModule => "CommonModules",
    }
}

/// Completes MDO (Metadata Objects) types and instances.
///
/// Returns completion items for:
/// - MDO plural forms (Справочники, Документы, РегистрыСведений, etc.)
/// - MDO instances from configuration (Валюты, ПКО, ОчередьЗапросовERP, etc.)
///
/// Symbols are filtered by prefix (case-insensitive).
fn complete_mdo_symbols<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    prefix: &str,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_mdo_symbols").entered();

    let mut completions = Vec::new();
    let prefix_lower = prefix.to_lowercase();

    // 1. Add MDO plural forms (collection types)
    for mdo_type in bsl_metadata::MdoType::all() {
        let plural_ru = mdo_type_plural_ru(mdo_type);
        let plural_en = mdo_type_plural_en(mdo_type);

        // Check if matches prefix (case-insensitive)
        if plural_ru.to_lowercase().starts_with(&prefix_lower)
            || plural_en.to_lowercase().starts_with(&prefix_lower)
        {
            completions.push(CompletionItem {
                label: plural_ru.to_string(),
                detail: Some(format!("Коллекция метаданных ({})", mdo_type.russian_name())),
                kind: CompletionItemKind::MdoType,
                insert_text: plural_ru.to_string(),
                documentation: Some(format!(
                    "{} / {}\n\nКоллекция объектов метаданных типа {}.",
                    plural_ru,
                    plural_en,
                    mdo_type.russian_name()
                )),
                sort_text: None,
                filter_text: None,
            });
        }
    }

    // 2. Add MDO instances from configuration
    if let Some(config) = db.get_configuration(file_id) {
        for mdo in config.metadata_objects() {
            let name = &mdo.name;

            // Filter by prefix
            if !name.to_lowercase().starts_with(&prefix_lower) {
                continue;
            }

            let detail = mdo.mdo_type.russian_name().to_string();

            completions.push(CompletionItem {
                label: name.clone(),
                detail: Some(detail),
                kind: CompletionItemKind::MdoObject,
                insert_text: name.clone(),
                documentation: Some(format!(
                    "{}\n\nОбъект метаданных типа {}.",
                    name,
                    mdo.mdo_type.russian_name()
                )),
                sort_text: None,
                filter_text: None,
            });
        }
    }

    tracing::debug!(
        count = completions.len(),
        prefix = ?prefix,
        "Completed MDO symbols"
    );

    completions
}

/// Completes user-defined symbols (module methods and variables).
///
/// Returns completion items for:
/// - Module procedures and functions
/// - Module variables
///
/// Symbols are filtered by prefix (case-insensitive).
fn complete_user_defined_symbols<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    prefix: &str,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_user_defined_symbols").entered();

    let mut completions = Vec::new();
    let prefix_lower = prefix.to_lowercase();

    // Get module for this file via Semantics API
    let sema = hir::Semantics::new(db);
    let module = sema.module_from_file(file_id);

    // Add procedures
    for procedure in module.procedures() {
        let name = procedure.name();
        let name_str = name.as_str();

        // Filter by prefix
        if !name_str.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }

        let is_export = procedure.is_export();
        let detail =
            if is_export { "Процедура Экспорт" } else { "Процедура" };

        completions.push(CompletionItem {
            label: name_str.to_string(),
            detail: Some(detail.to_string()),
            kind: CompletionItemKind::Function,
            insert_text: format!("{}()$0", name_str),
            documentation: None,
            sort_text: None,
            filter_text: None,
        });
    }

    // Add functions
    for function in module.functions() {
        let name = function.name();
        let name_str = name.as_str();

        // Filter by prefix
        if !name_str.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }

        let is_export = function.is_export();
        let detail = if is_export { "Функция Экспорт" } else { "Функция" };

        completions.push(CompletionItem {
            label: name_str.to_string(),
            detail: Some(detail.to_string()),
            kind: CompletionItemKind::Function,
            insert_text: format!("{}()$0", name_str),
            documentation: None,
            sort_text: None,
            filter_text: None,
        });
    }

    // Add module variables
    for variable in module.variables() {
        let name = variable.name();
        let name_str = name.as_str();

        // Filter by prefix
        if !name_str.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }

        let is_export = variable.is_export();
        let detail =
            if is_export { "Переменная Экспорт" } else { "Переменная" };

        completions.push(CompletionItem::simple(
            name_str.to_string(),
            CompletionItemKind::Field,
            name_str.to_string(),
        ));

        // Set detail after creation
        if let Some(item) = completions.last_mut() {
            item.detail = Some(detail.to_string());
        }
    }

    tracing::debug!(
        count = completions.len(),
        prefix = ?prefix,
        "Completed user-defined symbols"
    );

    completions
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
        sort_text: None,
        filter_text: None,
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

    #[test]
    fn test_complete_user_defined_symbols() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Перем МояПеременная Экспорт;
Перем ПриватнаяПеременная;

Процедура МояПроцедура() Экспорт
    // тело
КонецПроцедуры

Функция МояФункция()
    Возврат 42;
КонецФункции

Процедура ДругаяПроцедура()
    Моя
КонецПроцедуры
"#;

        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, source);

        // Test completion with prefix "Моя"
        let items = complete_user_defined_symbols(&db, file_id, "Моя");

        println!("Found {} items for prefix 'Моя'", items.len());
        for item in &items {
            println!("  - {} ({:?})", item.label, item.kind);
        }

        // Should find 3 items: МояПеременная, МояПроцедура, МояФункция
        assert_eq!(items.len(), 3, "Should find 3 items starting with 'Моя'");

        // Check that МояПроцедура is present
        let has_procedure = items.iter().any(|i| i.label == "МояПроцедура");
        assert!(has_procedure, "Should contain МояПроцедура");

        // Check that МояФункция is present
        let has_function = items.iter().any(|i| i.label == "МояФункция");
        assert!(has_function, "Should contain МояФункция");

        // Check that МояПеременная is present
        let has_variable = items.iter().any(|i| i.label == "МояПеременная");
        assert!(has_variable, "Should contain МояПеременная");

        // Check export flag for МояПроцедура
        let procedure_item = items.iter().find(|i| i.label == "МояПроцедура").unwrap();
        assert_eq!(
            procedure_item.detail,
            Some("Процедура Экспорт".to_string()),
            "МояПроцедура should be marked as Export"
        );

        // Check non-export МояФункция
        let function_item = items.iter().find(|i| i.label == "МояФункция").unwrap();
        assert_eq!(
            function_item.detail,
            Some("Функция".to_string()),
            "МояФункция should NOT be marked as Export"
        );
    }

    #[test]
    fn test_complete_user_defined_symbols_case_insensitive() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Процедура ТестоваяПроцедура()
КонецПроцедуры
"#;

        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, source);

        // Test with different cases
        let items_lower = complete_user_defined_symbols(&db, file_id, "тест");
        let items_upper = complete_user_defined_symbols(&db, file_id, "ТЕСТ");
        let items_mixed = complete_user_defined_symbols(&db, file_id, "Тест");

        // All should find the same procedure
        assert_eq!(items_lower.len(), 1);
        assert_eq!(items_upper.len(), 1);
        assert_eq!(items_mixed.len(), 1);

        assert_eq!(items_lower[0].label, "ТестоваяПроцедура");
        assert_eq!(items_upper[0].label, "ТестоваяПроцедура");
        assert_eq!(items_mixed[0].label, "ТестоваяПроцедура");
    }

    #[test]
    fn test_complete_mdo_plural_forms() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Процедура Тест()
    Справ
КонецПроцедуры
"#;

        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, source);

        // Test completion with prefix "Справ"
        let items = complete_mdo_symbols(&db, file_id, "Справ");

        println!("Found {} MDO items for prefix 'Справ'", items.len());
        for item in &items {
            println!("  - {} ({:?})", item.label, item.kind);
        }

        // Should find Справочники
        let has_catalogs = items.iter().any(|i| i.label == "Справочники");
        assert!(has_catalogs, "Should contain Справочники plural form");

        // Check kind
        let catalogs_item = items.iter().find(|i| i.label == "Справочники").unwrap();
        assert_eq!(catalogs_item.kind, CompletionItemKind::MdoType);
    }

    #[test]
    fn test_complete_mdo_symbols_bilingual() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Процедура Тест()
    Docu
КонецПроцедуры
"#;

        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, source);

        // Test with English prefix "Docu"
        let items = complete_mdo_symbols(&db, file_id, "Docu");

        println!("Found {} MDO items for prefix 'Docu'", items.len());

        // Should find Документы (Russian label, but matches English "Documents")
        let has_documents = items.iter().any(|i| i.label == "Документы");
        assert!(has_documents, "Should contain Документы (matched by English 'Documents')");
    }
}
