//! BSL code completion.
//!
//! Provides completion for BSL code context:
//! - Global platform functions (НачатьТранзакцию, Формат, Сообщить, etc.)
//! - BSL keywords (Процедура, Функция, Если, etc.)
//! - User-defined symbols (module functions, variables)
//! - Local symbols (parameters, local variables)

use bsl_platform::{GlobalFunction, PlatformDataInner};
use either::Either;
use hir::{ExprScopes, ScopeDef};
use ide_db::{RootDatabase, TextRange};
use syntax::{ast::AstNode, NodeOrToken, SyntaxKind};

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

    // Find token at position.
    //
    // When the cursor is on the boundary between IDENT and trivia (whitespace,
    // newline, punctuation) — e.g. right after typing "Foo" — `right_biased`
    // returns the trivia token. That would skip the typing branch and dump
    // every symbol in the project into the completion list. Prefer the
    // left-biased IDENT/keyword in that case so the prefix filter kicks in.
    let token = match root.token_at_offset(position.offset) {
        syntax::TokenAtOffset::None => {
            tracing::info!("No token at position - returning None");
            return None;
        }
        syntax::TokenAtOffset::Single(t) => t,
        syntax::TokenAtOffset::Between(left, right) => {
            if left.kind() == SyntaxKind::IDENT || left.kind().is_keyword() {
                left
            } else {
                right
            }
        }
    };

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
                source: None,
            };
            completions.push(keyword_item);
        }

        // Add local symbols (parameters, local variables) FIRST - they shadow everything
        completions.extend(complete_local_symbols(db, position.file_id, position.offset, prefix));

        // Add user-defined symbols (module methods, variables)
        completions.extend(complete_user_defined_symbols(db, position.file_id, prefix));

        // Managed-form attributes (Объект, Замечание, ТаблицаРасходов) come
        // AFTER locals AND user-defined symbols — symmetric with the
        // cascade in `infer_path_name`: parameters / `Перем` / module-level
        // methods all shadow a same-named form attribute. Suppress
        // collisions case-insensitively against everything we have so far.
        let shadow_labels: std::collections::HashSet<String> =
            completions.iter().map(|c| c.label.to_lowercase()).collect();
        completions.extend(
            complete_form_attributes(db, position.file_id, prefix)
                .into_iter()
                .filter(|c| !shadow_labels.contains(&c.label.to_lowercase())),
        );

        // Add MDO types and objects from metadata
        completions.extend(complete_mdo_symbols(db, position.file_id, prefix));

        // Also add global functions that match the prefix
        completions.extend(complete_global_functions(prefix));

        tracing::info!(count = completions.len(), "Returning BSL completions");
        return Some(completions);
    }

    // Check if we're at a trigger position where expression is expected
    // but nothing is typed yet (e.g., inside parentheses, after comma, empty line)
    if is_expression_start_position(&token) {
        tracing::info!(token_kind = ?token.kind(), "Expression start position - completing with empty prefix");
        let mut completions = Vec::new();

        // Local symbols first (highest priority)
        for mut item in complete_local_symbols(db, position.file_id, position.offset, "") {
            item.sort_text = Some(format!("0_{}", item.label));
            completions.push(item);
        }
        // User-defined methods
        for mut item in complete_user_defined_symbols(db, position.file_id, "") {
            item.sort_text = Some(format!("1_{}", item.label));
            completions.push(item);
        }
        // Managed-form attributes — symmetric with `infer_path_name`
        // cascade: parameters / `Перем` / module methods shadow same-named
        // form attributes. Dedup case-insensitively against everything
        // produced so far.
        let shadow_labels: std::collections::HashSet<String> =
            completions.iter().map(|c| c.label.to_lowercase()).collect();
        for mut item in complete_form_attributes(db, position.file_id, "")
            .into_iter()
            .filter(|c| !shadow_labels.contains(&c.label.to_lowercase()))
        {
            item.sort_text = Some(format!("1_5_{}", item.label));
            completions.push(item);
        }
        // Global functions
        for mut item in complete_global_functions("") {
            item.sort_text = Some(format!("2_{}", item.label));
            completions.push(item);
        }
        // MDO collections and common modules (lowest priority in argument context)
        for mut item in complete_mdo_symbols(db, position.file_id, "") {
            item.sort_text = Some(format!("3_{}", item.label));
            completions.push(item);
        }

        tracing::info!(count = completions.len(), "Returning BSL completions (trigger position)");
        return Some(completions);
    }

    // No BSL completion context
    tracing::info!("No BSL completion context - returning None");
    None
}

/// Completes managed-form attributes declared in `Form.xml`.
///
/// Inside a managed form, bare identifiers like `Замечание`, `Объект`,
/// `ТаблицаРасходов` resolve as реквизиты формы — type inference covers
/// them via [`hir_ty::form_attr::resolve_form_attribute`]; this surface
/// makes them visible in the unqualified completion list too.
///
/// Returns an empty list for non-form modules and ordinary forms (managed
/// gate is symmetric with the type-system layer).
fn complete_form_attributes<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    prefix: &str,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_form_attributes").entered();

    let module_id = hir::ModuleId::new(file_id);
    let metadata = db.module_metadata(module_id);
    let Some(form) = metadata.form.as_ref() else { return Vec::new() };
    if !form.is_managed() {
        return Vec::new();
    }

    let prefix_lower = prefix.to_lowercase();
    let mut items = Vec::with_capacity(form.attributes().len());
    for attr in form.attributes() {
        if !attr.name.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }
        let detail = if attr.is_main {
            format!("{} (основной реквизит формы)", attr.attr_type)
        } else {
            format!("{} (реквизит формы)", attr.attr_type)
        };
        items.push(CompletionItem {
            label: attr.name.clone(),
            detail: Some(detail),
            kind: CompletionItemKind::Field,
            insert_text: attr.name.clone(),
            documentation: None,
            sort_text: None,
            filter_text: None,
            source: None,
        });
    }
    items
}

/// Completes local symbols (parameters and local variables).
///
/// Returns completion items for symbols in the current method scope:
/// - Parameters (procedure/function parameters)
/// - Local variables (declared with Перем)
///
/// Symbols are filtered by prefix (case-insensitive).
fn complete_local_symbols<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    offset: syntax::TextSize,
    prefix: &str,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_local_symbols").entered();

    let mut completions = Vec::new();
    let prefix_lower = prefix.to_lowercase();

    // Parse file and find token at offset
    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let token = match root.token_at_offset(offset).right_biased() {
        Some(t) => t,
        None => return completions,
    };

    // Find containing method
    let (method_def, method_range) = match find_method_for_token(&token) {
        Some((def, range)) => (def, range),
        None => return completions, // Not inside a method
    };

    // Build ExprScopes for this method (parameters + Перем declarations)
    let scopes = match &method_def {
        Either::Left(proc) => ExprScopes::from_procedure(proc),
        Either::Right(func) => ExprScopes::from_function(func),
    };

    let root_scope = scopes.root_scope();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Get all entries from root scope (parameters + local variables)
    for (name, scope_def) in scopes.all_entries_in_scope(root_scope) {
        let name_str = name.as_str();
        seen.insert(name_str.to_lowercase());

        if !name_str.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }

        let (kind, detail) = match scope_def {
            ScopeDef::Parameter => (CompletionItemKind::Field, "Параметр"),
            ScopeDef::LocalVariable => (CompletionItemKind::Field, "Локальная переменная"),
        };

        completions.push(CompletionItem {
            label: name_str.to_string(),
            detail: Some(detail.to_string()),
            kind,
            insert_text: name_str.to_string(),
            documentation: None,
            sort_text: None,
            filter_text: None,
            source: None,
        });
    }

    // Collect implicit variables from assignment targets (e.g. Партнер = ...)
    // This is done here (not in ExprScopes) because ExprScopes doesn't know about
    // module-level variables, and we'd incorrectly shadow them.
    let body_node = match &method_def {
        Either::Left(proc) => proc.body().map(|b| b.syntax().clone()),
        Either::Right(func) => func.body().map(|b| b.syntax().clone()),
    };
    if let Some(body) = body_node {
        for node in body.descendants() {
            if node.kind() != SyntaxKind::ASSIGN_STMT {
                continue;
            }
            let Some(first) = node.first_child_or_token() else { continue };
            if first.kind() != SyntaxKind::IDENT {
                continue;
            }
            // The parser wraps identifiers in an IDENT node (not a bare token),
            // so first_child_or_token() returns NodeOrToken::Node for simple
            // assignments like `Партнер = ...`. We need to handle both cases.
            let text: String = match &first {
                NodeOrToken::Token(t) => t.text().to_string(),
                NodeOrToken::Node(n) => match n.first_token() {
                    Some(t) => t.text().to_string(),
                    None => continue,
                },
            };
            let lower = text.to_lowercase();
            if seen.contains(&lower) {
                continue;
            }
            if !lower.starts_with(&prefix_lower) {
                seen.insert(lower);
                continue;
            }
            seen.insert(lower);
            completions.push(CompletionItem {
                label: text.clone(),
                detail: Some("Переменная".to_string()),
                kind: CompletionItemKind::Field,
                insert_text: text,
                documentation: None,
                sort_text: None,
                filter_text: None,
                source: None,
            });
        }
    }

    tracing::debug!(
        count = completions.len(),
        prefix = ?prefix,
        method_range = ?method_range,
        "Completed local symbols"
    );

    completions
}

/// Find containing method for a token.
///
/// Returns the method AST node and its text range.
fn find_method_for_token(
    token: &syntax::SyntaxToken,
) -> Option<(Either<syntax::ast::ProcedureDef, syntax::ast::FunctionDef>, TextRange)> {
    use syntax::ast;

    // Walk up ancestors to find containing method
    for ancestor in token.parent()?.ancestors() {
        if let Some(proc) = ast::ProcedureDef::cast(ancestor.clone()) {
            let method_range = proc.syntax().text_range();
            return Some((Either::Left(proc), method_range));
        }
        if let Some(func) = ast::FunctionDef::cast(ancestor.clone()) {
            let method_range = func.syntax().text_range();
            return Some((Either::Right(func), method_range));
        }
    }
    None
}

/// Check if the token indicates a position where an expression is expected
/// but nothing has been typed yet (trigger position for empty-prefix completion).
///
/// Examples: `Foo(|)`, `Foo(x, |)`, empty line inside method body.
fn is_expression_start_position(token: &syntax::SyntaxToken) -> bool {
    match token.kind() {
        // Inside parentheses: Foo(|) or Foo(x, |)
        SyntaxKind::R_PAREN | SyntaxKind::L_PAREN | SyntaxKind::COMMA => true,
        // Semicolon: after end of statement, new statement expected
        SyntaxKind::SEMICOLON => true,
        // Whitespace/newline: check previous non-trivia token for context
        SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {
            // Walk backwards to find previous non-trivia token
            let mut prev = token.prev_token();
            while let Some(ref t) = prev {
                if !t.kind().is_trivia() {
                    break;
                }
                prev = t.prev_token();
            }
            match prev {
                Some(t) => !matches!(t.kind(), SyntaxKind::DOT),
                None => true,
            }
        }
        _ => false,
    }
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

/// Completes top-level MDO symbols (directly callable at statement start).
///
/// Returns completion items for:
/// - MDO plural forms (Справочники, Документы, РегистрыСведений, etc.)
/// - Common modules (callable directly: `МойМодуль.Функция()`)
///
/// Concrete metadata objects (Валюты, Номенклатура, etc.) are intentionally
/// excluded: in BSL they are only reachable through their collection
/// (`Справочники.Номенклатура`), and completion after the DOT is handled by
/// `mdo_completion.rs`.
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
                source: None,
            });
        }
    }

    // 2. Add common modules from all configurations (main + extensions).
    //    Common modules are the only metadata objects callable directly by
    //    name; other MDO objects must be accessed through their collection.
    for (ext_name, config) in db.get_all_configurations(file_id) {
        use bsl_metadata::traits::MdObject;
        for module in config.common_modules() {
            let name = module.name();

            if !name.to_lowercase().starts_with(&prefix_lower) {
                continue;
            }

            completions.push(CompletionItem {
                label: name.to_string(),
                detail: Some("Общий модуль".to_string()),
                kind: CompletionItemKind::MdoObject,
                insert_text: name.to_string(),
                documentation: Some(format!("{name}\n\nОбщий модуль конфигурации.")),
                sort_text: None,
                filter_text: None,
                source: ext_name.clone(),
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
            source: None,
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
            source: None,
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

/// Renders a global function as a completion item via the unified
/// `symbol_info` pipeline.
fn render_global_function(function: &GlobalFunction) -> CompletionItem {
    let docs = PlatformDataInner::instance().get_global_function_docs(function.id);
    let sig = symbol_info::from_global_function(function, docs.as_ref());
    super::platform_completion::item_from_signature(&sig)
}

/// Returns detail and documentation for a BSL keyword.
fn get_keyword_info(keyword: &str) -> (String, String) {
    // Try to get full keyword documentation from platform data.
    // allow: keyword docs (M3 exception) — keywords aren't part of the
    // type system, so they fall outside Invariant #3. Documented in
    // `docs/architecture/TYPE_SYSTEM.md`; `scripts/check-invariants.sh`
    // uses this comment as the white-list marker.
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

        // Snippet should end with $0)
        assert!(item.insert_text.ends_with("$0)"));
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

    #[test]
    fn test_complete_local_symbols() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Процедура Тест(Первый, Второй)
    Перем МояПеременная;
    Перем ДругаяПеременная;

    // Курсор здесь - вводим "Мо"
    Мо
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

        // Find offset of "Мо" in source
        let offset = source.find("Мо").expect("Should find 'Мо' in source");
        let offset = syntax::TextSize::from(offset as u32);

        // Test completion with prefix "Мо"
        let items = complete_local_symbols(&db, file_id, offset, "Мо");

        println!("Found {} local items for prefix 'Мо'", items.len());
        for item in &items {
            println!("  - {} ({:?}, {:?})", item.label, item.kind, item.detail);
        }

        // Should find МояПеременная
        assert_eq!(items.len(), 1, "Should find 1 local variable starting with 'Мо'");
        assert_eq!(items[0].label, "МояПеременная");
        assert_eq!(items[0].detail, Some("Локальная переменная".to_string()));
    }

    #[test]
    fn test_complete_local_symbols_parameters() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Функция Тест(ПервыйПараметр, ВторойПараметр)
    Перем ЛокальнаяПеременная;

    // Курсор здесь - вводим "Перв"
    Перв
КонецФункции
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

        // Find offset of "Перв" in source
        let offset = source.find("Перв").expect("Should find 'Перв' in source");
        let offset = syntax::TextSize::from(offset as u32);

        // Test completion with prefix "Перв"
        let items = complete_local_symbols(&db, file_id, offset, "Перв");

        println!("Found {} local items for prefix 'Перв'", items.len());
        for item in &items {
            println!("  - {} ({:?}, {:?})", item.label, item.kind, item.detail);
        }

        // Should find ПервыйПараметр
        assert_eq!(items.len(), 1, "Should find 1 parameter starting with 'Перв'");
        assert_eq!(items[0].label, "ПервыйПараметр");
        assert_eq!(items[0].detail, Some("Параметр".to_string()));
    }

    #[test]
    fn test_complete_local_symbols_all() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Процедура Тест(Параметр1, Параметр2)
    Перем Переменная1;
    Перем Переменная2;

    // Empty prefix - should return all

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

        // Find offset after "// Empty prefix"
        let offset = source.find("// Empty prefix").expect("Should find comment") + 20;
        let offset = syntax::TextSize::from(offset as u32);

        // Test completion with empty prefix
        let items = complete_local_symbols(&db, file_id, offset, "");

        println!("Found {} total local items", items.len());
        for item in &items {
            println!("  - {} ({:?}, {:?})", item.label, item.kind, item.detail);
        }

        // Should find all 4 symbols (2 parameters + 2 variables)
        assert_eq!(items.len(), 4, "Should find all local symbols");

        // Check we have both parameters
        let param_count = items.iter().filter(|i| i.detail == Some("Параметр".to_string())).count();
        assert_eq!(param_count, 2, "Should have 2 parameters");

        // Check we have both variables
        let var_count =
            items.iter().filter(|i| i.detail == Some("Локальная переменная".to_string())).count();
        assert_eq!(var_count, 2, "Should have 2 local variables");
    }

    #[test]
    fn test_implicit_variables_from_assignments() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Процедура Тест(Запрос)
    Партнер = Справочники.Партнеры.НайтиПоКоду("001");
    Результат = Новый Структура;
    Результат.Вставить("Партнер", Партнер);
    ВременнаяПеременная = 42;
КонецПроцедуры
"#;
        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);
        db.set_file_text(file_id, source);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);

        // Position inside the method body (after assignments)
        let offset = syntax::TextSize::from(source.find("ВременнаяПеременная").unwrap() as u32);

        let items = complete_local_symbols(&db, file_id, offset, "");

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        println!("Found local symbols: {:?}", labels);

        // Should find parameter
        assert!(labels.contains(&"Запрос"), "Should find parameter Запрос");

        // Should find implicit variables from assignments
        assert!(labels.contains(&"Партнер"), "Should find implicit var Партнер");
        assert!(labels.contains(&"Результат"), "Should find implicit var Результат");
        assert!(
            labels.contains(&"ВременнаяПеременная"),
            "Should find implicit var ВременнаяПеременная"
        );

        // Implicit vars should have detail "Переменная"
        let implicit_count =
            items.iter().filter(|i| i.detail == Some("Переменная".to_string())).count();
        assert_eq!(implicit_count, 3, "Should have 3 implicit variables");
    }

    #[test]
    fn test_complete_form_attributes_skips_non_form_module() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        // Plain BSL source — no Form.xml association in the VFS, so
        // `module_metadata.form` is None. The completion source must
        // gracefully return an empty list (gate is symmetric with the
        // type-system layer).
        let source = "Процедура Test() КонецПроцедуры\n";
        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, source);

        let items = complete_form_attributes(&db, file_id, "");
        assert!(items.is_empty(), "non-form file must not surface form attributes");
    }
}
