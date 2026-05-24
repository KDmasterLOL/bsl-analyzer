//! Walks the CST at a call site and classifies what is being called.
//!
//! This is the only place that has to know about source-syntax shape; adapters
//! and presenters operate on the resulting [`CalleeKind`].

use bsl_metadata::MdoType;
use bsl_platform::{
    global_function_query, platform_method_query, MethodLookupInput, TypeNameInput,
};
use hir::{ManagerType, ModuleId, Name, Resolver, Semantics, Ty};
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
pub fn resolve_callee_at<DB: RootDatabase>(
    db: &DB,
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

    // `Новый X(...)` — the arg list's parent is `NEW_EXPR`, not `CALL_EXPR`.
    // Detect it before the regular call-path resolver so we can route the
    // site to `CalleeKind::PlatformConstructor`. Phase 1 handles only the
    // single-identifier form (`Новый X`); dot-path (`Новый A.B`) is not
    // admitted by the parser and would never reach here.
    if let Some(new_expr) = arg_list.parent().filter(|p| p.kind() == SyntaxKind::NEW_EXPR) {
        if let Some(type_name) = extract_new_expr_type_name(&new_expr) {
            let active = ActiveParam { index: count_commas_before(&arg_list, offset) };
            return Some((CalleeKind::PlatformConstructor { type_name: type_name.into() }, active));
        }
        // `Новый` without an IDENT is syntactically malformed — no callee.
        return None;
    }

    let call_expr = find_call_expr(&arg_list)?;

    let (receiver, callee_name) = extract_callee_info(&call_expr)?;
    let active = ActiveParam { index: count_commas_before(&arg_list, offset) };

    if let Some(kind) = classify_mdo_chain(db, file_id, &call_expr, &callee_name) {
        return Some((kind, active));
    }

    if let Some((receiver_name, receiver_node)) = receiver {
        // Inferred-type platform method (primary): mirrors the completion
        // pipeline in `ide::completion::platform_completion` — same Ty
        // lookup, same `platform_type_name()` bridge. Runs **before** the
        // bare text-name check below because BSL happily lets a variable
        // shadow a platform type (`Массив = Новый СписокЗначений;`), and
        // `platform_method_query("Массив", "Добавить")` would otherwise
        // return the wrong overload for the shadowed receiver.
        let sema = Semantics::new(db);
        // Phase 3 §4.G.5b: `Semantics::type_of_expr` is kernel-native; bridge
        // to `Ty` for the still-`Ty` `platform_type_name`/`Union` matching
        // below (those move to the kernel in Phase 4).
        let ty = hir::ty_bridge::typeid_to_ty(db, sema.type_of_expr(file_id, &receiver_node));
        if let Some(type_name) = ty.platform_type_name() {
            if platform_method_query(
                db,
                MethodLookupInput::new(db, type_name.to_string(), callee_name.clone()),
            )
            .is_some()
            {
                return Some((
                    CalleeKind::PlatformMethod {
                        type_name: type_name.into(),
                        method_name: callee_name.into(),
                    },
                    active,
                ));
            }
        }

        // Union receivers (typical shape: `Запрос.Выполнить()` →
        // `Union(РезультатЗапроса, Неопределено)`) have no single
        // `platform_type_name()`. Mirror the completion pipeline in
        // `ide::completion::platform_completion`: skip `Undefined`/`Null`
        // sentinels and pick the first arm that owns the method. The
        // first match wins because signature help is a single-overload
        // surface — there is no UX way to render two competing arms.
        if let Ty::Union(members) = &ty {
            for member in members.iter().filter(|m| !matches!(m, Ty::Undefined | Ty::Null)) {
                let Some(type_name) = member.platform_type_name() else { continue };
                if platform_method_query(
                    db,
                    MethodLookupInput::new(db, type_name.to_string(), callee_name.clone()),
                )
                .is_some()
                {
                    return Some((
                        CalleeKind::PlatformMethod {
                            type_name: type_name.into(),
                            method_name: callee_name.into(),
                        },
                        active,
                    ));
                }
            }
        }

        // Text-name platform method (fallback): the receiver token **is**
        // a literal platform type name (`Строка.ВРег()`, `Формат.ДатаВремя()`)
        // with no `infer_path_name` binding — `type_of_expr` returns
        // Unknown for them, so only the text-name resolver can find the
        // method.
        if platform_method_query(
            db,
            MethodLookupInput::new(db, receiver_name.clone(), callee_name.clone()),
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

        // Final fallback: treat the receiver as a CommonModule name. The
        // downstream adapter returns `None` if no such module exists,
        // which surfaces to the LSP as "no signature help" for clearly
        // unmatched receivers.
        return Some((
            CalleeKind::CommonModuleMethod {
                module: Name::new(&receiver_name),
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
fn classify_mdo_chain<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    call_expr: &SyntaxNode,
    callee_name: &str,
) -> Option<CalleeKind> {
    let callee = call_expr.first_child()?;
    if callee.kind() != SyntaxKind::FIELD_EXPR {
        return None;
    }

    // Accept any name-token for path segments. The parser admits
    // keywords after `.` (e.g. `Документы.ПКО.Выполнить` with
    // `KW_EXECUTE`), so an IDENT-only filter would silently drop
    // keyword-shaped manager methods. Layer B unification — same
    // predicate as `crates/syntax/src/syntax_kind.rs::is_name_token`.
    let idents: Vec<String> = callee
        .descendants_with_tokens()
        .filter_map(|it| it.into_token())
        .filter(|t| t.kind().is_name_token())
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

/// Extracts the single name-token child of a `NEW_EXPR` (the
/// constructor type name). Delegates to the syntax-tier
/// `new_expr_type_name_token` helper so the predicate stays
/// consistent with hover / classifier / hir-def lowering — accepting
/// any `is_name_token()` (IDENT or keyword) future-proofs against
/// keyword-typed platform types even though none ship today.
fn extract_new_expr_type_name(new_expr: &SyntaxNode) -> Option<String> {
    Some(syntax::ast_utils::new_expr_type_name_token(new_expr)?.text().to_string())
}

/// Split `call_expr` into `(Option<(receiver_name, receiver_node)>, method_name)`.
///
/// The receiver node is the syntactic expression immediately left of the DOT
/// (e.g. the `КомпоновщикНастроек` IdentExpr in `КомпоновщикНастроек.M()`, or
/// the inner `FIELD_EXPR` in `a.b.c(...)`). It lets [`resolve_callee_at`] ask
/// `Semantics::type_of_expr` for the receiver's inferred `Ty` when the bare
/// text name does not match any platform type.
fn extract_callee_info(call_expr: &SyntaxNode) -> Option<(Option<(String, SyntaxNode)>, String)> {
    let first_child = call_expr.first_child()?;

    match first_child.kind() {
        SyntaxKind::FIELD_EXPR => {
            // Accept keyword-shaped name tokens — the parser admits any
            // `is_keyword()` token after `.`, so `Запрос.Выполнить(...)`
            // (where `Выполнить` is `KW_EXECUTE`) had its method name
            // dropped by the legacy IDENT filter, leaving callers
            // misclassified or routed without a receiver. Layer B
            // unification.
            let mut names: Vec<String> = Vec::new();
            for token in first_child.descendants_with_tokens().filter_map(|it| it.into_token()) {
                if token.kind().is_name_token() {
                    names.push(token.text().to_string());
                }
            }
            match names.len() {
                0 => None,
                1 => Some((None, names.pop().unwrap())),
                _ => {
                    let method = names.pop().unwrap();
                    let receiver_name = names.pop().unwrap();
                    let receiver_node = first_child.first_child()?;
                    Some((Some((receiver_name, receiver_node)), method))
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
