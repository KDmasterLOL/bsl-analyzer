//! MissingCommonModuleMethod diagnostic.
//!
//! Detects erroneous calls to methods of common modules.
//!
//! ## What it checks
//!
//! 1. **Method does not exist** - Method not defined in the referenced CommonModule
//! 2. **Non-export method** - Method exists but lacks `Экспорт` (Export) keyword
//! 3. **Missing source code** - CommonModule has no source code
//!
//! ## Why?
//!
//! Calling non-existent or private methods of CommonModules leads to runtime errors.
//! BSL (1C:Enterprise) allows calls to CommonModule methods only if they are exported.
//!
//! ## Bad practice
//!
//! ```bsl
//! // Method does not exist
//! ПервыйОбщийМодуль.МетодНесуществующий(1, 2);  // ERROR
//!
//! // Method exists but not exported (private)
//! ПервыйОбщийМодуль.РегистрацияИзмененийПередУдалением(Источник, Отказ);  // ERROR
//! ```
//!
//! ## Good practice
//!
//! ```bsl
//! // Method exported correctly
//! Процедура НеУстаревшаяПроцедура() Экспорт
//!     // implementation
//! КонецПроцедуры
//!
//! // Valid call
//! ПервыйОбщийМодуль.НеУстаревшаяПроцедура();  // OK
//! ```
//!
//! ## Excluded cases
//!
//! - Variable names that coincide with CommonModule names (treated as local variable)
//! - Internal calls within the same module (private methods OK within their own module)
//! - Manager module calls (`Справочники.X.Method`) - future scope
//!
//! ## Configuration
//!
//! - **Enabled by default:** Yes
//! - **Severity:** BLOCKER (ERROR)
//! - **Tags:** ERROR
//! - **Minutes to fix:** 5
//! - **No configurable parameters** (strict validator)
//!
//! ## Reference
//!
//! Ported from:
//! - MissingCommonModuleMethodDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::traits::{MdObject, Module};
use ide_db::hir_def::resolver::Resolver;
use ide_db::hir_def::{ModuleId, Name, PathResolution, QualifiedName};
use syntax::{SyntaxKind, SyntaxNode, TextRange};
use vfs::{FileId, VfsPath};

/// Creates diagnostic from HIR BodyDiagnostic (new HIR-based approach).
///
/// This is a temporary function during refactoring. Once Phase 4-5 are complete,
/// this will become the only `from_hir()` function.
///
/// # Arguments
///
/// * `module` - CommonModule name
/// * `method` - Method name
/// * `reason` - Error reason (MethodNotFound, NonExportMethod, ModuleNotFound)
/// * `range` - Source range for diagnostic
/// * `ctx` - Diagnostics context
pub fn from_hir_new(
    module: &str,
    method: &str,
    reason: &hir_def::CommonModuleMethodError,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::MissingCommonModuleMethod) {
        return None;
    }

    use hir_def::CommonModuleMethodError;

    let message = match reason {
        CommonModuleMethodError::MethodNotFound => {
            format!("Метод {} общего модуля {} не существует", method, module)
        }
        CommonModuleMethodError::NonExportMethod => {
            format!(
                "Исправьте обращение к закрытому, неэкспортному методу {} общего модуля {}",
                method, module
            )
        }
        CommonModuleMethodError::ModuleNotFound => {
            format!("Общий модуль {} не найден", module)
        }
    };

    Some(Diagnostic {
        code: DiagnosticCode::MissingCommonModuleMethod,
        message,
        severity: Severity::Blocker,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

/// Creates diagnostic from HIR BodyDiagnostic (old approach - to be removed).
///
/// Called from lib.rs dispatch when `BodyDiagnostic::MissingCommonModuleMethod` is encountered.
///
/// This function validates a qualified call using path resolution:
/// 1. Constructs QualifiedName from module and method names
/// 2. Uses Resolver with WorkspaceScope to resolve the qualified path
/// 3. PathResolution::Method(id) → check if method is exported (via metadata fallback)
/// 4. PathResolution::Unresolved → method or module doesn't exist
///
/// This approach leverages the new workspace indexing and path resolution infrastructure
/// from Phases 1-3, providing more accurate diagnostics than metadata-only checking.
pub fn from_hir(
    module: &str,
    method: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::MissingCommonModuleMethod) {
        return None;
    }

    // Build qualified path
    let qualified_name = QualifiedName::from_segments([Name::new(module), Name::new(method)]);

    // Create resolver with workspace scope for cross-module resolution
    let module_id = ModuleId::new(ctx.file_id);
    let resolver = Resolver::with_workspace_scope(module_id);

    // Resolve the qualified path using workspace symbols
    let resolution = resolver.resolve_path(ctx.db, &qualified_name);

    tracing::trace!(
        module_name = module,
        method_name = method,
        resolution = ?resolution,
        "Path resolution result in HIR diagnostic"
    );

    match resolution {
        PathResolution::Method(method_id) => {
            // Method found - check if it's exported via SymbolTree
            let method_module_id = method_id.module;
            let symbol_tree = ctx.db.symbol_tree(method_module_id);
            let method_name_obj = Name::new(method);

            if let Some(method_sym) = symbol_tree.find_method(&method_name_obj) {
                if !method_sym.is_export {
                    // Method exists but not exported
                    return Some(create_diagnostic_from_hir(
                        range,
                        ErrorType::NonExportMethod,
                        method,
                        module,
                    ));
                }
            }

            // Valid exported method
            None
        }
        PathResolution::Unresolved(_) => {
            // Could not resolve - method or module doesn't exist
            // Fallback to metadata check to distinguish between missing module and missing method
            if let Some(configuration) = ctx.load_configuration() {
                if let Some(common_module) = configuration.find_common_module(module) {
                    if find_common_module_file(ctx, common_module).is_some() {
                        // Module exists - method must be missing
                        return Some(create_diagnostic_from_hir(
                            range,
                            ErrorType::MethodNotFound,
                            method,
                            module,
                        ));
                    }
                }
            }

            // Module not found in metadata - might be a local variable or typo
            // Return diagnostic for method not found (conservative approach)
            Some(create_diagnostic_from_hir(range, ErrorType::MethodNotFound, method, module))
        }
        _ => None,
    }
}

/// Main entry point for MissingCommonModuleMethod diagnostic (AST-based fallback).
///
/// Detects missing or non-export methods in CommonModule calls.
/// This is kept for backward compatibility but uses HIR-based collection via lowering.
///
/// ## Algorithm
///
/// 1. Early return if disabled or no metadata
/// 2. Load Configuration metadata via Salsa (cached!)
/// 3. Find all qualified calls (Module.Method pattern) in AST
/// 4. For each qualified call:
///    - Extract module and method names
///    - Find CommonModule in metadata (case-insensitive)
///    - Resolve CommonModule file via VFS
///    - Build SymbolTree for CommonModule
///    - Lookup method in SymbolTree
///    - Create diagnostic if:
///      - Method not found → "method does not exist"
///      - Method found but not exported → "non-export method"
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::MissingCommonModuleMethod) {
        return Vec::new();
    }

    // Load metadata via ctx.load_configuration() for Salsa caching
    let configuration = match ctx.load_configuration() {
        Some(config) => config,
        None => return Vec::new(),
    };

    let root = ctx.db.parse(ctx.file_id).syntax_node();
    let mut diagnostics = Vec::new();
    // Track (module_name, method_name, start_offset) to avoid duplicates
    let mut seen_calls = std::collections::HashSet::new();

    // Find all ARG_LIST nodes - these indicate method/procedure calls
    // This approach works for all call contexts: statements, expressions, chains
    for node in root.descendants() {
        if node.kind() == SyntaxKind::ARG_LIST {
            // Get parent node (the call node)
            if let Some(parent) = node.parent() {
                if let Some((module_name, method_name)) = extract_qualified_call(&parent) {
                    tracing::trace!(
                        module_name,
                        method_name,
                        range = ?parent.text_range(),
                        "Found qualified call"
                    );

                    // Deduplicate BEFORE creating diagnostic
                    // Use (module, method, start_pos) as key to skip duplicate calls
                    let call_key =
                        (module_name.clone(), method_name.clone(), parent.text_range().start());

                    if !seen_calls.insert(call_key) {
                        tracing::trace!(range = ?parent.text_range(), "Skipped duplicate call");
                        continue;
                    }

                    // Skip if module name is a local variable/parameter (shadowing)
                    if is_local_variable(&parent, &module_name) {
                        tracing::trace!(module_name, "Skipped call to local variable");
                        continue;
                    }

                    // Check if it's a CommonModule call
                    if let Some(diag) = check_common_module_method(
                        ctx,
                        &parent,
                        &configuration,
                        &module_name,
                        &method_name,
                    ) {
                        tracing::trace!(range = ?diag.range, "Created diagnostic");
                        diagnostics.push(diag);
                    }
                }
            }
        }
    }

    diagnostics
}

/// Check if a name refers to a local variable or parameter.
///
/// This handles variable shadowing where a parameter/variable has the same name
/// as a CommonModule.
///
/// ## Example
/// ```bsl
/// Процедура Тест(ПервыйОбщийМодуль)  // ПервыйОбщийМодуль is a parameter
///     ПервыйОбщийМодуль.Method();    // Calls method on parameter, not CommonModule
/// КонецПроцедуры
/// ```
fn is_local_variable(call_node: &SyntaxNode, name: &str) -> bool {
    use syntax::ast;
    use syntax::ast::AstNode;

    // Find enclosing procedure or function
    let mut current = call_node.parent();
    while let Some(node) = current {
        // Check if it's a procedure
        if let Some(proc) = ast::ProcedureDef::cast(node.clone()) {
            // Check parameters
            if let Some(param_list) = proc.param_list() {
                for param in param_list.params() {
                    if let Some(param_name) = param.name() {
                        if param_name.text().eq_ignore_ascii_case(name) {
                            return true;
                        }
                    }
                }
            }
            // Check local variables (Перем statements)
            if let Some(body) = proc.body() {
                for var_def in body.var_decls() {
                    if let Some(var_name) = var_def.name() {
                        if var_name.text().eq_ignore_ascii_case(name) {
                            return true;
                        }
                    }
                }
            }
            // Found enclosing procedure, no need to search further
            break;
        }

        // Check if it's a function
        if let Some(func) = ast::FunctionDef::cast(node.clone()) {
            // Check parameters
            if let Some(param_list) = func.param_list() {
                for param in param_list.params() {
                    if let Some(param_name) = param.name() {
                        if param_name.text().eq_ignore_ascii_case(name) {
                            return true;
                        }
                    }
                }
            }
            // Check local variables (Перем statements)
            if let Some(body) = func.body() {
                for var_def in body.var_decls() {
                    if let Some(var_name) = var_def.name() {
                        if var_name.text().eq_ignore_ascii_case(name) {
                            return true;
                        }
                    }
                }
            }
            // Found enclosing function, no need to search further
            break;
        }

        current = node.parent();
    }

    false
}

/// Check a qualified CommonModule.Method() call for errors.
///
/// Returns diagnostic if:
/// - Method does not exist in CommonModule
/// - Method exists but is not exported
///
/// Returns None if:
/// - CommonModule not found in metadata (could be a variable)
/// - CommonModule file not found in VFS
/// - Method is valid and exported
fn check_common_module_method(
    ctx: &DiagnosticsContext,
    call_node: &SyntaxNode,
    configuration: &bsl_metadata::Configuration,
    module_name: &str,
    method_name: &str,
) -> Option<Diagnostic> {
    // Find CommonModule in metadata (case-insensitive)
    let common_module = configuration.find_common_module(module_name)?;

    // Resolve CommonModule file via VFS
    let module_file_id = find_common_module_file(ctx, common_module)?;

    // Build SymbolTree for CommonModule
    let module_id = ModuleId::new(module_file_id);

    // Check if the file parses correctly
    let module_parse = ctx.db.parse(module_file_id);
    tracing::trace!(module_name, errors = module_parse.errors().len(), "CommonModule parse result");

    // Check ItemTree
    let item_tree = ctx.db.item_tree(module_file_id);
    tracing::trace!(
        module_name,
        top_level_items = item_tree.top_level_items().len(),
        "ItemTree loaded"
    );

    let module_symbol_tree = ctx.db.symbol_tree(module_id);

    // Log all methods in the SymbolTree
    if tracing::enabled!(tracing::Level::TRACE) {
        for method in module_symbol_tree.methods() {
            tracing::trace!(
                method_name = method.name.as_str(),
                is_export = method.is_export,
                is_function = method.is_function,
                "SymbolTree method"
            );
        }
    }

    // Lookup method
    let method_name_obj = Name::new(method_name);
    let method = module_symbol_tree.find_method(&method_name_obj);

    tracing::trace!(
        module_name,
        method_name,
        found = method.is_some(),
        is_export = method.as_ref().map(|m| m.is_export).unwrap_or(false),
        "Method lookup result"
    );

    // Create diagnostic based on result
    match method {
        None => {
            // Method does not exist
            Some(create_diagnostic(call_node, ErrorType::MethodNotFound, method_name, module_name))
        }
        Some(m) if !m.is_export => {
            // Method exists but not exported
            Some(create_diagnostic(call_node, ErrorType::NonExportMethod, method_name, module_name))
        }
        Some(_) => {
            // Valid exported method
            None
        }
    }
}

/// Extract module and method names from qualified call (Module.Method).
///
/// Pattern: CALL_EXPR contains IDENT tokens before ARG_LIST
///          For two-level call: [ModuleName, MethodName]
///
/// Returns: Some((module_name, method_name)) or None for local calls
fn extract_qualified_call(call_node: &SyntaxNode) -> Option<(String, String)> {
    // Collect all IDENT tokens (from all descendants) but stop at ARG_LIST
    let mut idents: Vec<String> = Vec::new();

    for child in call_node.children_with_tokens() {
        if child.kind() == SyntaxKind::ARG_LIST {
            break;
        }

        // Collect IDENT tokens from this child
        if let Some(element) = child.as_node() {
            for token in element.descendants_with_tokens() {
                if let Some(t) = token.as_token() {
                    if t.kind() == SyntaxKind::IDENT {
                        idents.push(t.text().to_string());
                    }
                }
            }
        } else if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                idents.push(token.text().to_string());
            }
        }
    }

    // Need at least 2 idents (module + method) for qualified call
    if idents.len() < 2 {
        return None;
    }

    // Method name is always the last identifier
    let method_name = idents.pop()?;

    // For two-level call (CommonModule.Method), object name is second-to-last
    if idents.len() == 1 {
        let module_name = idents.pop()?;
        return Some((module_name, method_name));
    }

    // For three-level calls (Документы.ПКО.Method), skip for now
    // This is future scope, not in current implementation
    None
}

/// Find the FileId for a CommonModule by resolving its URI through VFS.
///
/// ## Implementation
///
/// 1. Get CommonModule URI from metadata
/// 2. Build absolute path: workspace_root + URI
/// 3. Resolve FileId via ctx.file_set (bypasses Salsa for performance)
///
/// ## Performance
///
/// - O(1) HashMap lookup in FileSet
fn find_common_module_file(
    ctx: &DiagnosticsContext,
    common_module: &bsl_metadata::CommonModule,
) -> Option<FileId> {
    let module_name = common_module.name();

    // Get the CommonModule's URI from metadata
    let uri = common_module.uri()?;

    // Get the configuration path (where metadata files are) to build absolute path
    // Configuration path has priority because URIs are relative to it
    let config_path = ctx.configuration_path.or(ctx.workspace_root)?;

    // Build full absolute path: config_path + URI
    let full_path = config_path.join(uri);
    let vfs_path = VfsPath::new(full_path.clone());

    // CRITICAL: Use ctx.file_set directly to bypass Salsa for performance
    let file_id = if let Some(file_set) = ctx.file_set {
        file_set.file_for_path(&vfs_path).copied()
    } else {
        // Fallback: Resolve via Salsa (slower, for tests)
        let source_root_input = ctx.db.file_source_root_input(ctx.file_id);
        let source_root_id = source_root_input.source_root_id(ctx.db);
        ctx.db.resolve_vfs_path(source_root_id, &vfs_path)
    };

    if file_id.is_none() {
        tracing::warn!(
            module_name,
            uri,
            full_path = ?full_path,
            "CommonModule file not found in VFS - ensure file is loaded"
        );
    }

    file_id
}

/// Error types for MissingCommonModuleMethod diagnostic.
enum ErrorType {
    /// Method does not exist in CommonModule
    MethodNotFound,
    /// Method exists but is not exported
    NonExportMethod,
}

/// Create a diagnostic for a missing or non-export CommonModule method.
///
/// ## Messages (Russian)
///
/// - **MethodNotFound:** "Метод {method} общего модуля {module} не существует"
/// - **NonExportMethod:** "Исправьте обращение к закрытому, неэкспортному методу {method} общего модуля {module}"
///
/// ## Range
///
/// Points to the method name in the qualified call (not the module name).
fn create_diagnostic(
    call_node: &SyntaxNode,
    error_type: ErrorType,
    method_name: &str,
    module_name: &str,
) -> Diagnostic {
    let message = match error_type {
        ErrorType::MethodNotFound => {
            format!("Метод {} общего модуля {} не существует", method_name, module_name)
        }
        ErrorType::NonExportMethod => {
            format!(
                "Исправьте обращение к закрытому, неэкспортному методу {} общего модуля {}",
                method_name, module_name
            )
        }
    };

    // Calculate range: from method name to end of call
    let range = calculate_method_name_range(call_node);

    Diagnostic {
        code: DiagnosticCode::MissingCommonModuleMethod,
        message,
        severity: Severity::Blocker,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

/// Create a diagnostic for HIR-based collection.
///
/// Similar to create_diagnostic but works with a range directly from HIR.
fn create_diagnostic_from_hir(
    range: TextRange,
    error_type: ErrorType,
    method_name: &str,
    module_name: &str,
) -> Diagnostic {
    let message = match error_type {
        ErrorType::MethodNotFound => {
            format!("Метод {} общего модуля {} не существует", method_name, module_name)
        }
        ErrorType::NonExportMethod => {
            format!(
                "Исправьте обращение к закрытому, неэкспортному методу {} общего модуля {}",
                method_name, module_name
            )
        }
    };

    Diagnostic {
        code: DiagnosticCode::MissingCommonModuleMethod,
        message,
        severity: Severity::Blocker,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

/// Calculate the range for the diagnostic.
///
/// Points to the method name in the qualified call.
/// For `Module.Method()`, the range covers from `Method` to the end of the call.
fn calculate_method_name_range(call_node: &SyntaxNode) -> TextRange {
    // Try to find the method name (last IDENT before ARG_LIST)
    let mut last_ident_range = None;
    let mut last_ident_text = String::new();

    for child in call_node.descendants_with_tokens() {
        // Stop at ARG_LIST
        if child.kind() == SyntaxKind::ARG_LIST {
            break;
        }

        // Track last IDENT
        if child.kind() == SyntaxKind::IDENT {
            if let Some(token) = child.as_token() {
                last_ident_range = Some(token.text_range());
                last_ident_text = token.text().to_string();
            }
        }
    }

    tracing::trace!(
        last_ident = ?last_ident_text,
        range = ?last_ident_range,
        "Found last identifier"
    );

    // If we found the method name, use range from method name to end of call
    if let Some(ident_range) = last_ident_range {
        let result = TextRange::new(ident_range.start(), call_node.text_range().end());
        tracing::trace!(
            result = ?result,
            start = ?ident_range.start(),
            end = ?call_node.text_range().end(),
            "Calculated method name range"
        );
        result
    } else {
        // Fallback: entire call expression
        call_node.text_range()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::{RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use vfs::FileSet;

    #[test]
    fn test_missing_common_module_method() {
        // Use real metadata fixtures
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");
        let fixtures_path = Path::new(fixtures_dir);

        // Read test code
        let code = include_str!("../../test_data/MissingCommonModuleMethodDiagnostic.bsl");

        // Read CommonModule file
        let common_module_path = fixtures_path
            .join("CommonModules")
            .join("ПервыйОбщийМодуль")
            .join("Ext")
            .join("Module.bsl");
        let common_module_code = std::fs::read_to_string(&common_module_path)
            .expect("CommonModule fixture should exist");

        // Set up database with proper VFS
        let mut db = RootDatabaseImpl::new();
        let test_file_id = vfs::FileId(0);
        let common_module_file_id = vfs::FileId(1);

        db.set_file_text(test_file_id, code);
        db.set_file_text(common_module_file_id, &common_module_code);

        // Create FileSet with proper path mappings
        let mut file_set = FileSet::new();
        file_set.insert(test_file_id, VfsPath::new(PathBuf::from("/test.bsl")));
        file_set.insert(common_module_file_id, VfsPath::new(common_module_path.clone()));

        // Create SourceRoot and register in database
        let source_root = SourceRoot::new_local(file_set);
        let source_root_id = SourceRootId(0);

        // Register SourceRoot in database
        db.set_source_root(source_root_id, source_root);

        // Link files to SourceRoot
        db.set_file_source_root(test_file_id, source_root_id);
        db.set_file_source_root(common_module_file_id, source_root_id);

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db);

        // Create configuration path input
        let config_path_str = fixtures_path.to_string_lossy().to_string();
        let path_input =
            ide_db::metadata::ConfigurationPathInput::new(db.as_ref(), config_path_str);

        // Run diagnostic
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &DiagnosticsConfig::default(),
            file_id: test_file_id,
            workspace_root: Some(fixtures_path),
            configuration_path: Some(fixtures_path),
            configuration_path_input: Some(path_input),
            file_set: None,
        };

        // Verify parsing
        let parse = db.parse(test_file_id);
        assert_eq!(parse.errors().len(), 0, "Test file should parse without errors");

        let diagnostics = check(&ctx);

        // Expected: 10 diagnostics (5 non-existent + 5 non-export)
        assert_eq!(diagnostics.len(), 10, "Expected 10 diagnostics");

        // Verify exact diagnostic positions.
        // Diagnostic range starts at method name and extends through call expression.
        // Positions are character-based (0-indexed).
        //
        // Note: Range currently includes the entire method call (Module.Method()),
        // not just the method name. This is due to FIELD_EXPR node boundaries.
        // Future optimization: refine extract_method_name_range() to return only
        // the method name identifier range.

        // Non-existent methods (5 diagnostics)
        assert_diagnostic_range(code, &diagnostics[0], 1, 22, 47); // МетодНесуществующий(1, 2)
        assert_diagnostic_range(code, &diagnostics[1], 2, 26, 53); // ДругойМетодНесуществующий()
        assert_diagnostic_range(code, &diagnostics[2], 3, 22, 46); // ЕщеМетодНесуществующий()
        assert_diagnostic_range(code, &diagnostics[3], 4, 22, 50); // ЕщеОдинМетодНесуществующий()
        assert_diagnostic_range(code, &diagnostics[4], 5, 26, 56); // ЕщеДругойМетодНесуществующий()

        // Non-export methods (5 diagnostics)
        assert_diagnostic_range(code, &diagnostics[5], 11, 22, 73); // РегистрацияИзмененийПередУдалением(...)
        assert_diagnostic_range(code, &diagnostics[6], 12, 26, 32); // Тест()
        assert_diagnostic_range(code, &diagnostics[7], 13, 22, 28); // Тест()
        assert_diagnostic_range(code, &diagnostics[8], 14, 22, 28); // Тест()
        assert_diagnostic_range(code, &diagnostics[9], 15, 26, 32); // Тест()
    }

    #[test]
    fn test_without_metadata() {
        let code = r#"
Процедура Тест()
    ПервыйОбщийМодуль.МетодНесуществующий(1, 2);
КонецПроцедуры
"#;

        let mut db = RootDatabaseImpl::new();
        let file_id = vfs::FileId(0);
        db.set_file_text(file_id, code);

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db);

        // No metadata context
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &DiagnosticsConfig::default(),
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);

        // Should return empty - no metadata available
        assert_eq!(diagnostics.len(), 0, "Expected 0 diagnostics without metadata");
    }

    #[test]
    fn test_local_variable_shadowing() {
        // Test that local variables shadow CommonModule names
        let code = r#"
Процедура Тест()
    Перем ПервыйОбщийМодуль;  // Local variable shadows CommonModule name
    ПервыйОбщийМодуль.Method();  // Should NOT trigger diagnostic - it's a variable
КонецПроцедуры

Функция ДругойТест()
    Перем ПервыйОбщийМодуль;
    Возврат ПервыйОбщийМодуль.SomeMethod();  // Should NOT trigger - local variable
КонецФункции
"#;

        let mut db = RootDatabaseImpl::new();
        let file_id = vfs::FileId(0);
        db.set_file_text(file_id, code);

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db);

        // No metadata needed for this test - shadowing check is purely syntax-based
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &DiagnosticsConfig::default(),
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);

        // Should return empty - all calls are to local variables, not CommonModules
        assert_eq!(
            diagnostics.len(),
            0,
            "Expected 0 diagnostics when local variables shadow CommonModule names"
        );
    }

    #[test]
    fn test_mixed_local_and_common_module() {
        // Use real metadata fixtures
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");
        let fixtures_path = Path::new(fixtures_dir);

        // Test with both local variable and actual CommonModule call
        let code = r#"
Процедура Тест()
    Перем ПервыйОбщийМодуль;
    ПервыйОбщийМодуль.Method();  // Local variable - no diagnostic
КонецПроцедуры

Процедура ДругойТест()
    // No local variable here
    ПервыйОбщийМодуль.МетодНесуществующий(1, 2);  // CommonModule - should trigger diagnostic
КонецПроцедуры
"#;

        // Read CommonModule file
        let common_module_path = fixtures_path
            .join("CommonModules")
            .join("ПервыйОбщийМодуль")
            .join("Ext")
            .join("Module.bsl");
        let common_module_code = std::fs::read_to_string(&common_module_path)
            .expect("CommonModule fixture should exist");

        // Set up database with proper VFS
        let mut db = RootDatabaseImpl::new();
        let test_file_id = vfs::FileId(0);
        let common_module_file_id = vfs::FileId(1);

        db.set_file_text(test_file_id, code);
        db.set_file_text(common_module_file_id, &common_module_code);

        // Create FileSet with proper path mappings
        let mut file_set = FileSet::new();
        file_set.insert(test_file_id, VfsPath::new(PathBuf::from("/test.bsl")));
        file_set.insert(common_module_file_id, VfsPath::new(common_module_path.clone()));

        // Create SourceRoot and register in database
        let source_root = SourceRoot::new_local(file_set);
        let source_root_id = SourceRootId(0);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(test_file_id, source_root_id);
        db.set_file_source_root(common_module_file_id, source_root_id);

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db);

        // Create configuration path input
        let config_path_str = fixtures_path.to_string_lossy().to_string();
        let path_input =
            ide_db::metadata::ConfigurationPathInput::new(db.as_ref(), config_path_str);

        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &DiagnosticsConfig::default(),
            file_id: test_file_id,
            workspace_root: Some(fixtures_path),
            configuration_path: Some(fixtures_path),
            configuration_path_input: Some(path_input),
            file_set: None,
        };

        let diagnostics = check(&ctx);

        // Expected: 1 diagnostic (only in ДругойТест where there's no shadowing)
        assert_eq!(
            diagnostics.len(),
            1,
            "Expected 1 diagnostic for non-existent method in non-shadowed CommonModule call"
        );

        // Verify it's the correct diagnostic
        assert!(
            diagnostics[0].message.contains("МетодНесуществующий"),
            "Diagnostic should be about МетодНесуществующий"
        );
    }
}
