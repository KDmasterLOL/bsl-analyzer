use bsl_metadata::MdoType;
use bsl_platform::{
    manager_methods_query, type_methods_query, type_properties_query, TypeNameInput,
};
use hir::{ManagerType, MetadataReferenceKind, Name, Semantics, TypeKind};
use ide_db::RootDatabase;
use stdx::case::CaseExt;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use super::{CompletionItem, CompletionItemKind, CompletionPosition};

pub(super) fn mdo_completions<DB: RootDatabase>(
    db: &DB,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let _span = tracing::info_span!("mdo_completions").entered();

    let parse = db.parse(position.file_id);
    let root = parse.syntax_node();

    let token = root.token_at_offset(position.offset).left_biased()?;

    let context = detect_mdo_context(&token)?;
    tracing::debug!(?context, "MDO completion context detected");

    match context {
        MdoContext::MetadataRoot { metadata_expr } => {
            if !metadata_root_is_available(db, position.file_id, &metadata_expr) {
                return None;
            }
            let items = complete_metadata_root_collections(db, position.locale);
            if !items.is_empty() {
                return Some(items);
            }
        }
        MdoContext::MetadataCollection { metadata_expr, collection } => {
            if !metadata_root_is_available(db, position.file_id, &metadata_expr) {
                return None;
            }
            let items = match collection {
                MetadataCollectionKind::Manager(mdo_type) => {
                    complete_mdo_objects(db, position.file_id, mdo_type)
                }
                MetadataCollectionKind::Reference(kind) => {
                    complete_metadata_reference_objects(db, position.file_id, kind)
                }
            };
            if !items.is_empty() {
                return Some(items);
            }
        }
        MdoContext::Collection { mdo_type } => {
            let items = complete_mdo_objects(db, position.file_id, mdo_type);
            if !items.is_empty() {
                return Some(items);
            }
        }
        MdoContext::Object { mdo_type, object_name } => {
            let mut items = Vec::new();

            if let Some(prefix) = mdo_type.manager_type_prefix() {
                items.extend(complete_manager_methods(db, prefix));
            }

            items.extend(complete_manager_module_methods(
                db,
                position.file_id,
                mdo_type,
                &object_name,
            ));

            items.extend(complete_predefined_items(db, position.file_id, mdo_type, &object_name));

            if !items.is_empty() {
                return Some(items);
            }
        }
    }

    None
}

#[derive(Debug)]
enum MdoContext {
    MetadataRoot { metadata_expr: SyntaxNode },
    MetadataCollection { metadata_expr: SyntaxNode, collection: MetadataCollectionKind },
    Collection { mdo_type: MdoType },
    Object { mdo_type: MdoType, object_name: String },
}

#[derive(Debug, Clone, Copy)]
enum MetadataCollectionKind {
    Manager(MdoType),
    Reference(MetadataReferenceKind),
}

fn detect_mdo_context(token: &SyntaxToken) -> Option<MdoContext> {
    if token.kind() == SyntaxKind::DOT {
        return detect_from_dot(token);
    }

    if token.kind().is_name_token() {
        return detect_from_ident_after_dot(token);
    }

    None
}

fn detect_from_dot(dot_token: &SyntaxToken) -> Option<MdoContext> {
    let receiver = find_receiver_before_dot(dot_token)?;

    if let Some(ident_text) = get_single_ident(&receiver) {
        if is_metadata_root_name(&ident_text) {
            return Some(MdoContext::MetadataRoot { metadata_expr: receiver });
        }
        if let Some(mdo_type) = MdoType::from_plural(&ident_text) {
            return Some(MdoContext::Collection { mdo_type });
        }
    }

    if receiver.kind() == SyntaxKind::FIELD_EXPR {
        if let Some((base, base_text, object_name)) = get_field_expr_parts(&receiver) {
            if is_metadata_root_name(&base_text) {
                if let Some(collection) = metadata_collection_from_plural(&object_name) {
                    return Some(MdoContext::MetadataCollection {
                        metadata_expr: base,
                        collection,
                    });
                }
            }
            if let Some(mdo_type) = MdoType::from_plural(&base_text) {
                return Some(MdoContext::Object { mdo_type, object_name });
            }
        }
    }

    None
}

fn detect_from_ident_after_dot(ident_token: &SyntaxToken) -> Option<MdoContext> {
    let has_dot_before = ident_token
        .siblings_with_tokens(syntax::Direction::Prev)
        .skip(1)
        .find(|s| s.kind() != SyntaxKind::WHITESPACE)
        .is_some_and(|s| s.kind() == SyntaxKind::DOT);

    if !has_dot_before {
        return None;
    }

    let field_expr = ident_token.parent()?;
    if field_expr.kind() != SyntaxKind::FIELD_EXPR {
        return None;
    }

    let base = field_expr.children().next()?;

    if let Some(base_text) = get_single_ident(&base) {
        if is_metadata_root_name(&base_text) {
            return Some(MdoContext::MetadataRoot { metadata_expr: base });
        }
        if let Some(mdo_type) = MdoType::from_plural(&base_text) {
            return Some(MdoContext::Collection { mdo_type });
        }
    }

    if base.kind() == SyntaxKind::FIELD_EXPR {
        if let Some((metadata_base, base_text, object_name)) = get_field_expr_parts(&base) {
            if is_metadata_root_name(&base_text) {
                if let Some(collection) = metadata_collection_from_plural(&object_name) {
                    return Some(MdoContext::MetadataCollection {
                        metadata_expr: metadata_base,
                        collection,
                    });
                }
            }
            if let Some(mdo_type) = MdoType::from_plural(&base_text) {
                return Some(MdoContext::Object { mdo_type, object_name });
            }
        }
    }

    None
}

fn find_receiver_before_dot(dot_token: &SyntaxToken) -> Option<SyntaxNode> {
    let parent = dot_token.parent()?;

    if parent.kind() == SyntaxKind::FIELD_EXPR {
        return parent.children().next();
    }

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

fn get_single_ident(node: &SyntaxNode) -> Option<String> {
    if node.kind() != SyntaxKind::IDENT {
        return None;
    }
    let token = node.first_token()?;
    if token.kind() == SyntaxKind::IDENT {
        Some(token.text().to_string())
    } else {
        None
    }
}

fn get_field_expr_parts(node: &SyntaxNode) -> Option<(SyntaxNode, String, String)> {
    let base = node.children().next()?;
    let base_text = get_single_ident(&base)?;

    let field_token = node
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .filter(|t| t.kind().is_name_token())
        .last()?;

    Some((base, base_text, field_token.text().to_string()))
}

fn is_metadata_root_name(text: &str) -> bool {
    matches!(text.fold_lower().as_str(), "метаданные" | "metadata")
}

fn metadata_collection_from_plural(text: &str) -> Option<MetadataCollectionKind> {
    if let Some(kind) = MetadataReferenceKind::from_plural(text) {
        return Some(MetadataCollectionKind::Reference(kind));
    }
    let mdo_type = MdoType::from_plural(text)?;
    mdo_type.manager_type_prefix().map(|_| MetadataCollectionKind::Manager(mdo_type))
}

fn metadata_root_is_available<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    metadata_expr: &SyntaxNode,
) -> bool {
    let sema = Semantics::new(db);
    let ty = sema.type_of_expr(file_id, metadata_expr);
    let TypeKind::PlatformObject(facet) = db.lookup_type(ty) else {
        return false;
    };
    matches!(facet.name.as_str(), "ОбъектМетаданныхКонфигурация" | "ConfigurationMetadataObject")
}

fn complete_mdo_objects<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    mdo_type: MdoType,
) -> Vec<CompletionItem> {
    let configs = db.get_all_configurations(file_id);
    let mut items = Vec::new();

    for (source_name, config) in &configs {
        let type_label = mdo_type.russian_name();

        for obj in config.metadata_objects() {
            if obj.mdo_type == mdo_type {
                items.push(CompletionItem {
                    label: obj.name.clone(),
                    detail: Some(type_label.to_string()),
                    kind: CompletionItemKind::MdoObject,
                    insert_text: obj.name.clone(),
                    documentation: None,
                    sort_text: None,
                    filter_text: None,
                    source: source_name.clone(),
                });
            }
        }

        for reg in config.registers() {
            if reg.mdo_type() == mdo_type {
                items.push(CompletionItem {
                    label: reg.name().to_string(),
                    detail: Some(type_label.to_string()),
                    kind: CompletionItemKind::MdoObject,
                    insert_text: reg.name().to_string(),
                    documentation: None,
                    sort_text: None,
                    filter_text: None,
                    source: source_name.clone(),
                });
            }
        }
    }

    tracing::debug!(count = items.len(), "MDO objects found");
    items
}

fn complete_metadata_root_collections<DB: RootDatabase>(
    db: &DB,
    locale: ide_db::base_db::Locale,
) -> Vec<CompletionItem> {
    let type_name = "ОбъектМетаданныхКонфигурация";
    let methods_input = TypeNameInput::new(db, type_name.to_string());
    let mut items: Vec<CompletionItem> = type_methods_query(db, methods_input)
        .iter()
        .map(super::platform_completion::render_platform_method)
        .collect();
    let props_input = TypeNameInput::new(db, type_name.to_string());
    items.extend(type_properties_query(db, props_input).iter().map(|prop| {
        if let Some(kind) = MetadataReferenceKind::from_plural(prop.name.as_str())
            .or_else(|| MetadataReferenceKind::from_plural(prop.english_name.as_str()))
        {
            render_metadata_reference_collection(kind, Some(prop))
        } else if let Some(mdo_type) = MdoType::from_plural(prop.name.as_str())
            .or_else(|| MdoType::from_plural(prop.english_name.as_str()))
            .filter(|mdo_type| mdo_type.manager_type_prefix().is_some())
        {
            render_manager_collection(mdo_type, Some(prop))
        } else {
            super::platform_completion::render_platform_property(prop, locale)
        }
    }));
    items
}

fn render_manager_collection(
    mdo_type: MdoType,
    prop: Option<&bsl_platform::PlatformProperty>,
) -> CompletionItem {
    let label =
        prop.map_or_else(|| mdo_type.russian_name().to_string(), |prop| prop.name.to_string());
    let filter_text = prop.map(|prop| format!("{} {}", prop.name, prop.english_name));
    CompletionItem {
        label: label.clone(),
        detail: Some(format!("Коллекция метаданных ({})", mdo_type.russian_name())),
        kind: CompletionItemKind::MdoType,
        insert_text: label,
        documentation: None,
        sort_text: None,
        filter_text,
        source: None,
    }
}

fn render_metadata_reference_collection(
    kind: MetadataReferenceKind,
    prop: Option<&bsl_platform::PlatformProperty>,
) -> CompletionItem {
    let label =
        prop.map_or_else(|| kind.russian_plural().to_string(), |prop| prop.name.to_string());
    let filter_text = prop
        .map(|prop| format!("{} {}", prop.name, prop.english_name))
        .or_else(|| Some(format!("{} {}", kind.russian_plural(), kind.english_plural())));
    CompletionItem {
        label: label.clone(),
        detail: Some(format!("Коллекция метаданных ({})", kind.russian_singular())),
        kind: CompletionItemKind::MdoType,
        insert_text: label,
        documentation: None,
        sort_text: None,
        filter_text,
        source: None,
    }
}

fn complete_metadata_reference_objects<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    kind: MetadataReferenceKind,
) -> Vec<CompletionItem> {
    let Some(config) = db.merged_visible_configuration(file_id) else {
        return Vec::new();
    };
    metadata_reference_names(&config, kind)
        .into_iter()
        .map(|name| CompletionItem {
            label: name.clone(),
            detail: Some(kind.russian_singular().to_string()),
            kind: CompletionItemKind::MdoObject,
            insert_text: name,
            documentation: None,
            sort_text: None,
            filter_text: None,
            source: None,
        })
        .collect()
}

fn metadata_reference_names(
    config: &bsl_metadata::Configuration,
    kind: MetadataReferenceKind,
) -> Vec<String> {
    match kind {
        MetadataReferenceKind::Role => {
            config.roles().iter().map(|item| item.name().to_string()).collect()
        }
        MetadataReferenceKind::EventSubscription => {
            config.event_subscriptions().iter().map(|item| item.name().to_string()).collect()
        }
        MetadataReferenceKind::ScheduledJob => {
            config.scheduled_jobs().iter().map(|item| item.name().to_string()).collect()
        }
        MetadataReferenceKind::HttpService => {
            config.http_services().iter().map(|item| item.name().to_string()).collect()
        }
        MetadataReferenceKind::WebService => {
            config.web_services().iter().map(|item| item.name().to_string()).collect()
        }
        MetadataReferenceKind::Subsystem => {
            config.subsystems().iter().map(|item| item.name().to_string()).collect()
        }
    }
}

fn complete_manager_methods<DB: RootDatabase>(
    db: &DB,
    manager_prefix: &str,
) -> Vec<CompletionItem> {
    let input = TypeNameInput::new(db, manager_prefix.to_string());
    let methods = manager_methods_query(db, input);

    tracing::debug!(manager_prefix, method_count = methods.len(), "Manager methods found");

    methods.iter().map(super::platform_completion::render_manager_method).collect()
}

fn complete_manager_module_methods<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    mdo_type: MdoType,
    object_name: &str,
) -> Vec<CompletionItem> {
    let manager_type = match ManagerType::from_mdo_type(mdo_type) {
        Some(mt) => mt,
        None => return Vec::new(),
    };

    let source_root_input = db.file_source_root_input(file_id);
    let source_root_id = source_root_input.source_root_id(db);
    let module_index = db.module_index(source_root_id);

    let object = Name::new(object_name);
    let module_file_id = match module_index.resolve_manager(manager_type, &object) {
        Some(id) => id,
        None => {
            tracing::debug!(
                mdo_type = ?mdo_type,
                object_name,
                "Manager module not found in module_index"
            );
            return Vec::new();
        }
    };

    let module_id = hir::ModuleId::new(module_file_id);
    let symbol_tree = db.symbol_tree(module_id);

    let items: Vec<CompletionItem> = symbol_tree
        .methods()
        .filter(|m| m.is_export)
        .filter_map(|method| {
            let callee = symbol_info::CalleeKind::ManagerModuleMethod {
                mdo_type,
                object: object.clone(),
                method: method.name.clone(),
            };
            let sig = symbol_info::build_signature(db, file_id, &callee)?;
            Some(super::platform_completion::item_from_signature(&sig))
        })
        .collect();

    tracing::debug!(
        mdo_type = ?mdo_type,
        object_name,
        count = items.len(),
        "Manager module exported methods found"
    );

    items
}

fn complete_predefined_items<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    mdo_type: MdoType,
    object_name: &str,
) -> Vec<CompletionItem> {
    let configs = db.get_all_configurations(file_id);
    let name_lower = object_name.fold_lower();

    for (_source_name, config) in &configs {
        let mdo = config
            .metadata_objects()
            .iter()
            .find(|obj| obj.mdo_type == mdo_type && obj.name.fold_lower() == name_lower);

        if let Some(mdo) = mdo {
            let items: Vec<CompletionItem> = mdo
                .predefined_items
                .iter()
                .map(|pi| CompletionItem {
                    label: pi.name.clone(),
                    detail: Some(format!("{}.{}", mdo_type.russian_name(), object_name)),
                    kind: CompletionItemKind::Constant,
                    insert_text: pi.name.clone(),
                    documentation: None,
                    sort_text: Some(format!("1_{}", pi.name)),
                    filter_text: None,
                    source: None,
                })
                .collect();

            tracing::debug!(object_name, count = items.len(), "Predefined items found");

            return items;
        }
    }

    Vec::new()
}
