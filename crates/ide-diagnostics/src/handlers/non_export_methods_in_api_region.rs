use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{
    ast::{self, AstNode},
    TextRange,
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if ctx.config.is_disabled(DiagnosticCode::NonExportMethodsInApiRegion) {
        return diagnostics;
    }

    let skip_annotated_methods = ctx
        .config
        .get_bool(DiagnosticCode::NonExportMethodsInApiRegion, "skipAnnotatedMethods")
        .unwrap_or(false);

    let method_regions = ctx.db.method_regions(ctx.file_id);
    let item_tree = ctx.db.item_tree(ctx.file_id);

    for (_, proc) in item_tree.procedures() {
        if !proc.is_export {
            if let Some(region_name) = method_regions.get(&proc.source_range) {
                if skip_annotated_methods && has_non_custom_annotations(ctx, &proc.source_range) {
                    continue;
                }

                let name_range = get_method_name_range(ctx, &proc.source_range);

                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::NonExportMethodsInApiRegion,
                    message: format!(
                        "Move non export method \"{}\" from \"{}\" region",
                        proc.name.as_str(),
                        region_name
                    ),
                    severity: Severity::Major,
                    range: name_range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }

    for (_, func) in item_tree.functions() {
        if !func.is_export {
            if let Some(region_name) = method_regions.get(&func.source_range) {
                if skip_annotated_methods && has_non_custom_annotations(ctx, &func.source_range) {
                    continue;
                }

                let name_range = get_method_name_range(ctx, &func.source_range);

                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::NonExportMethodsInApiRegion,
                    message: format!(
                        "Move non export method \"{}\" from \"{}\" region",
                        func.name.as_str(),
                        region_name
                    ),
                    severity: Severity::Major,
                    range: name_range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }

    diagnostics
}

fn has_non_custom_annotations(ctx: &DiagnosticsContext, source_range: &TextRange) -> bool {
    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    for node in root.descendants() {
        if let Some(proc) = ast::ProcedureDef::cast(node.clone()) {
            if let Some(name) = proc.name() {
                if source_range.contains_range(name.text_range()) {
                    return has_non_custom_annotations_ast(proc.annotations());
                }
            }
        } else if let Some(func) = ast::FunctionDef::cast(node) {
            if let Some(name) = func.name() {
                if source_range.contains_range(name.text_range()) {
                    return has_non_custom_annotations_ast(func.annotations());
                }
            }
        }
    }

    false
}

fn has_non_custom_annotations_ast(annotations: impl Iterator<Item = ast::Annotation>) -> bool {
    for ann in annotations {
        if let Some(token) = ann.kind_token() {
            let text = token.text().trim_start_matches('&');

            const BUILTIN_ANNOTATIONS: &[&str] = &[
                "НаКлиенте",
                "AtClient",
                "НаСервере",
                "AtServer",
                "НаКлиентеНаСервере",
                "AtClientAtServer",
                "НаКлиентеНаСервереБезКонтекста",
                "AtClientAtServerNoContext",
                "Вместо",
                "Instead",
                "До",
                "Before",
                "После",
                "After",
                "НаСервереБезКонтекста",
                "AtServerNoContext",
            ];

            let lower = text.to_lowercase();
            for builtin in BUILTIN_ANNOTATIONS {
                if lower == builtin.to_lowercase() {
                    return true;
                }
            }
        }
    }
    false
}

fn get_method_name_range(ctx: &DiagnosticsContext, source_range: &TextRange) -> TextRange {
    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    for node in root.descendants() {
        if let Some(proc) = ast::ProcedureDef::cast(node.clone()) {
            if let Some(name) = proc.name() {
                if source_range.contains_range(name.text_range()) {
                    return name.text_range();
                }
            }
        } else if let Some(func) = ast::FunctionDef::cast(node) {
            if let Some(name) = func.name() {
                if source_range.contains_range(name.text_range()) {
                    return name.text_range();
                }
            }
        }
    }

    *source_range
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
