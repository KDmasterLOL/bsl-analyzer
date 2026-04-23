//! UnresolvedMethodCall diagnostic.
//!
//! Emitted from `hir-ty::infer` when a qualified call
//! (`CommonModule.Method(...)`) fails to resolve through the workspace's
//! `module_index` + `symbol_tree` pipeline. Four distinct situations funnel
//! into one diagnostic code, disambiguated by
//! [`hir::UnresolvedMethodKind`]:
//!
//! - `MethodNotFound` — module resolved but no method by that name.
//! - `MethodNotExport` — module + method exist, but method lacks `Экспорт`.
//! - `CommonModuleNoSource` — module referenced in metadata, source file
//!   missing from VFS.
//! - `ReceiverNotResolved` — receiver couldn't be resolved at all.

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

/// Creates diagnostic from `InferenceDiagnostic::UnresolvedMethodCall`.
pub fn from_hir(
    receiver_name: &Name,
    method_name: &Name,
    kind: UnresolvedMethodKind,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let message = match kind {
        UnresolvedMethodKind::MethodNotFound => format!(
            "Метод '{}' не найден в модуле '{}'",
            method_name.as_str(),
            receiver_name.as_str()
        ),
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
}
