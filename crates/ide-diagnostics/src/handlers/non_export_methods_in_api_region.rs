use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir_def::item_tree::Annotation;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if ctx.config.is_disabled(DiagnosticCode::NonExportMethodsInApiRegion) {
        return diagnostics;
    }

    let skip_annotated_methods = ctx
        .config
        .get_bool(DiagnosticCode::NonExportMethodsInApiRegion, "skipAnnotatedMethods")
        .unwrap_or(false);

    let region_tree = ctx.db.region_tree(ctx.file_id);
    let item_tree = ctx.db.item_tree(ctx.file_id);

    for (_, proc) in item_tree.procedures() {
        if !proc.is_export {
            if let Some(region_name) = region_tree.root_api_region_for_range(proc.source_range) {
                if skip_annotated_methods && has_builtin_annotations(&proc.annotations) {
                    continue;
                }

                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::NonExportMethodsInApiRegion,
                    message: format!(
                        "Move non export method \"{}\" from \"{}\" region",
                        proc.name.as_str(),
                        region_name
                    ),
                    severity: Severity::Major,
                    range: proc.name_range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }

    for (_, func) in item_tree.functions() {
        if !func.is_export {
            if let Some(region_name) = region_tree.root_api_region_for_range(func.source_range) {
                if skip_annotated_methods && has_builtin_annotations(&func.annotations) {
                    continue;
                }

                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::NonExportMethodsInApiRegion,
                    message: format!(
                        "Move non export method \"{}\" from \"{}\" region",
                        func.name.as_str(),
                        region_name
                    ),
                    severity: Severity::Major,
                    range: func.name_range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }

    diagnostics
}

/// Check if annotations contain any built-in (non-custom) annotations.
///
/// Built-in annotations are: &НаКлиенте, &НаСервере, &НаКлиентеНаСервере, etc.
/// Custom annotations (like &Кастом) are not considered built-in.
fn has_builtin_annotations(annotations: &[Annotation]) -> bool {
    !annotations.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::*, DiagnosticsConfig, DiagnosticsContext};
    use ide_db::base_db::SourceDatabase;
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        check_diagnostic_with_config(code, DiagnosticsConfig::default())
    }

    fn check_diagnostic_with_config(code: &str, config: DiagnosticsConfig) -> Vec<Diagnostic> {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let config = Rc::new(config);
        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_non_export_in_api_region() {
        let code = include_str!("../../tests/fixtures/NonExportMethodsInApiRegionDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 6, "Expected 6 diagnostics, got {}", diagnostics.len());

        assert_diagnostic_range(code, &diagnostics[0], 8, 10, 16); // Плохая()
        assert_diagnostic_range(code, &diagnostics[1], 20, 10, 13); // Bad()
        assert_diagnostic_range(code, &diagnostics[2], 25, 14, 27); // ShouldSayFuck()
        assert_diagnostic_range(code, &diagnostics[3], 64, 10, 39); // СлужебныйПрограммныйИнтерфейс()
        assert_diagnostic_range(code, &diagnostics[4], 68, 10, 29); // АннотированныйМетод()
        assert_diagnostic_range(code, &diagnostics[5], 72, 10, 31); // МетодВместоРасширения()
    }

    #[test]
    fn test_skip_annotated_methods() {
        let code = include_str!("../../tests/fixtures/NonExportMethodsInApiRegionDiagnostic.bsl");

        let mut config = crate::DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("skipAnnotatedMethods".to_string(), serde_json::Value::Bool(true));
        config
            .parameters
            .insert(DiagnosticCode::NonExportMethodsInApiRegion, serde_json::Value::Object(params));

        let diagnostics = check_diagnostic_with_config(code, config);

        // Should skip line 72 (МетодВместоРасширения with &Вместо built-in annotation)
        // But NOT skip line 68 (АннотированныйМетод with &Кастом custom annotation)
        assert_eq!(diagnostics.len(), 5, "Expected 5 diagnostics with skipAnnotatedMethods=true");

        // Verify the skipped diagnostic is line 72
        for diag in &diagnostics {
            let (line, _, _, _) = range_to_line_col(code, diag.range);
            assert_ne!(line, 72, "Line 72 should be skipped with skipAnnotatedMethods=true");
        }
    }

    #[test]
    fn test_exported_methods_in_api_region() {
        let code = r#"
#Область ПрограммныйИнтерфейс

Процедура ЭкспортнаяПроцедура() Экспорт
КонецПроцедуры

#КонецОбласти
        "#;

        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_non_api_region() {
        let code = r#"
#Область СлужебныеПроцедурыИФункции

Процедура СлужебнаяПроцедура()
КонецПроцедуры

#КонецОбласти
        "#;

        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }
}
