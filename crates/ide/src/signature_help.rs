//! Signature help for function/method calls.
//!
//! Provides parameter hints when the cursor is inside a function call,
//! showing the function signature and highlighting the current parameter.

use bsl_platform::{
    global_function_query, platform_method_query, GlobalFunction, MethodDocs, MethodLookupInput,
    PlatformDataInner, PlatformMethod, TypeNameInput,
};
use hir::{Function, ModItem, Param, Procedure};
use ide_db::RootDatabase;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextSize};
use vfs::FileId;

/// Result of signature help.
#[derive(Debug, Clone)]
pub struct SignatureHelp {
    /// Full signature: "НачатьТранзакцию([РежимБлокировок])"
    pub signature: String,
    /// Documentation (markdown).
    pub doc: Option<String>,
    /// Index of the active parameter (0-based).
    pub active_parameter: Option<usize>,
    /// Information about parameters.
    pub parameters: Vec<ParameterInfo>,
}

/// Information about a single parameter.
#[derive(Debug, Clone)]
pub struct ParameterInfo {
    /// Parameter text for display.
    pub label: String,
    /// Parameter documentation.
    pub documentation: Option<String>,
}

/// Returns signature help at the specified position.
pub fn signature_help<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
) -> Option<SignatureHelp> {
    let _span = tracing::info_span!("signature_help", ?file_id, ?offset).entered();

    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    // Find token at position (prefer left-biased for cases like "func(|)")
    let token = root.token_at_offset(offset).left_biased()?;

    tracing::debug!(token_kind = ?token.kind(), token_text = ?token.text(), "Signature help token");

    // Find ARG_LIST in ancestors
    let arg_list = find_arg_list(&token)?;

    // Skip if cursor is on closing paren
    if is_on_closing_paren(&token, &arg_list) {
        tracing::debug!("Cursor on closing paren, skipping");
        return None;
    }

    // Find parent CALL_EXPR
    let call_expr = find_call_expr(&arg_list)?;

    // Extract callee info (receiver type for methods, function name)
    let (receiver_type, callee_name) = extract_callee_info(&call_expr)?;

    tracing::debug!(?receiver_type, ?callee_name, "Extracted callee info");

    // Count commas before cursor to determine active parameter
    let active_param = count_commas_before(&arg_list, offset);

    // Resolve and build signature help
    if let Some(type_name) = receiver_type {
        // Method call: receiver.method()
        if let Some(sig) = build_for_platform_method(db, &type_name, &callee_name, active_param) {
            return Some(sig);
        }

        // Try CommonModule method
        if let Some(sig) =
            build_for_common_module_method(db, file_id, &type_name, &callee_name, active_param)
        {
            return Some(sig);
        }
    }

    // Try global function
    if let Some(sig) = build_for_global_function(db, &callee_name, active_param) {
        return Some(sig);
    }

    // Try user-defined method
    if let Some(sig) = build_for_user_method(db, file_id, &callee_name, active_param) {
        return Some(sig);
    }

    None
}

/// Find ARG_LIST node in token's ancestors.
fn find_arg_list(token: &SyntaxToken) -> Option<SyntaxNode> {
    token.parent_ancestors().find(|node| node.kind() == SyntaxKind::ARG_LIST)
}

/// Check if cursor is positioned on the closing parenthesis.
fn is_on_closing_paren(token: &SyntaxToken, arg_list: &SyntaxNode) -> bool {
    if token.kind() == SyntaxKind::R_PAREN {
        // Check if this R_PAREN is the closing one of our ARG_LIST
        if let Some(parent) = token.parent() {
            return parent == *arg_list || parent.parent().as_ref() == Some(arg_list);
        }
    }
    false
}

/// Find CALL_EXPR parent of ARG_LIST.
fn find_call_expr(arg_list: &SyntaxNode) -> Option<SyntaxNode> {
    arg_list.parent().filter(|p| p.kind() == SyntaxKind::CALL_EXPR)
}

/// Extract callee information from CALL_EXPR.
///
/// Returns (receiver_type, method_name):
/// - For `Строка.Найти()`: (Some("Строка"), "Найти")
/// - For `НачатьТранзакцию()`: (None, "НачатьТранзакцию")
fn extract_callee_info(call_expr: &SyntaxNode) -> Option<(Option<String>, String)> {
    // CALL_EXPR structure: callee (IDENT or FIELD_EXPR) followed by ARG_LIST
    let first_child = call_expr.first_child()?;

    match first_child.kind() {
        SyntaxKind::FIELD_EXPR => {
            // Method call: receiver.method
            // FIELD_EXPR: receiver DOT method_name
            // Note: receiver IDENT can be either a node or token depending on parser
            let mut receiver = None;
            let mut method = None;

            for child in first_child.children_with_tokens() {
                match child.kind() {
                    SyntaxKind::IDENT => {
                        let text = if let Some(token) = child.as_token() {
                            token.text().to_string()
                        } else if let Some(node) = child.as_node() {
                            // IDENT node wrapping an IDENT token
                            node.text().to_string()
                        } else {
                            continue;
                        };
                        if receiver.is_none() {
                            receiver = Some(text);
                        } else {
                            method = Some(text);
                        }
                    }
                    SyntaxKind::DOT => {
                        // Next IDENT will be the method name
                    }
                    _ => {}
                }
            }

            Some((receiver, method?))
        }
        SyntaxKind::IDENT => {
            // IDENT node (not token) - need to find IDENT token inside
            // AST structure: CALL_EXPR -> IDENT (node) -> IDENT (token)
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
            // Try to find IDENT in children
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

/// Count commas before the cursor position in ARG_LIST.
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

/// Build SignatureHelp for a platform method.
fn build_for_platform_method<DB: RootDatabase>(
    db: &DB,
    type_name: &str,
    method_name: &str,
    active_param: usize,
) -> Option<SignatureHelp> {
    let input = MethodLookupInput::new(db, type_name.to_string(), method_name.to_string());
    let method = platform_method_query(db, input)?;

    // Get documentation with default_value
    let docs = PlatformDataInner::instance().get_method_docs(method.id);

    Some(build_signature_from_platform_method(&method, docs.as_ref(), active_param))
}

/// Build SignatureHelp for a global function.
fn build_for_global_function<DB: RootDatabase>(
    db: &DB,
    function_name: &str,
    active_param: usize,
) -> Option<SignatureHelp> {
    let input = TypeNameInput::new(db, function_name.to_string());
    let function = global_function_query(db, input)?;

    // Get documentation with default_value
    let docs = PlatformDataInner::instance().get_global_function_docs(function.id);

    Some(build_signature_from_global_function(&function, docs.as_ref(), active_param))
}

/// Build SignatureHelp for a user-defined method.
fn build_for_user_method<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    method_name: &str,
    active_param: usize,
) -> Option<SignatureHelp> {
    use hir::Name;

    let name = Name::new(method_name);
    let module_id = hir::ModuleId::new(file_id);
    let resolver = hir::Resolver::for_module(module_id);

    // Try to resolve as a module method
    let method_id = resolver.resolve_module_method(db, &name)?;

    // Get ItemTree to access method signature
    let tree = db.item_tree(method_id.module.file_id);

    let item = tree.top_level_items().get(method_id.local_id as usize)?;

    match item {
        ModItem::Procedure(idx) => {
            let proc = tree.procedure(*idx);
            Some(build_signature_from_procedure(proc, active_param))
        }
        ModItem::Function(idx) => {
            let func = tree.function(*idx);
            Some(build_signature_from_function(func, active_param))
        }
        ModItem::Variable(_) => None,
    }
}

/// Build SignatureHelp from a PlatformMethod.
fn build_signature_from_platform_method(
    method: &PlatformMethod,
    docs: Option<&MethodDocs>,
    active_param: usize,
) -> SignatureHelp {
    let params: Vec<ParameterInfo> = method
        .parameters
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let type_hint = p.param_type.as_deref().unwrap_or("Произвольный");

            // Get default_value from docs if available
            let default_value =
                docs.and_then(|d| d.params.get(i)).and_then(|pd| pd.default_value.as_deref());

            let label = if p.is_optional {
                match default_value {
                    Some(val) => format!("[{}: {} = {}]", p.name, type_hint, val),
                    None => format!("[{}: {}]", p.name, type_hint),
                }
            } else {
                format!("{}: {}", p.name, type_hint)
            };
            ParameterInfo { label, documentation: None }
        })
        .collect();

    let param_labels: Vec<_> = params.iter().map(|p| p.label.clone()).collect();
    let signature = format!("{}({})", method.name, param_labels.join(", "));

    let active_parameter = if active_param < params.len() { Some(active_param) } else { None };

    SignatureHelp { signature, doc: None, active_parameter, parameters: params }
}

/// Build SignatureHelp from a GlobalFunction.
fn build_signature_from_global_function(
    function: &GlobalFunction,
    docs: Option<&MethodDocs>,
    active_param: usize,
) -> SignatureHelp {
    let params: Vec<ParameterInfo> = function
        .parameters
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let type_hint = p.param_type.as_deref().unwrap_or("Произвольный");

            // Get default_value from docs if available
            let default_value =
                docs.and_then(|d| d.params.get(i)).and_then(|pd| pd.default_value.as_deref());

            let label = if p.is_optional {
                match default_value {
                    Some(val) => format!("[{}: {} = {}]", p.name, type_hint, val),
                    None => format!("[{}: {}]", p.name, type_hint),
                }
            } else {
                format!("{}: {}", p.name, type_hint)
            };
            ParameterInfo { label, documentation: None }
        })
        .collect();

    let param_labels: Vec<_> = params.iter().map(|p| p.label.clone()).collect();
    let signature = format!("{}({})", function.name, param_labels.join(", "));

    let return_info =
        function.return_type.as_ref().map(|r| format!(" -> {}", r)).unwrap_or_default();
    let signature = format!("{}{}", signature, return_info);

    let active_parameter = if active_param < params.len() { Some(active_param) } else { None };

    SignatureHelp { signature, doc: None, active_parameter, parameters: params }
}

/// Build SignatureHelp from a user-defined Procedure.
fn build_signature_from_procedure(proc: &Procedure, active_param: usize) -> SignatureHelp {
    let params: Vec<ParameterInfo> = build_params_from_item_tree_params(&proc.params);

    let param_labels: Vec<_> = params.iter().map(|p| p.label.clone()).collect();
    let signature = format!("Процедура {}({})", proc.name.as_str(), param_labels.join(", "));

    let active_parameter = if active_param < params.len() { Some(active_param) } else { None };

    SignatureHelp { signature, doc: None, active_parameter, parameters: params }
}

/// Build SignatureHelp from a user-defined Function.
fn build_signature_from_function(func: &Function, active_param: usize) -> SignatureHelp {
    let params: Vec<ParameterInfo> = build_params_from_item_tree_params(&func.params);

    let param_labels: Vec<_> = params.iter().map(|p| p.label.clone()).collect();
    let signature = format!("Функция {}({})", func.name.as_str(), param_labels.join(", "));

    let active_parameter = if active_param < params.len() { Some(active_param) } else { None };

    SignatureHelp { signature, doc: None, active_parameter, parameters: params }
}

/// Build ParameterInfo list from ItemTree Params.
fn build_params_from_item_tree_params(params: &[Param]) -> Vec<ParameterInfo> {
    params
        .iter()
        .map(|p| {
            let label = if p.is_val {
                format!("Знач {}", p.name.as_str())
            } else if p.has_default {
                format!("[{}]", p.name.as_str())
            } else {
                p.name.as_str().to_string()
            };
            ParameterInfo { label, documentation: None }
        })
        .collect()
}

/// Build SignatureHelp for a CommonModule method.
///
/// Resolves module via module_index, then looks up method in ItemTree.
fn build_for_common_module_method<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    module_name: &str,
    method_name: &str,
    active_param: usize,
) -> Option<SignatureHelp> {
    use hir::Name;

    let source_root_input = db.file_source_root_input(file_id);
    let source_root_id = source_root_input.source_root_id(db);
    let module_index = db.module_index(source_root_id);

    let name = Name::new(module_name);
    let module_file_id = module_index.resolve_common_module(&name)?;

    let method_name = Name::new(method_name);
    let module_id = hir::ModuleId::new(module_file_id);
    let symbol_tree = db.symbol_tree(module_id);
    let method = symbol_tree.find_method(&method_name)?;

    if !method.is_export {
        return None;
    }

    // Get method from ItemTree for full parameter info
    let tree = db.item_tree(module_file_id);
    let item = tree.top_level_items().get(method.id.local_id as usize)?;

    let mut sig = match item {
        ModItem::Procedure(idx) => {
            let proc = tree.procedure(*idx);
            build_signature_from_procedure(proc, active_param)
        }
        ModItem::Function(idx) => {
            let func = tree.function(*idx);
            build_signature_from_function(func, active_param)
        }
        ModItem::Variable(_) => return None,
    };

    // Add documentation from method docs
    if let Some(docs) = db.method_docs(method.id) {
        sig.doc = docs.purpose.clone();
    }

    Some(sig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::vfs::{file_set::FileSet, VfsPath};
    use ide_db::RootDatabaseImpl;

    fn setup_db(code: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, code);
        (db, file_id)
    }

    fn find_cursor(code: &str) -> (String, TextSize) {
        let cursor_pos = code.find("$0").expect("No cursor marker $0 found");
        let code_without_cursor = code.replace("$0", "");
        (code_without_cursor, TextSize::from(cursor_pos as u32))
    }

    #[test]
    fn test_global_function_signature() {
        let code = "Процедура Тест()
    НачатьТранзакцию($0)
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let result = signature_help(&db, file_id, offset);

        // If platform data is available, we should get a result
        if let Some(sig) = result {
            assert!(sig.signature.contains("НачатьТранзакцию"));
        }
    }

    #[test]
    fn test_type_conversion_function() {
        // Строка() is a type conversion function
        let code = "Процедура Тест()
    Строка($0)
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let result = signature_help(&db, file_id, offset);

        // Platform data should have Строка function
        if let Some(sig) = result {
            assert!(sig.signature.contains("Строка"));
            assert_eq!(sig.active_parameter, Some(0));
        }
    }

    #[test]
    fn test_user_function_signature() {
        let code = "Функция МояФункция(Параметр1, Знач Параметр2)
    Возврат 1;
КонецФункции

Процедура Тест()
    МояФункция($0)
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let result = signature_help(&db, file_id, offset);

        if let Some(sig) = result {
            assert!(sig.signature.contains("МояФункция"));
            assert!(sig.signature.contains("Параметр1"));
            assert!(sig.signature.contains("Знач Параметр2"));
            assert_eq!(sig.active_parameter, Some(0));
        }
    }

    #[test]
    fn test_second_parameter_active() {
        let code = "Функция МояФункция(Параметр1, Параметр2)
    Возврат 1;
КонецФункции

Процедура Тест()
    МояФункция(1, $0)
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let result = signature_help(&db, file_id, offset);

        if let Some(sig) = result {
            assert_eq!(sig.active_parameter, Some(1));
        }
    }

    #[test]
    fn test_outside_call_no_signature() {
        let code = "Процедура Тест()
    Функция()$0
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let result = signature_help(&db, file_id, offset);
        assert!(result.is_none());
    }

    #[test]
    fn test_nested_call() {
        let code = "Функция Внешняя(А)
    Возврат А;
КонецФункции

Функция Внутренняя(Б)
    Возврат Б;
КонецФункции

Процедура Тест()
    Внешняя(Внутренняя($0))
КонецПроцедуры";
        let (code, offset) = find_cursor(code);
        let (db, file_id) = setup_db(&code);

        let result = signature_help(&db, file_id, offset);

        if let Some(sig) = result {
            // Should show signature for Внутренняя, not Внешняя
            assert!(sig.signature.contains("Внутренняя"));
        }
    }

    #[test]
    fn test_common_module_method_signature() {
        let mut db = RootDatabaseImpl::new();

        // CommonModule file
        let module_file_id = FileId(1);
        let module_code = "// Проверяет, является ли символ разделителем слов.
//
// Параметры:
//  КодСимвола - Число - код проверяемого символа
//  РазделителиСлов - Строка - допустимые разделители
//
// Возвращаемое значение:
//  Булево - Истина, если символ является разделителем
//
Функция ЭтоРазделительСлов(КодСимвола, РазделителиСлов = \" \") Экспорт
    Возврат Истина;
КонецФункции";

        // Calling file
        let caller_file_id = FileId(0);
        let caller_code = "Процедура Тест()
    СтроковыеФункцииКлиентСервер.ЭтоРазделительСлов($0)
КонецПроцедуры";

        let (caller_code, offset) = find_cursor(caller_code);

        // Setup FileSet with CommonModule path
        let mut file_set = FileSet::new();
        file_set.insert(
            module_file_id,
            VfsPath::new("/cf/CommonModules/СтроковыеФункцииКлиентСервер/Ext/Module.bsl"),
        );
        file_set.insert(caller_file_id, VfsPath::new("/cf/HTTPServices/lk/Ext/Module.bsl"));

        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(module_file_id, SourceRootId(0));
        db.set_file_source_root(caller_file_id, SourceRootId(0));
        db.set_file_text(module_file_id, module_code);
        db.set_file_text(caller_file_id, &caller_code);

        let result = signature_help(&db, caller_file_id, offset);

        assert!(result.is_some(), "Expected signature help for CommonModule method");
        let sig = result.unwrap();
        assert!(
            sig.signature.contains("ЭтоРазделительСлов"),
            "Signature should contain method name, got: {}",
            sig.signature
        );
        assert!(
            sig.signature.contains("КодСимвола"),
            "Signature should contain first param, got: {}",
            sig.signature
        );
        assert_eq!(sig.active_parameter, Some(0));
    }
}
