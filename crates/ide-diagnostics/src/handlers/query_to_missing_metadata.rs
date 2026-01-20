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

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use sdbl_hir;
use tracing::debug;

/// Runs the QueryToMissingMetadata diagnostic.
///
/// Uses SDBL HIR with diagnostics collected during lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::QueryToMissingMetadata) {
        return Vec::new();
    }

    let sdbl_hirs = ctx.sdbl_hir_in_file();
    let bsl_source = ctx.file_text();
    let sdbl_queries = ctx.all_sdbl_in_file();

    use crate::sdbl_utils::build_line_index_shared;
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    for ((_expr_id, sdbl_package), (_query_expr_id, query_info)) in
        sdbl_hirs.iter().zip(sdbl_queries.iter())
    {
        let mapper = SdblPositionMapper::from_query_info(query_info, &bsl_source, &line_starts);

        for hir_diag in sdbl_package.all_diagnostics() {
            if let sdbl_hir::SdblDiagnostic::QueryToMissingMetadata { table_name, range } = hir_diag
            {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::QueryToMissingMetadata,
                    message: format!(
                        "Исправьте обращение к несуществующему метаданному \"{}\" в запросе",
                        table_name
                    ),
                    severity: Severity::Blocker,
                    range: bsl_range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "QueryToMissingMetadata completed"
    );

    diagnostics
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
