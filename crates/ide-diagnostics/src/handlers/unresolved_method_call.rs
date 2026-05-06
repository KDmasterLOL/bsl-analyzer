//! Reports qualified method calls that cannot be resolved semantically.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Name, UnresolvedMethodKind};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates a diagnostic from `InferenceDiagnostic::UnresolvedMethodCall`.
pub fn from_hir(
    receiver_name: &Name,
    method_name: &Name,
    kind: UnresolvedMethodKind,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let message = match kind {
        UnresolvedMethodKind::MethodNotFound => {
            format!("Метод '{}' не найден у '{}'", method_name.as_str(), receiver_name.as_str())
        }
        UnresolvedMethodKind::MethodNotExport => {
            format!("Метод '{}.{}' не экспортирован", receiver_name.as_str(), method_name.as_str())
        }
        UnresolvedMethodKind::CommonModuleNoSource => {
            format!("Для общего модуля '{}' не найден исходный файл", receiver_name.as_str())
        }
        UnresolvedMethodKind::ReceiverNotResolved => {
            format!("Не удалось разрешить модуль '{}'", receiver_name.as_str())
        }
    };
    crate::simple_hir_diagnostic(DiagnosticCode::UnresolvedMethodCall, message, range, ctx)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic_with_fixtures;
    use crate::DiagnosticCode;

    #[test]
    fn emits_when_method_not_found_on_existing_module() {
        // Module resolves through module_index but the method doesn't exist
        // in its symbol_tree. The channel must surface this to LSP.
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
Процедура СуществующийМетод() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ОбщийМодуль.НесуществующийМетод();
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let unresolved: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert_eq!(unresolved.len(), 1, "expected one UnresolvedMethodCall, got: {diags:?}");
        assert!(
            unresolved[0].message.contains("НесуществующийМетод"),
            "message must name the missing method, got: {}",
            unresolved[0].message
        );
    }

    #[test]
    fn stays_silent_when_method_exists_and_is_exported() {
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
Процедура Метод() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ОбщийМодуль.Метод();
КонецПроцедуры
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::UnresolvedMethodCall),
            "resolved call must not fire UnresolvedMethodCall: {diags:?}"
        );
    }

    // ---- Phase 5 cascade-gate regression coverage ----

    use crate::test_utils::check_hir_diagnostic;

    #[test]
    fn parameter_shadows_qualified_call() {
        // Cascade gate 1 (`Resolver::resolve_name`) catches a parameter
        // — `Local` resolution silences the call. Pre-Phase-2 this case
        // was filtered by `analyze_qualified_call`'s `param_names`
        // probe in lowering; the inference-side check covers it now.
        let code = r#"
Процедура Тест(М)
    М.КакойТоМетод();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let umc: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert!(
            umc.is_empty(),
            "parameter receiver must not fire UnresolvedMethodCall, got: {umc:?}"
        );
    }

    #[test]
    fn declared_local_var_shadows_qualified_call() {
        // Cascade gate 2 (`body_declares_binding`) catches a declared
        // `Перем` even when no prior assignment has typed it. The
        // implicit-local sibling case (`X = НеизвестнаяФункция(); X.Метод()`)
        // is pinned in `crates/ide/tests/resolve_qualified_call.rs`.
        let code = r#"
Процедура Тест()
    Перем М;
    М.Метод();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let umc: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert!(
            umc.is_empty(),
            "declared `Перем` receiver must not fire UnresolvedMethodCall, got: {umc:?}"
        );
    }

    #[test]
    fn platform_global_method_call_resolves_silently() {
        // Cascade gate 4 (`try_resolve_platform_global_member` →
        // `Resolved`) types the call directly off the platform
        // catalogue. `ОбработкаОшибок.КраткоеПредставлениеОшибки`
        // is a real method on the platform global container, so the
        // gate short-circuits with a typed return — no diagnostic.
        let code = r#"
Процедура Тест()
    ОбработкаОшибок.КраткоеПредставлениеОшибки(ИнформацияОбОшибке());
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let umc: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert!(
            umc.is_empty(),
            "valid platform-global member call must not fire UnresolvedMethodCall, got: {umc:?}"
        );
    }

    #[test]
    fn unregistered_module_emits_receiver_not_resolved() {
        // Cascade gate 5 — receiver name resolves nowhere
        // (no scope binding, no body binding, no CommonModule, no
        // platform global container). The kind is
        // `ReceiverNotResolved`, NOT `MethodNotFound` — Phase 2 split
        // the legacy collapse so the message points at the actual
        // failure (the receiver name itself).
        //
        // The kind is observable through the diagnostic message
        // because `from_hir` formats each `UnresolvedMethodKind` into
        // a distinct Russian phrase. Asserting on the
        // `ReceiverNotResolved`-specific phrase (`Не удалось
        // разрешить модуль`) catches a regression that would
        // silently demote the kind to `MethodNotFound` (which would
        // render `Метод '...' не найден у '...'`).
        let code = r#"
Процедура Тест()
    ЗаведомоНесуществующийПрефикс.КакойТоМетод();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let umc: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnresolvedMethodCall).collect();
        assert_eq!(
            umc.len(),
            1,
            "unresolved receiver must surface exactly one UnresolvedMethodCall, got: {umc:?}"
        );
        assert!(
            umc[0].message.contains("Не удалось разрешить модуль")
                && umc[0].message.contains("ЗаведомоНесуществующийПрефикс"),
            "message must use the ReceiverNotResolved phrasing and name the receiver; \
             a MethodNotFound-shaped phrasing here would be a regression. got: {}",
            umc[0].message
        );
    }
}
