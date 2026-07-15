use hir::{form_element_type, ModuleId, Name};
use ide_db::base_db::Locale;
use ide_db::RootDatabaseImpl;
use symbol_info::{build_signature, CalleeKind};
use vfs::FileId;

use super::ResolvedForm;
use crate::symbol_info::{
    card_from_method_sig, file_path, name_eq, type_label, SymbolContainer, SymbolInfoCard,
    SymbolInfoRequest, SymbolMember,
};

pub(super) fn form_card(
    db: &RootDatabaseImpl,
    symbol: &str,
    resolved: &ResolvedForm,
    req: &SymbolInfoRequest,
) -> SymbolInfoCard {
    let mut card = SymbolInfoCard::empty(symbol.to_string(), "form");
    card.container = Some(form_container(resolved));
    card.signature = Some(format!(
        "{} — {} реквизит(ов), {} элемент(ов), {} обработчик(ов)",
        resolved.qualified_form_name(),
        resolved.form.attributes().len(),
        resolved.form.elements().len(),
        resolved.form.event_handlers().len() + resolved.form.command_handlers().len()
    ));
    let fields = hir::module_implicit_fields(db, resolved.file_id);
    let mut members = Vec::new();
    for attr in resolved.form.attributes() {
        members.push(SymbolMember {
            name: attr.name.clone(),
            kind: form_attribute_kind(attr),
            ty: form_attribute_type_from_fields(db, &fields, &attr.name, req.locale),
        });
    }
    for element in resolved.form.elements() {
        members.push(SymbolMember {
            name: element.name.clone(),
            kind: form_item_kind(element, req.locale),
            ty: form_item_member_type(db, resolved, element, req.locale),
        });
    }
    for handler in resolved.form.event_handlers() {
        members.push(SymbolMember {
            name: handler.handler_name.clone(),
            kind: "Обработчик".to_string(),
            ty: None,
        });
    }
    for handler in resolved.form.command_handlers() {
        members.push(SymbolMember {
            name: handler.clone(),
            kind: "Обработчик".to_string(),
            ty: None,
        });
    }
    card.members = members;
    card
}

pub(super) fn form_member_card(
    db: &RootDatabaseImpl,
    symbol: &str,
    resolved: &ResolvedForm,
    member: &str,
    req: &SymbolInfoRequest,
) -> Option<SymbolInfoCard> {
    if let Some(attr) = resolved.form.find_attribute(member) {
        return Some(form_attribute_card(db, symbol, resolved, attr, req));
    }
    if let Some(element) = resolved.form.find_element(member) {
        return Some(form_item_card(db, symbol, resolved, element, req));
    }
    form_handler_card(db, symbol, resolved, member, req)
}

fn form_attribute_card(
    db: &RootDatabaseImpl,
    symbol: &str,
    resolved: &ResolvedForm,
    attr: &bsl_metadata::FormAttribute,
    req: &SymbolInfoRequest,
) -> SymbolInfoCard {
    let mut card = SymbolInfoCard::empty(symbol.to_string(), "form attribute");
    card.container = Some(form_container(resolved));
    if req.sections.type_ {
        let fields = hir::module_implicit_fields(db, resolved.file_id);
        card.return_type = form_attribute_type_from_fields(db, &fields, &attr.name, req.locale);
    }
    if attr.is_main {
        card.signature = Some("Объект — основной реквизит формы".to_string());
    }
    card
}

fn form_item_card(
    db: &RootDatabaseImpl,
    symbol: &str,
    resolved: &ResolvedForm,
    element: &bsl_metadata::FormElement,
    req: &SymbolInfoRequest,
) -> SymbolInfoCard {
    let mut card = SymbolInfoCard::empty(symbol.to_string(), "form item");
    card.container = Some(form_container(resolved));
    let kind = form_item_kind(element, req.locale);
    card.signature = Some(match element.data_path.as_deref() {
        Some(data_path) => format!("{kind}: {data_path}"),
        None => kind.clone(),
    });
    if req.sections.type_ {
        card.return_type =
            form_element_type_label(db, resolved, element, req.locale).or(Some(kind));
    }
    card
}

fn form_handler_card(
    db: &RootDatabaseImpl,
    symbol: &str,
    resolved: &ResolvedForm,
    member: &str,
    req: &SymbolInfoRequest,
) -> Option<SymbolInfoCard> {
    let module_id = ModuleId::new(resolved.file_id);
    let method = Name::new(member);
    let callee = CalleeKind::LocalMethod { module_id, method };
    let sigs = build_signature(db, resolved.file_id, &callee)?;
    let sig = sigs.first()?;
    let mut card = card_from_method_sig(db, symbol, sig, Some(form_container(resolved)), req);
    card.graph_id = form_handler_graph_id(
        db,
        resolved.file_id,
        &sig.name_russian,
        req.workspace_root.as_deref(),
    );
    Some(card)
}

fn form_container(resolved: &ResolvedForm) -> SymbolContainer {
    SymbolContainer {
        kind: "Форма".to_string(), name: resolved.container_name(), context: None
    }
}

fn form_attribute_kind(attr: &bsl_metadata::FormAttribute) -> String {
    if attr.is_main {
        "Реквизит формы (основной)"
    } else {
        "Реквизит формы"
    }
    .to_string()
}

fn form_attribute_type_from_fields(
    db: &RootDatabaseImpl,
    fields: &[hir::Field],
    attr_name: &str,
    locale: Locale,
) -> Option<String> {
    fields
        .iter()
        .find(|field| name_eq(field.name.as_str(), attr_name))
        .and_then(|field| type_label(db, field.ty, locale))
}

fn form_item_kind(element: &bsl_metadata::FormElement, locale: Locale) -> String {
    element
        .kind
        .base_platform_type_name()
        .unwrap_or_else(|| hir::form_element_kind_label(element.kind, locale))
        .to_string()
}

fn form_item_member_type(
    db: &RootDatabaseImpl,
    resolved: &ResolvedForm,
    element: &bsl_metadata::FormElement,
    locale: Locale,
) -> Option<String> {
    let label = form_element_type_label(db, resolved, element, locale)?;
    Some(match element.data_path.as_deref() {
        Some(data_path) => format!("{label}: {data_path}"),
        None => label,
    })
}

fn form_element_type_label(
    db: &RootDatabaseImpl,
    resolved: &ResolvedForm,
    element: &bsl_metadata::FormElement,
    locale: Locale,
) -> Option<String> {
    let ty = form_element_type(db, resolved.file_id, &resolved.form, element);
    type_label(db, ty, locale)
}

fn form_handler_graph_id(
    db: &RootDatabaseImpl,
    form_file: FileId,
    method_name: &str,
    workspace_root: Option<&std::path::Path>,
) -> Option<String> {
    let path = file_path(db, form_file)?;
    // The graph builder encodes a form handler's fallback id relative to the WORKSPACE root, not
    // the config root — so strip exactly that root here. Without a known root an absolute path has
    // no resolvable rel, so `method_graph_id` returns `None` (no usages) rather than a wrong id.
    crate::graph::method_graph_id(&path, method_name, workspace_root)
}
