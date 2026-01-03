//! MissedRequiredParameter diagnostic.
//!
//! Detects missing required parameters in method calls.
//!
//! ## Why?
//! BSL (1C:Enterprise) allows omitting parameters in method calls, using commas to skip them.
//! However, parameters without default values are required and must be provided.
//!
//! ## Bad practice
//! ```bsl
//! Функция Сложение(Левый, Правый) Экспорт
//!     Возврат Левый + Правый;
//! КонецФункции
//!
//! Результат = Сложение(, 2);      // ERROR: Missing required parameter 'Левый'
//! Результат = Сложение(5);        // ERROR: Missing required parameter 'Правый'
//! Результат = Сложение();         // ERROR: Missing 'Левый', 'Правый'
//! ```
//!
//! ## Good practice
//! ```bsl
//! Результат = Сложение(5, 2);     // OK: All required parameters provided
//!
//! // With optional parameters:
//! Функция Инкремент(Значение, Приращение = 1)  // Приращение is optional
//!     Возврат Значение + Приращение;
//! КонецФункции
//!
//! Результат = Инкремент(5);       // OK: Optional parameter can be omitted
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** ERROR (MAJOR)
//! - **Tags:** ERROR
//! - **Minutes to fix:** 1
//! - **No configurable parameters** (strict validator)
//!
//! ## Implementation Phases
//!
//! **Phase 1 (IMPLEMENTED):** Local method calls
//! - Detects missing parameters for methods defined in the same module
//! - Uses SymbolTree for O(1) method resolution
//! - Coverage: 5/14 diagnostics from Java test (35%)
//!
//! **Phase 2 (IN PROGRESS):** CommonModule calls
//! - Supports `CommonModule.Method()` calls with metadata
//! - Uses Configuration metadata to resolve CommonModule names
//! - **Limitation:** File resolution needs VFS integration (returns None for now)
//! - **Workaround:** Tests can manually provide FileIds via extended context
//!
//! **Phase 3 (Future - Iteration 12+):** Object model calls
//! - Will support `Object.Method()` with type inference
//! - Requires type system beyond Unknown
//! - Target coverage: 14/14 diagnostics (100%)
//!
//! ## Reference
//! Ported from:
//! - MissedRequiredParameterDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - Adapted to use Rowan SyntaxNode and SymbolTree

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::traits::{MdObject, Module};
use ide_db::hir_def::{symbol_tree::MethodSymbol, ModuleId, Name};
use ide_db::metadata;
use syntax::{SyntaxKind, SyntaxNode};
use vfs::{FileId, VfsPath};

/// Main entry point for MissedRequiredParameter diagnostic.
///
/// Detects when required parameters are missing or empty in method calls.
///
/// ## Algorithm
///
/// **Phase 1 - Local Methods:**
/// 1. Find all ARG_LIST nodes in AST (indicate method calls)
/// 2. Skip qualified calls (Object.Method) - handled in Phase 2
/// 3. Extract method name and resolve using SymbolTree for current module
/// 4. Extract provided arguments (Boolean array: true if has value, false if empty)
/// 5. Check which required parameters are missing
/// 6. Create diagnostic with formatted message
///
/// **Phase 2 - CommonModule Calls:**
/// 1. Detect qualified calls (contain FIELD_EXPR)
/// 2. Extract module name and method name
/// 3. Load configuration metadata (if available)
/// 4. Find CommonModule by name in metadata
/// 5. Resolve CommonModule's file by URI or name matching
/// 6. Build SymbolTree for that file
/// 7. Look up method and check missing parameters
///
/// ## Performance
/// - O(n) AST traversal where n = node count
/// - O(1) method lookup via SymbolTree HashMap
/// - O(m) argument check where m = parameter count (typically < 10)
/// - Metadata loading: cached by Salsa (< 1ms after first load)
/// - Expected: < 20ms for 100K-node files with metadata
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::MissedRequiredParameter) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let module_id = ModuleId::new(ctx.file_id);
    let symbol_tree = ctx.db.symbol_tree(module_id);

    let mut diagnostics = Vec::new();

    // Load configuration metadata for Phase 2 (CommonModule calls)
    // This is cached by Salsa, so subsequent calls are very fast (< 1ms)
    let configuration = load_configuration_if_available(ctx);

    // Find all method calls by looking for ARG_LIST nodes
    // Method calls in BSL are parsed as EXPR containing IDENT + ARG_LIST
    for node in root.descendants() {
        // Look for ARG_LIST nodes - these indicate a method call
        if node.kind() != SyntaxKind::ARG_LIST {
            continue;
        }

        // Get the parent expression node
        let Some(call_expr) = node.parent() else {
            continue;
        };

        // Check if this is a qualified call (Object.Method or Module.Method)
        if is_qualified_call(&call_expr) {
            // Phase 2: Handle CommonModule.Method() calls
            if let Some(ref config) = configuration {
                if let Some(diagnostic) = check_qualified_call(ctx, &call_expr, config) {
                    diagnostics.push(diagnostic);
                }
            }
            // Skip if no configuration available (can't resolve CommonModule)
            continue;
        }

        // Phase 1: Handle local method calls (Method() without qualification)

        // Extract method name (should be an IDENT sibling before ARG_LIST)
        let Some(method_name) = get_method_name(&call_expr) else {
            continue;
        };

        // Resolve method in current module using SymbolTree
        let name = Name::new(&method_name);
        let Some(method) = symbol_tree.find_method(&name) else {
            continue;
        };

        // Extract provided arguments
        let provided_args = extract_arguments(&call_expr);

        // Check for missing required parameters
        let missing = check_missing_params(method, &provided_args);

        if !missing.is_empty() {
            diagnostics.push(create_diagnostic(&call_expr, &missing));
        }
    }

    diagnostics
}

/// Load configuration metadata if available.
///
/// Returns `None` if no configuration path is set in the context.
/// Configuration is cached by Salsa, so this is fast after first load.
fn load_configuration_if_available(
    ctx: &DiagnosticsContext,
) -> Option<std::sync::Arc<bsl_metadata::Configuration>> {
    let config_path = ctx.configuration_path.or(ctx.workspace_root)?;
    let config_path_str = config_path.to_string_lossy().to_string();
    let path_input = metadata::ConfigurationPathInput::new(ctx.db, config_path_str);
    Some(metadata::load_configuration(ctx.db, path_input))
}

/// Check a qualified method call (CommonModule.Method or Object.Method).
///
/// **Phase 2 Implementation:**
/// - Detects CommonModule.Method() calls
/// - Resolves CommonModule by name in metadata
/// - Finds the CommonModule's BSL file
/// - Builds SymbolTree and validates parameters
///
/// **Phase 3 (Future):**
/// - Will handle Object.Method() with type inference
/// - Requires type system beyond Unknown
fn check_qualified_call(
    ctx: &DiagnosticsContext,
    call_expr: &SyntaxNode,
    configuration: &bsl_metadata::Configuration,
) -> Option<Diagnostic> {
    // Extract module/object name and method name
    let (object_name, method_name) = extract_qualified_call(call_expr)?;

    // Phase 2: Try to resolve as CommonModule
    // Configuration.find_common_module() is case-insensitive
    let common_module = configuration.find_common_module(&object_name)?;

    // Find the CommonModule's file in the workspace
    let module_file_id = find_common_module_file(ctx, common_module)?;

    // Build SymbolTree for the CommonModule file
    let module_id = ModuleId::new(module_file_id);
    let module_symbol_tree = ctx.db.symbol_tree(module_id);

    // Look up the method in the CommonModule's SymbolTree
    let method_name_obj = Name::new(&method_name);
    let method = module_symbol_tree.find_method(&method_name_obj)?;

    // Extract provided arguments
    let provided_args = extract_arguments(call_expr);

    // Check for missing required parameters
    let missing = check_missing_params(method, &provided_args);

    if missing.is_empty() {
        None
    } else {
        Some(create_diagnostic(call_expr, &missing))
    }
}

/// Find the FileId for a CommonModule by resolving its URI through VFS.
///
/// ## Implementation
///
/// 1. Get CommonModule URI from metadata
/// 2. Build absolute path: workspace_root + URI
/// 3. Resolve FileId via Salsa query (cached!)
///
/// ## Performance
/// - First call: ~1ms (FileSet lookup + Salsa overhead)
/// - Cached calls: ~10μs (Salsa returns cached result)
fn find_common_module_file(
    ctx: &DiagnosticsContext,
    common_module: &bsl_metadata::CommonModule,
) -> Option<FileId> {
    let module_name = common_module.name();

    // Get the CommonModule's URI from metadata
    // Example: "CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl"
    let uri = common_module.uri()?;

    // Get the workspace root or configuration path to build absolute path
    let workspace_root = ctx.workspace_root.or(ctx.configuration_path)?;

    // Build full absolute path: workspace_root + URI
    let full_path = workspace_root.join(uri);
    let vfs_path = VfsPath::new(full_path.clone());

    // Get current file's SourceRoot
    let source_root_input = ctx.db.file_source_root_input(ctx.file_id);
    let source_root_id = source_root_input.source_root_id(ctx.db);

    // Resolve via Salsa query (CACHED!)
    let file_id = ctx.db.resolve_vfs_path(source_root_id, &vfs_path);

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

/// Extract which arguments have values from a call node.
///
/// Returns a Boolean vector where:
/// - `true` = argument has an expression
/// - `false` = argument is empty (between commas with no value)
///
/// ## Examples
/// - `Method()` → `[]`
/// - `Method(5)` → `[true]`
/// - `Method(, 2)` → `[false, true]`
/// - `Method(5, 2)` → `[true, true]`
/// - `Method(5,)` → `[true, false]`
/// - `Method(,)` → `[false, false]`
///
/// ## Algorithm
/// The parser creates an ARG_LIST node containing argument expressions and COMMA tokens.
/// We traverse this node to determine which positions have values.
fn extract_arguments(call_node: &SyntaxNode) -> Vec<bool> {
    let Some(arg_list) = call_node.descendants().find(|n| n.kind() == SyntaxKind::ARG_LIST) else {
        return Vec::new();
    };

    let mut args = Vec::new();
    let mut has_expr = false;

    for child in arg_list.children_with_tokens() {
        match child.kind() {
            SyntaxKind::COMMA => {
                // Comma separates arguments
                args.push(has_expr);
                has_expr = false;
            }
            SyntaxKind::L_PAREN | SyntaxKind::R_PAREN => {
                // Skip parentheses
            }
            kind if kind.is_trivia() => {
                // Skip whitespace and comments
            }
            _ => {
                // Any other node indicates an expression is present
                has_expr = true;
            }
        }
    }

    // Handle last argument (after last comma or only argument)
    // Only push if we're inside the argument list (has children)
    if arg_list.children().count() > 0 || has_expr {
        args.push(has_expr);
    }

    args
}

/// Extract the method name from a CALL_EXPR or CALL_STMT node.
///
/// Finds the first IDENT token, which represents the method name.
/// Returns None if no identifier is found.
fn get_method_name(call_node: &SyntaxNode) -> Option<String> {
    call_node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
}

/// Check if a call is qualified (contains a dot, like Object.Method or Module.Method).
///
/// Phase 1 only handles unqualified calls (direct method names).
/// Qualified calls are handled in Phase 2 (CommonModule) and Phase 3 (Object model).
///
/// ## Detection
///
/// The parser creates this structure for `Module.Method()`:
/// ```text
/// CALL_STMT
///   IDENT "Module"
///   DOT "."
///   IDENT "Method"
///   ARG_LIST
/// ```
///
/// Returns true if the call contains a DOT token (indicates qualification).
fn is_qualified_call(call_node: &SyntaxNode) -> bool {
    // Check for DOT token in children (not descendants - must be direct child)
    call_node.children_with_tokens().any(|child| child.kind() == SyntaxKind::DOT)
}

/// Extract object name and method name from a qualified call.
///
/// Parses call structures like:
/// ```text
/// CALL_STMT
///   IDENT (node)
///     IDENT "CommonModule" (token)  <- object name
///   DOT "."
///   IDENT (node)
///     IDENT "Method" (token)        <- method name
///   ARG_LIST
/// ```
///
/// Returns `Some((object_name, method_name))` or `None` if not a qualified call.
fn extract_qualified_call(call_node: &SyntaxNode) -> Option<(String, String)> {
    // Collect all IDENT tokens (from all descendants) but stop at ARG_LIST
    let mut idents: Vec<String> = Vec::new();
    let mut found_arg_list = false;

    for child in call_node.children_with_tokens() {
        if child.kind() == SyntaxKind::ARG_LIST {
            found_arg_list = true;
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

    if !found_arg_list || idents.len() < 2 {
        return None;
    }

    // Method name is the last identifier
    let method_name = idents.pop()?;

    // Object/module name is the second-to-last identifier
    let object_name = idents.pop()?;

    Some((object_name, method_name))
}

/// Check which required parameters are missing from a method call.
///
/// Returns a vector of parameter names that are required but not provided.
///
/// ## Rules
/// - Parameters with `has_default == true` are optional (skip)
/// - Parameters with `has_default == false` are required (check)
/// - A parameter is missing if:
///   - Index >= provided_args.len() (not enough arguments), OR
///   - provided_args[i] == false (empty argument like `, ,`)
///
/// ## Example
/// ```bsl
/// Функция Test(A, B = 1, C)
///     // A and C are required (no default)
///     // B is optional (has default)
/// КонецФункции
///
/// Test(5)      // Missing C → returns ["C"]
/// Test(, 2, 3) // Missing A → returns ["A"]
/// Test()       // Missing A, C → returns ["A", "C"]
/// ```
fn check_missing_params(method: &MethodSymbol, provided_args: &[bool]) -> Vec<String> {
    let mut missing = Vec::new();

    for (i, param) in method.params.iter().enumerate() {
        // Skip optional parameters (have default value)
        if param.has_default {
            continue;
        }

        // Check if parameter is missing or empty
        let is_missing = i >= provided_args.len() || !provided_args[i];

        if is_missing {
            missing.push(param.name.as_str().to_string());
        }
    }

    missing
}

/// Create a diagnostic for missing required parameters.
///
/// ## Message Format
/// Matches Java implementation exactly:
/// - Single param: "Specify a required parameter 'ParamName'"
/// - Multiple params: "Specify a required parameter 'Param1', 'Param2'"
///
/// Each parameter name is quoted and comma-separated.
///
/// ## Range Calculation
///
/// For **qualified calls** (`Module.Method()`): Range from method name to end of ARG_LIST
/// - Java compatibility: `ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(1)` → range starts at `ВерсионированиеПриЗаписи`
///
/// For **local calls** (`Method()`): Range of entire call expression
fn create_diagnostic(call_node: &SyntaxNode, missing_params: &[String]) -> Diagnostic {
    let param_list =
        missing_params.iter().map(|name| format!("'{}'", name)).collect::<Vec<_>>().join(", ");

    let message = format!("Specify a required parameter {}", param_list);

    // Calculate range for Java compatibility
    let is_qualified = is_qualified_call(call_node);

    let range = if is_qualified {
        // For qualified calls, find the method name (last IDENT before ARG_LIST)
        // and return range from method name to end of call
        let mut last_ident_start = None;

        for child in call_node.children_with_tokens() {
            if child.kind() == SyntaxKind::ARG_LIST {
                break;
            }

            // Find last IDENT (either as node or token) before ARG_LIST
            if child.kind() == SyntaxKind::IDENT {
                if let Some(node) = child.as_node() {
                    // IDENT wrapped in node - get first token
                    if let Some(token) = node.first_token() {
                        last_ident_start = Some(token.text_range().start());
                    }
                } else if let Some(token) = child.as_token() {
                    // Direct IDENT token
                    last_ident_start = Some(token.text_range().start());
                }
            }
        }

        if let Some(start) = last_ident_start {
            syntax::TextRange::new(start, call_node.text_range().end())
        } else {
            // Fallback to full range if we can't find method name
            call_node.text_range()
        }
    } else {
        // For local calls, use full range
        call_node.text_range()
    };

    Diagnostic {
        code: DiagnosticCode::MissedRequiredParameter,
        message,
        severity: Severity::Error,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range_multiline;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::sync::Arc;
    use test_fixture::Fixture;

    #[test]
    fn test_missed_required_parameter_simple() {
        let code = r#"
Процедура Тест()
    Результат = Сложение(, 2);
КонецПроцедуры

Функция Сложение(Левый, Правый)
    Возврат Левый + Правый;
КонецФункции
"#;
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &crate::DiagnosticsConfig::default(),
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);

        // Should detect 1 diagnostic: Сложение(, 2) missing 'Левый'
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
        assert!(diagnostics[0].message.contains("Левый"));
    }

    #[test]
    fn test_missed_required_parameter_local_methods() {
        let code = include_str!("../../test_data/MissedRequiredParameterDiagnostic.bsl");
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &crate::DiagnosticsConfig::default(),
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);

        // Phase 1: expect 5 diagnostics for local methods only
        // Lines 25-29 (CommonModule calls) are skipped until Phase 2
        assert_eq!(diagnostics.len(), 5, "Expected 5 diagnostics for local method calls");

        // Verify exact positions match Java implementation (0-indexed)
        // Line 3: Сложение(, 2) - Missing 'Левый'
        assert_diagnostic_range_multiline(code, &diagnostics[0], 2, 16, 2, 29);

        // Line 9: Сложение(5) - Missing 'Правый'
        assert_diagnostic_range_multiline(code, &diagnostics[1], 8, 16, 8, 27);

        // Line 15: Сложение() - Missing 'Левый', 'Правый'
        assert_diagnostic_range_multiline(code, &diagnostics[2], 14, 16, 14, 26);

        // Line 18: Сложение(,) - Missing 'Левый', 'Правый'
        assert_diagnostic_range_multiline(code, &diagnostics[3], 17, 13, 17, 24);

        // Line 19: Менеджер("Справочник") - Missing 'Вид' (Тип has default)
        assert_diagnostic_range_multiline(code, &diagnostics[4], 18, 13, 18, 35);
    }

    #[test]
    fn test_optional_parameters_not_required() {
        let code = r#"
Процедура Тест()
    Инкремент(5);
КонецПроцедуры

Функция Инкремент(Значение, Приращение = 1)
    Возврат Значение + Приращение;
КонецФункции
"#;
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &crate::DiagnosticsConfig::default(),
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);

        // Should not trigger - Приращение has default value
        assert_eq!(diagnostics.len(), 0, "Optional parameters should not trigger diagnostic");
    }

    #[test]
    fn test_extra_parameters_allowed() {
        let code = r#"
Процедура Тест()
    Результат = Сложение(1, 2, 3, 4);
КонецПроцедуры

Функция Сложение(A, B)
    Возврат A + B;
КонецФункции
"#;
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &crate::DiagnosticsConfig::default(),
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);

        // BSL allows extra parameters
        assert_eq!(diagnostics.len(), 0, "Extra parameters should be allowed");
    }

    #[test]
    fn test_qualified_calls_skipped_phase1() {
        let code = r#"
Процедура Тест()
    ОбщийМодуль.Метод();
    Объект.Метод(1);
КонецПроцедуры

Функция Метод(A, B)
    Возврат A + B;
КонецФункции
"#;
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &crate::DiagnosticsConfig::default(),
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);

        // Phase 1: Qualified calls are skipped (no metadata available)
        assert_eq!(diagnostics.len(), 0, "Qualified calls should be skipped without metadata");
    }

    #[test]
    fn test_qualified_call_detection() {
        // Test BSL parser's qualified call structure (DOT token, not FIELD_EXPR)
        let code = "ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(1)";
        let parse = parser::parse(code);
        let root = parse.syntax_node();

        // Find CALL_STMT node
        let call_stmt = root
            .descendants()
            .find(|n| n.kind() == syntax::SyntaxKind::CALL_STMT)
            .expect("Should have CALL_STMT");

        // Verify qualified call detection
        assert!(is_qualified_call(&call_stmt), "Should detect qualified call with DOT token");

        // Verify extraction
        let (module, method) =
            extract_qualified_call(&call_stmt).expect("Should extract qualified call components");

        assert_eq!(module, "ПервыйОбщийМодуль");
        assert_eq!(method, "ВерсионированиеПриЗаписи");
    }

    #[test]
    fn test_phase2_common_module_with_metadata() {
        use ide_db::base_db::{SourceRoot, SourceRootId};
        use std::path::Path;
        use vfs::{FileSet, VfsPath};

        // Use the real metadata fixtures directory
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        // Verify fixtures exist
        let fixtures_path = Path::new(fixtures_dir);
        assert!(fixtures_path.exists(), "Metadata fixtures directory must exist: {}", fixtures_dir);

        // Test code that calls ПервыйОбщийМодуль.ВерсионированиеПриЗаписи()
        let test_code = r#"
Процедура Версионирование()
    ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(1); // Missing 'Отказ'
    ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(2,); // Missing 'Отказ'
    ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(); // Missing 'Источник', 'Отказ'
    Сообщить(ПервыйОбщийМодуль.ВерсионированиеПриЗаписи()); // Missing both
КонецПроцедуры
"#;

        // Read the CommonModule file from fixtures
        let common_module_path = fixtures_path
            .join("CommonModules")
            .join("ПервыйОбщийМодуль")
            .join("Ext")
            .join("Module.bsl");

        let common_module_code = std::fs::read_to_string(&common_module_path)
            .expect("Failed to read CommonModule fixture");

        // Set up database with proper SourceRoot and FileSet
        let mut db = RootDatabaseImpl::new();

        // Create FileIds for our files
        let test_file_id = vfs::FileId(0);
        let common_module_file_id = vfs::FileId(1);

        // Set file contents
        db.set_file_text(test_file_id, test_code);
        db.set_file_text(common_module_file_id, &common_module_code);

        // Create FileSet with proper path mappings
        let mut file_set = FileSet::new();

        // Add test file with a virtual path
        let test_vfs_path = VfsPath::new(fixtures_path.join("test.bsl"));
        file_set.insert(test_file_id, test_vfs_path);

        // Add CommonModule file with its REAL path (critical for resolution!)
        let common_module_vfs_path = VfsPath::new(common_module_path.clone());
        file_set.insert(common_module_file_id, common_module_vfs_path);

        // Create SourceRoot with the FileSet
        let source_root = SourceRoot::new_local(file_set);
        let source_root_id = SourceRootId(0);

        // Register SourceRoot in database
        db.set_source_root(source_root_id, source_root);

        // Link files to SourceRoot
        db.set_file_source_root(test_file_id, source_root_id);
        db.set_file_source_root(common_module_file_id, source_root_id);

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        // Create configuration_path_input for metadata loading
        let config_path_str = fixtures_path.to_string_lossy().to_string();
        let path_input = metadata::ConfigurationPathInput::new(db.as_ref(), config_path_str);

        // Set up context with workspace_root pointing to fixtures directory
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &crate::DiagnosticsConfig::default(),
            file_id: test_file_id,
            workspace_root: Some(fixtures_path),
            configuration_path: Some(fixtures_path),
            configuration_path_input: Some(path_input),
        };

        let diagnostics = check(&ctx);

        // Phase 2: Should detect missing parameters in CommonModule calls
        // Expected: 4 diagnostics (one per call to ПервыйОбщийМодуль.ВерсионированиеПриЗаписи)
        assert_eq!(
            diagnostics.len(),
            4,
            "Expected 4 diagnostics for missing CommonModule parameters"
        );

        // Verify all diagnostics mention the missing parameters
        for diag in &diagnostics {
            assert!(
                diag.message.contains("Источник") || diag.message.contains("Отказ"),
                "Diagnostic should mention missing parameters: {}",
                diag.message
            );
        }
    }

    #[test]
    fn test_full_java_compatibility() {
        // Comprehensive integration test using the full Java test fixture
        // Tests both Phase 1 (local methods) and Phase 2 (CommonModule calls)
        use ide_db::base_db::{SourceRoot, SourceRootId};
        use std::path::{Path, PathBuf};
        use std::rc::Rc;
        use vfs::{FileSet, VfsPath};

        let code = include_str!("../../test_data/MissedRequiredParameterDiagnostic.bsl");

        // Use real metadata fixtures for Phase 2
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");
        let fixtures_path = Path::new(fixtures_dir);

        // Set up database with both test file and CommonModule file
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let test_file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        db.set_file_text(test_file_id, code);

        // Read CommonModule file
        let common_module_path = fixtures_path
            .join("CommonModules")
            .join("ПервыйОбщийМодуль")
            .join("Ext")
            .join("Module.bsl");
        let common_module_code = std::fs::read_to_string(&common_module_path)
            .expect("CommonModule fixture should exist");

        let common_module_file_id = vfs::FileId(1);
        db.set_file_text(common_module_file_id, &common_module_code);

        // Set up FileSet with proper paths
        let test_vfs_path = VfsPath::new(PathBuf::from("/test.bsl"));
        let common_module_vfs_path = VfsPath::new(
            fixtures_path
                .join("CommonModules")
                .join("ПервыйОбщийМодуль")
                .join("Ext")
                .join("Module.bsl"),
        );

        let mut file_set = FileSet::new();
        file_set.insert(test_file_id, test_vfs_path);
        file_set.insert(common_module_file_id, common_module_vfs_path);

        let source_root = SourceRoot::new_local(file_set);
        let source_root_id = SourceRootId(0);
        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(test_file_id, source_root_id);
        db.set_file_source_root(common_module_file_id, source_root_id);

        let db = Rc::new(db) as Rc<dyn RootDatabase>;

        // Create configuration path input for metadata
        let config_path_str = fixtures_path.to_string_lossy().to_string();
        let path_input = metadata::ConfigurationPathInput::new(db.as_ref(), config_path_str);

        let config = crate::DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id: test_file_id,
            workspace_root: Some(fixtures_path),
            configuration_path: Some(fixtures_path),
            configuration_path_input: Some(path_input),
        };

        let diagnostics = check(&ctx);

        // Expected diagnostics:
        // Phase 1 (local methods): 5 diagnostics
        //   - Line 3: Сложение(, 2) - missing 'Левый'
        //   - Line 9: Сложение(5) - missing 'Правый'
        //   - Line 15: Сложение() - missing 'Левый', 'Правый'
        //   - Line 18: Сложение(,) - missing 'Левый', 'Правый'
        //   - Line 19: Менеджер("Справочник") - missing 'Вид'
        //
        // Phase 2 (CommonModule): 4 diagnostics
        //   - Line 25: ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(1) - missing 'Отказ'
        //   - Line 26: ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(2,) - missing 'Отказ'
        //   - Line 27: ПервыйОбщийМодуль.ВерсионированиеПриЗаписи() - missing both
        //   - Line 28: Сообщить(ПервыйОбщийМодуль.ВерсионированиеПриЗаписи()) - missing both
        //
        // Phase 3 (NOT implemented): Would add 5 more diagnostics for object model calls
        //
        // Total: 9 diagnostics (5 Phase 1 + 4 Phase 2)

        assert_eq!(
            diagnostics.len(),
            9,
            "Expected 9 diagnostics (5 Phase 1 + 4 Phase 2), got {}",
            diagnostics.len()
        );

        // Verify diagnostic messages contain the expected parameter names
        let mut phase1_count = 0;
        let mut phase2_count = 0;

        for diag in &diagnostics {
            if diag.message.contains("Левый")
                || diag.message.contains("Правый")
                || diag.message.contains("Вид")
            {
                phase1_count += 1;
            } else if diag.message.contains("Источник") || diag.message.contains("Отказ")
            {
                phase2_count += 1;
            }
        }

        assert_eq!(phase1_count, 5, "Expected 5 Phase 1 diagnostics");
        assert_eq!(phase2_count, 4, "Expected 4 Phase 2 diagnostics");
    }

    #[test]
    fn test_diagnostic_positions_java_compatibility() {
        // Test exact diagnostic positions match Java implementation
        // This ensures our ranges are compatible with bsl-language-server
        use crate::test_utils::assert_diagnostic_range_multiline;
        use ide_db::base_db::{SourceRoot, SourceRootId};
        use std::path::{Path, PathBuf};
        use std::rc::Rc;
        use vfs::{FileSet, VfsPath};

        let code = include_str!("../../test_data/MissedRequiredParameterDiagnostic.bsl");

        // Set up metadata for Phase 2
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");
        let fixtures_path = Path::new(fixtures_dir);

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let test_file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        db.set_file_text(test_file_id, code);

        // Set up CommonModule file
        let common_module_path = fixtures_path
            .join("CommonModules")
            .join("ПервыйОбщийМодуль")
            .join("Ext")
            .join("Module.bsl");
        let common_module_code = std::fs::read_to_string(&common_module_path)
            .expect("CommonModule fixture should exist");

        let common_module_file_id = vfs::FileId(1);
        db.set_file_text(common_module_file_id, &common_module_code);

        let test_vfs_path = VfsPath::new(PathBuf::from("/test.bsl"));
        let common_module_vfs_path = VfsPath::new(
            fixtures_path
                .join("CommonModules")
                .join("ПервыйОбщийМодуль")
                .join("Ext")
                .join("Module.bsl"),
        );

        let mut file_set = FileSet::new();
        file_set.insert(test_file_id, test_vfs_path);
        file_set.insert(common_module_file_id, common_module_vfs_path);

        let source_root = SourceRoot::new_local(file_set);
        let source_root_id = SourceRootId(0);
        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(test_file_id, source_root_id);
        db.set_file_source_root(common_module_file_id, source_root_id);

        let db = Rc::new(db) as Rc<dyn RootDatabase>;

        let config_path_str = fixtures_path.to_string_lossy().to_string();
        let path_input = metadata::ConfigurationPathInput::new(db.as_ref(), config_path_str);

        let config = crate::DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id: test_file_id,
            workspace_root: Some(fixtures_path),
            configuration_path: Some(fixtures_path),
            configuration_path_input: Some(path_input),
        };

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 9, "Should have 9 total diagnostics");

        // Sort diagnostics by position for predictable testing
        let mut diagnostics = diagnostics;
        diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));

        // Phase 1 diagnostics (local methods) - verify key positions from Java test
        // Line 3 (0-indexed line 2): Сложение(, 2) - Range(2, 16, 2, 29)
        let diag_line3 = diagnostics
            .iter()
            .find(|d| d.message.contains("Левый") && d.message.contains("Specify"))
            .expect("Should find diagnostic for missing 'Левый'");
        assert_diagnostic_range_multiline(code, diag_line3, 2, 16, 2, 29);

        // Line 9 (0-indexed line 8): Сложение(5) - Range(8, 16, 8, 27)
        let diag_line9 = diagnostics
            .iter()
            .find(|d| d.message.contains("Правый") && !d.message.contains("Левый"))
            .expect("Should find diagnostic for missing 'Правый' only");
        assert_diagnostic_range_multiline(code, diag_line9, 8, 16, 8, 27);

        // Line 19 (0-indexed line 18): Менеджер("Справочник") - Range(18, 13, 18, 35)
        let diag_line19 = diagnostics
            .iter()
            .find(|d| d.message.contains("Вид"))
            .expect("Should find diagnostic for missing 'Вид'");
        assert_diagnostic_range_multiline(code, diag_line19, 18, 13, 18, 35);

        // Phase 2 diagnostics (CommonModule) - verify positions
        // Line 25 (0-indexed line 24): ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(1)
        // Range(24, 22, 24, 49)
        let phase2_diagnostics: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("Отказ") || d.message.contains("Источник"))
            .collect();
        assert_eq!(phase2_diagnostics.len(), 4, "Should have 4 Phase 2 diagnostics");

        // Find diagnostic on line 24 (first CommonModule call with missing 'Отказ')
        let diag_line25 = phase2_diagnostics
            .iter()
            .find(|d| {
                let (start_line, _, _, _) = crate::test_utils::range_to_line_col(code, d.range);
                start_line == 24 && d.message.contains("Отказ") && !d.message.contains("Источник")
            })
            .expect("Should find diagnostic on line 24 for missing 'Отказ'");
        assert_diagnostic_range_multiline(code, diag_line25, 24, 22, 24, 49);
    }
}
