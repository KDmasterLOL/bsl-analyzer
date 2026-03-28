//! MDO (Metadata Object) completion.
//!
//! Provides completion for metadata collection access:
//! - `Справочники.` / `Catalogs.` → MDO objects from project metadata
//! - `Справочники.Валюты.` / `Catalogs.Currencies.` → manager methods from platform data

use bsl_metadata::MdoType;
use bsl_platform::PlatformData;
use ide_db::RootDatabase;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use super::{CompletionItem, CompletionItemKind, CompletionPosition};

/// Attempts to provide MDO completions.
///
/// Returns Some(items) if this is an MDO completion context (after DOT on a manager collection),
/// otherwise returns None to allow other completion providers to handle it.
///
/// Handles two cursor positions:
/// - Right after DOT: `Справочники.|` → token is DOT
/// - Inside IDENT after DOT: `Справочники.Допол|нительные` → token is IDENT
pub(super) fn mdo_completions<DB: RootDatabase>(
    db: &DB,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let _span = tracing::info_span!("mdo_completions").entered();

    let parse = db.parse(position.file_id);
    let root = parse.syntax_node();

    let token = root.token_at_offset(position.offset).left_biased()?;

    // Detect MDO context from either DOT or IDENT-after-DOT
    let context = detect_mdo_context(&token)?;
    tracing::debug!(?context, "MDO completion context detected");

    match context {
        // `Справочники.` or `Справочники.Доп|` → complete MDO objects
        MdoContext::CollectionDot { mdo_type } => {
            let items = complete_mdo_objects(db, position.file_id, mdo_type);
            if !items.is_empty() {
                return Some(items);
            }
        }
        // `Справочники.Валюты.` or `Справочники.Валюты.Найти|` → complete manager methods + predefined items
        MdoContext::ObjectDot { mdo_type, object_name } => {
            let mut items = Vec::new();

            // Manager methods (НайтиПоКоду, СоздатьЭлемент, ...)
            if let Some(prefix) = mdo_type.manager_type_prefix() {
                items.extend(complete_manager_methods(prefix));
            }

            // Predefined items (EmailПартнера, Россия, ...)
            items.extend(complete_predefined_items(db, position.file_id, mdo_type, &object_name));

            if !items.is_empty() {
                return Some(items);
            }
        }
    }

    None
}

/// MDO completion context.
#[derive(Debug)]
enum MdoContext {
    /// Cursor after `Справочники.` — complete with MDO object names
    CollectionDot { mdo_type: MdoType },
    /// Cursor after `Справочники.Валюты.` — complete with manager methods + predefined items
    ObjectDot { mdo_type: MdoType, object_name: String },
}

/// Detect MDO completion context from the token at cursor position.
///
/// Walks the syntax tree to find if we're in a `ManagerCollection.` or
/// `ManagerCollection.Object.` context.
fn detect_mdo_context(token: &SyntaxToken) -> Option<MdoContext> {
    match token.kind() {
        // Cursor right after DOT: `Справочники.|` or `Справочники.Валюты.|`
        SyntaxKind::DOT => detect_from_dot(token),

        // Cursor inside IDENT after DOT: `Справочники.Доп|` or `Справочники.Валюты.Найти|`
        SyntaxKind::IDENT => detect_from_ident_after_dot(token),

        _ => None,
    }
}

/// Detect context when cursor is right after a DOT token.
fn detect_from_dot(dot_token: &SyntaxToken) -> Option<MdoContext> {
    let receiver = find_receiver_before_dot(dot_token)?;

    // Case: `Справочники.` — receiver is simple IDENT
    if let Some(ident_text) = get_single_ident(&receiver) {
        if let Some(mdo_type) = MdoType::from_plural(&ident_text) {
            return Some(MdoContext::CollectionDot { mdo_type });
        }
    }

    // Case: `Справочники.Валюты.` — receiver is FIELD_EXPR
    if receiver.kind() == SyntaxKind::FIELD_EXPR {
        if let Some((base_text, object_name)) = get_field_expr_parts(&receiver) {
            if let Some(mdo_type) = MdoType::from_plural(&base_text) {
                return Some(MdoContext::ObjectDot { mdo_type, object_name });
            }
        }
    }

    None
}

/// Detect context when cursor is inside an IDENT that follows a DOT.
///
/// The IDENT is inside a FIELD_EXPR: `base . ident|`
/// We walk up to find the MDO context from the parent FIELD_EXPR structure.
fn detect_from_ident_after_dot(ident_token: &SyntaxToken) -> Option<MdoContext> {
    // Verify there's a DOT before this IDENT
    let has_dot_before = ident_token
        .siblings_with_tokens(syntax::Direction::Prev)
        .skip(1)
        .find(|s| s.kind() != SyntaxKind::WHITESPACE)
        .is_some_and(|s| s.kind() == SyntaxKind::DOT);

    if !has_dot_before {
        return None;
    }

    // Parent should be FIELD_EXPR: `base.ident`
    let field_expr = ident_token.parent()?;
    if field_expr.kind() != SyntaxKind::FIELD_EXPR {
        return None;
    }

    // Get the base (first child node of the FIELD_EXPR)
    let base = field_expr.children().next()?;

    // Case: `Справочники.Доп|` — base is simple IDENT
    if let Some(base_text) = get_single_ident(&base) {
        if let Some(mdo_type) = MdoType::from_plural(&base_text) {
            return Some(MdoContext::CollectionDot { mdo_type });
        }
    }

    // Case: `Справочники.Валюты.Найти|` — base is FIELD_EXPR
    if base.kind() == SyntaxKind::FIELD_EXPR {
        if let Some((base_text, object_name)) = get_field_expr_parts(&base) {
            if let Some(mdo_type) = MdoType::from_plural(&base_text) {
                return Some(MdoContext::ObjectDot { mdo_type, object_name });
            }
        }
    }

    None
}

/// Find the receiver node before the DOT token.
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

/// Extract identifier text from a simple IDENT node.
fn get_single_ident(node: &SyntaxNode) -> Option<String> {
    let token = node.first_token()?;
    if token.kind() == SyntaxKind::IDENT {
        Some(token.text().to_string())
    } else {
        None
    }
}

/// Extract base and field from a FIELD_EXPR node (base.field).
fn get_field_expr_parts(node: &SyntaxNode) -> Option<(String, String)> {
    let base = node.children().next()?;
    let base_text = get_single_ident(&base)?;

    let field_token = node
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .last()?;

    Some((base_text, field_token.text().to_string()))
}

/// Complete MDO objects from project metadata.
///
/// Example: `Справочники.` → [Валюты, Контрагенты, Номенклатура, ...]
fn complete_mdo_objects<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    mdo_type: MdoType,
) -> Vec<CompletionItem> {
    let configs = db.get_all_configurations(file_id);
    let mut items = Vec::new();

    for (source_name, config) in &configs {
        let type_label = mdo_type.russian_name();

        // Regular metadata objects
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

        // Registers are stored separately
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

/// Complete manager methods from platform data.
///
/// Example: `Справочники.Валюты.` → [НайтиПоКоду, НайтиПоНаименованию, СоздатьЭлемент, ...]
fn complete_manager_methods(manager_prefix: &str) -> Vec<CompletionItem> {
    let data = PlatformData::instance();
    let methods = data.get_manager_methods(manager_prefix);

    tracing::debug!(manager_prefix, method_count = methods.len(), "Manager methods found");

    methods
        .iter()
        .map(|method| super::platform_completion::render_platform_method(method))
        .collect()
}

/// Complete predefined items from project metadata.
///
/// Example: `Справочники.ВидыКонтактнойИнформации.` → [EmailПартнера, АдресПартнера, ...]
fn complete_predefined_items<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    mdo_type: MdoType,
    object_name: &str,
) -> Vec<CompletionItem> {
    let configs = db.get_all_configurations(file_id);
    let name_lower = object_name.to_lowercase();

    for (_source_name, config) in &configs {
        let mdo = config
            .metadata_objects()
            .iter()
            .find(|obj| obj.mdo_type == mdo_type && obj.name.to_lowercase() == name_lower);

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
                    sort_text: Some(format!("1_{}", pi.name)), // Sort after methods (default "0_")
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
