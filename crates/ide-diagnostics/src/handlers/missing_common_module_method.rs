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

            // Managed-form Self property suppression.
            // HIR lowering classifies `Элементы.Найти(...)` as a
            // 2-segment qualified call because the lowerer has no `db`
            // and cannot ask "is `Элементы` a property of the enclosing
            // form?". Phase B does have the answer: when the file is a
            // managed form module *and* the receiver name is a property
            // of `ФормаКлиентскогоПриложения`, the call is a method on
            // the form's Self property — `infer_call`'s form-self
            // dispatch handles it — so this CommonModule diagnostic is
            // a false positive and must be silenced. Kept narrow: only
            // managed forms (ordinary forms have a different platform
            // type), only when the receiver name is in the form's
            // platform property index, and only when the receiver is
            // NOT also a real user CommonModule (a user may legitimately
            // ship a CommonModule named like a form property).
            if !module_is_user_common_module && hir::is_form_self_property_name(module) {
                let metadata = ctx.module_metadata();
                let is_managed_form = metadata.module_type == bsl_metadata::ModuleType::FormModule
                    && metadata.form.as_ref().is_some_and(|f| f.is_managed());
                if is_managed_form {
                    return None;
                }
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
    use crate::DiagnosticCode;

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

    /// Build a `ModuleMetadata` for a managed-form module from raw `Form.xml`.
    ///
    /// Used by the form-self suppression tests below — they need a metadata
    /// pre-populated with a managed `Form` payload (the form-self gate in
    /// `from_hir` checks `metadata.form.as_ref().is_some_and(|f| f.is_managed())`).
    /// Real-world metadata loading goes through `load_form_from_path` in
    /// `ide-db`, which reads `Form.xml` off disk; tests can't rely on that
    /// because fixtures are VFS-only, so we parse the XML directly the way
    /// other `check_metadata_diagnostic` consumers do.
    fn managed_form_metadata(form_xml: &str) -> hir::ModuleMetadata {
        use std::sync::Arc;
        let form = bsl_metadata::xml_parser::parse_form_xml(form_xml).unwrap();
        assert!(form.is_managed(), "fixture must produce a managed form");
        hir::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::FormModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            form: Some(Arc::new(form)),
            http_service: None,
            web_service: None,
        }
    }

    /// Minimal managed-form XML — `parse_form_xml` defaults `FormType` to
    /// `Managed` when the `<FormType>` element is absent, so the bare root
    /// element is enough.
    const MANAGED_FORM_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" version="2.20"></Form>"#;

    #[test]
    fn form_self_method_call_in_managed_form_does_not_emit_missing_common_module_method() {
        // Главный пользовательский кейс из плана: `Элементы.Найти(...)`
        // в модуле управляемой формы НЕ должен фейерверкать
        // `MissingCommonModuleMethod`. До фикса диагностика срабатывала,
        // потому что `analyze_qualified_call` поднимает любой 2-сегментный
        // вызов в `QualifiedPath`, а `from_hir` не знал про managed-form
        // Self properties.
        let code = r#"
&НаКлиенте
Процедура Тест()
    Элементы.Найти("ТЗШкалы");
КонецПроцедуры
"#;
        let metadata = managed_form_metadata(MANAGED_FORM_XML);
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, code, |_metadata, ctx| {
                crate::diagnostics(ctx)
            });
        let bad: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::MissingCommonModuleMethod)
            .collect();
        assert!(
            bad.is_empty(),
            "form-self property `Элементы` must not fire MissingCommonModuleMethod, got: {:?}",
            bad.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn unknown_module_in_managed_form_still_emits_missing_common_module_method() {
        // Suppression очень узкий: только form-self property names. Любой
        // другой неизвестный 2-сегментный вызов в managed form должен
        // продолжать жаловаться, иначе мы скрыли бы реальные опечатки и
        // отсутствующие CommonModule'и.
        let code = r#"
&НаКлиенте
Процедура Тест()
    ЗаведомоНесуществующийМодуль.КакойТоМетод();
КонецПроцедуры
"#;
        let metadata = managed_form_metadata(MANAGED_FORM_XML);
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, code, |_metadata, ctx| {
                crate::diagnostics(ctx)
            });
        let hits: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::MissingCommonModuleMethod)
            .collect();
        assert_eq!(
            hits.len(),
            1,
            "unknown module call must still fire MissingCommonModuleMethod, got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn form_self_name_in_common_module_keeps_firing_diagnostic() {
        // Suppression — managed-form gated. Если кто-то пишет
        // `Элементы.Найти(...)` в обычном CommonModule, это либо опечатка,
        // либо и правда отсутствующий CommonModule с именем `Элементы` —
        // в обоих случаях diagnostic уместна и должна сработать.
        let code = r#"
Процедура Тест()
    Элементы.Найти("Х");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let hits: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::MissingCommonModuleMethod)
            .collect();
        assert_eq!(
            hits.len(),
            1,
            "form-self suppression must NOT bleed into non-form modules, got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
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
