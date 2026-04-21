//! Walks the CST at a call site and classifies what is being called.
//!
//! This is the only place that has to know about source-syntax shape; adapters
//! and presenters operate on the resulting [`CalleeKind`].

use bsl_metadata::MdoType;
use bsl_platform::{
    global_function_query, platform_method_query, MethodLookupInput, TypeNameInput,
};
use hir::{ManagerType, ModuleId, Name, Resolver};
use ide_db::RootDatabase;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextSize};
use vfs::FileId;

use crate::domain::CalleeKind;

/// Position of the cursor relative to the parameter list of a call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveParam {
    /// 0-based index of the parameter the cursor sits in.
    pub index: usize,
}

/// Resolve the callee at a syntactic position.
///
/// Returns `None` when the cursor is not inside any call expression, or when
/// the callee cannot be classified.
///
/// **Resolution precedence for `Coll.Object.Method` chains** is user-first,
/// platform-fallback: a method declared in a project-local `ManagerModule.bsl`
/// shadows a same-name platform manager method (matches BSL runtime semantics).
pub fn resolve_callee_at(
    db: &dyn RootDatabase,
    file_id: FileId,
    offset: TextSize,
) -> Option<(CalleeKind, ActiveParam)> {
    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let token = root.token_at_offset(offset).left_biased()?;

    let arg_list = find_arg_list(&token)?;
    if is_on_closing_paren(&token, &arg_list) {
        return None;
    }
    let call_expr = find_call_expr(&arg_list)?;

    let (receiver, callee_name) = extract_callee_info(&call_expr)?;
    let active = ActiveParam { index: count_commas_before(&arg_list, offset) };

    if let Some(kind) = classify_mdo_chain(db, file_id, &call_expr, &callee_name) {
        return Some((kind, active));
    }

    if let Some(receiver_name) = receiver.as_deref() {
        // 2-segment: try platform method, then common module.
        if platform_method_query(
            db,
            MethodLookupInput::new(db, receiver_name.to_string(), callee_name.to_string()),
        )
        .is_some()
        {
            return Some((
                CalleeKind::PlatformMethod {
                    type_name: receiver_name.into(),
                    method_name: callee_name.into(),
                },
                active,
            ));
        }
        return Some((
            CalleeKind::CommonModuleMethod {
                module: Name::new(receiver_name),
                method: Name::new(&callee_name),
            },
            active,
        ));
    }

    // 1-segment: global function or local method.
    if global_function_query(db, TypeNameInput::new(db, callee_name.clone())).is_some() {
        return Some((CalleeKind::GlobalFunction { name: callee_name.into() }, active));
    }

    let module_id = ModuleId::new(file_id);
    let resolver = Resolver::for_module(module_id);
    let name = Name::new(&callee_name);
    if resolver.resolve_module_method(db, &name).is_some() {
        return Some((CalleeKind::LocalMethod { module_id, method: name }, active));
    }

    None
}

/// Try to classify a 3-segment MDO chain `Collection.Object.Method`.
///
/// Returns `Some(ManagerModuleMethod)` when a user-defined method exists in
/// the matching `ManagerModule.bsl`; falls back to `PlatformManagerMethod`
/// when the platform exposes the method; otherwise `None`.
fn classify_mdo_chain(
    db: &dyn RootDatabase,
    file_id: FileId,
    call_expr: &SyntaxNode,
    callee_name: &str,
) -> Option<CalleeKind> {
    let callee = call_expr.first_child()?;
    if callee.kind() != SyntaxKind::FIELD_EXPR {
        return None;
    }

    let idents: Vec<String> = callee
        .descendants_with_tokens()
        .filter_map(|it| it.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
        .collect();

    if idents.len() < 3 {
        return None;
    }

    let mdo_type = MdoType::from_plural(&idents[0])?;
    let object = Name::new(&idents[1]);
    let method = Name::new(callee_name);

    // User-defined manager method takes precedence.
    if let Some(manager_type) = ManagerType::from_mdo_type(mdo_type) {
        let source_root_input = db.file_source_root_input(file_id);
        let source_root_id = source_root_input.source_root_id(db);
        let module_index = db.module_index(source_root_id);
        if let Some(module_file_id) = module_index.resolve_manager(manager_type, &object) {
            let module_id = ModuleId::new(module_file_id);
            let symbol_tree = db.symbol_tree(module_id);
            if let Some(method_symbol) = symbol_tree.find_method(&method) {
                if method_symbol.is_export {
                    return Some(CalleeKind::ManagerModuleMethod { mdo_type, object, method });
                }
            }
        }
    }

    // Platform fallback.
    Some(CalleeKind::PlatformManagerMethod { mdo_type, method })
}

fn find_arg_list(token: &SyntaxToken) -> Option<SyntaxNode> {
    token.parent_ancestors().find(|node| node.kind() == SyntaxKind::ARG_LIST)
}

fn is_on_closing_paren(token: &SyntaxToken, arg_list: &SyntaxNode) -> bool {
    if token.kind() == SyntaxKind::R_PAREN {
        if let Some(parent) = token.parent() {
            return parent == *arg_list || parent.parent().as_ref() == Some(arg_list);
        }
    }
    false
}

fn find_call_expr(arg_list: &SyntaxNode) -> Option<SyntaxNode> {
    arg_list.parent().filter(|p| p.kind() == SyntaxKind::CALL_EXPR)
}

fn extract_callee_info(call_expr: &SyntaxNode) -> Option<(Option<String>, String)> {
    let first_child = call_expr.first_child()?;

    match first_child.kind() {
        SyntaxKind::FIELD_EXPR => {
            let mut idents: Vec<String> = Vec::new();
            for token in first_child.descendants_with_tokens().filter_map(|it| it.into_token()) {
                if token.kind() == SyntaxKind::IDENT {
                    idents.push(token.text().to_string());
                }
            }
            match idents.len() {
                0 => None,
                1 => Some((None, idents.pop().unwrap())),
                _ => {
                    let method = idents.pop().unwrap();
                    let receiver = idents.pop().unwrap();
                    Some((Some(receiver), method))
                }
            }
        }
        SyntaxKind::IDENT => {
            for child in first_child.children_with_tokens() {
                if child.kind() == SyntaxKind::IDENT {
                    if let Some(token) = child.as_token() {
                        return Some((None, token.text().to_string()));
                    }
                }
            }
            None
        }
        _ => {
            for child in first_child.children_with_tokens() {
                if child.kind() == SyntaxKind::IDENT {
                    let name = child.as_token()?.text().to_string();
                    return Some((None, name));
                }
            }
            None
        }
    }
}

fn count_commas_before(arg_list: &SyntaxNode, offset: TextSize) -> usize {
    let mut count = 0;
    for child in arg_list.children_with_tokens() {
        if child.text_range().start() >= offset {
            break;
        }
        if child.kind() == SyntaxKind::COMMA {
            count += 1;
        }
    }
    count
}
