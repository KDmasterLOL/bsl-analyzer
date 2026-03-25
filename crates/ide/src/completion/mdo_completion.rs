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
pub(super) fn mdo_completions<DB: RootDatabase>(
    db: &DB,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let _span = tracing::info_span!("mdo_completions").entered();

    let parse = db.parse(position.file_id);
    let root = parse.syntax_node();

    let token = root.token_at_offset(position.offset).left_biased()?;

    if token.kind() != SyntaxKind::DOT {
        return None;
    }

    let receiver = find_receiver_before_dot(&token)?;

    // Case 1: `Справочники.` → complete with MDO objects from metadata
    if let Some(ident_text) = get_single_ident(&receiver) {
        if let Some(mdo_type) = MdoType::from_plural(&ident_text) {
            tracing::debug!(?mdo_type, "MDO collection completion");
            let items = complete_mdo_objects(db, position.file_id, mdo_type);
            if !items.is_empty() {
                return Some(items);
            }
        }
    }

    // Case 2: `Справочники.Валюты.` → complete with manager methods
    if receiver.kind() == SyntaxKind::FIELD_EXPR {
        if let Some((base_text, object_name)) = get_field_expr_parts(&receiver) {
            if let Some(mdo_type) = MdoType::from_plural(&base_text) {
                tracing::debug!(?mdo_type, %object_name, "MDO manager method completion");
                if let Some(prefix) = mdo_type.manager_type_prefix() {
                    let items = complete_manager_methods(prefix);
                    if !items.is_empty() {
                        return Some(items);
                    }
                }
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
