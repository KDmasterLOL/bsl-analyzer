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
use hir::{classify_token, MetadataKind, NameClass, Semantics, Ty};
use ide_db::RootDatabase;
use symbol_info::{from_global_function, from_platform_method, render_hover_markdown, Lang};
use syntax::{SyntaxNode, SyntaxToken, TextRange, TextSize};
use vfs::FileId;

use crate::HoverResult;

/// Returns hover information at the specified position.
///
/// Dispatch is driven by the unified [`hir::classify_token`]
/// name-position classifier — every consumer of token resolution in
/// the IDE layer matches on the same `NameClass` taxonomy, so a
/// keyword that sits in a name slot (e.g. `Запрос.Выполнить`, where
/// `Выполнить` is `KW_EXECUTE`) is dispatched as `FieldName`, not
/// `Keyword`. The previous chain of fall-through handlers each with
/// its own `if token.kind() != IDENT` gate is gone.
pub(crate) fn hover<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
) -> Option<HoverResult> {
    let _span = tracing::info_span!("hover", ?file_id, ?offset).entered();

    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let token = root.token_at_offset(offset).right_biased()?;

    tracing::debug!(token_kind = ?token.kind(), token_text = ?token.text(), "Hover token");

    match classify_token(&token) {
        NameClass::FieldName { receiver, token, is_call } => {
            hover_field(db, file_id, &receiver, &token, is_call)
        }
        NameClass::FreeName { token } => hover_free_name(db, file_id, &token),
        NameClass::TypeRef { token } => {
            hover_for_platform_type(db, token.text(), token.text_range())
        }
        NameClass::Keyword { token } => hover_keyword(&token),
        // Future work: literal-aware hover. For now we stay silent —
        // `Истина`/`Ложь`/`Неопределено`/`Null` already render fine in
        // their assignment positions through other surfaces.
        NameClass::Literal { .. } => None,
        NameClass::Other => None,
    }
}

/// Hover for a name in a `FieldName` slot (`receiver.name` or
/// `receiver.name(...)`).
///
/// Precedence — `is_call` breaks ties on names that exist in both
/// slots on the same receiver type:
///
/// - **`is_call = true`** (parens follow): try platform method first,
///   fall back to platform property, then to qualified-name resolution
///   for cross-module calls (`ОбщегоНазначения.МойМетод`).
/// - **`is_call = false`** (bare field access): platform property
///   first, then platform method, then qualified-name resolution.
///
/// The qualified-name fallback covers the `Документы.ПКО` /
/// `ОбщегоНазначения.МойМетод` shapes that
/// `Semantics::resolve_name_to_definition` already handles via
/// `try_resolve_qualified_name_for_token` *before* its
/// `field_name_receiver` guard.
fn hover_field<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    receiver: &SyntaxNode,
    token: &SyntaxToken,
    is_call: bool,
) -> Option<HoverResult> {
    let sema = Semantics::new(db);
    let receiver_ty = sema.type_of_expr(file_id, receiver);
    let name = token.text();
    let range = token.text_range();

    let property = || hover_platform_property_on_ty(db, &receiver_ty, name, range);
    let method = || hover_platform_method_on_token(db, &sema, file_id, token);

    if is_call {
        if let Some(r) = method() {
            return Some(r);
        }
        if let Some(r) = property() {
            return Some(r);
        }
    } else {
        if let Some(r) = property() {
            return Some(r);
        }
        if let Some(r) = method() {
            return Some(r);
        }
    }

    // Fallback to qualified-name resolution. `resolve_name_to_definition`
    // calls `try_resolve_qualified_name_for_token` first — that's how
    // `Документы.ПКО`, `ОбщегоНазначения.МойМетод` and `Метаданные.Х`
    // resolve. The `field_name_receiver` guard on the same function
    // returns `None` only after the qualified-name branch fired, so we
    // get the cross-module hover for free.
    let inferred_ty = type_of_token(&sema, file_id, token);
    if let Some(definition) = sema.resolve_name_to_definition(file_id, token) {
        return definition_to_hover(db, &definition, range, inferred_ty.as_ref());
    }

    None
}

/// Hover for a name in a `FreeName` slot.
///
/// Consolidates the previous `hover_user_defined` and the
/// global-function / type-literal branches of `hover_platform`. Order:
///
/// 1. User-defined symbol via `Semantics::resolve_name_to_definition`
///    (locals, parameters, module methods/variables, MDO plurals,
///    builtin functions — locals shadow builtins per BSL).
/// 2. Type-only fallback for implicit variables (BSL has no `Перем`
///    decl for first-assignment locals, so `resolve_name_to_definition`
///    misses them; the inferred type still tells the user what the
///    expression is).
/// 3. Global platform function (e.g. `НачатьТранзакцию()`).
/// 4. Bare type literal in expression position (e.g. `Строка`).
fn hover_free_name<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    token: &SyntaxToken,
) -> Option<HoverResult> {
    let sema = Semantics::new(db);
    let inferred_ty = type_of_token(&sema, file_id, token);

    if let Some(definition) = sema.resolve_name_to_definition(file_id, token) {
        if let Some(r) =
            definition_to_hover(db, &definition, token.text_range(), inferred_ty.as_ref())
        {
            return Some(r);
        }
    } else if let Some(ty) = inferred_ty.as_ref() {
        // Implicit variable (no `Перем`) — surface its inferred type.
        let mut markup = format!("**{}**\n\n", token.text());
        if let Some(type_block) = ty_info_markup(db, ty) {
            markup.push_str(&type_block);
            return Some(HoverResult { markup, range: Some(token.text_range()) });
        }
    }

    if let Some(r) = hover_for_global_function(db, token.text(), token.text_range()) {
        return Some(r);
    }

    hover_for_platform_type(db, token.text(), token.text_range())
}

/// Look up a property on `receiver_ty` and render its hover markup.
/// No-op for unknown receivers or non-platform-value type shapes.
fn hover_platform_property_on_ty<DB: RootDatabase>(
    db: &DB,
    receiver_ty: &Ty,
    prop_name: &str,
    range: TextRange,
) -> Option<HoverResult> {
    if receiver_ty.is_unknown() {
        return None;
    }
    let type_key = receiver_ty.platform_type_name()?;
    let input = MethodLookupInput::new(db, type_key.to_string(), prop_name.to_string());
    let prop = platform_property_query(db, input)?;
    Some(render_property_hover(&prop, range))
}

/// Look up a method on the receiver type via the type-aware Semantics
/// API and render the platform-method hover markup.
fn hover_platform_method_on_token<DB: RootDatabase>(
    db: &DB,
    sema: &Semantics<'_, DB>,
    file_id: FileId,
    token: &SyntaxToken,
) -> Option<HoverResult> {
    let definition = sema.resolve_method_call_to_definition(file_id, token)?;
    let hir::Definition::BuiltinMethod { type_name, method_name } = definition else {
        return None;
    };
    hover_for_platform_method(db, type_name.as_str(), method_name.as_str(), token.text_range())
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
        MetadataKind::InformationRegisterRecordSet => "РегистрСведенийНаборЗаписей",
        MetadataKind::AccumulationRegisterRef => "РегистрНакопленияКлючЗаписи",
        MetadataKind::AccumulationRegisterRecordSet => "РегистрНакопленияНаборЗаписей",
        MetadataKind::AccountingRegisterRef => "РегистрБухгалтерииКлючЗаписи",
        MetadataKind::AccountingRegisterRecordSet => "РегистрБухгалтерииНаборЗаписей",
        MetadataKind::CalculationRegisterRef => "РегистрРасчётаКлючЗаписи",
        MetadataKind::CalculationRegisterRecordSet => "РегистрРасчетаНаборЗаписей",
        MetadataKind::RegisterDimension { .. } => "Измерение",
        MetadataKind::RegisterResource { .. } => "Ресурс",
        MetadataKind::RegisterAttribute { .. } => "Реквизит",
        MetadataKind::RegisterFilter { .. } => "Отбор",
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
