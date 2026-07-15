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
    let names = match kind {
        MetadataReferenceKind::Role => db.role_names(file_id),
        MetadataReferenceKind::EventSubscription => db.event_subscription_names(file_id),
        MetadataReferenceKind::ScheduledJob => db.scheduled_job_names(file_id),
        MetadataReferenceKind::HttpService => db.http_service_names(file_id),
        MetadataReferenceKind::WebService => db.web_service_names(file_id),
        MetadataReferenceKind::Subsystem => db.subsystem_names(file_id),
    };
    names
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
            let sigs = symbol_info::build_signature(db, file_id, &callee)?;
            let sig = sigs.first()?;
            Some(super::platform_completion::item_from_signature(sig))
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

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::{
        base_db::{SourceDatabase, SourceRoot, SourceRootId},
        metadata::{MetadataListingData, SubsystemEntry},
        RootDatabaseImpl,
    };
    use vfs::{file_set::FileSet, FileId, VfsPath};

    fn subsystem_xml(name: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns:xr="http://v8.1c.ru/8.3/xcf/readable" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
    <Subsystem uuid="00000000-0000-0000-0000-000000000094">
        <Properties>
            <Name>{name}</Name>
            <Content/>
        </Properties>
        <ChildObjects/>
    </Subsystem>
</MetaDataObject>"#
        )
    }

    #[test]
    fn complete_metadata_reference_subsystems_use_listed_substrate() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cf");
        let subsystem_path = root.join("Subsystems/МояПодсистема.xml");
        std::fs::create_dir_all(subsystem_path.parent().unwrap()).unwrap();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();

        let subsystem_file = FileId(20);
        let consumer_file = FileId(21);
        let consumer_path = root.join("CompletionConsumer.bsl");

        let mut db = RootDatabaseImpl::new();
        let mut file_set = FileSet::new();
        file_set.insert(subsystem_file, VfsPath::new(subsystem_path.to_string_lossy().as_ref()));
        file_set.insert(consumer_file, VfsPath::new(consumer_path.to_string_lossy().as_ref()));
        db.set_source_root(SourceRootId(1), SourceRoot::new_local(file_set));
        db.set_file_source_root(subsystem_file, SourceRootId(1));
        db.set_file_source_root(consumer_file, SourceRootId(1));
        db.set_file_text(subsystem_file, &subsystem_xml("МояПодсистема"));
        db.set_file_text(consumer_file, "Процедура Т() КонецПроцедуры");

        db.set_all_config_paths(vec![(None, root.clone())]);
        db.set_metadata_listing(
            &root.to_string_lossy(),
            MetadataListingData {
                entries: Vec::new(),
                defined_types: Vec::new(),
                common_modules: Vec::new(),
                event_subscriptions: Vec::new(),
                scheduled_jobs: Vec::new(),
                roles: Vec::new(),
                http_services: Vec::new(),
                web_services: Vec::new(),
                integration_services: Vec::new(),
                subsystems: vec![SubsystemEntry {
                    name: "МояПодсистема".to_string(),
                    main: subsystem_file,
                }],
            },
        );

        let items = complete_metadata_reference_objects(
            &db,
            consumer_file,
            MetadataReferenceKind::Subsystem,
        );
        let labels: Vec<String> = items.into_iter().map(|item| item.label).collect();

        assert_eq!(labels, vec!["МояПодсистема".to_string()]);
    }
}
