//! BSL code completion.
//!
//! Provides completion for BSL code context:
//! - Global platform functions (НачатьТранзакцию, Формат, Сообщить, etc.)
//! - BSL keywords (Процедура, Функция, Если, etc.)
//! - User-defined symbols (module functions, variables)
//! - Local symbols (parameters, local variables)

use std::collections::HashSet;

use bsl_metadata::MdoType;
use bsl_platform::{GlobalFunction, PlatformDataInner};
use either::Either;
use hir::{DefWithBodyId, ExprScopes, ScopeDef};
use ide_db::base_db::Locale;
use ide_db::{RootDatabase, TextRange};
use syntax::{ast::AstNode, SyntaxKind};

use super::{CompletionItem, CompletionItemKind, CompletionPosition};
use crate::completion::platform_completion::{render_mdo_field, render_platform_property};

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

        completions.extend(complete_top_level(
            db,
            position.file_id,
            position.offset,
            prefix,
            position.locale,
            false,
        ));

        tracing::info!(count = completions.len(), "Returning BSL completions");
        return Some(completions);
    }

    // Check if we're at a trigger position where expression is expected
    // but nothing is typed yet (e.g., inside parentheses, after comma, empty line)
    if is_expression_start_position(&token) {
        tracing::info!(token_kind = ?token.kind(), "Expression start position - completing with empty prefix");
        let completions =
            complete_top_level(db, position.file_id, position.offset, "", position.locale, true);

        tracing::info!(count = completions.len(), "Returning BSL completions (trigger position)");
        return Some(completions);
    }

    // No BSL completion context
    tracing::info!("No BSL completion context - returning None");
    None
}

/// Top-level completion accumulator. Iterates sources in the order that
/// mirrors `infer.rs::infer_path_name` cascade (`crates/hir-ty/src/infer.rs:1315-1503`):
///
/// | Band   | Source                              | infer.rs step |
/// |--------|-------------------------------------|---------------|
/// | `00_`  | locals (parameters + Перем)         | 1             |
/// | `10_`  | user-defined module symbols         | 2 (Resolution::Method/Variable) |
/// | `15_`  | managed-form attributes             | 5b            |
/// | `20_`  | MDO plurals                         | 4             |
/// | `25_`  | HBK globals (properties + functions)| 6             |
/// | `30_`  | workspace CommonModules             | (workspace_owns_common_module shadow gate) |
///
/// Two-digit zero-padded prefixes give correct lexicographic ordering across
/// any label charset (ASCII identifiers vs Cyrillic). A single-digit `2_`
/// would sort after `2_5_` for any non-digit label suffix, since `_` (0x5F)
/// > `5` (0x35); the padded form `20_` < `25_` regardless of suffix.
///
/// First-wins dedup by lowercase label: once a name is emitted in an earlier
/// band, later bands skip it. The HBK band additionally consults
/// [`hir::Resolver::user_common_module_exists`] (same gate `infer.rs:1493`
/// uses) so it does not emit a property shadowed by a workspace CommonModule
/// that's about to render in band 30.
fn complete_top_level<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    offset: syntax::TextSize,
    prefix: &str,
    locale: Locale,
    with_sort_text: bool,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_top_level").entered();

    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    push_band(
        &mut out,
        &mut seen,
        with_sort_text,
        "00_",
        complete_local_symbols(db, file_id, offset, prefix),
    );
    push_band(
        &mut out,
        &mut seen,
        with_sort_text,
        "10_",
        complete_user_defined_symbols(db, file_id, prefix),
    );
    push_band(
        &mut out,
        &mut seen,
        with_sort_text,
        "15_",
        complete_module_self_attributes(db, file_id, prefix, locale),
    );
    push_band(&mut out, &mut seen, with_sort_text, "20_", complete_mdo_plurals(prefix));
    push_band(
        &mut out,
        &mut seen,
        with_sort_text,
        "25_",
        complete_hbk_globals(db, file_id, prefix, locale),
    );
    push_band(
        &mut out,
        &mut seen,
        with_sort_text,
        "30_",
        complete_user_common_modules(db, file_id, prefix),
    );

    out
}

/// Append `items` to `out`, dropping any whose lowercase label is already in
/// `seen` (first-wins per cascade). When `with_sort_text` is set, stamps each
/// item's `sort_text` with `<band_prefix><label>` for stable lexicographic
/// banding in the LSP client.
fn push_band(
    out: &mut Vec<CompletionItem>,
    seen: &mut HashSet<String>,
    with_sort_text: bool,
    band_prefix: &str,
    items: Vec<CompletionItem>,
) {
    for mut item in items {
        let key = item.label.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        if with_sort_text {
            item.sort_text = Some(format!("{band_prefix}{}", item.label));
        }
        out.push(item);
    }
}

fn complete_module_self_attributes<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    prefix: &str,
    locale: ide_db::base_db::Locale,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_module_self_attributes").entered();
    let prefix_lower = prefix.to_lowercase();
    hir::module_implicit_fields(db, file_id)
        .into_iter()
        .filter(|field| field.name.as_str().to_lowercase().starts_with(&prefix_lower))
        .map(|field| render_mdo_field(&field, locale))
        .collect()
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

    // Collect implicit locals from inference, not by re-scanning assignment
    // syntax. Inference is the layer that knows whether `X = ...` is really
    // a local variable or a typed form/self property assignment.
    if let Some(owner) = owner_for_method_range(db, file_id, method_range) {
        // Phase O.17: route through the per-owner cell. Owner is
        // already known from `owner_for_method_range`, so we hit
        // exactly one query (the matching method or module-code).
        let routed = hir::infer_owner(db, file_id, owner);
        let implicit_locals = routed.implicit_locals();
        if !implicit_locals.is_empty() {
            for (lower, info) in implicit_locals {
                if seen.contains(lower) {
                    continue;
                }
                if !lower.starts_with(&prefix_lower) {
                    seen.insert(lower.clone());
                    continue;
                }
                seen.insert(lower.clone());
                let text = info.name.as_str().to_string();
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
    }

    tracing::debug!(
        count = completions.len(),
        prefix = ?prefix,
        method_range = ?method_range,
        "Completed local symbols"
    );

    completions
}

fn owner_for_method_range<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    method_range: TextRange,
) -> Option<DefWithBodyId> {
    let tree = db.item_tree(file_id);
    for (idx, item) in tree.top_level_items().iter().enumerate() {
        let range = match item {
            hir::ModItem::Procedure(proc_idx) => tree.procedure(*proc_idx).source_range,
            hir::ModItem::Function(func_idx) => tree.function(*func_idx).source_range,
            _ => continue,
        };
        if range == method_range {
            return Some(DefWithBodyId::Method(idx as u32));
        }
    }
    None
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

/// MDO plural completion items (band 2). `Справочники`, `Документы`,
/// `РегистрыСведений`, … rendered as `CompletionItemKind::MdoType` with detail
/// `"Коллекция метаданных (…)"`. HBK also declares these names as global
/// properties typed `<X>Менеджер`, but workspace shape `Ty::ManagerCollection`
/// is strictly more specific, so this branch owns the rendering and
/// `complete_hbk_globals` skips names matching `MdoType::from_plural`.
///
/// HBK-driven iteration (Phase C). The candidate set comes from
/// `PlatformDataInner::all_global_properties()` partitioned by
/// `MdoType::from_plural` on both Russian and English names. This makes HBK
/// the registry-of-record for which MDO plurals are bareword-accessible:
/// `Cube`, `DimensionTable`, `CommonModule` never appear here because HBK
/// classifies them as nested type descriptors, not global-context
/// properties. Display name, readonly, docs all sourced from HBK; only
/// the `Ty::ManagerCollection` carrier still flows through `MdoType`.
fn complete_mdo_plurals(prefix: &str) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_mdo_plurals").entered();

    let prefix_lower = prefix.to_lowercase();
    let mut completions = Vec::new();

    for prop in PlatformDataInner::instance().all_global_properties() {
        let Some(mdo_type) = MdoType::from_plural(prop.name.as_str())
            .or_else(|| MdoType::from_plural(prop.english_name.as_str()))
        else {
            continue;
        };
        if !matches_prefix_bilingual(&prop.name, &prop.english_name, &prefix_lower) {
            continue;
        }
        completions.push(render_mdo_plural_with_hbk(mdo_type, prop));
    }

    completions
}

/// Render an HBK-backed MDO plural property as a band-2 completion item.
///
/// - `label` / `insert_text` use HBK's Russian name (`prop.name`), which is
///   the bareword form the user actually types.
/// - `detail` keeps the legacy `"Коллекция метаданных (…)"` prefix pinned by
///   `completion_globals.rs::completion_mdo_plural_not_duplicated`. When
///   HBK marks the property `is_readonly`, `" [Только чтение]"` is appended
///   so the readonly flag surfaces in the completion popup the same way
///   non-MDO globals show it via `render_platform_property`.
/// - `documentation` composes `PropertyDocs { description, notes, see_also }`
///   from `get_property_docs(prop.id)`. When docs are absent the legacy
///   "Коллекция объектов метаданных типа X." line is used so the doc panel
///   is never empty.
/// - `filter_text` mirrors `render_platform_property`'s bilingual filter
///   shape so English prefixes (`Doc|`) still match.
fn render_mdo_plural_with_hbk(
    mdo_type: bsl_metadata::MdoType,
    prop: &bsl_platform::PlatformProperty,
) -> CompletionItem {
    let label = prop.name.to_string();
    let mut detail = format!("Коллекция метаданных ({})", mdo_type.russian_name());
    if prop.is_readonly {
        detail.push_str(" [Только чтение]");
    }
    let documentation = compose_mdo_plural_documentation(mdo_type, prop);
    let filter_text = format!("{} {}", prop.name, prop.english_name);

    CompletionItem {
        label: label.clone(),
        detail: Some(detail),
        kind: CompletionItemKind::MdoType,
        insert_text: label,
        documentation: Some(documentation),
        sort_text: None,
        filter_text: Some(filter_text),
        source: None,
    }
}

/// Compose the documentation panel for an MDO-plural completion item from
/// HBK's `PropertyDocs`. Falls back to the legacy generic line when HBK
/// ships no description for this property.
fn compose_mdo_plural_documentation(
    mdo_type: bsl_metadata::MdoType,
    prop: &bsl_platform::PlatformProperty,
) -> String {
    let mut out = format!("{} / {}\n\n", prop.name, prop.english_name);
    let docs = PlatformDataInner::instance().get_property_docs(prop.id);
    match docs {
        Some(d) if !d.description.trim().is_empty() => {
            out.push_str(d.description.trim());
            if let Some(notes) = d.notes.as_ref().filter(|n| !n.trim().is_empty()) {
                out.push_str("\n\n");
                out.push_str(notes.trim());
            }
            if !d.see_also.is_empty() {
                out.push_str("\n\nСм. также:");
                for link in &d.see_also {
                    out.push_str("\n- ");
                    out.push_str(link);
                }
            }
        }
        _ => {
            out.push_str("Коллекция объектов метаданных типа ");
            out.push_str(mdo_type.russian_name());
            out.push('.');
        }
    }
    out
}

/// Workspace CommonModule completion items (band 3). Reads names from the
/// authoritative `module_index` — the same source `Resolver::user_common_module_exists`
/// consults at inference time — so completion stays in lockstep with
/// resolution. Common modules are the only metadata objects callable
/// directly by name; other MDO objects must be accessed through their
/// collection, and dot-completion is handled by `mdo_completion.rs`.
fn complete_user_common_modules<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    prefix: &str,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_user_common_modules").entered();

    let prefix_lower = prefix.to_lowercase();
    let mut completions = Vec::new();

    for name in module_index_for(db, file_id).common_module_display_names() {
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
            source: None,
        });
    }

    completions
}

fn module_index_for<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
) -> std::sync::Arc<hir::ModuleIndex> {
    let source_root_id = db.file_source_root_input(file_id).source_root_id(db);
    db.module_index(source_root_id)
}

/// HBK globals — global properties + global functions merged. Skips:
///
/// - MDO plurals (band 2 owns them with richer workspace shape; both RU and
///   EN aliases are gated because each HBK property is emitted once and
///   either alias would route through `band 2`).
/// - HBK **properties** whose literal RU name resolves to a workspace
///   CommonModule via [`Resolver::user_common_module_exists`] — same gate
///   `infer.rs:1493` applies to platform-global property resolution. Uses
///   the full Resolver (workspace scope + module id + configuration
///   visibility), not a raw `module_index` probe, so extensions that hide
///   a module in the active configuration don't over-shadow HBK.
///
/// HBK **global functions** are intentionally NOT shadowed: builtin platform
/// functions have the highest resolution priority in BSL and are not
/// preempted by user code (a workspace `CommonModule/НачатьТранзакцию` can
/// only be invoked as `НачатьТранзакцию.Method()`; bareword call still
/// binds to the platform function).
fn complete_hbk_globals<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    prefix: &str,
    locale: Locale,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_hbk_globals").entered();

    let data = PlatformDataInner::instance();
    let module_id = hir::ModuleId::new(file_id);
    let resolver = hir::Resolver::with_workspace_scope(module_id);

    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let prefix_lower = prefix.to_lowercase();

    for prop in data.all_global_properties() {
        let ru_lower = prop.name.to_lowercase();

        if MdoType::from_plural(prop.name.as_str()).is_some()
            || MdoType::from_plural(prop.english_name.as_str()).is_some()
        {
            continue;
        }
        if resolver.user_common_module_exists(db, &hir::Name::new(&prop.name)) {
            continue;
        }
        if !matches_prefix_bilingual(&prop.name, &prop.english_name, &prefix_lower) {
            continue;
        }
        if !seen.insert(ru_lower) {
            continue;
        }

        items.push(render_platform_property(prop, locale));
    }

    items.extend(complete_global_functions(prefix));
    items
}

/// Case-insensitive bilingual prefix match. `lower` is already lowercased.
fn matches_prefix_bilingual(ru: &str, en: &str, lower: &str) -> bool {
    ru.to_lowercase().starts_with(lower) || en.to_lowercase().starts_with(lower)
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
        let items = complete_mdo_plurals("Справ");
        let _ = (&db, file_id); // silence unused locals retained for setup parity

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
        let items = complete_mdo_plurals("Docu");
        let _ = (&db, file_id); // silence unused locals retained for setup parity

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
        db.set_file_source_root(file_id, SourceRootId(0));

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
    fn test_complete_module_self_attributes_skips_non_self_module() {
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

        let items = complete_module_self_attributes(&db, file_id, "", ide_db::base_db::Locale::Ru);
        assert!(items.is_empty(), "non-self file must not surface implicit attributes");
    }
}
