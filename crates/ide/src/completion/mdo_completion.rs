use bsl_metadata::MdoType;
use bsl_platform::{manager_methods_query, TypeNameInput};
use hir::{ManagerType, Name};
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
        MdoContext::CollectionDot { mdo_type } => {
            let items = complete_mdo_objects(db, position.file_id, mdo_type);
            if !items.is_empty() {
                return Some(items);
            }
        }
        MdoContext::ObjectDot { mdo_type, object_name } => {
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
    CollectionDot { mdo_type: MdoType },
    ObjectDot { mdo_type: MdoType, object_name: String },
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
        if let Some(mdo_type) = MdoType::from_plural(&ident_text) {
            return Some(MdoContext::CollectionDot { mdo_type });
        }
    }

    if receiver.kind() == SyntaxKind::FIELD_EXPR {
        if let Some((base_text, object_name)) = get_field_expr_parts(&receiver) {
            if let Some(mdo_type) = MdoType::from_plural(&base_text) {
                return Some(MdoContext::ObjectDot { mdo_type, object_name });
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
        if let Some(mdo_type) = MdoType::from_plural(&base_text) {
            return Some(MdoContext::CollectionDot { mdo_type });
        }
    }

    if base.kind() == SyntaxKind::FIELD_EXPR {
        if let Some((base_text, object_name)) = get_field_expr_parts(&base) {
            if let Some(mdo_type) = MdoType::from_plural(&base_text) {
                return Some(MdoContext::ObjectDot { mdo_type, object_name });
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

fn get_field_expr_parts(node: &SyntaxNode) -> Option<(String, String)> {
    let base = node.children().next()?;
    let base_text = get_single_ident(&base)?;

    let field_token = node
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .filter(|t| t.kind().is_name_token())
        .last()?;

    Some((base_text, field_token.text().to_string()))
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
