//! Hover information provider.
//!
//! This module provides hover information for BSL code, including:
//! - Platform types (Строка, Число, Массив, etc.)
//! - Platform methods with signatures and documentation
//! - User-defined symbols (methods, variables, parameters)

use bsl_platform::{
    global_function_query, platform_method_query, platform_property_query, platform_type_query,
    type_methods_query, ContextAvailability, MethodLookupInput, PlatformDataInner, PlatformMethod,
    PlatformProperty, TypeNameInput,
};
use hir::{MetadataKind, Semantics, Ty};
use ide_db::RootDatabase;
use symbol_info::{from_global_function, from_platform_method, render_hover_markdown, Lang};
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

    // Try platform-property hover BEFORE `hover_platform`. The method/type
    // hover in `hover_platform` uses the bare receiver IDENT text as the
    // platform type key (`Строка.ВРег` works because `Строка` is literally
    // the type), but platform-typed variables (`Зап = Новый Запрос; Зап.`)
    // need `Semantics::type_of_expr` on the receiver. Keeping the property
    // path separate keeps `hover_platform`'s purely-syntactic shortcut
    // fast and reserves Salsa-backed type resolution for cases that
    // actually need it.
    if let Some(result) = hover_platform_property(db, file_id, &token) {
        return Some(result);
    }

    // Type-aware platform-method hover for chained receivers like
    // `Запрос.Выполнить().Выгрузить().ВыгрузитьКолонку(...)`. The
    // syntactic `try_extract_method_call` inside `hover_platform`
    // only handles `IDENT.method()`; for any chained / parenthesised /
    // indexed receiver it bails out, which previously let
    // `hover_user_defined` match the method name against an unrelated
    // workspace free function (e.g. БСП's
    // `ОбщегоНазначения.ВыгрузитьКолонку`). With the
    // `field_name_receiver` guard in `Semantics::resolve_name_to_definition`
    // the `hover_user_defined` branch above already returns `None` for
    // such tokens — this branch then resolves the method by inferring
    // the receiver's type and looking it up through the fluent-aware
    // [`hir_ty::method_lookup::lookup_method_with_key`] dispatch.
    if let Some(result) = hover_platform_method_via_ty(db, file_id, &token) {
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
    let sema = Semantics::new(db);

    // Resolve inferred type for the expression surrounding this token — used
    // both to enrich named bindings (Variable / Local / Parameter) and to
    // fall back to a type-only hover for implicit variables BSL creates at
    // first assignment (those have no `Перем` in the item tree, so
    // `resolve_name_to_definition` returns `None`).
    let inferred_ty = type_of_token(&sema, file_id, token);

    match sema.resolve_name_to_definition(file_id, token) {
        Some(definition) => {
            definition_to_hover(db, &definition, token.text_range(), inferred_ty.as_ref())
        }
        None => inferred_ty.as_ref().and_then(|ty| {
            let mut markup = format!("**{}**\n\n", token.text());
            let type_block = ty_info_markup(db, ty)?;
            markup.push_str(&type_block);
            Some(HoverResult { markup, range: Some(token.text_range()) })
        }),
    }
}

/// Resolve the inferred [`Ty`] of a single identifier token.
///
/// Walks upward only through same-range wrappers (an IdentExpr / EXPR shell
/// whose `text_range()` equals the token's), stopping as soon as an
/// ancestor spans more than the token. That bound is load-bearing:
///
/// - For `A + B`, the token `B` has a `BINARY_EXPR` ancestor that would
///   otherwise report the sum's type (`Number` / `String`) as if it were
///   `B`'s type (`crates/hir-ty/src/infer.rs::infer_binary_op`).
/// - For `Новый КомпоновщикНастроекКомпоновкиДанных`, the constructor-name
///   token lives under a wider `NEW_EXPR`, whose `type_of_expr` returns
///   the *result* type of the `Новый`. Letting that leak through would
///   suppress the platform-type hover for the same token.
/// - For `obj.Method()`, the method-name token lives under a wider
///   `FIELD_EXPR` / `CALL_EXPR`; same reasoning — the enclosing
///   expression's type is not the token's type.
///
/// Returns `None` when no same-range wrapper carries an inferred type —
/// callers treat that the same as "no info", which preserves the existing
/// fallbacks (`hover_platform`, `hover_keyword`).
fn type_of_token<DB: RootDatabase>(
    sema: &Semantics<'_, DB>,
    file_id: FileId,
    token: &SyntaxToken,
) -> Option<Ty> {
    let token_range = token.text_range();
    let mut node = token.parent()?;
    while node.text_range() == token_range {
        let ty = sema.type_of_expr(file_id, &node);
        if !ty.is_unknown() {
            return Some(ty);
        }
        node = node.parent()?;
    }
    None
}

/// Converts a Definition to HoverResult.
///
/// `inferred_ty` carries the type inference result for the token's
/// surrounding expression, so named bindings (variables, locals, parameters)
/// can surface the same type block as [`hover_for_platform_type`] — the
/// caller in [`hover_user_defined`] computes it once via
/// [`Semantics::type_of_expr`] and routes it through here.
fn definition_to_hover<DB: RootDatabase>(
    db: &DB,
    definition: &hir::Definition,
    range: TextRange,
    inferred_ty: Option<&Ty>,
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

            if let Some(ty) = inferred_ty {
                if let Some(block) = ty_info_markup(db, ty) {
                    markup.push_str(&block);
                }
            }
        }

        hir::Definition::Parameter { param_name, .. } => {
            markup.push_str(&format!("**Параметр {}**\n\n", param_name.as_str()));
            if let Some(ty) = inferred_ty {
                if let Some(block) = ty_info_markup(db, ty) {
                    markup.push_str(&block);
                }
            }
        }

        hir::Definition::Local { var_name, .. } => {
            markup.push_str(&format!("**Локальная переменная {}**\n\n", var_name.as_str()));
            if let Some(ty) = inferred_ty {
                if let Some(block) = ty_info_markup(db, ty) {
                    markup.push_str(&block);
                }
            }
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

/// Attempts to provide hover information for a platform-type property
/// access (`Зап.Параметры`, `РезультатЗапроса.Колонки`, …).
///
/// Unlike [`hover_platform`], this path resolves the receiver's actual
/// `Ty` via `Semantics::type_of_expr` instead of treating the bare
/// identifier text as a type name — it has to, because the receiver is
/// typically a variable (`Зап` in `Зап = Новый Запрос;`), not a type
/// literal. Once the receiver type is known, the property is looked up
/// through the bilingual platform index, and the hover markup includes
/// the declared value type(s), the `[Только чтение]` marker, and the
/// free-prose documentation from `PropertyDocs`.
///
/// Returns `None` when:
/// - the token is not an identifier,
/// - the token's parent is not a `FIELD_EXPR` (i.e. we're not actually
///   hovering a property access),
/// - the receiver's type is not a platform-value shape with a scalar
///   key (managers / metadata refs go through their own adapters),
/// - the property name does not exist on the resolved type.
fn hover_platform_property<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    token: &SyntaxToken,
) -> Option<HoverResult> {
    if token.kind() != SyntaxKind::IDENT {
        return None;
    }

    // Identify the property-access shape. The token must sit under a
    // FIELD_EXPR whose first child is the receiver — mirrors what
    // `platform_completion::find_receiver_expr` does for completion.
    let parent = token.parent()?;
    if parent.kind() != SyntaxKind::FIELD_EXPR {
        return None;
    }
    let receiver_node = parent.children().next()?;

    // Resolve the receiver's inferred type via Semantics. Anything
    // unknown / not a platform value type stays silent.
    let sema = Semantics::new(db);
    let receiver_ty = sema.type_of_expr(file_id, &receiver_node);
    if receiver_ty.is_unknown() {
        return None;
    }

    // Route through the same bilingual property index the IDE
    // completion path uses. `platform_type_name()` only yields a key
    // for platform-value receivers (PlatformObject, Array, Map,
    // Structure, ValueTable, primitives); managers / metadata refs
    // return None, so we naturally skip those here.
    let type_key = receiver_ty.platform_type_name()?;
    let prop_name = token.text().to_string();
    let input = MethodLookupInput::new(db, type_key.to_string(), prop_name.clone());
    let prop = platform_property_query(db, input)?;

    Some(render_property_hover(&prop, token.text_range()))
}

/// Type-aware hover for `recv.method(...)` calls where the receiver is
/// not a bare identifier (chained calls, indexed access, etc.).
///
/// Resolves the receiver's inferred [`hir::Ty`] via
/// [`hir::Semantics::resolve_method_call_to_definition`], which routes
/// through `hir_ty::method_lookup::lookup_method_with_key` and gives us
/// back a `(type_key, method_name)` pair when the method exists on the
/// receiver type in [`bsl_platform::PlatformData`]. The actual hover
/// markdown is produced by the existing [`hover_for_platform_method`]
/// renderer so the output is identical to what users see for the
/// syntactic-shortcut path (`Строка.ВРег()`).
///
/// Returns [`None`] when:
/// - the token isn't an IDENT under a `FIELD_EXPR` field-name slot,
/// - the receiver's inferred type is `Ty::Unknown`,
/// - the receiver's type yields no scalar key (manager / metadata-ref
///   shapes are served by their own paths),
/// - the method name does not exist in platform data.
fn hover_platform_method_via_ty<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    token: &SyntaxToken,
) -> Option<HoverResult> {
    if token.kind() != SyntaxKind::IDENT {
        return None;
    }
    let sema = Semantics::new(db);
    let definition = sema.resolve_method_call_to_definition(file_id, token)?;
    let hir::Definition::BuiltinMethod { type_name, method_name } = definition else {
        return None;
    };
    hover_for_platform_method(db, type_name.as_str(), method_name.as_str(), token.text_range())
}

/// Build the markdown block for a platform-property hover.
///
/// Emits in this order:
/// 1. H4 title with the bilingual name (`**Параметры (Parameters)**`).
/// 2. `[Только чтение]` marker when `is_readonly`; otherwise no line
///    (read-write is the default, not worth surfacing).
/// 3. `**Тип:**` with the declared value types joined by `, `.
/// 4. The free-prose description / notes from `PropertyDocs` if any.
fn render_property_hover(prop: &PlatformProperty, range: TextRange) -> HoverResult {
    let mut markup = format!("**{} ({})**\n\n", prop.name, prop.english_name);
    if prop.is_readonly {
        markup.push_str("*Только чтение*\n\n");
    }
    if !prop.property_types.is_empty() {
        let types: Vec<&str> = prop.property_types.iter().map(|s| s.as_str()).collect();
        markup.push_str(&format!("**Тип:** {}\n\n", types.join(", ")));
    }
    if let Some(docs) = PlatformDataInner::instance().get_property_docs(prop.id) {
        if !docs.description.is_empty() {
            markup.push_str(&docs.description);
            markup.push_str("\n\n");
        }
        if let Some(notes) = docs.notes {
            if !notes.is_empty() {
                markup.push_str("**Примечание:** ");
                markup.push_str(&notes);
                markup.push('\n');
            }
        }
    }
    HoverResult { markup, range: Some(range) }
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
    let markup = platform_type_markup(db, type_name)?;
    Some(HoverResult { markup, range: Some(range) })
}

/// Build the "platform type" markup block without a `HoverResult` wrapper.
///
/// Shared between [`hover_for_platform_type`] (whose caller already owns the
/// hover range) and [`ty_info_markup`], which appends this block to
/// bindings whose inferred [`Ty`] resolves to a platform object.
fn platform_type_markup<DB: RootDatabase>(db: &DB, type_name: &str) -> Option<String> {
    let input = TypeNameInput::new(db, type_name.to_string());
    let platform_type = platform_type_query(db, input)?;

    let mut markup = String::new();
    markup
        .push_str(&format!("**Тип:** {} / {}\n\n", platform_type.name, platform_type.english_name));

    if let Some(ctx) = &platform_type.context {
        markup.push_str(&format!("**Доступность:** {}\n\n", format_context_availability(ctx)));
    }

    if let Some(version) = &platform_type.min_version {
        markup.push_str(&format!("**Версия:** {}+\n\n", version));
    }

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

    Some(markup)
}

/// Format an inferred [`Ty`] as hover markdown.
///
/// - `Ty::Unknown` → `None` (hover stays silent rather than printing a
///   useless "Unknown" label).
/// - `Ty::PlatformObject(name)` → delegates to [`platform_type_markup`],
///   which fetches canonical docs and a methods preview. Falls back to a
///   bare `**Тип:** name` line when the platform data has no entry for
///   `name`, so IDE output stays informative even if the index is
///   incomplete.
/// - Anything else → bilingual `**Тип:** Русское / English` line built
///   from [`render_ty_ru`] and [`Ty::display_name`].
fn ty_info_markup<DB: RootDatabase>(db: &DB, ty: &Ty) -> Option<String> {
    if ty.is_unknown() {
        return None;
    }

    if let Ty::PlatformObject(name) = ty {
        if let Some(block) = platform_type_markup(db, name.as_str()) {
            return Some(block);
        }
        return Some(format!("**Тип:** {}\n\n", name.as_str()));
    }

    let ru = render_ty_ru(ty);
    let en = ty.display_name();
    if ru == en {
        Some(format!("**Тип:** {}\n\n", ru))
    } else {
        Some(format!("**Тип:** {} / {}\n\n", ru, en))
    }
}

/// Render a [`Ty`] using Russian BSL type names.
///
/// `Ty::display_name` returns English identifiers ("Number", "String") that
/// match the platform metadata keys but feel foreign in hover text for a
/// Russian-first language. This helper flips the leaf variants to their
/// idiomatic Russian spelling and builds fully-qualified labels for MDO
/// variants (`ДокументСсылка.ПКО`, `Справочник.Валюты`). Union rendering
/// joins components with " | " in the smart-constructor-imposed order.
///
/// `PlatformObject` is not expected to reach this path — `ty_info_markup`
/// handles it directly with richer platform-data enrichment.
fn render_ty_ru(ty: &Ty) -> String {
    match ty {
        Ty::Number => "Число".into(),
        Ty::String => "Строка".into(),
        Ty::Boolean => "Булево".into(),
        Ty::Date => "Дата".into(),
        Ty::Undefined => "Неопределено".into(),
        Ty::Null => "Null".into(),
        Ty::Array => "Массив".into(),
        Ty::Structure => "Структура".into(),
        Ty::Map => "Соответствие".into(),
        Ty::Type => "Тип".into(),
        Ty::ValueTable => "ТаблицаЗначений".into(),
        Ty::ValueList => "СписокЗначений".into(),
        Ty::Function { .. } => "Функция".into(),
        Ty::ThisObject { .. } => "ЭтотОбъект".into(),
        Ty::FormData { kind, underlying } => {
            // Surface the projected MDO so hover on `Объект` reads
            // "ДанныеФормыСтруктура (ДокументОбъект.ПКО)" — the wrapper
            // name explains method semantics, the parenthetical reminds
            // the reader which catalog/document fields are visible.
            // The `*Object` MetadataKind is the same surface a manual
            // `ЭтотОбъект` cast would expose, so we reuse `metadata_kind_ru`
            // for consistency with `Ty::MetadataRef` rendering.
            let wrapper = kind.platform_type_name();
            match underlying
                .as_ref()
                .and_then(|(mdo, name)| MetadataKind::object_kind_for(*mdo).map(|k| (k, name)))
            {
                Some((object_kind, name)) => {
                    format!("{} ({}.{})", wrapper, metadata_kind_ru(object_kind), name.as_str())
                }
                None => wrapper.into(),
            }
        }
        Ty::MetadataRef { kind, name } => {
            format!("{}.{}", metadata_kind_ru(*kind), name.as_str())
        }
        Ty::ObjectManager { kind, name } => {
            format!("{}.{}", kind.russian_name(), name.as_str())
        }
        Ty::ManagerCollection(kind) => kind.russian_name().into(),
        Ty::PlatformObject(name) => name.as_str().into(),
        Ty::Union(types) => {
            let mut parts = Vec::with_capacity(types.len());
            for t in types.iter() {
                parts.push(render_ty_ru(t));
            }
            parts.join(" | ")
        }
        Ty::Unknown => ty.display_name().into(),
    }
}

/// Map a [`MetadataKind`] to the canonical Russian BSL type name.
///
/// Parametric variants (`TabularSection { parent }`, `RegisterDimension { parent }`,
/// …) carry the parent [`bsl_metadata::MdoType`] in the value itself; the
/// enclosing `Ty::MetadataRef`'s `name` already encodes the `"Parent.Section"`
/// suffix, so the label stays focused on the kind tag and the full path is
/// still visible as `{label}.{name}`.
fn metadata_kind_ru(kind: MetadataKind) -> &'static str {
    match kind {
        MetadataKind::CatalogRef => "СправочникСсылка",
        MetadataKind::CatalogObject => "СправочникОбъект",
        MetadataKind::DocumentRef => "ДокументСсылка",
        MetadataKind::DocumentObject => "ДокументОбъект",
        MetadataKind::EnumRef => "ПеречислениеСсылка",
        MetadataKind::TaskRef => "ЗадачаСсылка",
        MetadataKind::BusinessProcessRef => "БизнесПроцессСсылка",
        MetadataKind::ExchangePlanRef => "ПланОбменаСсылка",
        MetadataKind::ExchangePlanObject => "ПланОбменаОбъект",
        MetadataKind::ChartOfAccountsRef => "ПланСчетовСсылка",
        MetadataKind::ChartOfAccountsObject => "ПланСчетовОбъект",
        MetadataKind::InformationRegisterRef => "РегистрСведенийКлючЗаписи",
        MetadataKind::InformationRegisterRecordManager => "РегистрСведенийМенеджерЗаписи",
        MetadataKind::AccumulationRegisterRef => "РегистрНакопленияКлючЗаписи",
        MetadataKind::AccumulationRegisterRecordSet => "РегистрНакопленияНаборЗаписей",
        MetadataKind::AccountingRegisterRef => "РегистрБухгалтерииКлючЗаписи",
        MetadataKind::CalculationRegisterRef => "РегистрРасчётаКлючЗаписи",
        MetadataKind::RegisterDimension { .. } => "Измерение",
        MetadataKind::RegisterResource { .. } => "Ресурс",
        MetadataKind::RegisterAttribute { .. } => "Реквизит",
        MetadataKind::TabularSection { .. } => "ТабличнаяЧасть",
        MetadataKind::TabularSectionRow { .. } => "СтрокаТабличнойЧасти",
    }
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
    let docs = PlatformDataInner::instance().get_method_docs(method.id);

    let sig = from_platform_method(&method, docs.as_ref());
    let mut markup = render_hover_markdown(&sig, Lang::Russian);
    append_availability(&mut markup, method.context.as_ref());

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
    let docs = PlatformDataInner::instance().get_global_function_docs(function.id);

    let sig = from_global_function(&function, docs.as_ref());
    let mut markup = render_hover_markdown(&sig, Lang::Russian);
    append_availability(&mut markup, function.context.as_ref());

    Some(HoverResult { markup, range: Some(range) })
}

/// Append `**Доступность:** …` to existing hover markdown when a context is
/// available. Kept here (rather than inside `symbol_info::HoverPresenter`) so
/// the domain entity stays free of platform-specific availability flags.
fn append_availability(markup: &mut String, ctx: Option<&ContextAvailability>) {
    if let Some(ctx) = ctx {
        if !markup.is_empty() && !markup.ends_with("\n\n") {
            if markup.ends_with('\n') {
                markup.push('\n');
            } else {
                markup.push_str("\n\n");
            }
        }
        markup.push_str(&format!("**Доступность:** {}", format_context_availability(ctx)));
    }
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

/// Provides hover information for BSL keywords.
fn hover_keyword(token: &SyntaxToken) -> Option<HoverResult> {
    // Check if this is a keyword token
    if !token.kind().is_keyword() {
        return None;
    }

    let keyword_text = token.text();

    // Try to get keyword documentation.
    // allow: keyword docs (M3 exception) — keywords aren't part of the
    // type system, so they fall outside Invariant #3. Documented in
    // `docs/architecture/TYPE_SYSTEM.md`; `scripts/check-invariants.sh`
    // uses this comment as the white-list marker.
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
}
