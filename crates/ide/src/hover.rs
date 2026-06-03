use bsl_platform::{
    global_function_query, platform_property_query, platform_type_query, type_methods_query,
    ContextAvailability, MethodLookupInput, PlatformDataInner, PlatformMethod, PlatformProperty,
    TypeNameInput,
};
use hir::{
    classify_token, kernel_type_label, platform_type_key_id, Builders, Field, NameClass, Semantics,
    Type as HirType, TypeId, TypeKind,
};
use ide_db::base_db::Locale;
use ide_db::RootDatabase;
use symbol_info::{from_global_function, from_platform_method, render_hover_markdown, Lang};
use syntax::{SyntaxNode, SyntaxToken, TextRange, TextSize};
use vfs::FileId;

use crate::HoverResult;

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
        NameClass::Literal { .. } => None,
        NameClass::Other => None,
    }
}

fn hover_field<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    receiver: &SyntaxNode,
    token: &SyntaxToken,
    is_call: bool,
    locale: Locale,
) -> Option<HoverResult> {
    let sema = Semantics::new(db);
    let receiver_id = sema.type_of_expr(file_id, receiver);
    let name = token.text();
    let range = token.text_range();

    let property = || hover_platform_property_on_ty(db, receiver_id, name, range);
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

    if let Some(field) = mdo_field_on_id(db, file_id, receiver_id, name) {
        if field.value_ty.is_some() {
            return Some(render_mdo_field_hover(db, &field, name, range, locale));
        }
    }

    let inferred_ty = type_of_token(db, &sema, file_id, token);
    if let Some(definition) = sema.resolve_name_to_definition(file_id, token) {
        return definition_to_hover(db, &definition, range, inferred_ty, locale);
    }

    if let Some(parent) = token.parent() {
        let id = sema.type_of_expr(file_id, &parent);
        if !matches!(db.lookup_type(id), TypeKind::Unknown) {
            let mut markup = format!("**{}**\n\n", name);
            if let Some(type_block) = ty_info_markup(db, id, locale) {
                markup.push_str(&type_block);
                return Some(HoverResult { markup, range: Some(range) });
            }
        }
    }

    None
}

fn mdo_field_on_id<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    receiver: TypeId,
    field_name: &str,
) -> Option<Field> {
    let needle = field_name.to_lowercase();
    HirType::from_id(db, file_id, receiver).fields().into_iter().find(|field| {
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
    kernel_type_label(db, id, locale, true)
}

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
            definition_to_hover(db, &definition, token.text_range(), inferred_ty, locale)
        {
            return Some(r);
        }
    } else {
        if let Some(r) =
            hover_for_global_property(db, file_id, token.text(), token.text_range(), inferred_ty)
        {
            return Some(r);
        }
        if let Some(ty) = inferred_ty {
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

fn hover_for_global_property<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    name: &str,
    range: TextRange,
    inferred_ty: Option<TypeId>,
) -> Option<HoverResult> {
    if bsl_metadata::MdoType::from_plural(name).is_some() {
        return None;
    }
    let resolver = hir::Resolver::with_workspace_scope(hir::ModuleId::new(file_id));
    if resolver.user_common_module_exists(db, &hir::Name::new(name)) {
        return None;
    }
    let prop = PlatformDataInner::instance().get_global_property(name)?;
    if let Some(id) = inferred_ty {
        if let Some(declared) = prop.property_types.first() {
            let expected = hir::TyLoweringContext::new()
                .lower_bare_name_id(db, &hir::Name::new(declared.as_str()));
            if id != expected {
                return None;
            }
        }
    }
    Some(render_property_hover(prop, range))
}

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

fn hover_platform_property_on_ty<DB: RootDatabase>(
    db: &DB,
    receiver: TypeId,
    prop_name: &str,
    range: TextRange,
) -> Option<HoverResult> {
    if let TypeKind::FormControl { kind, .. } = db.lookup_type(receiver) {
        let kind = *kind;
        for type_name in hir::form_control_platform_type_chain(kind).iter().rev() {
            let input = MethodLookupInput::new(db, type_name.to_string(), prop_name.to_string());
            if let Some(prop) = platform_property_query(db, input) {
                return Some(render_property_hover(&prop, range));
            }
        }
        return None;
    }
    let type_key = platform_type_key_id(db, receiver)?;
    let input = MethodLookupInput::new(db, type_key, prop_name.to_string());
    let prop = platform_property_query(db, input)?;
    Some(render_property_hover(&prop, range))
}

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

fn type_of_token<DB: RootDatabase>(
    db: &DB,
    sema: &Semantics<'_, DB>,
    file_id: FileId,
    token: &SyntaxToken,
) -> Option<TypeId> {
    let token_range = token.text_range();
    let mut node = token.parent()?;
    while node.text_range() == token_range {
        let id = sema.type_of_expr(file_id, &node);
        if !matches!(db.lookup_type(id), TypeKind::Unknown) {
            return Some(id);
        }
        node = node.parent()?;
    }
    sema.type_of_binding_at(file_id, token_range)
}

fn definition_to_hover<DB: RootDatabase>(
    db: &DB,
    definition: &hir::Definition,
    range: TextRange,
    inferred_ty: Option<TypeId>,
    locale: Locale,
) -> Option<HoverResult> {
    let mut markup = String::new();

    match definition {
        hir::Definition::Method(_method_id) => {
            let label = definition.label(db);
            markup.push_str(&format!("**{}**\n\n", label));

            if definition.is_export(db) {
                markup.push_str("*Экспортная*\n\n");
            }

            if let Some(docs) = definition.docs(db) {
                if let Some(ref purpose) = docs.purpose {
                    if !purpose.is_empty() {
                        markup.push_str("**Назначение:**\n");
                        markup.push_str(purpose);
                        markup.push_str("\n\n");
                    }
                }

                if !docs.parameters.is_empty() {
                    markup.push_str("**Параметры:**\n");
                    for param in &docs.parameters {
                        markup.push_str(&format!("- **{}**", param.name));

                        if !param.types.is_empty() {
                            let type_names: Vec<_> =
                                param.types.iter().map(|t| t.name.as_str()).collect();
                            markup.push_str(&format!(": {}", type_names.join(", ")));
                        }

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

                if !docs.returned_value.is_empty() {
                    markup.push_str("**Возвращаемое значение:**\n");
                    let type_names: Vec<_> =
                        docs.returned_value.iter().map(|t| t.name.as_str()).collect();
                    markup.push_str(&format!("Тип: {}\n", type_names.join(", ")));

                    if let Some(first_type) = docs.returned_value.first() {
                        if let Some(ref desc) = first_type.description {
                            if !desc.is_empty() {
                                markup.push_str(&format!("{}\n", desc));
                            }
                        }
                    }
                    markup.push('\n');
                }

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
            let inferred_disagrees = match inferred_ty {
                None => false,
                Some(id) => match db.lookup_type(id) {
                    TypeKind::Unknown => false,
                    TypeKind::ManagerCollection(t) if t == mdo_type => false,
                    _ => true,
                },
            };
            if inferred_disagrees {
                if let Some(id) = inferred_ty {
                    if let Some(block) = ty_info_markup(db, id, locale) {
                        markup.push_str(&block);
                    }
                }
            } else if let Some(prop) = mdo_type.hbk_global_property() {
                markup.push_str(&format!("**{} ({})**\n\n", prop.name, prop.english_name));
                if let Some(ty_block) = ty_info_markup(db, db.manager_collection(*mdo_type), locale)
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

        hir::Definition::BuiltinFunction(_)
        | hir::Definition::BuiltinMethodHandle { .. }
        | hir::Definition::VirtualTableField { .. }
        | hir::Definition::Unresolved => return None,
    }

    Some(HoverResult { markup, range: Some(range) })
}

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

fn hover_for_platform_type<DB: RootDatabase>(
    db: &DB,
    type_name: &str,
    range: TextRange,
) -> Option<HoverResult> {
    let markup = platform_type_markup(db, type_name)?;
    Some(HoverResult { markup, range: Some(range) })
}

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

fn ty_info_markup<DB: RootDatabase>(db: &DB, id: TypeId, locale: Locale) -> Option<String> {
    let kind = db.lookup_type(id);
    if matches!(kind, TypeKind::Unknown) {
        return None;
    }

    if let TypeKind::PlatformObject(facet) = kind {
        if let Some(block) = platform_type_markup(db, facet.name.as_str()) {
            return Some(block);
        }
        return Some(format!("**Тип:** {}\n\n", facet.name.as_str()));
    }

    if let Some(platform_key) = query_variant_platform_key(kind) {
        let mut block = platform_type_markup(db, platform_key).unwrap_or_else(|| {
            format!("**Тип:** {}\n\n", kernel_type_label(db, id, locale, false))
        });
        if let Some(fields_block) = projection_fields_markup(db, id, locale) {
            block.push_str(&fields_block);
        }
        return Some(block);
    }

    Some(format!("**Тип:** {}\n\n", kernel_type_label(db, id, locale, false)))
}

fn projection_fields_markup<DB: RootDatabase>(
    db: &DB,
    id: TypeId,
    locale: Locale,
) -> Option<String> {
    let projection = match db.lookup_type(id) {
        TypeKind::QueryResultSelection(facet) => facet.projection.as_ref(),
        TypeKind::ValueTable(facet) | TypeKind::ValueTableRow(facet) => facet.projection.as_ref(),
        _ => return None,
    }?;
    if projection.fields.is_empty() {
        return None;
    }
    let mut out = String::from("\n\n**Поля:** ");
    let shadows = projection.raw_sdbl_types.as_deref();
    for (i, field) in projection.fields.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(field.name.as_str());
        out.push_str(": ");
        let label = shadows
            .and_then(|s| s.get(i))
            .map(|shadow| shadow.display.clone())
            .unwrap_or_else(|| hir::kernel_type_label(db, field.ty, locale, false));
        out.push_str(&label);
    }
    out.push_str("\n\n");
    Some(out)
}

fn query_variant_platform_key(kind: &TypeKind) -> Option<&'static str> {
    match kind {
        TypeKind::Query { .. } => Some("Запрос"),
        TypeKind::QueryResult(_) => Some("РезультатЗапроса"),
        TypeKind::QueryResultSelection(_) => Some("ВыборкаИзРезультатаЗапроса"),
        TypeKind::QueryBatchResult { .. } => Some("Массив"),
        TypeKind::ValueTable(_) => Some("ТаблицаЗначений"),
        TypeKind::ValueTableRow(_) => Some("СтрокаТаблицыЗначений"),
        _ => None,
    }
}

fn hover_for_platform_method<DB: RootDatabase>(
    db: &DB,
    handle: &hir::PlatformMethodHandle,
    range: TextRange,
) -> Option<HoverResult> {
    let method = handle.lookup(db)?;
    let docs = PlatformDataInner::instance().get_method_docs(method.id);

    let sig = from_platform_method(&method, docs.as_ref());
    let mut markup = render_hover_markdown(&sig, Lang::Russian);
    append_availability(&mut markup, method.context.as_ref());

    Some(HoverResult { markup, range: Some(range) })
}

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

fn hover_keyword(token: &SyntaxToken) -> Option<HoverResult> {
    if !token.kind().is_keyword() {
        return None;
    }

    let keyword_text = token.text();

    let keyword_docs = bsl_platform::PlatformData::instance().get_keyword_docs(keyword_text)?;

    let mut markup = String::new();

    markup.push_str(&format!(
        "**{}** / **{}**\n\n",
        keyword_docs.keyword_ru, keyword_docs.keyword_en
    ));

    if !keyword_docs.syntax.is_empty() {
        markup.push_str("**Синтаксис:**\n```bsl\n");
        markup.push_str(&keyword_docs.syntax);
        markup.push_str("\n```\n\n");
    }

    if !keyword_docs.description.is_empty() {
        markup.push_str(&keyword_docs.description);
        markup.push_str("\n\n");
    }

    if !keyword_docs.params.is_empty() {
        markup.push_str("**Параметры:**\n");
        for param in &keyword_docs.params {
            markup.push_str(&format!("- **{}**: {}\n", param.name, param.description));
        }
        markup.push('\n');
    }

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
        let data = PlatformDataInner::instance();
        if data.all_methods().is_empty() {
            println!("Skipping test: no platform methods available");
            return;
        }

        let method = data
            .all_methods()
            .iter()
            .find(|m| !m.parameters.is_empty())
            .expect("Should have at least one method with parameters");

        let sig = format_method_signature(method);

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
