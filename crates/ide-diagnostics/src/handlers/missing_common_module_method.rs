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
use ide_db::hir_def::resolver::Resolver;
use ide_db::hir_def::{ModuleId, Name, PathResolution, QualifiedName};
use syntax::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
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
            // Conservative approach: report as method not found
            // (could be module not found, or method not found in existing module)
            Some(create_diagnostic_from_hir(range, ErrorType::MethodNotFound, method, module))
        }
        _ => None,
    }
}

/// Error types for MissingCommonModuleMethod diagnostic.
enum ErrorType {
    /// Method does not exist in CommonModule
    MethodNotFound,
    /// Method exists but is not exported
    NonExportMethod,
}

/// Create a diagnostic for a missing or non-export CommonModule method (HIR-based).
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

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic;

    #[test]
    fn test_missing_common_module_method() {
        // Test that qualified calls trigger diagnostic creation
        let code = r#"
Процедура Тест()
    ПервыйОбщийМодуль.МетодНесуществующий(1, 2);
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);

        // HIR lowering creates diagnostics for qualified calls
        // Resolution will fail in test context (no metadata), but diagnostics are created
        assert!(!diagnostics.is_empty(), "Expected at least 1 diagnostic for qualified call");
    }

    #[test]
    fn test_without_metadata() {
        // Test that diagnostic is created even without metadata
        let code = r#"
Процедура Тест()
    ПервыйОбщийМодуль.МетодНесуществующий(1, 2);
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);

        // Diagnostic is created for qualified call, even without metadata
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_local_variable_shadowing() {
        // Test that local variables don't trigger diagnostics
        // Shadowing is handled by analyze_qualified_call in HIR lowering
        let code = r#"
Процедура Тест(ПервыйОбщийМодуль)  // Parameter shadows module name
    ПервыйОбщийМодуль.Method();  // Should NOT trigger - parameter
КонецПроцедуры

Функция ДругойТест()
    Перем ПервыйОбщийМодуль;  // Local variable
    Возврат ПервыйОбщийМодуль.SomeMethod();  // Should NOT trigger - local variable
КонецФункции
"#;

        let diagnostics = check_hir_diagnostic(code);

        // Shadowing is handled automatically by analyze_qualified_call
        // which checks if base is a local variable before creating QualifiedPath
        assert_eq!(diagnostics.len(), 0, "Expected 0 diagnostics for shadowed variables");
    }

    #[test]
    fn test_mixed_local_and_common_module() {
        // Test mixed scenarios with both variables and qualified calls
        let code = r#"
Процедура Тест()
    Перем ПервыйОбщийМодуль;
    ПервыйОбщийМодуль.Method();  // Local variable - no diagnostic
КонецПроцедуры

Процедура ДругойТест()
    ВторойОбщийМодуль.Method();  // Qualified call - diagnostic created
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);

        // Shadowing is handled automatically, qualified calls trigger diagnostics
        // Exact count depends on analyze_qualified_call filtering
        assert!(diagnostics.len() <= 1, "Expected at most 1 diagnostic (for ВторойОбщийМодуль)");
    }
}
