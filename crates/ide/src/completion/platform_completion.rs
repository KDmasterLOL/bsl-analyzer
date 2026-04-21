//! Platform method completion.
//!
//! Provides completion for platform types and methods:
//! - Method completion after DOT (e.g., `Строка.` shows ВРег, НРег, etc.)
//! - CommonModule method completion (e.g., `ОбщегоНазначения.` shows exported methods)
//! - Snippets with parameter placeholders

use bsl_platform::{
    type_methods_query, PlatformData, PlatformDataInner, PlatformMethod, TypeNameInput,
};
use hir::{InferenceResult, MethodSymbol, Name, Ty};
use ide_db::RootDatabase;
use symbol_info::{
    build_signature, from_platform_method, render_completion_detail, CalleeKind, CompletionDetail,
    MethodKind, SignatureSource, SymbolSignature,
};
use syntax::ast::{self, AstNode};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use vfs::FileId;

use super::{CompletionItem, CompletionItemKind, CompletionPosition};

/// Attempts to provide platform method completions.
///
/// Returns Some(items) if this is a method completion context (after DOT),
/// otherwise returns None to allow other completion providers to handle it.
///
/// Supports:
/// - Simple variable: `МойМассив.` → methods of Массив
/// - Direct type: `Строка.` → methods of Строка
/// - Fluent chains: `Запрос.Выполнить().Выбрать().` → methods of return type
pub(super) fn platform_completions(
    db: &dyn RootDatabase,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let _span = tracing::info_span!("platform_completions").entered();

    let parse = db.parse(position.file_id);
    let root = parse.syntax_node();

    let token = root.token_at_offset(position.offset).left_biased()?;

    tracing::debug!(token_kind = ?token.kind(), token_text = ?token.text(), "Completion token");

    if token.kind() != SyntaxKind::DOT {
        return None;
    }

    let receiver_expr = find_receiver_expr(&token)?;

    // Fast path: bare-IDENT receiver (`ОбщегоНазначения.`) is almost always a
    // CommonModule call. Resolve via `module_index` (path-only, cheap) before
    // paying for `db.infer()` — which transitively warms `workspace_symbols`
    // across the whole source root (~50 s for a 12k-file workspace on cold
    // start, because every qualified call in the file triggers it).
    if let Some(receiver_name) = extract_receiver_ident(&receiver_expr) {
        tracing::debug!(receiver_name = %receiver_name, "Trying CommonModule fast path");
        if let Some(items) = complete_common_module_methods(db, &position, &receiver_name) {
            return Some(items);
        }
    }

    // Slow path: full inference for non-trivial receivers
    // (`expr.Method().`, `ЭтотОбъект.`, fluent chains).
    let infer_result = db.infer(position.file_id);

    let receiver_ty = resolve_syntax_expr_type(&receiver_expr, &infer_result);
    tracing::debug!(receiver_ty = ?receiver_ty, "Resolved receiver type");

    // Try platform type completion first
    if let Some(type_name) = receiver_ty.platform_type_name() {
        tracing::debug!(type_name = ?type_name, "Platform type for completion");
        return Some(complete_platform_methods(db, type_name));
    }

    None
}

/// Find the receiver expression node before the DOT token.
///
/// Walks up the syntax tree from DOT to find the parent expression,
/// then returns its first child (the receiver).
///
/// Handles cases like:
/// - `ident.` → parent is FIELD_EXPR, receiver is IDENT
/// - `expr.Method().` → parent is FIELD_EXPR, receiver is CALL_EXPR
fn find_receiver_expr(dot_token: &SyntaxToken) -> Option<SyntaxNode> {
    let Some(parent) = dot_token.parent() else {
        tracing::debug!("find_receiver_expr: dot has no parent");
        return None;
    };
    tracing::debug!(parent_kind = ?parent.kind(), "find_receiver_expr: DOT parent kind");

    // DOT is inside a FIELD_EXPR: the first child node is the receiver
    if parent.kind() == SyntaxKind::FIELD_EXPR {
        let child = parent.children().next();
        tracing::debug!(child_found = child.is_some(), child_kind = ?child.as_ref().map(|c| c.kind()), "find_receiver_expr: FIELD_EXPR first child");
        return child;
    }

    // Fallback: look at the previous sibling node of the DOT
    for sibling in dot_token.siblings_with_tokens(syntax::Direction::Prev).skip(1) {
        if sibling.kind() == SyntaxKind::WHITESPACE {
            continue;
        }
        if let Some(node) = sibling.as_node() {
            tracing::debug!(sibling_kind = ?node.kind(), "find_receiver_expr: fallback found node sibling");
            return Some(node.clone());
        }
        if let Some(token) = sibling.as_token() {
            tracing::debug!(token_kind = ?token.kind(), "find_receiver_expr: fallback returning token.parent()");
            return token.parent();
        }
        return None;
    }

    tracing::debug!("find_receiver_expr: fallback exhausted, returning None");
    None
}

/// Recursively resolve the type of a syntax expression node.
///
/// This is a lightweight syntax-level type resolver for completion.
/// It uses inference results (var_types) for variables and platform data
/// for method return types to support fluent chains.
fn resolve_syntax_expr_type(node: &SyntaxNode, infer_result: &InferenceResult) -> Ty {
    match node.kind() {
        // Simple identifier — look up in var_types or treat as type name
        SyntaxKind::IDENT | SyntaxKind::EXPR => {
            if let Some(ident_token) = node.first_token() {
                if ident_token.kind() == SyntaxKind::IDENT {
                    return resolve_ident_type(ident_token.text(), infer_result);
                }
            }
            Ty::Unknown
        }

        // Call expression: callee(args) — resolve callee type
        // For method calls like `obj.Method()`, this is a CALL_EXPR wrapping a FIELD_EXPR
        SyntaxKind::CALL_EXPR => resolve_call_expr_type(node, infer_result),

        // Field expression: base.field — resolve for intermediate chains
        SyntaxKind::FIELD_EXPR => resolve_field_expr_type(node, infer_result),

        // New expression: Новый Type(...)
        SyntaxKind::NEW_EXPR => {
            if let Some(new_expr) = ast::NewExpr::cast(node.clone()) {
                Ty::from_new_expr(&new_expr)
            } else {
                Ty::Unknown
            }
        }

        // Parenthesized expression
        SyntaxKind::PAREN_EXPR => node
            .children()
            .next()
            .map(|child| resolve_syntax_expr_type(&child, infer_result))
            .unwrap_or(Ty::Unknown),

        _ => {
            // Try to find an IDENT token in this node (fallback)
            if let Some(ident) = node.first_token().filter(|t| t.kind() == SyntaxKind::IDENT) {
                return resolve_ident_type(ident.text(), infer_result);
            }
            Ty::Unknown
        }
    }
}

/// Resolve type of an identifier (variable name or type name).
fn resolve_ident_type(name: &str, infer_result: &InferenceResult) -> Ty {
    let key = name.to_lowercase();

    // Check var_types from inference
    if let Some(ty) = infer_result.var_types.get(&key) {
        if !ty.is_unknown() {
            return ty.clone();
        }
    }

    // Check if it's a known platform type name directly (e.g., `Строка.`)
    let ty = Ty::from_type_name(name);
    if !ty.is_unknown() {
        return ty;
    }

    // Check if it's a known platform type (e.g., `Запрос.`)
    let data = PlatformData::instance();
    if !data.get_type_methods(name).is_empty() {
        return Ty::PlatformObject(Name::new(name));
    }

    Ty::Unknown
}

/// Resolve type of a CALL_EXPR node.
///
/// Structure: CALL_EXPR → [callee_expr, ARG_LIST]
/// If callee is a FIELD_EXPR (method call), resolve receiver type + method return type.
fn resolve_call_expr_type(node: &SyntaxNode, infer_result: &InferenceResult) -> Ty {
    let callee = node.children().next();
    let callee = match callee {
        Some(c) => c,
        None => return Ty::Unknown,
    };

    // Method call: callee is FIELD_EXPR (base.method)
    if callee.kind() == SyntaxKind::FIELD_EXPR {
        let mut children = callee.children();
        let base = children.next();
        // Skip to find method name (IDENT after DOT)
        let method_name = callee
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::IDENT)
            .last();

        if let (Some(base_node), Some(method_token)) = (base, method_name) {
            let base_ty = resolve_syntax_expr_type(&base_node, infer_result);
            if let Some(type_name) = base_ty.platform_type_name() {
                let data = PlatformData::instance();
                if let Some(method) = data.get_method(type_name, method_token.text()) {
                    if let Some(ret_type) = &method.return_type {
                        let ty = Ty::from_type_name(ret_type);
                        if !ty.is_unknown() {
                            return ty;
                        }
                        return Ty::PlatformObject(Name::new(ret_type));
                    }
                    return Ty::Undefined;
                }
            }
        }
    }

    // Simple function call — could check global function return types
    // For now, return Unknown
    Ty::Unknown
}

/// Resolve type of a FIELD_EXPR node (base.field).
///
/// Used for intermediate chain resolution.
fn resolve_field_expr_type(node: &SyntaxNode, infer_result: &InferenceResult) -> Ty {
    // FIELD_EXPR has: base_expr, DOT, IDENT(field_name)
    let base = node.children().next();
    let field_name = node
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .last();

    if let (Some(base_node), Some(_field_token)) = (base, field_name) {
        // For now, resolve base type (field access on platform types returns Unknown)
        resolve_syntax_expr_type(&base_node, infer_result)
    } else {
        Ty::Unknown
    }
}

/// Extract identifier text from receiver expression node.
fn extract_receiver_ident(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::IDENT | SyntaxKind::EXPR => {
            let token = node.first_token()?;
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
            None
        }
        _ => {
            let token = node.first_token().filter(|t| t.kind() == SyntaxKind::IDENT)?;
            Some(token.text().to_string())
        }
    }
}

/// Completes exported methods from a CommonModule.
///
/// Uses module_index for O(1) name lookup, then symbol_tree for the specific module.
/// symbol_tree is typically already cached as a dependency of the open file.
fn complete_common_module_methods(
    db: &dyn RootDatabase,
    position: &CompletionPosition,
    module_name: &str,
) -> Option<Vec<CompletionItem>> {
    let source_root_input = db.file_source_root_input(position.file_id);
    let source_root_id = source_root_input.source_root_id(db);
    let module_index = db.module_index(source_root_id);

    let name = Name::new(module_name);
    let module_file_id = module_index.resolve_common_module(&name)?;

    tracing::debug!(
        module_name = %module_name,
        file_id = ?module_file_id,
        "Found CommonModule in module_index"
    );

    let module_id = hir::ModuleId::new(module_file_id);
    let symbol_tree = db.symbol_tree(module_id);

    let items: Vec<CompletionItem> = symbol_tree
        .methods()
        .filter(|m| m.is_export)
        .map(|method| render_common_module_method(db, position.file_id, &name, method))
        .collect();

    tracing::debug!(item_count = items.len(), "CommonModule completion items");

    if items.is_empty() {
        return None;
    }

    Some(items)
}

/// Renders a CommonModule method as a completion item via the unified
/// `symbol_info` pipeline.
fn render_common_module_method(
    db: &dyn RootDatabase,
    file_id: FileId,
    module_name: &Name,
    method: &MethodSymbol,
) -> CompletionItem {
    let callee =
        CalleeKind::CommonModuleMethod { module: module_name.clone(), method: method.name.clone() };
    match build_signature(db, file_id, &callee) {
        Some(sig) => item_from_signature(&sig),
        None => fallback_item(method),
    }
}

/// Wrap a [`CompletionDetail`] from `symbol_info` into the IDE's
/// [`CompletionItem`].
///
/// `kind` is derived from the signature *source* rather than from
/// `MethodKind`: platform members (including procedures) are surfaced as
/// `Method`, global procedures/functions as `Function`, and user-defined
/// items split by procedure-vs-function — matching the legacy classification
/// the editor presents.
pub(super) fn item_from_signature(sig: &SymbolSignature) -> CompletionItem {
    let detail = render_completion_detail(sig);
    let kind = match sig.source {
        SignatureSource::Platform | SignatureSource::PlatformManager => CompletionItemKind::Method,
        SignatureSource::GlobalFunction => CompletionItemKind::Function,
        SignatureSource::CommonModule | SignatureSource::ManagerModule | SignatureSource::Local => {
            match sig.kind {
                MethodKind::Function => CompletionItemKind::Function,
                MethodKind::Procedure => CompletionItemKind::Method,
            }
        }
    };
    item_from_detail(detail, kind)
}

fn item_from_detail(detail: CompletionDetail, kind: CompletionItemKind) -> CompletionItem {
    let CompletionDetail { label, detail, documentation, insert_text, filter_text } = detail;
    let documentation = if documentation.is_empty() { None } else { Some(documentation) };
    CompletionItem {
        label,
        detail: Some(detail),
        kind,
        insert_text,
        documentation,
        sort_text: None,
        filter_text,
        source: None,
    }
}

/// Minimal completion item used when `symbol_info` cannot build a signature
/// (e.g. metadata corruption). Keeps completion responsive.
fn fallback_item(method: &MethodSymbol) -> CompletionItem {
    let label = method.name.to_string();
    let kind =
        if method.is_function { CompletionItemKind::Function } else { CompletionItemKind::Method };
    let insert_text = format!("{}($0)", label);
    CompletionItem {
        label: label.clone(),
        detail: Some(label),
        kind,
        insert_text,
        documentation: None,
        sort_text: None,
        filter_text: None,
        source: None,
    }
}

/// Completes platform methods for a receiver type.
///
/// Example: For receiver "Строка", shows: ВРег, НРег, Длина, etc.
fn complete_platform_methods(db: &dyn RootDatabase, receiver_type: &str) -> Vec<CompletionItem> {
    let input = TypeNameInput::new(db, receiver_type.to_string());
    let methods = type_methods_query(db, input);

    tracing::debug!(method_count = methods.len(), "Found platform methods");

    methods.iter().map(render_platform_method).collect()
}

/// Renders a platform manager method (`Справочники.Склады.НайтиПоКоду`, …) as
/// a completion item via the unified `symbol_info` pipeline.
///
/// Manager methods carry `name="<Имя"` in platform data; the real Russian
/// name lives in `MethodDocs.syntax` and is recovered by `from_platform_method`.
pub(super) fn render_manager_method(method: &PlatformMethod) -> CompletionItem {
    let docs = PlatformDataInner::instance().get_method_docs(method.id);
    let mut sig = from_platform_method(method, docs.as_ref());
    sig.source = SignatureSource::PlatformManager;
    item_from_signature(&sig)
}

/// Renders a platform method as a completion item via the unified
/// `symbol_info` pipeline.
pub(super) fn render_platform_method(method: &PlatformMethod) -> CompletionItem {
    let docs = PlatformDataInner::instance().get_method_docs(method.id);
    let sig = from_platform_method(method, docs.as_ref());
    item_from_signature(&sig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_platform::{ContextAvailability, MethodParam, PlatformMethod};

    fn create_test_method() -> PlatformMethod {
        PlatformMethod {
            id: 999999, // Use invalid ID to ensure fallback to basic docs
            type_name: "Строка".into(),
            name: "ВРег".into(),
            english_name: "Upper".into(),
            return_type: Some("Строка".into()),
            parameters: vec![MethodParam {
                name: "Значение".into(),
                param_type: Some("Строка".into()),
                is_optional: false,
            }],
            min_version: Some("8.0".into()),
            context: Some(ContextAvailability {
                thick_client: true,
                thin_client: true,
                web_client: true,
                server: true,
                mobile_client: false,
                external_connection: true,
            }),
        }
    }

    #[test]
    fn test_render_platform_method() {
        // Public surface check: render produces a sensibly-shaped completion
        // item. Format details are tested in `symbol_info::presenters`.
        let method = create_test_method();
        let item = render_platform_method(&method);

        assert_eq!(item.label, "ВРег");
        assert_eq!(item.kind, CompletionItemKind::Method);
        assert!(item.insert_text.starts_with("ВРег("));
        assert!(item.insert_text.ends_with("$0)"));
        let detail = item.detail.as_deref().unwrap_or("");
        assert!(
            detail.contains("ВРег") && detail.contains("Строка"),
            "Detail should contain method name and parameter type, got: {detail}"
        );
    }

    #[test]
    fn test_end_to_end_platform_completion() {
        use bsl_platform::PlatformDataInner;
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use syntax::TextSize;
        use vfs::{FileId, FileSet, VfsPath};

        // Skip if no platform data available
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() || data.all_methods().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        // Create database and add file
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // BSL code with cursor after DOT
        // Use "XBase" type which has 30 methods in platform data
        let code = r#"Процедура Тест()
    Результат = XBase.
КонецПроцедуры"#;

        // Set file content + source root. The CommonModule fast-path in
        // `platform_completions` queries `module_index` via
        // `file_source_root_input`, so even tests that hit the "platform
        // type" path need a source root wired up.
        db.set_file_text(file_id, code);
        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
        db.set_file_source_root(file_id, SourceRootId(0));

        // Position is right after the DOT (end of "XBase.")
        // We want left_biased to catch the DOT token
        let dot_end = code.find("XBase.").unwrap() + "XBase.".len();
        let offset = TextSize::from(dot_end as u32);

        // Request completions at the DOT position
        let position = CompletionPosition { file_id, offset, workspace_root: None };

        let items = platform_completions(&db, position);

        // Should have platform method completions
        assert!(items.is_some(), "Expected platform completions after DOT on XBase type");

        let items = items.unwrap();
        assert!(!items.is_empty(), "Expected at least one method completion");

        // All items should be methods
        for item in &items {
            assert_eq!(item.kind, CompletionItemKind::Method);
        }

        // Should contain common String methods if platform data is available
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        println!("Found {} method completions", labels.len());

        // If we have methods, verify snippet format
        for item in &items {
            // All methods should have snippets ending with $0)
            assert!(
                item.insert_text.ends_with("$0)"),
                "Method snippet should end with $0): {}",
                item.insert_text
            );

            // All methods should have parentheses
            assert!(
                item.insert_text.contains('(') && item.insert_text.contains(')'),
                "Method snippet should have parentheses: {}",
                item.insert_text
            );
        }
    }
}
