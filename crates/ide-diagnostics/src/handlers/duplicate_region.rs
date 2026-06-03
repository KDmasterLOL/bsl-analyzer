use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::RegionTree;
use ide_db::TextRange;
use std::collections::HashMap;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Compatibility8320,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("DuplicateRegion::check").entered();

    let code = DiagnosticCode::DuplicateRegion;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let region_tree = ctx.region_tree();
    let diagnostics = report_duplicates(&region_tree, code, ctx);

    tracing::debug!(count = diagnostics.len(), "DuplicateRegion diagnostics found");
    diagnostics
}

fn canonical_key(name: &str) -> String {
    hir::module_structure::canonical::canonical_alias(name)
        .map(str::to_string)
        .unwrap_or_else(|| name.to_string())
}

fn report_duplicates(
    region_tree: &RegionTree,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let mut groups: HashMap<String, Vec<(String, TextRange)>> = HashMap::new();

    for idx in region_tree.module_level_regions() {
        let region = region_tree.region(idx);
        let canonical = canonical_key(region.name.as_str());
        groups
            .entry(canonical)
            .or_default()
            .push((region.name.as_str().to_string(), region.directive_range));
    }

    let mut diagnostics = Vec::new();

    for (_canonical, group) in groups {
        if group.len() > 1 {
            let (first_name, first_range) = &group[0];

            diagnostics.push(Diagnostic {
                code,
                message: format!("Нужно удалить дубли раздела \"{}\"", first_name),
                severity: ctx.severity(code),
                range: *first_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic, check_diagnostics_snapshot_for, format_diags};
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_duplicate_internal_regions() {
        let code = r#"#Область СлужебныйПрограммныйИнтерфейс
// код
#КонецОбласти

#Region Internal
// код
#EndRegion"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DuplicateRegion @ 1:1..1:39
              message: Нужно удалить дубли раздела "СлужебныйПрограммныйИнтерфейс"
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &diagnostics));
        assert!(diagnostics[0].message.contains("СлужебныйПрограммныйИнтерфейс"));
    }

    #[test]
    fn test_duplicate_private_regions() {
        let code = r#"#Область СлужебныеПроцедурыИФункции
// код
#КонецОбласти

#Region Private
// код
#EndRegion"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DuplicateRegion @ 1:1..1:36
              message: Нужно удалить дубли раздела "СлужебныеПроцедурыИФункции"
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &diagnostics));
        assert!(diagnostics[0].message.contains("СлужебныеПроцедурыИФункции"));
    }

    #[test]
    fn test_duplicate_event_handlers_regions() {
        let code = r#"#Region EventHandlers
// код
#EndRegion

#Область ОбработчикиСобытий
// код
#КонецОбласти"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DuplicateRegion @ 1:1..1:22
              message: Нужно удалить дубли раздела "EventHandlers"
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &diagnostics));
        assert!(diagnostics[0].message.contains("EventHandlers"));
    }

    #[test]
    fn test_no_duplicates() {
        let code = r#"
#Область ПрограммныйИнтерфейс
#КонецОбласти

#Область СлужебныйПрограммныйИнтерфейс
#КонецОбласти

#Область СлужебныеПроцедурыИФункции
#КонецОбласти
        "#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::DuplicateRegion, expect![[r#""#]]);
    }

    #[test]
    fn test_nested_regions_ignored() {
        let code = r#"
#Область ПрограммныйИнтерфейс
    Процедура Тест()
        #Область ВложеннаяОбласть
        #КонецОбласти
    КонецПроцедуры
#КонецОбласти

#Область ВложеннаяОбласть
// This is module-level, not nested - different from the one inside procedure
#КонецОбласти
        "#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::DuplicateRegion, expect![[r#""#]]);
    }

    #[test]
    fn test_standard_ru_en_duplicate() {
        let code = r#"
#Область СлужебныйПрограммныйИнтерфейс
#КонецОбласти

#Region Internal
// This is a duplicate - Russian and English variants map to same canonical name
#EndRegion
        "#;

        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DuplicateRegion @ 2:1..2:39
              message: Нужно удалить дубли раздела "СлужебныйПрограммныйИнтерфейс"
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &diagnostics));
        assert!(diagnostics[0].message.contains("СлужебныйПрограммныйИнтерфейс"));
    }

    #[test]
    fn test_case_insensitive_canonical() {
        let code = r#"
#Region Internal
#EndRegion

#Region INTERNAL
// This is a duplicate - case-insensitive canonical matching
#EndRegion
        "#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::DuplicateRegion,
            expect![[r#"
                DuplicateRegion @ 2:1..2:17
                  message: Нужно удалить дубли раздела "Internal"
                  severity: Hint"#]],
        );
    }

    #[test]
    fn test_non_standard_exact_match() {
        let code = r#"
#Область КастомнаяОбласть
#КонецОбласти

#Область кастомнаяобласть
// Different case - non-standard regions are case-sensitive
#КонецОбласти
        "#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::DuplicateRegion, expect![[r#""#]]);
    }

    #[test]
    fn test_non_standard_exact_duplicate() {
        let code = r#"
#Область КастомнаяОбласть
#КонецОбласти

#Область КастомнаяОбласть
// Exact match - this is a duplicate
#КонецОбласти
        "#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::DuplicateRegion,
            expect![[r#"
                DuplicateRegion @ 2:1..2:26
                  message: Нужно удалить дубли раздела "КастомнаяОбласть"
                  severity: Hint"#]],
        );
    }
}
