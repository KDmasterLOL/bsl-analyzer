//! Hover information provider.
//!
//! This module provides hover information for BSL code, including:
//! - Platform types (Строка, Число, Массив, etc.)
//! - Platform methods with signatures and documentation
//! - User-defined symbols (methods, variables, parameters)

use bsl_platform::{
    global_function_query, platform_property_query, platform_type_query, type_methods_query,
    ContextAvailability, MethodLookupInput, PlatformDataInner, PlatformMethod, PlatformProperty,
    TypeNameInput,
};
use hir::{
    classify_token, kernel_type_label, Field, NameClass, Semantics, Ty, Type as HirType, TypeId,
};
use ide_db::base_db::Locale;
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
    locale: Locale,
) -> Option<HoverResult> {
    let _span = tracing::info_span!("hover", ?file_id, ?offset, ?locale).entered();

    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let token = root.token_at_offset(offset).right_biased()?;

    tracing::debug!(token_kind = ?token.kind(), token_text = ?token.text(), "Hover token");

    match classify_token(&token) {
        NameClass::FieldName { receiver, token, is_call } => {
            hover_field(db, file_id, &receiver, &token, is_call, locale)
        }
        NameClass::FreeName { token } => hover_free_name(db, file_id, &token, locale),
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
    locale: Locale,
) -> Option<HoverResult> {
    let sema = Semantics::new(db);
    // Phase 3 §4.G.5b: `Semantics::type_of_expr` is kernel-native; bridge to
    // `Ty` for the still-`Ty` hover helpers below (those move to the kernel
    // in Phase 4).
    let receiver_ty = hir::ty_bridge::typeid_to_ty(db, sema.type_of_expr(file_id, receiver));
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

    if let Some(field) = mdo_field_on_ty(db, file_id, &receiver_ty, name) {
        if field.value_ty.is_some() {
            return Some(render_mdo_field_hover(db, &field, name, range, locale));
        }
    }

    // Fallback to qualified-name resolution. `resolve_name_to_definition`
    // calls `try_resolve_qualified_name_for_token` first — that's how
    // `Документы.ПКО`, `ОбщегоНазначения.МойМетод` and `Метаданные.Х`
    // resolve. The `field_name_receiver` guard on the same function
    // returns `None` only after the qualified-name branch fired, so we
    // get the cross-module hover for free.
    let inferred_ty = type_of_token(db, &sema, file_id, token);
    if let Some(definition) = sema.resolve_name_to_definition(file_id, token) {
        return definition_to_hover(db, &definition, range, inferred_ty.as_ref(), locale);
    }

    // Final fallback: ask `type_of_expr` on the surrounding FieldExpr
    // (parent of the field-name token). Catches form-element names —
    // `Элементы.<X>` resolves to `Ty::FormControl{kind, binding}` via
    // `infer.rs`'s `form_items::lookup_form_item_field`, but the
    // platform property/method lookups above don't see X (it lives in
    // `Form.xml`, not platform_data) and `resolve_name_to_definition`
    // doesn't classify form elements as definitions. Without this
    // fallback hover would silently say "No information available" on
    // every form-element name.
    if let Some(parent) = token.parent() {
        let ty = hir::ty_bridge::typeid_to_ty(db, sema.type_of_expr(file_id, &parent));
        if !ty.is_unknown() {
            let mut markup = format!("**{}**\n\n", name);
            if let Some(type_block) = ty_info_markup(db, &ty, locale) {
                markup.push_str(&type_block);
                return Some(HoverResult { markup, range: Some(range) });
            }
        }
    }

    None
}

fn mdo_field_on_ty<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    receiver_ty: &Ty,
    field_name: &str,
) -> Option<Field> {
    let needle = field_name.to_lowercase();
    HirType::new(db, file_id, receiver_ty.clone()).fields().into_iter().find(|field| {
        field.name.as_str().to_lowercase() == needle
            || field.english_name.as_str().to_lowercase() == needle
    })
}

fn render_mdo_field_hover<DB: RootDatabase>(
    db: &DB,
    field: &Field,
    name: &str,
    range: TextRange,
    locale: Locale,
) -> HoverResult {
    let mut markup = format!("**{}**\n\n", name);
    let detail = if let Some(value_ty) = field.value_ty {
        format!(
            "{} → {}",
            render_hover_ty_detail(db, field.ty, locale),
            render_hover_ty_detail(db, value_ty, locale),
        )
    } else {
        render_hover_ty_detail(db, field.ty, locale)
    };
    markup.push_str(&format!("**Тип:** {detail}\n\n"));
    HoverResult { markup, range: Some(range) }
}

fn render_hover_ty_detail<DB: RootDatabase>(db: &DB, id: TypeId, locale: Locale) -> String {
    // Phase 3 §4.G.5d: kernel display is the single source of rendering truth.
    kernel_type_label(db, id, locale, true)
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
    locale: Locale,
) -> Option<HoverResult> {
    let sema = Semantics::new(db);
    let inferred_ty = type_of_token(db, &sema, file_id, token);

    if let Some(definition) = sema.resolve_name_to_definition(file_id, token) {
        if let Some(r) =
            definition_to_hover(db, &definition, token.text_range(), inferred_ty.as_ref(), locale)
        {
            return Some(r);
        }
    } else {
        // HBK global property hover for non-MDO names. Runs BEFORE the
        // implicit-variable branch so bare `Метаданные` / `ОбработкаОшибок`
        // surface the rich HBK markup (readonly / min_version / availability)
        // rather than the coarser inferred-type rendering. Gates on
        // `inferred_ty` matching the HBK property's declared platform type
        // so an earlier `infer.rs` cascade step (var_types, form-self,
        // form-attr, ThisObject, MDO plural) keeps its workspace-specific
        // markup — see `hover_for_global_property` for the full rule.
        if let Some(r) = hover_for_global_property(
            db,
            file_id,
            token.text(),
            token.text_range(),
            inferred_ty.as_ref(),
        ) {
            return Some(r);
        }
        if let Some(ty) = inferred_ty.as_ref() {
            // Implicit variable (no `Перем`) — surface its inferred type.
            let mut markup = format!("**{}**\n\n", token.text());
            if let Some(type_block) = ty_info_markup(db, ty, locale) {
                markup.push_str(&type_block);
                return Some(HoverResult { markup, range: Some(token.text_range()) });
            }
        }
    }

    if let Some(r) = hover_for_global_function(db, token.text(), token.text_range()) {
        return Some(r);
    }

    hover_for_platform_type(db, token.text(), token.text_range())
}

/// HBK global property hover for bare identifiers. Returns `None` when:
///
/// - the name is an MDO plural — band 4 of `infer.rs::infer_path_name`
///   resolves it to `Ty::ManagerCollection` ahead of the HBK step;
/// - a workspace CommonModule with the same literal label exists in the
///   current source root — mirrors `Resolver::user_common_module_exists`,
///   the same gate `infer.rs:1493` uses;
/// - `inferred_ty` is present and does NOT match the HBK property's
///   declared platform type. An earlier cascade step (`var_types`,
///   form-self, form-attr, ThisObject, …) already claimed this name with
///   a different `Ty`; rendering HBK markup would mask the authoritative
///   resolution.
fn hover_for_global_property<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    name: &str,
    range: TextRange,
    inferred_ty: Option<&Ty>,
) -> Option<HoverResult> {
    if bsl_metadata::MdoType::from_plural(name).is_some() {
        return None;
    }
    let resolver = hir::Resolver::with_workspace_scope(hir::ModuleId::new(file_id));
    if resolver.user_common_module_exists(db, &hir::Name::new(name)) {
        return None;
    }
    let prop = PlatformDataInner::instance().get_global_property(name)?;
    if let Some(ty) = inferred_ty {
        // `infer.rs:1500` lowers `prop.property_types.first()` via
        // `TyLoweringContext::lower_bare_name`. Replay the same lowering
        // here and compare exactly — primitive declared types (`Число`,
        // `Строка`, `Булево`) lift to `Ty::Number` / `Ty::String` /
        // `Ty::Boolean`, none of which carry a `platform_type_name()`
        // string matching `Число` directly. A string-equality gate would
        // false-negative those; full `Ty` equality is what `infer.rs`
        // step 6 actually produces, so it's the right comparison.
        if let Some(declared) = prop.property_types.first() {
            let expected =
                hir::TyLoweringContext::new().lower_bare_name(&hir::Name::new(declared.as_str()));
            if *ty != expected {
                return None;
            }
        }
    }
    Some(render_property_hover(prop, range))
}

/// Append HBK-derived enrichment for an MDO plural to existing hover
/// markup: readonly marker, free-prose description / notes, `min_version`,
/// and availability context. Used by [`definition_to_hover`]'s
/// [`hir::Definition::MdoCollectionType`] arm to surface HBK metadata on top
/// of the workspace `ManagerCollection` shape.
///
/// No-op when [`bsl_metadata::MdoType::hbk_global_property`] returns `None`
/// — the three non-bareword variants (`Cube` / `DimensionTable` /
/// `CommonModule`) have no Global-context HBK entry, so the caller
/// keeps its legacy minimal markup for those.
fn append_hbk_mdo_plural_metadata(markup: &mut String, mdo_type: bsl_metadata::MdoType) {
    let Some(prop) = mdo_type.hbk_global_property() else {
        return;
    };
    if prop.is_readonly {
        markup.push_str("*Только чтение*\n\n");
    }
    if let Some(docs) = PlatformDataInner::instance().get_property_docs(prop.id) {
        if !docs.description.is_empty() {
            markup.push_str(&docs.description);
            markup.push_str("\n\n");
        }
        if let Some(notes) = docs.notes.filter(|n| !n.is_empty()) {
            markup.push_str("**Примечание:** ");
            markup.push_str(&notes);
            markup.push_str("\n\n");
        }
    }
    if let Some(ver) = prop.min_version.as_ref() {
        markup.push_str(&format!("**Доступен с версии:** {ver}"));
    }
    append_availability(markup, prop.context.as_ref());
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
    // Form-control receivers carry an ordered platform-type chain
    // `[base, extension?]` — walk reversed (extension first) so
    // hover for `<Pages>.ТекущаяСтраница` finds the extension docs,
    // and falls back to base for shared properties (`Видимость`, …).
    if let Ty::FormControl { kind, .. } = receiver_ty {
        for type_name in hir::form_control_platform_type_chain(*kind).iter().rev() {
            let input = MethodLookupInput::new(db, type_name.to_string(), prop_name.to_string());
            if let Some(prop) = platform_property_query(db, input) {
                return Some(render_property_hover(&prop, range));
            }
        }
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
    let hir::Definition::BuiltinMethodHandle { handle, .. } = definition else {
        return None;
    };
    hover_for_platform_method(db, &handle, token.text_range())
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
    db: &DB,
    sema: &Semantics<'_, DB>,
    file_id: FileId,
    token: &SyntaxToken,
) -> Option<Ty> {
    let token_range = token.text_range();
    let mut node = token.parent()?;
    while node.text_range() == token_range {
        // Phase 3 §4.G.5b: kernel-native boundary; bridge to `Ty` for the
        // still-`Ty` hover rendering (Phase 4 removes the bridge).
        let ty = hir::ty_bridge::typeid_to_ty(db, sema.type_of_expr(file_id, &node));
        if !ty.is_unknown() {
            return Some(ty);
        }
        node = node.parent()?;
    }
    // Declaration-site fallback: identifiers like the loop variable in
    // `Для Каждого X Из …`, the counter in classic `Для X = … По …`,
    // procedure parameters, and `Перем X` are bound through `BindingId`
    // and have no `Expr::Path` at their declaration site. The wrapper
    // walk above never finds a typed expression for them. Reach into
    // the per-body `var_types` (M3 Task 9 sibling map) by range and
    // surface the loop-element / counter / param type so hover on the
    // declaration matches hover at the use site.
    sema.type_of_binding_at(file_id, token_range).map(|id| hir::ty_bridge::typeid_to_ty(db, id))
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
    locale: Locale,
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
                if let Some(block) = ty_info_markup(db, ty, locale) {
                    markup.push_str(&block);
                }
            }
        }

        hir::Definition::Parameter { param_name, .. } => {
            markup.push_str(&format!("**Параметр {}**\n\n", param_name.as_str()));
            if let Some(ty) = inferred_ty {
                if let Some(block) = ty_info_markup(db, ty, locale) {
                    markup.push_str(&block);
                }
            }
        }

        hir::Definition::Local { var_name, .. } => {
            markup.push_str(&format!("**Локальная переменная {}**\n\n", var_name.as_str()));
            if let Some(ty) = inferred_ty {
                if let Some(block) = ty_info_markup(db, ty, locale) {
                    markup.push_str(&block);
                }
            }
        }

        hir::Definition::Module(_module_id) => {
            markup.push_str("**Модуль**\n\n");
        }

        hir::Definition::MdoCollectionType(mdo_type) => {
            // HBK-enriched rendering for the 17 bareword-valid MDO plurals
            // (Phase D). Title uses HBK's bilingual `name (english_name)`;
            // `Тип:` uses the workspace `ManagerCollection` shape, kept
            // authoritative through `ty_info_markup`. HBK metadata
            // (readonly / description / min_version / availability) is
            // appended via `append_hbk_mdo_plural_metadata`. For the 3
            // non-bareword MdoType variants (`Cube` / `DimensionTable` /
            // `CommonModule`), HBK has no Global-context entry and the
            // helper is a no-op; falls back to the legacy minimal markup.
            //
            // Inferred-type whitelist gate. An implicit assignment
            // (`Документы = Справочники`, `Документы = "x"`, …) rebinds the
            // bareword while `resolve_name_to_definition` still surfaces
            // `Definition::MdoCollectionType(Document)` — implicit locals
            // are not visible to the name classifier. The binding's
            // authoritative inferred type wins: HBK enrichment fires only
            // when the inferred type either is unknown (no rebind signal)
            // or matches `Ty::ManagerCollection(self)`. Anything else —
            // foreign-MDO manager (`ManagerCollection(other)` /
            // `ObjectManager { kind: other, .. }`) or a primitive shadow
            // (`Ty::String`, `Ty::Number`, …) — falls through to render
            // only the rebound shape via `ty_info_markup`.
            let inferred_disagrees = match inferred_ty {
                None => false,
                Some(ty) if ty.is_unknown() => false,
                Some(Ty::ManagerCollection(t)) if t == mdo_type => false,
                Some(_) => true,
            };
            if inferred_disagrees {
                if let Some(ty) = inferred_ty {
                    if let Some(block) = ty_info_markup(db, ty, locale) {
                        markup.push_str(&block);
                    }
                }
            } else if let Some(prop) = mdo_type.hbk_global_property() {
                markup.push_str(&format!("**{} ({})**\n\n", prop.name, prop.english_name));
                if let Some(ty_block) =
                    ty_info_markup(db, &Ty::ManagerCollection(*mdo_type), locale)
                {
                    markup.push_str(&ty_block);
                }
                append_hbk_mdo_plural_metadata(&mut markup, *mdo_type);
            } else {
                markup.push_str(&format!("**Тип метаданных:** {}\n\n", mdo_type.russian_name()));
                markup.push_str("*Коллекция объектов метаданных*");
            }
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
        | hir::Definition::BuiltinMethodHandle { .. }
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
    if let Some(ver) = prop.min_version.as_ref() {
        if markup.ends_with("\n\n") {
            // already padded
        } else if markup.ends_with('\n') {
            markup.push('\n');
        } else {
            markup.push_str("\n\n");
        }
        markup.push_str(&format!("**Доступен с версии:** {ver}"));
    }
    append_availability(&mut markup, prop.context.as_ref());
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
/// - `Ty::Query` / `Ty::QueryResult` / `Ty::QueryResultSelection` /
///   `Ty::QueryBatchResult` (projection-typed receivers seeded by the
///   SDBL ↔ Ty bridge) → routed through [`platform_type_markup`] using
///   the same `Запрос` / `РезультатЗапроса` / `ВыборкаИзРезультатаЗапроса`
///   / `Массив` keys their `Ty::PlatformObject` counterparts use. The
///   projection payload is not yet rendered inline — Phase 1.5+ will
///   surface field names from `SdblProjection.fields` when present.
/// - Anything else → single locale-aware `**Тип:** <label>` line via
///   [`Ty::display`]. Renders unions, MDO refs, manager refs, and
///   form-data wrappers richly (`СправочникСсылка.Товары` /
///   `CatalogRef.Товары`, `Справочник.Товары` / `Catalog.Товары`,
///   `ДанныеФормыСтруктура (ДокументОбъект.ПКО)`).
fn ty_info_markup<DB: RootDatabase>(db: &DB, ty: &Ty, locale: Locale) -> Option<String> {
    if ty.is_unknown() {
        return None;
    }

    if let Ty::PlatformObject(name) = ty {
        if let Some(block) = platform_type_markup(db, name.as_str()) {
            return Some(block);
        }
        return Some(format!("**Тип:** {}\n\n", name.as_str()));
    }

    // Route projection-typed receivers through the same platform docs
    // their `Ty::PlatformObject("Запрос" / "РезультатЗапроса" / …)`
    // equivalents would reach. Without this branch, hovering over a
    // `Новый Запрос` site once Phase 1.3 starts synthesizing `Ty::Query`
    // would silently fall to the bare `**Тип:**` line and lose the
    // rich platform docs the user gets today.
    if let Some(platform_key) = query_variant_platform_key(ty) {
        let mut block = platform_type_markup(db, platform_key)
            .unwrap_or_else(|| format!("**Тип:** {}\n\n", ty.display(locale)));
        // Phase E enrichment — if this is a `Ty::QueryResultSelection`
        // with a resolved SDBL projection, append the per-column
        // shape so the user sees the schema directly on hover. Uses
        // `SdblTypeShadow.display` when present (`Строка(50)` /
        // `Число(15,2)`) and falls back to the bridged `Ty.display`
        // when the bridge didn't capture the shadow.
        if let Some(fields_block) = projection_fields_markup(ty, locale) {
            block.push_str(&fields_block);
        }
        return Some(block);
    }

    // Phase 3 §4.G.5d: this receiver-type block stays `Ty`-rendered — kernel
    // display does not yet reproduce the rich `ManagerCollection` / MDO-plural
    // workspace shape (it falls back to a bare `MdoType` label). Deferred to
    // Phase 4 alongside polishing `bsl_types::display` for manager shapes.
    Some(format!("**Тип:** {}\n\n", ty.display(locale)))
}

/// Render the SDBL projection of a [`Ty::QueryResultSelection`] as a
/// trailing `**Поля:** ...` markup block, or `None` when the receiver
/// is not projection-typed.
///
/// Format: a single bold heading followed by a comma-separated list
/// `Имя: Строка(50), Цена: Число(15,2), …`. Per-column labels prefer
/// the SDBL shadow when the bridge captured it (precision / scale /
/// length survive); otherwise fall back to the bridged `Ty.display`
/// in the caller's locale.
fn projection_fields_markup(ty: &Ty, locale: Locale) -> Option<String> {
    let projection = match ty {
        Ty::QueryResultSelection { projection: Some(p) }
        | Ty::ValueTable { projection: Some(p) }
        | Ty::ValueTableRow { projection: Some(p) } => p,
        _ => return None,
    };
    if projection.fields.is_empty() {
        return None;
    }
    // Lead with a blank-line separator: the upstream
    // `platform_type_markup` block doesn't always end with one (e.g.
    // the "и еще N методов" trailer is non-terminated), and without
    // the separator the projection heading collides with the
    // preceding line in the rendered markup.
    let mut out = String::from("\n\n**Поля:** ");
    let shadows = projection.raw_sdbl_types.as_deref();
    for (i, (name, field_ty)) in projection.fields.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(name.as_str());
        out.push_str(": ");
        let label = shadows
            .and_then(|s| s.get(i))
            .map(|shadow| shadow.display.clone())
            .unwrap_or_else(|| field_ty.display(locale).to_string());
        out.push_str(&label);
    }
    out.push_str("\n\n");
    Some(out)
}

/// Map a projection-typed `Ty` to the platform-data key under which its
/// methods and docs are indexed.
///
/// Mirrors `hir_ty::method_lookup::platform_type_key` for the four
/// projection variants seeded in Phase 0 — once Phase 1.3 starts
/// synthesizing these, hover must reach the same `bsl-platform` row
/// `method_lookup` reaches, or the IDE shows different surfaces for a
/// chained `.Выполнить()` value vs a freshly-typed `Новый Запрос`.
fn query_variant_platform_key(ty: &Ty) -> Option<&'static str> {
    match ty {
        Ty::Query { .. } => Some("Запрос"),
        Ty::QueryResult { .. } => Some("РезультатЗапроса"),
        Ty::QueryResultSelection { .. } => Some("ВыборкаИзРезультатаЗапроса"),
        // `ВыполнитьПакет()` returns an array of `РезультатЗапроса` —
        // share the `Array` table for iteration / `.Количество()` so
        // chained access stays consistent.
        Ty::QueryBatchResult { .. } => Some("Массив"),
        // Phase H — projected `Ty::ValueTable` / `Ty::ValueTableRow`
        // route to the same `ТаблицаЗначений` / `СтрокаТаблицыЗначений`
        // platform docs their projection-less counterparts use. The
        // projection block is then appended by
        // [`projection_fields_markup`] for the `Some(p)` shape.
        Ty::ValueTable { .. } => Some("ТаблицаЗначений"),
        Ty::ValueTableRow { .. } => Some("СтрокаТаблицыЗначений"),
        _ => None,
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
    handle: &hir::PlatformMethodHandle,
    range: TextRange,
) -> Option<HoverResult> {
    // Use the handle's stable id-walk to fetch the underlying
    // `PlatformMethod`. Covers both scalar (`(type_name, method_name)`)
    // and composite-prefix (`<Prefix>.<MDO>` with placeholder name)
    // shapes uniformly — the resolution path that produced the handle
    // already disambiguated which index to consult, so handle.lookup
    // re-fetches without re-routing.
    let method = handle.lookup(db)?;
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
