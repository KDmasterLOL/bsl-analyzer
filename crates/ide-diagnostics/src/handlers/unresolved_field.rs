//! Reports field access that cannot be resolved for a known receiver type.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Name, Ty};
use ide_db::TextRange;

// Major/Error is appropriate here because the emit side is intentionally
// conservative: only high-confidence typed receivers produce this diagnostic.
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

/// Creates a diagnostic from `InferenceDiagnostic::UnresolvedField`.
pub fn from_hir(
    receiver_ty: &Ty,
    field_name: &Name,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let message = format!(
        "Поле '{}' не найдено у типа '{}'",
        field_name.as_str(),
        receiver_ty.display_name()
    );
    crate::simple_hir_diagnostic(DiagnosticCode::UnresolvedField, message, range, ctx)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic_with_fixtures;
    use crate::DiagnosticCode;

    #[test]
    fn emits_on_module_level_code_not_only_inside_methods() {
        // Coverage for the `DefWithBodyId::ModuleCode` branch in
        // `hir_inference_dispatch` — statements outside any procedure go
        // through `module_code_result()`'s source map, which is a
        // different path from method bodies (keyed on `MethodId`).
        // Without this test the module-code branch is compiled but
        // never exercised.
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникСсылка.Справочник1
Функция Ссылка() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
С = ОбщийМодуль.Ссылка();
Х = С.НесуществующееПоле;
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let unresolved: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::UnresolvedField).collect();
        assert_eq!(
            unresolved.len(),
            1,
            "UnresolvedField must surface for module-level expressions too, got: {diags:?}"
        );
    }

    #[test]
    fn emits_on_missing_field_of_known_catalog_ref() {
        // JSDoc annotates the return as CatalogRef.Справочник1, so the
        // receiver `С` gets `Ty::MetadataRef { CatalogRef, Справочник1 }`.
        // `С.НесуществующееПоле` must fire UnresolvedField.
        let fixture = r#"
//- /CommonModules/ОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникСсылка.Справочник1
Функция Ссылка() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    С = ОбщийМодуль.Ссылка();
    Возврат С.НесуществующееПоле;
КонецФункции
"#;
        let diags = check_hir_diagnostic_with_fixtures(fixture);
        let unresolved: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::UnresolvedField).collect();
        assert_eq!(unresolved.len(), 1, "expected one UnresolvedField, got: {diags:?}");
        assert!(
            unresolved[0].message.contains("НесуществующееПоле"),
            "message must name the missing field, got: {}",
            unresolved[0].message
        );
    }
}
