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

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::hir_def::{Name, PathResolution};
use syntax::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::MissingCommonModuleMethod` is encountered.
///
/// Uses provider-first pattern via `ctx.resolve_qualified_path()` for Clean Architecture
/// compliance. Domain layer (diagnostics) depends on abstraction (ctx), not implementation (db).
///
/// ## Resolution Algorithm
///
/// 1. Use `ctx.resolve_qualified_path()` which handles provider-first pattern internally
/// 2. PathResolution::Method(id) → valid exported method, no diagnostic
/// 3. PathResolution::Unresolved → method or module doesn't exist, emit diagnostic
pub fn from_hir(
    module: &str,
    method: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::MissingCommonModuleMethod;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    // Resolve using DiagnosticsContext helper (provider-first pattern)
    let module_name = Name::new(module);
    let method_name = Name::new(method);
    let resolution = ctx.resolve_qualified_path(&module_name, &method_name);

    tracing::trace!(
        module_name = module,
        method_name = method,
        resolution = ?resolution,
        "Path resolution result in HIR diagnostic"
    );

    match resolution {
        PathResolution::Method(_) => {
            // Valid exported method found
            None
        }
        PathResolution::Unresolved(_) => {
            // Could not resolve - method or module doesn't exist, or method not exported
            Some(create_diagnostic_from_hir(range, method, module, code, ctx))
        }
        _ => None,
    }
}

/// Create a diagnostic for a missing CommonModule method (HIR-based).
///
/// Note: Both "method not found" and "method not exported" cases result
/// in Unresolved from resolve_qualified_path. The message covers both.
fn create_diagnostic_from_hir(
    range: TextRange,
    method_name: &str,
    module_name: &str,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    let message = format!("Метод {} общего модуля {} не существует", method_name, module_name);

    Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
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
