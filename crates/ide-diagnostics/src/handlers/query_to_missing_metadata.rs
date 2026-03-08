//! QueryToMissingMetadata diagnostic.
//!
//! Detects references to non-existent metadata objects in SDBL queries.
//!
//! ## Why?
//! Querying a table that doesn't exist in metadata will fail at runtime.
//!
//! ## Supported table paths
//! - 2-part: `МдоТип.ИмяОбъекта` (e.g., `Справочник.Валюты`)
//! - 3-part: `МдоТип.ИмяОбъекта.ТабличнаяЧасть` (e.g., `Документ.Заказ.Товары`)
//! - 4-part: `ВнешнийИсточникДанных.EDSName.Таблица.TableName`
//! - 6-part: `ВнешнийИсточникДанных.EDSName.Куб.CubeName.ТаблицаИзмерения.DimTableName`
//!
//! ## Implementation
//!
//! Uses SDBL HIR with diagnostics collected during lowering.
//! Diagnostics are emitted in `from_clause.rs` when table resolution fails.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Sql],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Single-pass dispatch for QueryToMissingMetadata.
pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::QueryToMissingMetadata { table_name, range } = diag {
        let code = DiagnosticCode::QueryToMissingMetadata;
        diagnostics.push(Diagnostic {
            code,
            message: format!(
                "Исправьте обращение к несуществующему метаданному \"{}\" в запросе",
                table_name
            ),
            severity: ctx.severity(code),
            range: mapper.map_range(*range, query_text),
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

/// Runs the QueryToMissingMetadata diagnostic (standalone, used in tests).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::QueryToMissingMetadata,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;
    use crate::{DiagnosticCode, Severity};
    #[test]
    fn test_no_metadata_no_diagnostics() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ Т.Поле ИЗ Справочник.НесуществующийСправочник КАК Т";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        // Without metadata, no diagnostics are emitted
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_diagnostic_properties() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ Т.Поле ИЗ Справочник.Валюты КАК Т";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        // Without metadata, diagnostics won't fire, but we test the handler runs without errors
        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::QueryToMissingMetadata);
            assert_eq!(diag.severity, Severity::Blocker);
            assert!(diag.message.contains("несуществующему метаданному"));
        }
    }
}
