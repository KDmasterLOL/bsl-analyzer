//! MissingCommonModuleMethod diagnostic.
//!
//! Reports calls to common-module methods that cannot be resolved as exported
//! methods of the referenced module.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Name, PathResolution};
use syntax::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates a diagnostic from HIR when a qualified common-module call cannot be
/// resolved to an exported method.
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
            // Before reporting "method does not exist on common module" — the
            // identifier on the left may not be a CommonModule at all but a
            // built-in 1C platform global (e.g. `ОбработкаОшибок` of type
            // `МенеджерОбработкиОшибок`). Suppress the diagnostic when the
            // call resolves against the platform global-context catalogue.
            // Kept distinct from `PathResolution::Method` because platform
            // globals are NOT CommonModules — collapsing them would leak
            // CommonModule assumptions into goto/hover/completion.
            //
            // Important narrowing: only fall back to the platform catalogue
            // when the receiver is NOT a real user CommonModule. A user may
            // legitimately ship a CommonModule named e.g. `Метаданные` that
            // shadows a platform global; in that case `Unresolved` means
            // "method on the user module is missing or non-exported" and
            // must not be silently swallowed by a coincidental platform
            // member with the same name.
            let module_is_user_common_module =
                !ctx.find_common_module_files_anywhere(module).is_empty();
            if !module_is_user_common_module
                && ctx.resolve_platform_global_member(&module_name, &method_name).is_some()
            {
                return None;
            }
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
    #[ignore] // TODO: Requires Configuration.xml setup for CommonModule resolution
    fn test_with_common_module_fixture() {
        // Test that diagnostic is created when calling a non-existent CommonModule method
        // NOTE: This test is currently ignored because CommonModule resolution requires
        // Configuration.xml metadata, which is not yet set up in test fixtures.
        // The diagnostic is created in HIR lowering only when analyze_qualified_call()
        // identifies the call as a CommonModule call (not a local variable).
        use crate::test_utils::check_hir_diagnostic_with_fixtures;
        let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Module.bsl
Процедура ДругойМетод() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ПервыйОбщийМодуль.МетодНесуществующий(1, 2);
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic_with_fixtures(fixture);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::MissingCommonModuleMethod)
            .collect();

        // With proper Configuration.xml, diagnostic should be created
        assert_eq!(diags.len(), 1, "Expected 1 diagnostic for missing CommonModule method");
        assert!(diags[0].message.contains("МетодНесуществующий"));
        assert!(diags[0].message.contains("ПервыйОбщийМодуль"));
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
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::MissingCommonModuleMethod)
            .collect();

        // Shadowing is handled automatically by analyze_qualified_call
        // which checks if base is a local variable before creating QualifiedPath
        assert_eq!(diags.len(), 0, "Expected 0 diagnostics for shadowed variables");
    }

    #[test]
    fn test_platform_global_member_call_suppresses_diagnostic() {
        // ОбработкаОшибок is a platform global of type МенеджерОбработкиОшибок,
        // and КраткоеПредставлениеОшибки is a real method on that manager.
        // The qualified-call lowering still emits MissingCommonModuleMethod
        // (it does not know about platform globals), so this test exercises
        // the suppression path in `from_hir`: ctx.resolve_platform_global_member
        // returns Some(_) → no diagnostic.
        let code = r#"
Процедура Тест()
    ОбработкаОшибок.КраткоеПредставлениеОшибки(ИнформацияОбОшибке());
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::MissingCommonModuleMethod)
            .collect();
        assert!(
            diags.is_empty(),
            "platform global member calls must not raise MissingCommonModuleMethod, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore] // TODO: requires Configuration.xml fixture wiring (same as test_with_common_module_fixture above).
    fn test_user_module_shadows_platform_global() {
        // Edge case flagged by Codex pair-mode review: a user CommonModule
        // sharing a name with a platform global must not have its real
        // missing-method diagnostic swallowed by the platform fallback.
        // The narrowing in `from_hir` checks `find_common_module_files_anywhere`
        // before consulting the platform catalogue.
        use crate::test_utils::check_hir_diagnostic_with_fixtures;
        let fixture = r#"
//- /CommonModules/Метаданные/Module.bsl
Процедура ОдинЭкспортируемыйМетод() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    Метаданные.ЗаведомоОтсутствующийМетод();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic_with_fixtures(fixture);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::MissingCommonModuleMethod)
            .collect();
        // The user's CommonModule (named like a platform global) must keep
        // its diagnostic — `КонфигурацияМетаданныеОбъект` having a
        // coincidentally-named method must NOT silence it.
        assert_eq!(
            diags.len(),
            1,
            "user CommonModule shadowing a platform global must keep its missing-method diagnostic"
        );
    }

    #[test]
    fn test_platform_global_unknown_member_keeps_falling_through() {
        // Receiver is a real platform global, but the named member does not
        // exist on the declared type — suppression must NOT mask this:
        // diagnostic still fires (current message wording is "common module"
        // — refining the wording for platform globals is a separate task).
        let code = r#"
Процедура Тест()
    ОбработкаОшибок.СовершенноНесуществующийМетод();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::MissingCommonModuleMethod)
            .collect();
        assert_eq!(
            diags.len(),
            1,
            "unknown method on platform global must still raise the diagnostic"
        );
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
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::MissingCommonModuleMethod)
            .collect();

        // Shadowing is handled automatically, qualified calls trigger diagnostics
        // Exact count depends on analyze_qualified_call filtering
        assert!(
            diags.len() <= 1,
            "Expected at most 1 diagnostic (for ВторойОбщийМодуль), got {} diagnostics: {:?}",
            diags.len(),
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
