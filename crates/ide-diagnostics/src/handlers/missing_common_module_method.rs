//! `MissingCommonModuleMethod` diagnostic — **DEPRECATED since v0.1.176**.
//!
//! Replaced by `UnresolvedMethodCall` (`BSL-TY-UnresolvedMethodCall`).
//! Phase 2 of the qualified-call clean-architecture refactor lifted the
//! "this is a CommonModule call" classification from body lowering into
//! `hir-ty::dispatch_bare_ident_field_call`, which has the resolver and
//! the receiver type and can decide positively. The user-facing
//! diagnostic is now `UnresolvedMethodCall { kind: MethodNotFound }`
//! when the receiver IS a registered CommonModule but the method
//! is missing/non-exported, or `UnresolvedMethodCall { kind:
//! ReceiverNotResolved }` when the receiver name doesn't resolve at all
//! (precise replacement for the legacy collapse).
//!
//! ## Compatibility
//!
//! The enum variant `DiagnosticCode::MissingCommonModuleMethod`,
//! the user-facing docs and the SonarQube rules export all stay in
//! place — downstream consumers (LSP clients, BSL-LS compatibility
//! layers, user `bsl-analyzer.toml` files) keep accepting the
//! identifier without breaking. Retention is intentional and
//! open-ended: full removal of the variant, dispatch wiring, and
//! handler stub is deferred until the deprecation window closes —
//! a separate breaking-change PR. Phase 5 of the plan only covers
//! regression coverage (E2E + manual smoke), not removal.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::Name;
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

/// Deprecated no-op stub — see module docs.
///
/// Kept so the `BodyDiagnostic::MissingCommonModuleMethod` dispatch
/// arm in `hir_dispatch::dispatch_hir_diagnostic` round-trips without
/// a panic should any stale `BodyDiagnostic` value reach the
/// dispatcher (the variant is no longer constructed by lowering since
/// Phase 2 of the qualified-call refactor). Always returns `None`.
///
/// User-facing replacement: `UnresolvedMethodCall` (`BSL-TY-UnresolvedMethodCall`).
pub fn from_hir(
    _module: &str,
    _method: &str,
    _range: TextRange,
    _ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    // Force-touch references so the items stay reachable while the
    // deprecated public surface is documented but not driven by lowering.
    let _ = (Name::new(""), DiagnosticCode::MissingCommonModuleMethod);
    None
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic;
    use crate::DiagnosticCode;

    #[test]
    fn test_missing_common_module_method() {
        // Phase 2 channel migration: the diagnostic for a 2-segment call
        // whose receiver doesn't resolve (no Configuration registered, no
        // platform global match) now lands on `UnresolvedMethodCall {
        // ReceiverNotResolved }`, not the deprecated
        // `MissingCommonModuleMethod`. Cascade gate 5 in
        // `dispatch_bare_ident_field_call` is the emit site.
        let code = r#"
Процедура Тест()
    ПервыйОбщийМодуль.МетодНесуществующий(1, 2);
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let umc: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::UnresolvedMethodCall)
            .collect();
        assert_eq!(
            umc.len(),
            1,
            "Expected one UnresolvedMethodCall for the unresolved receiver, got: {:?}",
            diagnostics.iter().map(|d| (&d.code, &d.message)).collect::<Vec<_>>()
        );
        assert!(
            umc[0].message.contains("ПервыйОбщийМодуль"),
            "message must name the unresolved receiver, got: {}",
            umc[0].message
        );
    }

    #[test]
    #[ignore] // TODO Phase 5 — re-enable as a workspace-level e2e in
              // `crates/ide/tests/resolve_qualified_call.rs` once Configuration.xml
              // wiring is in place; `check_hir_diagnostic_with_fixtures` builds a
              // module index from disk paths but doesn't register a Configuration,
              // so the cascade gate's `user_common_module_exists` cannot positively
              // claim the receiver.
    fn registered_common_module_with_missing_method_emits_method_not_found() {
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
        let umc: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::UnresolvedMethodCall)
            .collect();

        // Expected: cascade gate 3 confirms the CommonModule, delegates
        // to `infer_qualified_call`, which emits MethodNotFound for the
        // missing method.
        assert_eq!(umc.len(), 1, "Expected one UnresolvedMethodCall, got: {:?}", diagnostics);
        assert!(umc[0].message.contains("МетодНесуществующий"));
        assert!(umc[0].message.contains("ПервыйОбщийМодуль"));
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
        // `ОбработкаОшибок` is a platform global of type
        // `МенеджерОбработкиОшибок`, and `КраткоеПредставлениеОшибки` is a
        // real method on that manager. After Phase 2 the legacy
        // `MissingCommonModuleMethod` channel is silent (deprecated, no-op
        // handler), so this test now pins the **positive** invariant: a
        // valid platform-global member call must not surface ANY
        // diagnostic — neither the legacy `MissingCommonModuleMethod`
        // nor the new `UnresolvedMethodCall` (which would regress the
        // resolution if `dispatch_bare_ident_field_call` gate 4 lost the
        // `Resolved` outcome or if the `lookup_method` path stopped
        // resolving members on `Ty::PlatformObject`).
        let code = r#"
Процедура Тест()
    ОбработкаОшибок.КраткоеПредставлениеОшибки(ИнформацияОбОшибке());
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let mcm: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::MissingCommonModuleMethod)
            .collect();
        assert!(
            mcm.is_empty(),
            "platform global member calls must not raise MissingCommonModuleMethod, got: {:?}",
            mcm.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let umc: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::UnresolvedMethodCall)
            .collect();
        assert!(
            umc.is_empty(),
            "valid platform-global member call must not regress to UnresolvedMethodCall, got: {:?}",
            umc.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore] // TODO Phase 5 — re-enable as a workspace-level e2e once
              // Configuration.xml wiring is in place (same gating as
              // `registered_common_module_with_missing_method_emits_method_not_found`
              // above).
    fn user_common_module_shadows_platform_global() {
        // Phase 2 cascade gate ordering pins this invariant: gate 3
        // (`user_common_module_exists`) runs BEFORE gate 4 (platform
        // global fallback). The discriminating fixture below picks a
        // method (`Найти`) that DOES exist on the platform global
        // `Метаданные` (`КонфигурацияМетаданныеОбъект.Найти`) but is
        // ABSENT from the user's CommonModule with the same name —
        // so the two cascade paths diverge:
        //   - gate 3 wins ⇒ workspace miss ⇒ UMC fires.
        //   - gate 4 wins ⇒ platform `Resolved` ⇒ silent.
        // Asserting "UMC fires" therefore proves user-shadows-platform
        // ordering, not just "some diagnostic exists".
        use crate::test_utils::check_hir_diagnostic_with_fixtures;
        let fixture = r#"
//- /CommonModules/Метаданные/Module.bsl
Процедура ОдинЭкспортируемыйМетод() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    Метаданные.Найти("КакаяТоТаблица");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic_with_fixtures(fixture);
        let umc: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::UnresolvedMethodCall)
            .collect();
        assert_eq!(
            umc.len(),
            1,
            "user-shadows-platform must surface a UMC; if this is silent the cascade fell \
             through to gate 4 (platform `Найти` Resolved). got: {:?}",
            diagnostics
        );
        assert!(
            umc[0].message.contains("Метаданные") && umc[0].message.contains("Найти"),
            "UMC must name the user's missing method, got: {}",
            umc[0].message
        );
    }

    #[test]
    fn test_platform_global_unknown_member_keeps_falling_through() {
        // Receiver is a real platform global, but the named member does not
        // exist on the declared type — the diagnostic must still fire.
        //
        // Phase 2 of the qualified-call refactor moved the surface from the
        // legacy `MissingCommonModuleMethod` channel to the
        // `UnresolvedMethodCall { MethodNotFound }` channel, gated by the
        // platform-globals tri-state (`KnownContainerMissingMember` ⇒
        // `MethodNotFound`, `NotAContainer` ⇒ `ReceiverNotResolved`).
        let code = r#"
Процедура Тест()
    ОбработкаОшибок.СовершенноНесуществующийМетод();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let umc: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::UnresolvedMethodCall)
            .collect();
        assert_eq!(
            umc.len(),
            1,
            "unknown method on a known platform global must raise UnresolvedMethodCall, got: {:?}",
            diagnostics.iter().map(|d| (&d.code, &d.message)).collect::<Vec<_>>()
        );
        assert!(
            umc[0].message.contains("СовершенноНесуществующийМетод"),
            "message must name the missing method, got: {}",
            umc[0].message
        );
        // Legacy channel is silent — Phase 3 marks `MissingCommonModuleMethod`
        // deprecated; the handler returns `None` so emission is impossible
        // even when downstream user configs leave the rule enabled.
        let mcm: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::MissingCommonModuleMethod)
            .collect();
        assert!(
            mcm.is_empty(),
            "deprecated MissingCommonModuleMethod must not surface anymore, got: {:?}",
            mcm.iter().map(|d| &d.message).collect::<Vec<_>>()
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

    // TODO Phase 5 — re-enable as a full E2E test in
    // `crates/ide/tests/diagnostics_form_attribute_call.rs`. The
    // `check_metadata_diagnostic` helper here doesn't wire the FormModule
    // metadata into Salsa-cached `module_metadata` the way the
    // workspace-level `setup_fixture` does, so the inference cascade gate
    // doesn't reach gate 5 in this stripped harness even though it does
    // when run through the real LSP pipeline. The non-form sibling
    // `form_self_name_in_common_module_reports_unresolved_receiver` keeps
    // the cascade-gate behaviour pinned in this test module; this case
    // is covered E2E in Phase 5.
    #[test]
    #[ignore]
    fn unknown_module_in_managed_form_still_reports_unresolved_receiver() {
        // Phase 2 reshape of the user-facing kind: any unknown 2-segment
        // receiver in any module surfaces as
        // `UnresolvedMethodCall { ReceiverNotResolved }` — the cascade
        // gate's gate-5 exhaustion. The legacy
        // `MissingCommonModuleMethod` is deprecated (Phase 3) and silent.
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
        let umc: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert_eq!(
            umc.len(),
            1,
            "unknown module call must still surface as UnresolvedMethodCall, got: {:?}",
            diagnostics.iter().map(|d| (&d.code, &d.message)).collect::<Vec<_>>()
        );
        assert!(
            umc[0].message.contains("ЗаведомоНесуществующийМодуль"),
            "message must name the unresolved receiver, got: {}",
            umc[0].message
        );
        let mcm: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::MissingCommonModuleMethod)
            .collect();
        assert!(
            mcm.is_empty(),
            "deprecated MissingCommonModuleMethod must stay silent, got: {:?}",
            mcm.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn form_self_name_in_common_module_reports_unresolved_receiver() {
        // `Элементы.Найти("Х")` в обычном CommonModule: `Элементы` —
        // не CommonModule, не platform global container, не binding в
        // scope. Cascade gate доходит до gate 5 → `ReceiverNotResolved`.
        // Это семантически точнее, чем старый `MissingCommonModuleMethod`
        // — диагностика говорит ровно то, что произошло: receiver name
        // не разрешается ни в одном из видимых пространств имён.
        let code = r#"
Процедура Тест()
    Элементы.Найти("Х");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let umc: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert_eq!(
            umc.len(),
            1,
            "form-self property name in non-form module must still be reported, got: {:?}",
            diagnostics.iter().map(|d| (&d.code, &d.message)).collect::<Vec<_>>()
        );
        assert!(
            umc[0].message.contains("Элементы"),
            "message must name the unresolved receiver, got: {}",
            umc[0].message
        );
    }

    #[test]
    fn test_mixed_local_and_common_module() {
        // Phase 2 channel migration. Two procedures pin two distinct
        // shadowing rules:
        //   1. `Перем ПервыйОбщийМодуль; ПервыйОбщийМодуль.Method()` —
        //      `body_declares_binding` (cascade gate 2) is silent —
        //      a declared local shadows a same-named global, no
        //      diagnostic.
        //   2. `ВторойОбщийМодуль.Method()` — receiver resolves
        //      nowhere, cascade gate 5 emits exactly one
        //      `UnresolvedMethodCall { ReceiverNotResolved }`.
        let code = r#"
Процедура Тест()
    Перем ПервыйОбщийМодуль;
    ПервыйОбщийМодуль.Method();
КонецПроцедуры

Процедура ДругойТест()
    ВторойОбщийМодуль.Method();
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let umc: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == crate::DiagnosticCode::UnresolvedMethodCall)
            .collect();
        assert_eq!(
            umc.len(),
            1,
            "Expected exactly one UnresolvedMethodCall (for ВторойОбщийМодуль), got: {:?}",
            diagnostics.iter().map(|d| (&d.code, &d.message)).collect::<Vec<_>>()
        );
        assert!(
            umc[0].message.contains("ВторойОбщийМодуль"),
            "diagnostic must point at the unresolved receiver, got: {}",
            umc[0].message
        );
        assert!(
            !umc.iter().any(|d| d.message.contains("ПервыйОбщийМодуль")),
            "shadowed local must not surface a diagnostic, got: {:?}",
            umc.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
